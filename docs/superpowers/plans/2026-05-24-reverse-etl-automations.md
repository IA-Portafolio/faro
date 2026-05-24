# Reverse ETL Automations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build simple Reverse ETL automations that trigger webhooks when unified product users match event-based behavioral rules.

**Architecture:** Add `automation_rules` and `automation_deliveries` tables, Rust storage models, an `/api/v1/automations` API, and an `automation_runner` worker. The backend owns rule validation, SQL generation, webhook payload shape, secret redaction, and delivery idempotency; the frontend provides a dense CRUD/preview screen.

**Tech Stack:** Rust, Axum, serde, reqwest, ClickHouse, SvelteKit, TypeScript, Vitest, Cargo tests.

---

## File Structure

- Create `clickhouse/init/88-automations.sql`: canonical fresh-db schema for automation rules and deliveries.
- Create `clickhouse/migrations/017-automations.sql`: migration equivalent of the init schema.
- Modify `clickhouse/test-migrations.sh`: include the new migration/table expectations if the script has an expected list.
- Modify `backend/src/storage/models.rs`: add `AutomationRuleRow` and `AutomationDeliveryRow`.
- Create `backend/src/automations.rs`: pure domain helpers for definitions, webhook config, validation, redaction, SQL query construction, payload construction, and JSON parsing.
- Create `backend/src/api/automations.rs`: REST CRUD, preview, and deliveries endpoints.
- Modify `backend/src/api/mod.rs`: register `automations` module/router.
- Create `backend/src/workers/automation_runner.rs`: interval worker that evaluates active rules and posts webhooks.
- Modify `backend/src/workers/mod.rs`: export `start_automation_runner`.
- Modify `backend/src/config.rs`: add automation runner env vars.
- Modify `backend/src/main.rs`: start the worker.
- Modify `backend/src/lib.rs`: export `automations` if modules are explicitly listed there.
- Modify `.env.example`, `.env.prod.template`, `docs/reference/environment.md`: document new env vars if these files list worker config.
- Modify `frontend/src/lib/api.ts`: add automation types and endpoint helpers.
- Create `frontend/src/routes/automations/+page.svelte`: CRUD/preview/deliveries UI.
- Modify `frontend/src/lib/components/Sidebar.svelte`: add Automations navigation item.

---

### Task 1: ClickHouse Schema

**Files:**
- Create: `clickhouse/init/88-automations.sql`
- Create: `clickhouse/migrations/017-automations.sql`
- Modify: `clickhouse/test-migrations.sh`

- [ ] **Step 1: Write the failing migration check**

Add `017-automations.sql` to the migration expectation in `clickhouse/test-migrations.sh`. If the script contains an `EXPECTED=(...)` array, add these entries:

```bash
"017-automations.sql"
```

If the script checks expected tables, add:

```bash
"automation_rules"
"automation_deliveries"
```

- [ ] **Step 2: Run migration test to verify it fails**

Run:

```bash
cd clickhouse
./test-migrations.sh
```

Expected: FAIL because `clickhouse/migrations/017-automations.sql` does not exist yet or because automation tables are missing.

- [ ] **Step 3: Create fresh-db schema**

Create `clickhouse/init/88-automations.sql`:

```sql
-- Reverse ETL automations: event-based user segments that trigger webhooks.
--
-- Rules are declarative JSON because the grammar will grow over time. Deliveries
-- are append-only audit rows used for cooldown/idempotency and troubleshooting.

CREATE TABLE IF NOT EXISTS faro.automation_rules
(
    id          UUID,
    project_id  LowCardinality(String) DEFAULT 'default',
    name        String,
    description String                 DEFAULT '',
    enabled     UInt8                  DEFAULT 1,
    definition  String,
    webhook     String,
    created_at  DateTime64(3, 'UTC')   DEFAULT now64(3),
    updated_at  DateTime64(3, 'UTC')   DEFAULT now64(3),
    created_by  String                 DEFAULT '',
    deleted     UInt8                  DEFAULT 0,
    version     UInt64                 DEFAULT 1
)
ENGINE = ReplacingMergeTree(version)
ORDER BY id;

CREATE TABLE IF NOT EXISTS faro.automation_deliveries
(
    id             UUID,
    rule_id        UUID,
    project_id     LowCardinality(String) DEFAULT 'default',
    distinct_id    String                 CODEC(ZSTD(1)),
    status         LowCardinality(String),
    matched_at     DateTime64(3, 'UTC')   DEFAULT now64(3),
    delivered_at   DateTime64(3, 'UTC')   DEFAULT now64(3),
    trigger_count  UInt64                 DEFAULT 0 CODEC(T64, ZSTD(1)),
    response_status UInt16                DEFAULT 0,
    error          String                 DEFAULT '' CODEC(ZSTD(3)),
    INDEX idx_rule rule_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_distinct distinct_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(delivered_at)
ORDER BY (project_id, rule_id, distinct_id, delivered_at)
TTL toDateTime(delivered_at) + INTERVAL 365 DAY
SETTINGS index_granularity = 8192;
```

- [ ] **Step 4: Create migration schema**

Create `clickhouse/migrations/017-automations.sql` with the same SQL as `clickhouse/init/88-automations.sql`.

- [ ] **Step 5: Run migration test to verify it passes**

Run:

```bash
cd clickhouse
./test-migrations.sh
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add clickhouse/init/88-automations.sql clickhouse/migrations/017-automations.sql clickhouse/test-migrations.sh
git commit -m "feat(automations): add clickhouse schema"
```

---

### Task 2: Storage Models

**Files:**
- Modify: `backend/src/storage/models.rs`

- [ ] **Step 1: Write failing model serialization tests**

Add this test module near the bottom of `backend/src/storage/models.rs`:

```rust
#[cfg(test)]
mod automation_model_tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn automation_rule_row_round_trips_json() {
        let now = Utc::now();
        let row = AutomationRuleRow {
            id: Uuid::nil(),
            project_id: "default".into(),
            name: "Pricing intent".into(),
            description: "pricing without upgrade".into(),
            enabled: 1,
            definition: r#"{"trigger_event":"pricing_viewed"}"#.into(),
            webhook: r#"{"url":"https://example.test"}"#.into(),
            created_at: now,
            updated_at: now,
            created_by: String::new(),
            deleted: 0,
            version: 1,
        };

        let encoded = serde_json::to_string(&row).unwrap();
        let decoded: AutomationRuleRow = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.id, Uuid::nil());
        assert_eq!(decoded.project_id, "default");
        assert_eq!(decoded.enabled, 1);
        assert_eq!(decoded.deleted, 0);
    }

    #[test]
    fn automation_delivery_row_round_trips_json() {
        let now = Utc::now();
        let row = AutomationDeliveryRow {
            id: Uuid::nil(),
            rule_id: Uuid::nil(),
            project_id: "default".into(),
            distinct_id: "user_42".into(),
            status: "delivered".into(),
            matched_at: now,
            delivered_at: now,
            trigger_count: 4,
            response_status: 200,
            error: String::new(),
        };

        let encoded = serde_json::to_string(&row).unwrap();
        let decoded: AutomationDeliveryRow = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.distinct_id, "user_42");
        assert_eq!(decoded.status, "delivered");
        assert_eq!(decoded.trigger_count, 4);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd backend
cargo test automation_model_tests --lib
```

Expected: FAIL with missing `AutomationRuleRow` and `AutomationDeliveryRow`.

- [ ] **Step 3: Add storage models**

Add this section before the Cohorts section in `backend/src/storage/models.rs`:

```rust
// ---------- Automations (Reverse ETL simple) ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutomationRuleRow {
    pub id: Uuid,
    #[serde(default = "default_project")]
    pub project_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_true")]
    pub enabled: u8,
    /// JSON serializado de `automations::AutomationDefinition`.
    #[serde(default)]
    pub definition: String,
    /// JSON serializado de `automations::AutomationWebhookConfig`.
    #[serde(default)]
    pub webhook: String,
    #[serde(serialize_with = "rfc3339_millis", deserialize_with = "de_dt", default = "Utc::now")]
    pub created_at: DateTime<Utc>,
    #[serde(serialize_with = "rfc3339_millis", deserialize_with = "de_dt", default = "Utc::now")]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub deleted: u8,
    #[serde(default = "default_version")]
    pub version: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutomationDeliveryRow {
    pub id: Uuid,
    pub rule_id: Uuid,
    #[serde(default = "default_project")]
    pub project_id: String,
    pub distinct_id: String,
    pub status: String,
    #[serde(serialize_with = "rfc3339_millis", deserialize_with = "de_dt", default = "Utc::now")]
    pub matched_at: DateTime<Utc>,
    #[serde(serialize_with = "rfc3339_millis", deserialize_with = "de_dt", default = "Utc::now")]
    pub delivered_at: DateTime<Utc>,
    #[serde(default)]
    pub trigger_count: u64,
    #[serde(default)]
    pub response_status: u16,
    #[serde(default)]
    pub error: String,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cd backend
cargo test automation_model_tests --lib
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/src/storage/models.rs
git commit -m "feat(automations): add storage models"
```

