use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::config::Config;

/// Cliente HTTP delgado sobre ClickHouse. Hablamos JSONEachRow al insertar y JSON al leer.
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
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/x-ndjson"),
        );
        // Pool HTTP explícito hacia ClickHouse:
        // - `pool_max_idle_per_host`: el default de reqwest es ilimitado, lo que en
        //   bursts deja el pool inflado sin recortarse. Lo acotamos a un valor que
        //   cubre el paralelismo realista (workers de ingest + queries del dashboard).
        // - `pool_idle_timeout(30s)`: recorta conexiones inactivas más rápido que el
        //   default de 90s, para liberar FDs cuando el tráfico baja.
        // - `tcp_keepalive(60s)`: probes TCP para que load balancers/NAT no nos cierren
        //   silenciosamente conexiones idle (sin esto, el primer reuso post-idle puede
        //   fallar con "broken pipe" y costar un reintento).
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .pool_max_idle_per_host(cfg.clickhouse_pool_max_idle)
            .pool_idle_timeout(Duration::from_secs(30))
            .tcp_keepalive(Duration::from_secs(60))
            .tcp_nodelay(true)
            .build()?;

        Ok(Self {
            http,
            url: url::Url::parse(&cfg.clickhouse_url).context("CLICKHOUSE_URL inválida")?,
            database: cfg.clickhouse_database.clone(),
            auth: (cfg.clickhouse_user.clone(), cfg.clickhouse_password.clone()),
        })
    }

    /// Hace polling a ClickHouse hasta que `SELECT 1` tenga éxito. Se llama al arrancar.
    pub async fn wait_until_ready(&self) -> Result<()> {
        for attempt in 1..=60 {
            match self.query_raw("SELECT 1").await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    tracing::warn!(attempt, error = %e, "clickhouse no está listo, reintentando");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
        Err(anyhow!("clickhouse no llegó a estar listo"))
    }

    fn endpoint(&self) -> url::Url {
        self.url.clone()
    }

    /// Ejecuta un query SELECT/DDL/INSERT. Devuelve el cuerpo de respuesta crudo.
    pub async fn query_raw(&self, sql: &str) -> Result<String> {
        self.query_raw_with_params(sql, &[]).await
    }

    /// Variante parametrizada de `query_raw`. Cada `(name, value)` se envía como
    /// `param_<name>=<value>` y se referencia desde el SQL como `{name:Type}`. ClickHouse
    /// hace el binding del lado servidor, sin interpolación de strings — esta es la única
    /// forma segura de pasar input controlado por el usuario a un query. Ver docs oficiales
    /// (HTTP interface → parameterized queries).
    pub async fn query_raw_with_params(
        &self,
        sql: &str,
        params: &[(&str, &str)],
    ) -> Result<String> {
        let mut url = self.endpoint();
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("database", &self.database)
                // emite enteros de 64 bits como números para que serde_json -> u64 deserialice directo
                .append_pair("output_format_json_quote_64bit_integers", "0")
                // tolera NaN/Inf en columnas metric/value
                .append_pair("output_format_json_quote_denormals", "1")
                // acepta filas donde faltan columnas opcionales (se aplican defaults)
                .append_pair("input_format_defaults_for_omitted_fields", "1")
                .append_pair("date_time_input_format", "best_effort");
            for (name, value) in params {
                qp.append_pair(&format!("param_{name}"), value);
            }
        }
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

    /// SELECT que devuelve filas como Vec<T>. Añade `FORMAT JSONEachRow` automáticamente.
    pub async fn select<T: DeserializeOwned>(&self, sql: &str) -> Result<Vec<T>> {
        self.select_with_params(sql, &[]).await
    }

    /// Variante parametrizada de `select`. Ver `query_raw_with_params` para el formato.
    pub async fn select_with_params<T: DeserializeOwned>(
        &self,
        sql: &str,
        params: &[(&str, &str)],
    ) -> Result<Vec<T>> {
        let q = format!("{} FORMAT JSONEachRow", sql);
        let body = self.query_raw_with_params(&q, params).await?;
        let mut rows = Vec::new();
        for line in body.lines() {
            if line.trim().is_empty() {
                continue;
            }
            rows.push(
                serde_json::from_str(line)
                    .with_context(|| format!("decodificando fila: {line}"))?,
            );
        }
        Ok(rows)
    }

    /// SELECT que devuelve un único valor escalar.
    pub async fn select_one<T: DeserializeOwned>(&self, sql: &str) -> Result<Option<T>> {
        let rows: Vec<T> = self.select(sql).await?;
        Ok(rows.into_iter().next())
    }

    /// Variante parametrizada de `select_one`.
    pub async fn select_one_with_params<T: DeserializeOwned>(
        &self,
        sql: &str,
        params: &[(&str, &str)],
    ) -> Result<Option<T>> {
        let rows: Vec<T> = self.select_with_params(sql, params).await?;
        Ok(rows.into_iter().next())
    }

    /// Inserta filas en formato JSONEachRow. Lotes de hasta ~10 MB son aceptables; los mantenemos pequeños.
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
            // espera al flush del async-insert para que los errores de parseo emerjan como 4xx/5xx
            // en lugar de ser descartados silenciosamente del buffer.
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
            return Err(anyhow!(
                "clickhouse insert {} into {}: {}",
                status,
                table,
                text
            ));
        }
        Ok(())
    }
}
