use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::config::Config;

/// Thin HTTP client around ClickHouse. We talk JSONEachRow on insert and JSON on read.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    url: url::Url,
    database: String,
    auth: (String, String),
}

impl Client {
    pub async fn new(cfg: &Config) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/x-ndjson"));
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .pool_idle_timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            http,
            url: url::Url::parse(&cfg.clickhouse_url).context("invalid CLICKHOUSE_URL")?,
            database: cfg.clickhouse_database.clone(),
            auth: (cfg.clickhouse_user.clone(), cfg.clickhouse_password.clone()),
        })
    }

    /// Poll ClickHouse until `SELECT 1` succeeds. Run on startup.
    pub async fn wait_until_ready(&self) -> Result<()> {
        for attempt in 1..=60 {
            match self.query_raw("SELECT 1").await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    tracing::warn!(attempt, error = %e, "clickhouse not ready, retrying");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
        Err(anyhow!("clickhouse did not become ready"))
    }

    fn endpoint(&self) -> url::Url {
        self.url.clone()
    }

    /// Run a SELECT/DDL/INSERT query. Returns raw response body.
    pub async fn query_raw(&self, sql: &str) -> Result<String> {
        let mut url = self.endpoint();
        url.query_pairs_mut()
            .append_pair("database", &self.database)
            // emit 64-bit integers as numbers so serde_json -> u64 deserialises directly
            .append_pair("output_format_json_quote_64bit_integers", "0")
            // tolerate NaN/Inf in metric/value columns
            .append_pair("output_format_json_quote_denormals", "1")
            // accept rows where optional columns are missing (defaults applied)
            .append_pair("input_format_defaults_for_omitted_fields", "1")
            .append_pair("date_time_input_format", "best_effort");
        let resp = self
            .http
            .post(url)
            .basic_auth(&self.auth.0, Some(&self.auth.1))
            .body(sql.to_string())
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(anyhow!("clickhouse {}: {}", status, text));
        }
        Ok(text)
    }

    /// SELECT returning rows as Vec<T>. Adds `FORMAT JSONEachRow` automatically.
    pub async fn select<T: DeserializeOwned>(&self, sql: &str) -> Result<Vec<T>> {
        let q = format!("{} FORMAT JSONEachRow", sql);
        let body = self.query_raw(&q).await?;
        let mut rows = Vec::new();
        for line in body.lines() {
            if line.trim().is_empty() {
                continue;
            }
            rows.push(serde_json::from_str(line).with_context(|| format!("decoding row: {line}"))?);
        }
        Ok(rows)
    }

    /// SELECT one scalar value.
    pub async fn select_one<T: DeserializeOwned>(&self, sql: &str) -> Result<Option<T>> {
        let rows: Vec<T> = self.select(sql).await?;
        Ok(rows.into_iter().next())
    }

    /// Insert rows in JSONEachRow format. Batches up to ~10 MB are fine; we keep them small.
    pub async fn insert<T: Serialize>(&self, table: &str, rows: &[T]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut body = String::with_capacity(rows.len() * 256);
        for r in rows {
            let line = serde_json::to_string(r)?;
            body.push_str(&line);
            body.push('\n');
        }
        let mut url = self.endpoint();
        url.query_pairs_mut()
            .append_pair("database", &self.database)
            // wait for async-insert flush so parse errors surface as 4xx/5xx instead
            // of being silently dropped from the buffer.
            .append_pair("wait_for_async_insert", "1")
            .append_pair("date_time_input_format", "best_effort")
            .append_pair("input_format_defaults_for_omitted_fields", "1")
            .append_pair("query", &format!("INSERT INTO {table} FORMAT JSONEachRow"));

        let resp = self
            .http
            .post(url)
            .basic_auth(&self.auth.0, Some(&self.auth.1))
            .header(CONTENT_TYPE, "application/x-ndjson")
            .body(body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("clickhouse insert {} into {}: {}", status, table, text));
        }
        Ok(())
    }
}