---

### Task 3: Automation Domain Helpers

**Files:**
- Create: `backend/src/automations.rs`
- Modify: `backend/src/lib.rs`

- [ ] **Step 1: Write failing domain tests**

Create `backend/src/automations.rs` with tests first:

```rust
use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::ApiError;
use crate::storage::{AutomationRuleRow, ProductUserRow};

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_definition() -> AutomationDefinition {
        AutomationDefinition {
            trigger_event: "pricing_viewed".into(),
            trigger_op: ">=".into(),
            trigger_count: 3,
            window_days: 7,
            exclude_event: "upgrade_completed".into(),
            exclude_window_days: 365,
            cooldown_days: 30,
            filters: vec![AutomationFilter {
                key: "plan".into(),
                value: "free".into(),
            }],
        }
    }

    #[test]
    fn validate_definition_accepts_pricing_without_upgrade_rule() {
        validate_definition(&valid_definition()).unwrap();
    }

    #[test]
    fn validate_definition_rejects_bad_operator_and_empty_event() {
        let mut def = valid_definition();
        def.trigger_op = "; DROP TABLE faro.product_events; --".into();
        assert!(validate_definition(&def).is_err());

        let mut def = valid_definition();
        def.trigger_event = " ".into();
        assert!(validate_definition(&def).is_err());
    }

    #[test]
    fn build_match_query_uses_placeholders_for_user_values() {
        let q = build_match_query(&valid_definition(), "default", 50).unwrap();

        assert!(q.sql.contains("{trigger_event:String}"));
        assert!(q.sql.contains("{project:String}"));
        assert!(q.sql.contains("HAVING count() >= {trigger_count:UInt32}"));
        assert!(q.sql.contains("JSONExtractString(properties, {filter_key_0:String})"));
        assert!(!q.sql.contains("pricing_viewed"));
        assert!(!q.sql.contains("free"));
        assert!(q.owned.iter().any(|(k, v)| k == "filter_value_0" && v == "free"));
    }

    #[test]
    fn redact_webhook_masks_secret_headers() {
        let mut headers = BTreeMap::new();
        headers.insert("Authorization".into(), "Bearer secret".into());
        headers.insert("X-API-Key".into(), "abc".into());
        headers.insert("X-Trace".into(), "visible".into());
        let cfg = AutomationWebhookConfig {
            url: "https://example.test".into(),
            headers,
        };

        let redacted = redact_webhook(&cfg);

        assert_eq!(redacted.headers["Authorization"], "********");
        assert_eq!(redacted.headers["X-API-Key"], "********");
        assert_eq!(redacted.headers["X-Trace"], "visible");
    }

    #[test]
    fn build_payload_includes_segment_and_user_profile() {
        let now = Utc::now();
        let user = ProductUserRow {
            project_id: "default".into(),
            distinct_id: "user_42".into(),
            first_seen: now,
            last_seen: now,
            anonymous_ids: vec!["anon-a".into()],
            sources: vec!["web".into()],
            event_count: 10,
            properties: r#"{"email":"v@example.test"}"#.into(),
        };
        let rule = AutomationRuleRow {
            id: Uuid::nil(),
            project_id: "default".into(),
            name: "Pricing intent".into(),
            description: String::new(),
            enabled: 1,
            definition: serde_json::to_string(&valid_definition()).unwrap(),
            webhook: r#"{"url":"https://example.test","headers":{}}"#.into(),
            created_at: now,
            updated_at: now,
            created_by: String::new(),
            deleted: 0,
            version: 1,
        };

        let payload = build_payload(&rule, &valid_definition(), &user, 4, now);

        assert_eq!(payload["type"], "faro.automation.triggered");
        assert_eq!(payload["distinct_id"], "user_42");
        assert_eq!(payload["segment"]["trigger_count"], 4);
        assert_eq!(payload["user"]["properties"]["email"], "v@example.test");
        assert_eq!(payload["user"]["anonymous_ids"][0], "anon-a");
    }
}
```

- [ ] **Step 2: Export the module and run tests to verify failure**

Add to `backend/src/lib.rs` if it has module exports:

```rust
pub mod automations;
```

Run:

```bash
cd backend
cargo test automations::tests --lib
```

Expected: FAIL with missing `AutomationDefinition`, `validate_definition`, `build_match_query`, `AutomationWebhookConfig`, `redact_webhook`, and `build_payload`.

- [ ] **Step 3: Add domain implementation**

Fill `backend/src/automations.rs` above the tests:

```rust
const MAX_FILTERS: usize = 3;
const MAX_DAYS: u32 = 365;
const MAX_COUNT: u32 = 1_000_000;
const MAX_KEY_LEN: usize = 128;
const MAX_VALUE_LEN: usize = 256;
const MAX_EVENT_LEN: usize = 128;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutomationDefinition {
    pub trigger_event: String,
    pub trigger_op: String,
    pub trigger_count: u32,
    pub window_days: u32,
    #[serde(default)]
    pub exclude_event: String,
    #[serde(default = "default_exclude_window_days")]
    pub exclude_window_days: u32,
    #[serde(default = "default_cooldown_days")]
    pub cooldown_days: u32,
    #[serde(default)]
    pub filters: Vec<AutomationFilter>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutomationFilter {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AutomationWebhookConfig {
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AutomationMatch {
    pub distinct_id: String,
    pub trigger_count: u64,
}

pub struct AutomationQuery {
    pub sql: String,
    pub owned: Vec<(String, String)>,
}

impl AutomationQuery {
    pub fn params(&self) -> Vec<(&str, &str)> {
        self.owned
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }
}

fn default_exclude_window_days() -> u32 { 365 }
fn default_cooldown_days() -> u32 { 30 }

pub fn parse_definition(raw: &str) -> Result<AutomationDefinition, ApiError> {
    serde_json::from_str(raw)
        .map_err(|e| ApiError::BadRequest(format!("automation definition inválida: {e}")))
}

pub fn parse_webhook(raw: &str) -> Result<AutomationWebhookConfig, ApiError> {
    serde_json::from_str(raw)
        .map_err(|e| ApiError::BadRequest(format!("automation webhook inválido: {e}")))
}

pub fn validate_definition(def: &AutomationDefinition) -> Result<(), ApiError> {
    validate_event("trigger_event", &def.trigger_event)?;
    validated_op(&def.trigger_op)?;
    if def.trigger_count == 0 || def.trigger_count > MAX_COUNT {
        return Err(ApiError::BadRequest(format!("trigger_count fuera de rango [1, {MAX_COUNT}]")));
    }
    validate_days("window_days", def.window_days)?;
    validate_days("exclude_window_days", def.exclude_window_days)?;
    validate_days("cooldown_days", def.cooldown_days)?;
    if !def.exclude_event.trim().is_empty() {
        validate_event("exclude_event", &def.exclude_event)?;
    }
    if def.filters.len() > MAX_FILTERS {
        return Err(ApiError::BadRequest(format!("máximo {MAX_FILTERS} filtros")));
    }
    for f in &def.filters {
        if f.key.trim().is_empty() || f.value.trim().is_empty() {
            return Err(ApiError::BadRequest("filters requieren key y value".into()));
        }
        if f.key.len() > MAX_KEY_LEN || f.value.len() > MAX_VALUE_LEN {
            return Err(ApiError::BadRequest("filter demasiado largo".into()));
        }
    }
    Ok(())
}

pub fn validate_webhook(cfg: &AutomationWebhookConfig) -> Result<(), ApiError> {
    if cfg.url.trim().is_empty() {
        return Err(ApiError::BadRequest("webhook.url es requerido".into()));
    }
    let url = reqwest::Url::parse(&cfg.url)
        .map_err(|e| ApiError::BadRequest(format!("webhook.url inválida: {e}")))?;
    match url.scheme() {
        "http" | "https" => Ok(()),
        _ => Err(ApiError::BadRequest("webhook.url debe usar http o https".into())),
    }
}

fn validate_event(field: &str, value: &str) -> Result<(), ApiError> {
    let v = value.trim();
    if v.is_empty() {
        return Err(ApiError::BadRequest(format!("{field} es requerido")));
    }
    if v.len() > MAX_EVENT_LEN {
        return Err(ApiError::BadRequest(format!("{field} demasiado largo")));
    }
    Ok(())
}

fn validate_days(field: &str, value: u32) -> Result<(), ApiError> {
    if value == 0 || value > MAX_DAYS {
        return Err(ApiError::BadRequest(format!("{field} fuera de rango [1, {MAX_DAYS}]")));
    }
    Ok(())
}

pub fn validated_op(op: &str) -> Result<&'static str, ApiError> {
    match op {
        "==" | "=" => Ok("="),
        ">=" => Ok(">="),
        ">" => Ok(">"),
        "<=" => Ok("<="),
        "<" => Ok("<"),
        _ => Err(ApiError::BadRequest(format!("operador no soportado: {op}"))),
    }
}

pub fn build_match_query(
    def: &AutomationDefinition,
    project_id: &str,
    limit: u32,
) -> Result<AutomationQuery, ApiError> {
    validate_definition(def)?;
    let op = validated_op(&def.trigger_op)?;
    let mut owned = vec![
        ("trigger_event".into(), def.trigger_event.clone()),
        ("trigger_count".into(), def.trigger_count.to_string()),
        ("window_days".into(), def.window_days.to_string()),
        ("project".into(), project_id.to_string()),
        ("limit".into(), limit.max(1).to_string()),
        ("cooldown_days".into(), def.cooldown_days.to_string()),
    ];

    let mut filter_sql = String::new();
    for (i, f) in def.filters.iter().enumerate() {
        let key = format!("filter_key_{i}");
        let value = format!("filter_value_{i}");
        filter_sql.push_str(&format!(
            " AND JSONExtractString(properties, {{{key}:String}}) = {{{value}:String}}"
        ));
        owned.push((key, f.key.clone()));
        owned.push((value, f.value.clone()));
    }

    let mut exclude_sql = String::new();
    if !def.exclude_event.trim().is_empty() {
        owned.push(("exclude_event".into(), def.exclude_event.clone()));
        owned.push(("exclude_window_days".into(), def.exclude_window_days.to_string()));
        exclude_sql.push_str(
            " AND distinct_id NOT IN ( \
                 SELECT distinct_id FROM faro.product_events \
                 WHERE project_id = {project:String} \
                   AND event_name = {exclude_event:String} \
                   AND timestamp >= now() - toIntervalDay({exclude_window_days:UInt32}) \
                   AND distinct_id != '' \
             )",
        );
    }

    let sql = format!(
        "SELECT distinct_id, toUInt64(count()) AS trigger_count \
         FROM faro.product_events \
         WHERE project_id = {{project:String}} \
           AND event_name = {{trigger_event:String}} \
           AND timestamp >= now() - toIntervalDay({{window_days:UInt32}}) \
           AND distinct_id != ''{filter_sql}{exclude_sql} \
           AND distinct_id NOT IN ( \
                SELECT distinct_id FROM faro.automation_deliveries \
                WHERE project_id = {{project:String}} \
                  AND rule_id = {{rule_id:UUID}} \
                  AND status = 'delivered' \
                  AND delivered_at >= now() - toIntervalDay({{cooldown_days:UInt32}}) \
           ) \
         GROUP BY distinct_id \
         HAVING count() {op} {{trigger_count:UInt32}} \
         ORDER BY trigger_count DESC \
         LIMIT {{limit:UInt32}}"
    );

    Ok(AutomationQuery { sql, owned })
}

pub fn redact_webhook(cfg: &AutomationWebhookConfig) -> AutomationWebhookConfig {
    let mut headers = BTreeMap::new();
    for (k, v) in &cfg.headers {
        let lower = k.to_ascii_lowercase();
        let secret = lower == "authorization"
            || lower == "x-api-key"
            || lower == "api-key"
            || lower.contains("token")
            || lower.contains("secret");
        headers.insert(k.clone(), if secret { "********".into() } else { v.clone() });
    }
    AutomationWebhookConfig {
        url: cfg.url.clone(),
        headers,
    }
}

pub fn build_payload(
    rule: &AutomationRuleRow,
    def: &AutomationDefinition,
    user: &ProductUserRow,
    trigger_count: u64,
    matched_at: DateTime<Utc>,
) -> Value {
    let props = serde_json::from_str::<Value>(&user.properties)
        .ok()
        .filter(|v| v.is_object())
        .unwrap_or_else(|| json!({}));
    json!({
        "type": "faro.automation.triggered",
        "rule_id": rule.id,
        "rule_name": rule.name,
        "project_id": rule.project_id,
        "distinct_id": user.distinct_id,
        "matched_at": matched_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "segment": {
            "trigger_event": def.trigger_event,
            "trigger_count": trigger_count,
            "window_days": def.window_days,
            "exclude_event": def.exclude_event,
        },
        "user": {
            "properties": props,
            "anonymous_ids": user.anonymous_ids,
            "sources": user.sources,
        }
    })
}
```

Important: `build_match_query` contains `{rule_id:UUID}` but does not own that parameter. Callers must push `("rule_id", rule.id.to_string())` before executing.

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cd backend
cargo test automations::tests --lib
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/src/automations.rs backend/src/lib.rs
git commit -m "feat(automations): add domain helpers"
```

---

### Task 4: Automations API

**Files:**
- Create: `backend/src/api/automations.rs`
- Modify: `backend/src/api/mod.rs`

- [ ] **Step 1: Write failing API helper tests**

Create `backend/src/api/automations.rs` with helper tests first:

```rust
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::Query;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::automations::{
    build_match_query, parse_definition, parse_webhook, redact_webhook, validate_definition,
    validate_webhook, AutomationDefinition, AutomationWebhookConfig,
};
use crate::error::{ApiError, ApiResult};
use crate::state::SharedState;
use crate::storage::{AutomationDeliveryRow, AutomationRuleRow};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automation_view_redacts_webhook_headers() {
        let now = Utc::now();
        let row = AutomationRuleRow {
            id: Uuid::nil(),
            project_id: "default".into(),
            name: "Pricing".into(),
            description: String::new(),
            enabled: 1,
            definition: r#"{"trigger_event":"pricing_viewed","trigger_op":">=","trigger_count":3,"window_days":7,"exclude_event":"upgrade_completed","exclude_window_days":365,"cooldown_days":30,"filters":[]}"#.into(),
            webhook: r#"{"url":"https://example.test","headers":{"Authorization":"Bearer secret","X-Trace":"visible"}}"#.into(),
            created_at: now,
            updated_at: now,
            created_by: String::new(),
            deleted: 0,
            version: 1,
        };

        let view = AutomationView::from_row(row).unwrap();

        assert_eq!(view.webhook.headers["Authorization"], "********");
        assert_eq!(view.webhook.headers["X-Trace"], "visible");
    }

    #[test]
    fn preserve_redacted_secret_keeps_existing_value() {
        let mut existing = AutomationWebhookConfig::default();
        existing.headers.insert("Authorization".into(), "Bearer secret".into());
        let mut incoming = AutomationWebhookConfig::default();
        incoming.headers.insert("Authorization".into(), "********".into());

        let merged = merge_preserving_redacted_headers(incoming, &existing);

        assert_eq!(merged.headers["Authorization"], "Bearer secret");
    }
}
```

- [ ] **Step 2: Register module and run tests to verify failure**

Modify `backend/src/api/mod.rs`:

```rust
pub mod automations;
```

In `v1_router()`, add:

```rust
.merge(automations::router())
```

Run:

```bash
cd backend
cargo test api::automations::tests --lib
```

Expected: FAIL because `AutomationView` and `merge_preserving_redacted_headers` are missing.

- [ ] **Step 3: Add API implementation**

Fill `backend/src/api/automations.rs` above the tests:

```rust
pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/automations", get(list_automations).post(create_automation))
        .route("/automations/preview", post(preview_automation))
        .route(
            "/automations/:id",
            get(get_automation).put(update_automation).delete(delete_automation),
        )
        .route("/automations/:id/deliveries", get(list_deliveries))
}

const AUTOMATION_COLS: &str =
    "id, project_id, name, description, enabled, definition, webhook, created_at, updated_at, created_by, deleted, version";
const DELIVERY_COLS: &str =
    "id, rule_id, project_id, distinct_id, status, matched_at, delivered_at, trigger_count, response_status, error";

#[derive(Debug, Serialize)]
pub struct AutomationView {
    pub id: Uuid,
    pub project_id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub definition: AutomationDefinition,
    pub webhook: AutomationWebhookConfig,
    pub created_at: String,
    pub updated_at: String,
}

impl AutomationView {
    fn from_row(row: AutomationRuleRow) -> ApiResult<Self> {
        let definition = parse_definition(&row.definition)?;
        let webhook = redact_webhook(&parse_webhook(&row.webhook)?);
        Ok(Self {
            id: row.id,
            project_id: row.project_id,
            name: row.name,
            description: row.description,
            enabled: row.enabled == 1,
            definition,
            webhook,
            created_at: row.created_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            updated_at: row.updated_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AutomationInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_project_in")]
    pub project: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub definition: AutomationDefinition,
    pub webhook: AutomationWebhookConfig,
}

fn default_project_in() -> String { "default".into() }
fn default_enabled() -> bool { true }

fn validate_input(input: &AutomationInput) -> ApiResult<()> {
    if input.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name no puede ser vacío".into()));
    }
    if input.name.len() > 200 {
        return Err(ApiError::BadRequest("name demasiado largo".into()));
    }
    validate_definition(&input.definition)?;
    validate_webhook(&input.webhook)?;
    Ok(())
}

fn merge_preserving_redacted_headers(
    mut incoming: AutomationWebhookConfig,
    existing: &AutomationWebhookConfig,
) -> AutomationWebhookConfig {
    for (k, v) in incoming.headers.iter_mut() {
        if v == "********" {
            if let Some(old) = existing.headers.get(k) {
                *v = old.clone();
            }
        }
    }
    incoming
}

async fn list_automations(
    State(state): State<SharedState>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Vec<AutomationView>>> {
    let mut params: Vec<(&str, &str)> = Vec::new();
    let project_clause = match q.project.as_deref().filter(|p| !p.is_empty()) {
        Some(p) => {
            params.push(("project", p));
            " AND project_id = {project:String}"
        }
        None => "",
    };
    let sql = format!(
        "SELECT {AUTOMATION_COLS} FROM faro.automation_rules FINAL WHERE deleted = 0{project_clause} ORDER BY updated_at DESC"
    );
    let rows: Vec<AutomationRuleRow> = state.ch.select_with_params(&sql, &params).await?;
    rows.into_iter()
        .map(AutomationView::from_row)
        .collect::<ApiResult<Vec<_>>>()
        .map(Json)
}

async fn create_automation(
    State(state): State<SharedState>,
    Json(input): Json<AutomationInput>,
) -> ApiResult<Json<AutomationView>> {
    validate_input(&input)?;
    let now = Utc::now();
    let row = AutomationRuleRow {
        id: Uuid::new_v4(),
        project_id: if input.project.is_empty() { "default".into() } else { input.project },
        name: input.name.trim().to_string(),
        description: input.description,
        enabled: if input.enabled { 1 } else { 0 },
        definition: serde_json::to_string(&input.definition)
            .map_err(|e| ApiError::BadRequest(format!("definition inválida: {e}")))?,
        webhook: serde_json::to_string(&input.webhook)
            .map_err(|e| ApiError::BadRequest(format!("webhook inválido: {e}")))?,
        created_at: now,
        updated_at: now,
        created_by: String::new(),
        deleted: 0,
        version: now.timestamp_millis() as u64,
    };
    state.ch.insert("faro.automation_rules", &[row.clone()]).await?;
    Ok(Json(AutomationView::from_row(row)?))
}

async fn get_automation(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AutomationView>> {
    let id_s = id.to_string();
    let sql = format!("SELECT {AUTOMATION_COLS} FROM faro.automation_rules FINAL WHERE id = {{id:UUID}} AND deleted = 0 LIMIT 1");
    let row = state
        .ch
        .select_one_with_params::<AutomationRuleRow>(&sql, &[("id", &id_s)])
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(AutomationView::from_row(row)?))
}

async fn update_automation(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(input): Json<AutomationInput>,
) -> ApiResult<Json<AutomationView>> {
    validate_input(&input)?;
    let id_s = id.to_string();
    let sql = format!("SELECT {AUTOMATION_COLS} FROM faro.automation_rules FINAL WHERE id = {{id:UUID}} AND deleted = 0 LIMIT 1");
    let mut row = state
        .ch
        .select_one_with_params::<AutomationRuleRow>(&sql, &[("id", &id_s)])
        .await?
        .ok_or(ApiError::NotFound)?;
    let existing_webhook = parse_webhook(&row.webhook)?;
    let merged_webhook = merge_preserving_redacted_headers(input.webhook, &existing_webhook);
    validate_webhook(&merged_webhook)?;
    let now = Utc::now();
    row.name = input.name.trim().to_string();
    row.description = input.description;
    row.enabled = if input.enabled { 1 } else { 0 };
    row.definition = serde_json::to_string(&input.definition)
        .map_err(|e| ApiError::BadRequest(format!("definition inválida: {e}")))?;
    row.webhook = serde_json::to_string(&merged_webhook)
        .map_err(|e| ApiError::BadRequest(format!("webhook inválido: {e}")))?;
    row.updated_at = now;
    row.version = now.timestamp_millis() as u64;
    state.ch.insert("faro.automation_rules", &[row.clone()]).await?;
    Ok(Json(AutomationView::from_row(row)?))
}

async fn delete_automation(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let id_s = id.to_string();
    let sql = format!("SELECT {AUTOMATION_COLS} FROM faro.automation_rules FINAL WHERE id = {{id:UUID}} AND deleted = 0 LIMIT 1");
    let mut row = state
        .ch
        .select_one_with_params::<AutomationRuleRow>(&sql, &[("id", &id_s)])
        .await?
        .ok_or(ApiError::NotFound)?;
    let now = Utc::now();
    row.deleted = 1;
    row.updated_at = now;
    row.version = now.timestamp_millis() as u64;
    state.ch.insert("faro.automation_rules", &[row]).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct PreviewInput {
    #[serde(default = "default_project_in")]
    pub project: String,
    pub definition: AutomationDefinition,
    #[serde(default)]
    pub sample_limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct PreviewResult {
    pub size: u64,
    pub sample: Vec<String>,
    pub took_ms: u64,
}

async fn preview_automation(
    State(state): State<SharedState>,
    Json(input): Json<PreviewInput>,
) -> ApiResult<Json<PreviewResult>> {
    let started = std::time::Instant::now();
    validate_definition(&input.definition)?;
    let project = if input.project.is_empty() { "default" } else { input.project.as_str() };
    let limit = input.sample_limit.unwrap_or(20).clamp(1, 500);
    let q = build_match_query(&input.definition, project, limit)?;
    let fake_rule = Uuid::nil().to_string();
    let mut params = q.params();
    params.push(("rule_id", fake_rule.as_str()));
    let rows: Vec<crate::automations::AutomationMatch> =
        state.ch.select_with_params(&q.sql, &params).await?;
    Ok(Json(PreviewResult {
        size: rows.len() as u64,
        sample: rows.into_iter().map(|r| r.distinct_id).collect(),
        took_ms: started.elapsed().as_millis() as u64,
    }))
}

#[derive(Debug, Deserialize)]
pub struct DeliveriesQuery {
    #[serde(default)]
    pub limit: Option<u32>,
}

async fn list_deliveries(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Query(q): Query<DeliveriesQuery>,
) -> ApiResult<Json<Vec<AutomationDeliveryRow>>> {
    let id_s = id.to_string();
    let limit_s = q.limit.unwrap_or(100).clamp(1, 500).to_string();
    let sql = format!(
        "SELECT {DELIVERY_COLS} FROM faro.automation_deliveries WHERE rule_id = {{rule_id:UUID}} ORDER BY delivered_at DESC LIMIT {{limit:UInt32}}"
    );
    let rows = state
        .ch
        .select_with_params(&sql, &[("rule_id", &id_s), ("limit", &limit_s)])
        .await?;
    Ok(Json(rows))
}
```

- [ ] **Step 4: Run API helper tests**

Run:

```bash
cd backend
cargo test api::automations::tests --lib
```

Expected: PASS.

- [ ] **Step 5: Run route compile check**

Run:

```bash
cd backend
cargo check
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add backend/src/api/automations.rs backend/src/api/mod.rs
git commit -m "feat(automations): add api"
```

---

### Task 5: Automation Runner Worker

**Files:**
- Create: `backend/src/workers/automation_runner.rs`
- Modify: `backend/src/workers/mod.rs`
- Modify: `backend/src/config.rs`
- Modify: `backend/src/main.rs`

- [ ] **Step 1: Write failing worker helper tests**

Create `backend/src/workers/automation_runner.rs` with helper tests first:

```rust
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use tokio::time::{interval, MissedTickBehavior};
use uuid::Uuid;

use crate::automations::{
    build_match_query, build_payload, parse_definition, parse_webhook, validate_definition,
    validate_webhook, AutomationMatch, AutomationWebhookConfig,
};
use crate::state::SharedState;
use crate::storage::{AutomationDeliveryRow, AutomationRuleRow, ProductUserRow};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_error_keeps_short_message() {
        assert_eq!(truncate_error("small"), "small");
    }

    #[test]
    fn truncate_error_caps_long_message() {
        let long = "x".repeat(600);
        let out = truncate_error(&long);
        assert_eq!(out.len(), 512);
    }

    #[test]
    fn build_headers_rejects_invalid_header_name() {
        let mut cfg = AutomationWebhookConfig::default();
        cfg.headers.insert("bad header".into(), "x".into());
        assert!(build_headers(&cfg).is_err());
    }
}
```

- [ ] **Step 2: Wire exports/config and run tests to verify failure**

Modify `backend/src/workers/mod.rs`:

```rust
pub mod automation_runner;
pub use automation_runner::start_automation_runner;
```

Modify `backend/src/config.rs` struct:

```rust
pub automation_runner_enabled: bool,
pub automation_runner_interval_secs: u64,
pub automation_runner_max_matches_per_rule: u32,
```

Add to `Config::from_env()`:

```rust
automation_runner_enabled: matches!(
    env_or("FARO_AUTOMATION_RUNNER_ENABLED", "true").to_lowercase().as_str(),
    "1" | "true" | "yes" | "on"
),
automation_runner_interval_secs: env_or("FARO_AUTOMATION_RUNNER_INTERVAL_SECS", "60")
    .parse()
    .unwrap_or(60),
automation_runner_max_matches_per_rule: env_or("FARO_AUTOMATION_RUNNER_MAX_MATCHES_PER_RULE", "100")
    .parse()
    .unwrap_or(100),
```

Modify `backend/src/main.rs` after `workers::start_session_aggregator(state.clone());`:

```rust
workers::start_automation_runner(state.clone());
```

Run:

```bash
cd backend
cargo test workers::automation_runner::tests --lib
```

Expected: FAIL because helper functions are missing.

- [ ] **Step 3: Add worker implementation**

Fill `backend/src/workers/automation_runner.rs` above tests:

```rust
pub fn start_automation_runner(state: SharedState) {
    if !state.cfg.automation_runner_enabled {
        tracing::info!("automation_runner deshabilitado");
        return;
    }
    let every = state.cfg.automation_runner_interval_secs.max(10);
    tracing::info!(every_secs = every, "arrancando automation_runner");
    tokio::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "automation_runner: no pude construir reqwest client");
                return;
            }
        };
        let mut tick = interval(Duration::from_secs(every));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        tick.tick().await;
        loop {
            tick.tick().await;
            if let Err(e) = run_once(&state, &client).await {
                tracing::warn!(error = %e, "automation_runner: tick falló");
            }
        }
    });
}

async fn run_once(state: &SharedState, client: &reqwest::Client) -> Result<()> {
    let rules = load_active_rules(state).await?;
    for rule in rules {
        if let Err(e) = evaluate_rule(state, client, &rule).await {
            tracing::warn!(rule_id = %rule.id, error = %e, "automation_runner: regla falló");
        }
    }
    Ok(())
}

async fn load_active_rules(state: &SharedState) -> Result<Vec<AutomationRuleRow>> {
    let sql = "SELECT id, project_id, name, description, enabled, definition, webhook, created_at, updated_at, created_by, deleted, version \
               FROM faro.automation_rules FINAL \
               WHERE deleted = 0 AND enabled = 1 \
               ORDER BY updated_at DESC";
    Ok(state.ch.select(sql).await?)
}

async fn evaluate_rule(
    state: &SharedState,
    client: &reqwest::Client,
    rule: &AutomationRuleRow,
) -> Result<()> {
    let def = parse_definition(&rule.definition).map_err(|e| anyhow!("{e}"))?;
    validate_definition(&def).map_err(|e| anyhow!("{e}"))?;
    let webhook = parse_webhook(&rule.webhook).map_err(|e| anyhow!("{e}"))?;
    validate_webhook(&webhook).map_err(|e| anyhow!("{e}"))?;
    let q = build_match_query(
        &def,
        &rule.project_id,
        state.cfg.automation_runner_max_matches_per_rule,
    )
    .map_err(|e| anyhow!("{e}"))?;
    let rule_id_s = rule.id.to_string();
    let mut params = q.params();
    params.push(("rule_id", rule_id_s.as_str()));
    let matches: Vec<AutomationMatch> = state.ch.select_with_params(&q.sql, &params).await?;
    for m in matches {
        match load_user(state, &rule.project_id, &m.distinct_id).await? {
            Some(user) => {
                let delivery = deliver_match(state, client, rule, &def, &webhook, &user, m.trigger_count).await;
                if let Err(e) = delivery {
                    tracing::warn!(rule_id = %rule.id, distinct_id = %m.distinct_id, error = %e, "automation_runner: delivery falló");
                }
            }
            None => {
                tracing::debug!(rule_id = %rule.id, distinct_id = %m.distinct_id, "automation_runner: user profile no encontrado");
            }
        }
    }
    Ok(())
}

async fn load_user(
    state: &SharedState,
    project_id: &str,
    distinct_id: &str,
) -> Result<Option<ProductUserRow>> {
    state
        .ch
        .select_one_with_params(
            "SELECT project_id, distinct_id, first_seen, last_seen, anonymous_ids, sources, event_count, properties \
             FROM faro.product_users FINAL \
             WHERE project_id = {project:String} AND distinct_id = {distinct_id:String} \
             LIMIT 1",
            &[("project", project_id), ("distinct_id", distinct_id)],
        )
        .await
}

async fn deliver_match(
    state: &SharedState,
    client: &reqwest::Client,
    rule: &AutomationRuleRow,
    def: &crate::automations::AutomationDefinition,
    webhook: &AutomationWebhookConfig,
    user: &ProductUserRow,
    trigger_count: u64,
) -> Result<()> {
    let matched_at = Utc::now();
    let payload = build_payload(rule, def, user, trigger_count, matched_at);
    let result = post_webhook(client, webhook, &payload).await;
    let (status, response_status, error) = match result {
        Ok(code) => ("delivered".to_string(), code, String::new()),
        Err(e) => ("failed".to_string(), 0, truncate_error(&e.to_string())),
    };
    let row = AutomationDeliveryRow {
        id: Uuid::new_v4(),
        rule_id: rule.id,
        project_id: rule.project_id.clone(),
        distinct_id: user.distinct_id.clone(),
        status,
        matched_at,
        delivered_at: Utc::now(),
        trigger_count,
        response_status,
        error,
    };
    state.ch.insert("faro.automation_deliveries", &[row]).await?;
    Ok(())
}

async fn post_webhook(
    client: &reqwest::Client,
    cfg: &AutomationWebhookConfig,
    payload: &serde_json::Value,
) -> Result<u16> {
    let headers = build_headers(cfg)?;
    let resp = client
        .post(&cfg.url)
        .headers(headers)
        .json(payload)
        .send()
        .await
        .context("webhook send")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("webhook respondió {status}: {}", truncate_error(&body)));
    }
    Ok(status.as_u16())
}

fn build_headers(cfg: &AutomationWebhookConfig) -> Result<HeaderMap> {
    let mut out = HeaderMap::new();
    for (k, v) in &cfg.headers {
        let name = HeaderName::from_bytes(k.as_bytes())
            .with_context(|| format!("header inválido: {k}"))?;
        let value = HeaderValue::from_str(v)
            .with_context(|| format!("valor inválido para header: {k}"))?;
        out.insert(name, value);
    }
    Ok(out)
}

fn truncate_error(s: &str) -> String {
    s.chars().take(512).collect()
}
```

- [ ] **Step 4: Run worker tests**

Run:

```bash
cd backend
cargo test workers::automation_runner::tests --lib
```

Expected: PASS.

- [ ] **Step 5: Run backend check**

Run:

```bash
cd backend
cargo check
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add backend/src/workers/automation_runner.rs backend/src/workers/mod.rs backend/src/config.rs backend/src/main.rs
git commit -m "feat(automations): add runner worker"
```

---

### Task 6: Environment Documentation

**Files:**
- Modify: `.env.example`
- Modify: `.env.prod.template`
- Modify: `docs/reference/environment.md`

- [ ] **Step 1: Write failing env reference check**

Run:

```bash
./scripts/check-env-reference.sh
```

Expected: FAIL because the new `FARO_AUTOMATION_RUNNER_*` variables are in Rust config but not documented.

- [ ] **Step 2: Document env vars**

Add to `.env.example` and `.env.prod.template`:

```dotenv
# Reverse ETL automations worker.
FARO_AUTOMATION_RUNNER_ENABLED=true
FARO_AUTOMATION_RUNNER_INTERVAL_SECS=60
FARO_AUTOMATION_RUNNER_MAX_MATCHES_PER_RULE=100
```

Add to `docs/reference/environment.md` in the worker/config table:

```markdown
| `FARO_AUTOMATION_RUNNER_ENABLED` | `true` | Enables the Reverse ETL automations worker. |
| `FARO_AUTOMATION_RUNNER_INTERVAL_SECS` | `60` | Seconds between automation evaluation ticks. |
| `FARO_AUTOMATION_RUNNER_MAX_MATCHES_PER_RULE` | `100` | Maximum users processed per rule per tick. |
```

- [ ] **Step 3: Run env reference check**

Run:

```bash
./scripts/check-env-reference.sh
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add .env.example .env.prod.template docs/reference/environment.md
git commit -m "docs(automations): document runner env"
```

---

### Task 7: Frontend API Helpers

**Files:**
- Modify: `frontend/src/lib/api.ts`

- [ ] **Step 1: Write failing frontend API tests**

If no API helper tests exist, create `frontend/src/lib/api.test.ts`. Add:

```ts
import { describe, expect, it } from 'vitest';
import { parseAutomationDefinition, type AutomationDefinition } from './api';

describe('automation api helpers', () => {
  it('parses valid automation definitions', () => {
    const raw = JSON.stringify({
      trigger_event: 'pricing_viewed',
      trigger_op: '>=',
      trigger_count: 3,
      window_days: 7,
      exclude_event: 'upgrade_completed',
      exclude_window_days: 365,
      cooldown_days: 30,
      filters: [{ key: 'plan', value: 'free' }]
    } satisfies AutomationDefinition);

    expect(parseAutomationDefinition(raw)?.trigger_event).toBe('pricing_viewed');
    expect(parseAutomationDefinition(raw)?.filters?.[0]?.key).toBe('plan');
  });

  it('returns null for invalid automation definitions', () => {
    expect(parseAutomationDefinition('{')).toBeNull();
    expect(parseAutomationDefinition(JSON.stringify({ trigger_event: '' }))).toBeNull();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd frontend
npm test -- src/lib/api.test.ts
```

Expected: FAIL because automation types/helpers are missing.

- [ ] **Step 3: Add automation API helpers**

Add to `frontend/src/lib/api.ts` after Cohorts or before Auth:

```ts
// ---------- Automations (Reverse ETL) ----------
export type AutomationFilter = { key: string; value: string };
export type AutomationDefinition = {
  trigger_event: string;
  trigger_op: '==' | '>=' | '>' | '<=' | '<';
  trigger_count: number;
  window_days: number;
  exclude_event?: string;
  exclude_window_days?: number;
  cooldown_days: number;
  filters?: AutomationFilter[];
};

export type AutomationWebhook = {
  url: string;
  headers: Record<string, string>;
};

export type Automation = {
  id: string;
  project_id: string;
  name: string;
  description: string;
  enabled: boolean;
  definition: AutomationDefinition;
  webhook: AutomationWebhook;
  created_at: string;
  updated_at: string;
};

export type AutomationInput = {
  name: string;
  description?: string;
  project?: string;
  enabled: boolean;
  definition: AutomationDefinition;
  webhook: AutomationWebhook;
};

export type AutomationPreview = {
  size: number;
  sample: string[];
  took_ms: number;
};

export type AutomationDelivery = {
  id: string;
  rule_id: string;
  project_id: string;
  distinct_id: string;
  status: 'delivered' | 'failed' | string;
  matched_at: string;
  delivered_at: string;
  trigger_count: number;
  response_status: number;
  error: string;
};

export function parseAutomationDefinition(raw: string): AutomationDefinition | null {
  try {
    const v = JSON.parse(raw) as AutomationDefinition;
    if (!v.trigger_event || typeof v.trigger_event !== 'string') return null;
    if (!v.trigger_op) return null;
    if (typeof v.trigger_count !== 'number') return null;
    if (typeof v.window_days !== 'number') return null;
    if (typeof v.cooldown_days !== 'number') return null;
    return v;
  } catch {
    return null;
  }
}

export const listAutomations = (r: { project?: string } = {}) =>
  api<Automation[]>(`/api/v1/automations${qs(r)}`);
export const getAutomation = (id: string) =>
  api<Automation>(`/api/v1/automations/${encodeURIComponent(id)}`);
export const createAutomation = (body: AutomationInput) =>
  api<Automation>(`/api/v1/automations`, { method: 'POST', body: JSON.stringify(body) });
export const updateAutomation = (id: string, body: AutomationInput) =>
  api<Automation>(`/api/v1/automations/${encodeURIComponent(id)}`, {
    method: 'PUT',
    body: JSON.stringify(body)
  });
export const deleteAutomation = (id: string) =>
  api<{ ok: boolean }>(`/api/v1/automations/${encodeURIComponent(id)}`, { method: 'DELETE' });
export const previewAutomation = (body: {
  project?: string;
  definition: AutomationDefinition;
  sample_limit?: number;
}) =>
  api<AutomationPreview>(`/api/v1/automations/preview`, {
    method: 'POST',
    body: JSON.stringify(body)
  });
export const fetchAutomationDeliveries = (id: string, params: { limit?: number } = {}) =>
  api<AutomationDelivery[]>(
    `/api/v1/automations/${encodeURIComponent(id)}/deliveries${qs(params)}`
  );
```

- [ ] **Step 4: Run frontend API tests**

Run:

```bash
cd frontend
npm test -- src/lib/api.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/api.ts frontend/src/lib/api.test.ts
git commit -m "feat(automations): add frontend api helpers"
```

---

### Task 8: Automations UI

**Files:**
- Create: `frontend/src/routes/automations/+page.svelte`
- Modify: `frontend/src/lib/components/Sidebar.svelte`

- [ ] **Step 1: Write failing route smoke test**

If route/component smoke tests are not established, use Svelte check as the failing test. First add the route import references in `Sidebar.svelte` only:

```ts
{ href: '/automations', label: 'Automations', icon: '↗' },
```

Run:

```bash
cd frontend
npm run check
```

Expected: FAIL or no route page exists for navigation/build completeness. If SvelteKit does not fail for missing route, continue with the next step and rely on `npm run check` after creating the page.

- [ ] **Step 2: Create UI page**

Create `frontend/src/routes/automations/+page.svelte`:

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import {
    createAutomation,
    deleteAutomation,
    fetchAutomationDeliveries,
    listAutomations,
    previewAutomation,
    updateAutomation,
    type Automation,
    type AutomationDefinition,
    type AutomationDelivery,
    type AutomationFilter,
    type AutomationPreview
  } from '$lib/api';
  import { selectedProject } from '$lib/stores';
  import { toast } from '$lib/toasts';
  import Skeleton from '$lib/components/Skeleton.svelte';

  let automations: Automation[] = [];
  let loading = true;
  let listError = '';
  let editing: Automation | null = null;

  let formName = '';
  let formDescription = '';
  let formEnabled = true;
  let triggerEvent = 'pricing_viewed';
  let triggerOp: AutomationDefinition['trigger_op'] = '>=';
  let triggerCount = 3;
  let windowDays = 7;
  let excludeEvent = 'upgrade_completed';
  let excludeWindowDays = 365;
  let cooldownDays = 30;
  let filters: AutomationFilter[] = [];
  let webhookUrl = '';
  let webhookHeaders = '';
  let formError = '';
  let saving = false;

  let preview: AutomationPreview | null = null;
  let previewBusy = false;
  let previewError = '';
  let previewSeq = 0;
  let previewTimer: ReturnType<typeof setTimeout> | null = null;

  let deliveries: AutomationDelivery[] = [];
  let deliveriesBusy = false;
  let deliveriesError = '';

  async function loadList(): Promise<void> {
    loading = true;
    listError = '';
    try {
      automations = await listAutomations({ project: $selectedProject || undefined });
    } catch (e: unknown) {
      listError = e instanceof Error ? e.message : String(e);
      automations = [];
    } finally {
      loading = false;
    }
  }

  function newAutomation(): void {
    editing = null;
    formName = '';
    formDescription = '';
    formEnabled = true;
    triggerEvent = 'pricing_viewed';
    triggerOp = '>=';
    triggerCount = 3;
    windowDays = 7;
    excludeEvent = 'upgrade_completed';
    excludeWindowDays = 365;
    cooldownDays = 30;
    filters = [];
    webhookUrl = '';
    webhookHeaders = '';
    formError = '';
    deliveries = [];
    void schedulePreview();
  }

  function loadEditing(a: Automation): void {
    editing = a;
    formName = a.name;
    formDescription = a.description;
    formEnabled = a.enabled;
    triggerEvent = a.definition.trigger_event;
    triggerOp = a.definition.trigger_op;
    triggerCount = a.definition.trigger_count;
    windowDays = a.definition.window_days;
    excludeEvent = a.definition.exclude_event ?? '';
    excludeWindowDays = a.definition.exclude_window_days ?? 365;
    cooldownDays = a.definition.cooldown_days;
    filters = (a.definition.filters ?? []).map((f) => ({ ...f }));
    webhookUrl = a.webhook.url;
    webhookHeaders = JSON.stringify(a.webhook.headers ?? {}, null, 2);
    formError = '';
    void schedulePreview();
    void loadDeliveries(a.id);
  }

  function buildDefinition(): AutomationDefinition {
    return {
      trigger_event: triggerEvent.trim(),
      trigger_op: triggerOp,
      trigger_count: Math.max(1, Math.floor(triggerCount || 0)),
      window_days: Math.max(1, Math.floor(windowDays || 0)),
      exclude_event: excludeEvent.trim(),
      exclude_window_days: Math.max(1, Math.floor(excludeWindowDays || 0)),
      cooldown_days: Math.max(1, Math.floor(cooldownDays || 0)),
      filters: filters.filter((f) => f.key.trim() && f.value.trim())
    };
  }

  function parseHeaders(): Record<string, string> {
    if (!webhookHeaders.trim()) return {};
    const parsed = JSON.parse(webhookHeaders) as Record<string, string>;
    return parsed ?? {};
  }

  async function schedulePreview(): Promise<void> {
    if (previewTimer) clearTimeout(previewTimer);
    if (!triggerEvent.trim() || !triggerCount || !windowDays) {
      preview = null;
      previewError = '';
      return;
    }
    previewTimer = setTimeout(runPreview, 250);
  }

  async function runPreview(): Promise<void> {
    const seq = ++previewSeq;
    previewBusy = true;
    previewError = '';
    try {
      const r = await previewAutomation({
        project: $selectedProject || undefined,
        definition: buildDefinition(),
        sample_limit: 20
      });
      if (seq !== previewSeq) return;
      preview = r;
    } catch (e: unknown) {
      if (seq !== previewSeq) return;
      previewError = e instanceof Error ? e.message : String(e);
      preview = null;
    } finally {
      if (seq === previewSeq) previewBusy = false;
    }
  }

  async function saveAutomation(): Promise<void> {
    formError = '';
    if (!formName.trim()) {
      formError = 'name es obligatorio';
      return;
    }
    if (!triggerEvent.trim()) {
      formError = 'trigger_event es obligatorio';
      return;
    }
    if (!webhookUrl.trim()) {
      formError = 'webhook URL es obligatoria';
      return;
    }
    let headers: Record<string, string>;
    try {
      headers = parseHeaders();
    } catch (e: unknown) {
      formError = e instanceof Error ? `headers JSON inválido: ${e.message}` : 'headers JSON inválido';
      return;
    }
    saving = true;
    try {
      const payload = {
        name: formName.trim(),
        description: formDescription,
        project: $selectedProject || 'default',
        enabled: formEnabled,
        definition: buildDefinition(),
        webhook: { url: webhookUrl.trim(), headers }
      };
      const saved = editing
        ? await updateAutomation(editing.id, payload)
        : await createAutomation(payload);
      toast.success(editing ? 'Automation actualizada' : 'Automation creada');
      await loadList();
      loadEditing(automations.find((a) => a.id === saved.id) ?? saved);
    } catch (e: unknown) {
      formError = e instanceof Error ? e.message : String(e);
    } finally {
      saving = false;
    }
  }

  async function removeAutomation(): Promise<void> {
    if (!editing) return;
    if (!window.confirm(`¿Eliminar la automation "${editing.name}"?`)) return;
    await deleteAutomation(editing.id);
    toast.success('Automation eliminada');
    await loadList();
    newAutomation();
  }

  async function loadDeliveries(id: string): Promise<void> {
    deliveriesBusy = true;
    deliveriesError = '';
    try {
      deliveries = await fetchAutomationDeliveries(id, { limit: 50 });
    } catch (e: unknown) {
      deliveriesError = e instanceof Error ? e.message : String(e);
      deliveries = [];
    } finally {
      deliveriesBusy = false;
    }
  }

  function addFilter(): void {
    if (filters.length >= 3) return;
    filters = [...filters, { key: '', value: '' }];
  }

  function removeFilter(i: number): void {
    filters = filters.filter((_, j) => j !== i);
  }

  function describe(a: Automation): string {
    const d = a.definition;
    const exclude = d.exclude_event ? ` sin ${d.exclude_event}` : '';
    return `${d.trigger_event} ${d.trigger_op} ${d.trigger_count} en ${d.window_days}d${exclude}`;
  }

  function fmtCount(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
    return n.toLocaleString();
  }

  let prevProject = $selectedProject;
  $: if (prevProject !== $selectedProject) {
    prevProject = $selectedProject;
    void loadList();
    newAutomation();
  }

  $: triggerEvent, triggerOp, triggerCount, windowDays, excludeEvent, excludeWindowDays, cooldownDays, filters, void schedulePreview();

  onMount(() => {
    void loadList();
    newAutomation();
  });
</script>

<div class="page-header">
  <h1 class="page-title">Automations</h1>
  <button on:click={newAutomation} class="primary">+ Nueva automation</button>
</div>

<div class="layout">
  <aside class="pane list" aria-label="Automations guardadas">
    <h2 class="pane-title">Reglas</h2>
    {#if loading}
      {#each Array(4) as _}
        <Skeleton width="100%" height="48px" radius="6px" />
      {/each}
    {:else if listError}
      <div class="error">{listError}</div>
    {:else if automations.length === 0}
      <div class="muted empty small">No hay automations guardadas.</div>
    {:else}
      <ul class="items">
        {#each automations as a (a.id)}
          <li>
            <button class="item" class:active={editing?.id === a.id} on:click={() => loadEditing(a)}>
              <span class="item-title">{a.name}</span>
              <span class="muted mono item-desc">{describe(a)}</span>
              <span class:enabled={a.enabled} class="status">{a.enabled ? 'activa' : 'pausada'}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </aside>

  <section class="pane builder">
    <div class="pane-head">
      <h2 class="pane-title">{editing ? 'Editar' : 'Nueva automation'}</h2>
      {#if previewBusy}<span class="muted small"><span class="spinner"></span> preview</span>{/if}
    </div>

    <label class="check"><input type="checkbox" bind:checked={formEnabled} /> Activa</label>
    <div class="field"><label>Nombre</label><input bind:value={formName} placeholder="Pricing intent without upgrade" /></div>
    <div class="field"><label>Descripción</label><input bind:value={formDescription} placeholder="Usuarios con alta intención de pricing" /></div>

    <div class="rule-grid">
      <div class="field"><label>Evento trigger</label><input class="mono" bind:value={triggerEvent} /></div>
      <div class="field"><label>Op</label><select bind:value={triggerOp}><option>==</option><option>&gt;=</option><option>&gt;</option><option>&lt;=</option><option>&lt;</option></select></div>
      <div class="field"><label>Veces</label><input type="number" min="1" bind:value={triggerCount} /></div>
      <div class="field"><label>Ventana días</label><input type="number" min="1" max="365" bind:value={windowDays} /></div>
    </div>

    <div class="rule-grid three">
      <div class="field"><label>Excluir evento</label><input class="mono" bind:value={excludeEvent} /></div>
      <div class="field"><label>Excluir días</label><input type="number" min="1" max="365" bind:value={excludeWindowDays} /></div>
      <div class="field"><label>Cooldown días</label><input type="number" min="1" max="365" bind:value={cooldownDays} /></div>
    </div>

    <div class="filters">
      <div class="filters-head">
        <span>Filtros properties</span>
        <button type="button" class="ghost small" on:click={addFilter} disabled={filters.length >= 3}>+ Añadir</button>
      </div>
      {#if filters.length === 0}
        <div class="muted empty small">Sin filtros.</div>
      {:else}
        {#each filters as f, i (i)}
          <div class="filter-row">
            <input class="mono" bind:value={f.key} placeholder="plan" />
            <span class="muted">=</span>
            <input class="mono" bind:value={f.value} placeholder="free" />
            <button type="button" class="ghost icon" on:click={() => removeFilter(i)}>×</button>
          </div>
        {/each}
      {/if}
    </div>

    <div class="field"><label>Webhook URL</label><input class="mono" bind:value={webhookUrl} placeholder="https://..." /></div>
    <div class="field"><label>Headers JSON</label><textarea class="mono" rows="5" bind:value={webhookHeaders} placeholder='{"Authorization":"Bearer ..."}'></textarea></div>

    {#if formError}<div class="error">{formError}</div>{/if}
    <div class="actions">
      <button class="primary" on:click={saveAutomation} disabled={saving}>{saving ? 'Guardando...' : editing ? 'Actualizar' : 'Crear'}</button>
      {#if editing}<button class="ghost danger" on:click={removeAutomation}>Eliminar</button>{/if}
    </div>

    <div class="preview">
      <h3 class="block-title">Preview</h3>
      {#if previewError}
        <div class="error">{previewError}</div>
      {:else if preview}
        <div class="preview-row"><span class="big mono">{fmtCount(preview.size)}</span><span class="muted small">usuarios · {preview.took_ms} ms</span></div>
        {#if preview.sample.length > 0}
          <ul class="sample mono">{#each preview.sample as id}<li>{id}</li>{/each}</ul>
        {/if}
      {:else}
        <div class="muted empty small">Completá la regla para calcular candidatos.</div>
      {/if}
    </div>
  </section>

  <section class="pane detail">
    <h2 class="pane-title">Deliveries</h2>
    {#if !editing}
      <div class="muted empty">Seleccioná una automation guardada.</div>
    {:else if deliveriesBusy}
      <Skeleton width="100%" height="160px" radius="6px" />
    {:else if deliveriesError}
      <div class="error">{deliveriesError}</div>
    {:else if deliveries.length === 0}
      <div class="muted empty">Todavía no hay envíos.</div>
    {:else}
      <table>
        <thead><tr><th>Usuario</th><th>Estado</th><th>Count</th><th>HTTP</th></tr></thead>
        <tbody>
          {#each deliveries as d}
            <tr>
              <td class="mono">{d.distinct_id}</td>
              <td class:failed={d.status === 'failed'}>{d.status}</td>
              <td class="mono">{d.trigger_count}</td>
              <td class="mono">{d.response_status || '-'}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </section>
</div>

<style>
  .layout { display: grid; grid-template-columns: 260px 1.25fr 1fr; gap: 16px; align-items: start; }
  @media (max-width: 1100px) { .layout { grid-template-columns: 1fr; } }
  .pane { background: var(--bg-elev); border: 1px solid var(--border); border-radius: 8px; padding: 12px; display: flex; flex-direction: column; gap: 10px; }
  .pane-title, .block-title { font-size: 12px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.5px; color: var(--text-muted); margin: 0; }
  .pane-head, .filters-head, .actions, .preview-row { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .items { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 4px; }
  .item { width: 100%; text-align: left; background: transparent; border: 1px solid var(--border); border-radius: 6px; padding: 8px 10px; display: flex; flex-direction: column; gap: 3px; color: var(--text); }
  .item:hover, .item.active { background: var(--bg-hover); border-color: var(--accent); }
  .item-title { font-weight: 600; font-size: 13px; }
  .item-desc { font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .status { font-size: 10px; color: var(--text-muted); }
  .status.enabled { color: var(--success); }
  .field { display: flex; flex-direction: column; gap: 4px; }
  .field label, .check { font-size: 11px; color: var(--text-muted); }
  .field input, .field select, .field textarea { width: 100%; }
  .rule-grid { display: grid; grid-template-columns: 1fr 70px 90px 110px; gap: 8px; }
  .rule-grid.three { grid-template-columns: 1fr 110px 110px; }
  .filters, .preview { background: var(--bg); border: 1px solid var(--border); border-radius: 6px; padding: 8px; display: flex; flex-direction: column; gap: 6px; }
  .filter-row { display: grid; grid-template-columns: 1fr 14px 1fr auto; gap: 6px; align-items: center; }
  .ghost { background: transparent; border: 1px solid var(--border); color: var(--text-muted); }
  .ghost.small { font-size: 11px; padding: 2px 8px; }
  .ghost.icon { padding: 0 6px; }
  .danger:hover { color: var(--danger); border-color: var(--danger); }
  .big { font-size: 28px; font-weight: 700; }
  .sample { list-style: none; padding: 0; margin: 0; max-height: 140px; overflow: auto; font-size: 11px; }
  .empty { padding: 14px 6px; text-align: center; }
  .small { font-size: 11.5px; }
  .error { color: var(--danger); background: var(--badge-error-bg); border: 1px solid var(--danger); padding: 6px 10px; border-radius: 6px; font-size: 12px; }
  table { width: 100%; border-collapse: collapse; font-size: 12px; }
  th, td { padding: 7px 6px; border-bottom: 1px solid var(--border); text-align: left; }
  th { color: var(--text-muted); font-weight: 600; }
  .failed { color: var(--danger); }
</style>
```

- [ ] **Step 3: Run frontend check**

Run:

```bash
cd frontend
npm run check
```

Expected: PASS.

- [ ] **Step 4: Run frontend tests**

Run:

```bash
cd frontend
npm test
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/routes/automations/+page.svelte frontend/src/lib/components/Sidebar.svelte
git commit -m "feat(automations): add ui"
```

---

### Task 9: Final Verification

**Files:**
- All files touched by previous tasks.

- [ ] **Step 1: Format Rust**

Run:

```bash
cd backend
cargo fmt
```

Expected: no errors.

- [ ] **Step 2: Run backend tests**

Run:

```bash
cd backend
cargo test automations automation_model_tests api::automations workers::automation_runner --lib
```

Expected: PASS.

- [ ] **Step 3: Run backend compile check**

Run:

```bash
cd backend
cargo check
```

Expected: PASS.

- [ ] **Step 4: Run frontend checks**

Run:

```bash
cd frontend
npm test
npm run check
```

Expected: PASS.

- [ ] **Step 5: Run migration check**

Run:

```bash
cd clickhouse
./test-migrations.sh
```

Expected: PASS.

- [ ] **Step 6: Commit verification-only formatting if needed**

If `cargo fmt` changed files:

```bash
git add backend
git commit -m "style: format automations"
```

If no files changed, do not create a commit.

---

## Self-Review

- Spec coverage: schema, API CRUD, preview, worker, webhook payload, secret redaction, frontend, env config, and testing are covered.
- No placeholders: all tasks contain concrete files, code, commands, and expected outcomes.
- Type consistency: backend `AutomationDefinition` maps to frontend `AutomationDefinition`; `automation_rules.definition` and `automation_rules.webhook` are JSON strings in storage and structured values in API responses.
