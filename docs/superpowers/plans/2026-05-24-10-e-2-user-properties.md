# User Properties Enrichment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist `identify` traits into `faro.product_users.properties` and let cohorts filter users by those JSON properties.

**Architecture:** Keep event filters and user filters separate in `CohortDefinition`. `user_unifier` merges `$identify.user_properties` into the existing user JSON row. Cohort SQL keeps counting behavior from `product_events` and joins `product_users FINAL` only when `user_filters` are present.

**Tech Stack:** Rust, serde/serde_json, Axum backend modules, ClickHouse SQL generated with existing parameterized query helpers.

---

### Task 1: Extend The Cohort Definition Model

**Files:**
- Modify: `backend/src/storage/models.rs`
- Test: `backend/src/storage/models.rs`

- [ ] **Step 1: Write the failing serde compatibility test**

Add this test module near the bottom of `backend/src/storage/models.rs`, after `CohortRow`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cohort_definition_defaults_user_filters_for_existing_json() {
        let raw = r#"{
            "event": "checkout_completed",
            "op": ">=",
            "count": 1,
            "last_days": 30,
            "filters": [{"key": "currency", "value": "USD"}]
        }"#;

        let def: CohortDefinition = serde_json::from_str(raw).unwrap();

        assert_eq!(def.event, "checkout_completed");
        assert_eq!(def.filters.len(), 1);
        assert!(def.user_filters.is_empty());
    }

    #[test]
    fn cohort_definition_round_trips_user_filters() {
        let raw = r#"{
            "event": "checkout_completed",
            "op": ">=",
            "count": 1,
            "last_days": 30,
            "filters": [],
            "user_filters": [
                {"key": "plan", "value": "pro"},
                {"key": "industry", "value": "fintech"}
            ]
        }"#;

        let def: CohortDefinition = serde_json::from_str(raw).unwrap();

        assert_eq!(def.user_filters.len(), 2);
        assert_eq!(def.user_filters[0].key, "plan");
        assert_eq!(def.user_filters[0].value, "pro");
        assert_eq!(def.user_filters[1].key, "industry");
        assert_eq!(def.user_filters[1].value, "fintech");
    }
}
```

- [ ] **Step 2: Run the model test and verify it fails**

Run:

```powershell
cargo test --manifest-path backend/Cargo.toml storage::models::tests::cohort_definition_defaults_user_filters_for_existing_json
```

Expected: compile failure mentioning `no field user_filters on type storage::models::CohortDefinition`.

- [ ] **Step 3: Add `user_filters` to `CohortDefinition`**

In `backend/src/storage/models.rs`, replace the `CohortDefinition` struct with:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CohortDefinition {
    /// Nombre del evento a contar.
    pub event: String,
    /// Comparador entre `count()` y `count`. Valores: `==`, `>=`, `>`, `<=`, `<`.
    pub op: String,
    /// Umbral del comparador.
    pub count: u32,
    /// Tamaño de la ventana hacia atrás desde "ahora", en días. Acotado por el
    /// backend a [1, 365] al evaluar.
    pub last_days: u32,
    /// Filtros opcionales sobre `JSONExtractString(properties, key) = value`.
    /// Estos son predicates del evento, no traits persistidos del usuario.
    #[serde(default)]
    pub filters: Vec<CohortFilter>,
    /// Filtros opcionales sobre `faro.product_users.properties`.
    /// Estos son traits persistidos por `identify(user_id, traits)`.
    #[serde(default)]
    pub user_filters: Vec<CohortFilter>,
}
```

- [ ] **Step 4: Run the model tests and verify they pass**

Run:

```powershell
cargo test --manifest-path backend/Cargo.toml storage::models::tests::cohort_definition_
```

Expected: both `cohort_definition_defaults_user_filters_for_existing_json` and `cohort_definition_round_trips_user_filters` pass.

- [ ] **Step 5: Commit the model change**

Run:

```powershell
git add backend/src/storage/models.rs
git commit -m "feat(cohorts): add user filters to definition"
```

### Task 2: Validate User Filters With Existing Guardrails

**Files:**
- Modify: `backend/src/api/cohorts.rs`
- Test: `backend/src/api/cohorts.rs`

- [ ] **Step 1: Write failing validation tests**

In `backend/src/api/cohorts.rs`, inside the existing `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn validate_def_counts_event_and_user_filters_together() {
        let def = CohortDefinition {
            event: "checkout_completed".into(),
            op: ">=".into(),
            count: 1,
            last_days: 30,
            filters: vec![
                CohortFilter { key: "currency".into(), value: "USD".into() },
                CohortFilter { key: "coupon".into(), value: "SPRING".into() },
            ],
            user_filters: vec![
                CohortFilter { key: "plan".into(), value: "pro".into() },
                CohortFilter { key: "industry".into(), value: "fintech".into() },
            ],
        };

        assert!(validate_def(&def).is_err());
    }

    #[test]
    fn validate_def_rejects_empty_user_filter_key_or_value() {
        let empty_key = CohortDefinition {
            event: "checkout_completed".into(),
            op: ">=".into(),
            count: 1,
            last_days: 30,
            filters: vec![],
            user_filters: vec![CohortFilter { key: "".into(), value: "pro".into() }],
        };
        assert!(validate_def(&empty_key).is_err());

        let empty_value = CohortDefinition {
            event: "checkout_completed".into(),
            op: ">=".into(),
            count: 1,
            last_days: 30,
            filters: vec![],
            user_filters: vec![CohortFilter { key: "plan".into(), value: "".into() }],
        };
        assert!(validate_def(&empty_value).is_err());
    }
```

Also update every existing `CohortDefinition` literal in this test module to include `user_filters: vec![]`.

- [ ] **Step 2: Run the validation tests and verify they fail**

Run:

```powershell
cargo test --manifest-path backend/Cargo.toml api::cohorts::tests::validate_def_
```

Expected: `validate_def_counts_event_and_user_filters_together` fails because only `filters.len()` is capped, and the empty user filter test fails because `user_filters` are not validated.

- [ ] **Step 3: Update `validate_def`**

In `backend/src/api/cohorts.rs`, replace the filter-count and filter-loop block in `validate_def` with:

```rust
    let total_filters = def.filters.len() + def.user_filters.len();
    if total_filters > MAX_FILTERS {
        return Err(ApiError::BadRequest(format!(
            "máximo {MAX_FILTERS} filtros combinados sobre properties de evento y usuario"
        )));
    }
    for f in def.filters.iter().chain(def.user_filters.iter()) {
        if f.key.trim().is_empty() || f.value.trim().is_empty() {
            return Err(ApiError::BadRequest(
                "filtros sobre properties: key y value no pueden ser vacíos".into(),
            ));
        }
        if f.key.len() > MAX_KEY_LEN || f.value.len() > MAX_VAL_LEN {
            return Err(ApiError::BadRequest("filtro demasiado largo".into()));
        }
    }
```

- [ ] **Step 4: Run the validation tests and verify they pass**

Run:

```powershell
cargo test --manifest-path backend/Cargo.toml api::cohorts::tests::validate_def_
```

Expected: validation tests pass.

- [ ] **Step 5: Commit the validation change**

Run:

```powershell
git add backend/src/api/cohorts.rs
git commit -m "feat(cohorts): validate user property filters"
```

### Task 3: Join Product Users In Cohort Queries When Needed

**Files:**
- Modify: `backend/src/api/cohorts.rs`
- Test: `backend/src/api/cohorts.rs`

- [ ] **Step 1: Write the failing SQL shape test**

In `backend/src/api/cohorts.rs`, inside `mod tests`, add:

```rust
    #[test]
    fn build_cohort_query_with_user_filters_shape() {
        let def = CohortDefinition {
            event: "checkout_completed".into(),
            op: ">=".into(),
            count: 1,
            last_days: 30,
            filters: vec![CohortFilter {
                key: "currency".into(),
                value: "USD".into(),
            }],
            user_filters: vec![
                CohortFilter { key: "plan".into(), value: "pro".into() },
                CohortFilter { key: "industry".into(), value: "fintech".into() },
            ],
        };

        let q = build_cohort_query(&def, "default", "").unwrap();

        assert!(q.sql.contains("FROM faro.product_events AS e"));
        assert!(q.sql.contains("INNER JOIN faro.product_users FINAL AS u"));
        assert!(q.sql.contains("u.project_id = e.project_id"));
        assert!(q.sql.contains("u.distinct_id = e.distinct_id"));
        assert!(q.sql.contains("JSONExtractString(e.properties, {fk_0:String})"));
        assert!(q.sql.contains("JSONExtractString(u.properties, {ufk_0:String})"));
        assert!(q.sql.contains("JSONExtractString(u.properties, {ufk_1:String})"));
        assert!(q.sql.contains("GROUP BY e.distinct_id"));
        assert!(!q.sql.contains("pro"));
        assert!(!q.sql.contains("fintech"));
        assert!(q.owned.iter().any(|(k, v)| k == "ufv_0" && v == "pro"));
        assert!(q.owned.iter().any(|(k, v)| k == "ufv_1" && v == "fintech"));
    }
```

Update the existing `build_cohort_query_shape` test literals to include `user_filters: vec![]`. Keep its current assertions so the no-user-filter query remains compatible.

- [ ] **Step 2: Run the SQL builder tests and verify the new one fails**

Run:

```powershell
cargo test --manifest-path backend/Cargo.toml api::cohorts::tests::build_cohort_query
```

Expected: `build_cohort_query_with_user_filters_shape` fails because no join or `u.properties` filter exists yet.

- [ ] **Step 3: Update `build_cohort_query`**

In `backend/src/api/cohorts.rs`, replace the filter-clause construction and final SQL construction inside `build_cohort_query` with:

```rust
    let mut filter_clauses = String::new();
    for (i, f) in def.filters.iter().enumerate() {
        let kp = format!("{prefix}fk_{i}");
        let vp = format!("{prefix}fv_{i}");
        if def.user_filters.is_empty() {
            filter_clauses.push_str(&format!(
                " AND JSONExtractString(properties, {{{kp}:String}}) = {{{vp}:String}}"
            ));
        } else {
            filter_clauses.push_str(&format!(
                " AND JSONExtractString(e.properties, {{{kp}:String}}) = {{{vp}:String}}"
            ));
        }
        owned.push((kp, f.key.clone()));
        owned.push((vp, f.value.clone()));
    }

    let mut user_filter_clauses = String::new();
    for (i, f) in def.user_filters.iter().enumerate() {
        let kp = format!("{prefix}ufk_{i}");
        let vp = format!("{prefix}ufv_{i}");
        user_filter_clauses.push_str(&format!(
            " AND JSONExtractString(u.properties, {{{kp}:String}}) = {{{vp}:String}}"
        ));
        owned.push((kp, f.key.clone()));
        owned.push((vp, f.value.clone()));
    }
```

Then replace the current `let sql = format!(...)` block with:

```rust
    let sql = if def.user_filters.is_empty() {
        format!(
            "SELECT distinct_id \
             FROM faro.product_events \
             WHERE event_name = {{{event_p}:String}} \
               AND timestamp >= now() - toIntervalDay({{{last_p}:UInt32}}) \
               AND project_id = {{{proj_p}:String}}{filter_clauses} \
             GROUP BY distinct_id \
             HAVING count() {op} {{{count_p}:UInt32}}"
        )
    } else {
        format!(
            "SELECT e.distinct_id AS distinct_id \
             FROM faro.product_events AS e \
             INNER JOIN faro.product_users FINAL AS u \
               ON u.project_id = e.project_id AND u.distinct_id = e.distinct_id \
             WHERE e.event_name = {{{event_p}:String}} \
               AND e.timestamp >= now() - toIntervalDay({{{last_p}:UInt32}}) \
               AND e.project_id = {{{proj_p}:String}}{filter_clauses}{user_filter_clauses} \
             GROUP BY e.distinct_id \
             HAVING count() {op} {{{count_p}:UInt32}}"
        )
    };
```

- [ ] **Step 4: Run the SQL builder tests and verify they pass**

Run:

```powershell
cargo test --manifest-path backend/Cargo.toml api::cohorts::tests::build_cohort_query
```

Expected: both cohort query shape tests pass.

- [ ] **Step 5: Commit the query builder change**

Run:

```powershell
git add backend/src/api/cohorts.rs
git commit -m "feat(cohorts): filter by user properties"
```

### Task 4: Merge Identify Traits Into Product Users

**Files:**
- Modify: `backend/src/workers/user_unifier.rs`
- Test: `backend/src/workers/user_unifier.rs`

- [ ] **Step 1: Write failing merge tests**

In `backend/src/workers/user_unifier.rs`, inside the existing `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn merge_user_properties_preserves_existing_keys_and_latest_wins() {
        let merged = merge_user_properties(
            r#"{"plan":"free","signup_date":"2026-01-01"}"#,
            r#"{"plan":"pro","industry":"fintech"}"#,
        );
        let v: serde_json::Value = serde_json::from_str(&merged).unwrap();

        assert_eq!(v["plan"], "pro");
        assert_eq!(v["signup_date"], "2026-01-01");
        assert_eq!(v["industry"], "fintech");
    }

    #[test]
    fn merge_user_properties_ignores_empty_or_invalid_latest_payload() {
        let existing = r#"{"plan":"pro"}"#;

        assert_eq!(merge_user_properties(existing, ""), existing);
        assert_eq!(merge_user_properties(existing, "not-json"), existing);
        assert_eq!(merge_user_properties(existing, "[]"), existing);
        assert_eq!(merge_user_properties(existing, "{}"), existing);
    }

    #[test]
    fn merge_user_properties_uses_latest_when_existing_is_empty_or_invalid() {
        let latest = r#"{"plan":"pro"}"#;

        assert_eq!(merge_user_properties("", latest), latest);
        assert_eq!(merge_user_properties("not-json", latest), latest);
    }
```

- [ ] **Step 2: Run the worker tests and verify they fail**

Run:

```powershell
cargo test --manifest-path backend/Cargo.toml workers::user_unifier::tests::merge_user_properties
```

Expected: compile failure mentioning `cannot find function merge_user_properties`.

- [ ] **Step 3: Add the merge helper**

In `backend/src/workers/user_unifier.rs`, add this function after `dedupe`:

```rust
fn merge_user_properties(existing: &str, latest: &str) -> String {
    let latest_trimmed = latest.trim();
    if latest_trimmed.is_empty() {
        return existing.to_string();
    }

    let latest_value: serde_json::Value = match serde_json::from_str(latest_trimmed) {
        Ok(v) => v,
        Err(_) => return existing.to_string(),
    };
    let serde_json::Value::Object(latest_obj) = latest_value else {
        return existing.to_string();
    };
    if latest_obj.is_empty() {
        return existing.to_string();
    }

    let mut merged = match serde_json::from_str::<serde_json::Value>(existing.trim()) {
        Ok(serde_json::Value::Object(obj)) => obj,
        _ => serde_json::Map::new(),
    };

    for (key, value) in latest_obj {
        merged.insert(key, value);
    }

    serde_json::Value::Object(merged).to_string()
}
```

- [ ] **Step 4: Use the helper in `unify_once`**

In the `Some(ex)` merge branch inside `unify_once`, replace:

```rust
                let props = if row.latest_props.is_empty() {
                    ex.properties
                } else {
                    row.latest_props.clone()
                };
```

with:

```rust
                let props = merge_user_properties(&ex.properties, &row.latest_props);
```

In the `None` branch, replace `row.latest_props.clone()` with:

```rust
                merge_user_properties("", &row.latest_props)
```

- [ ] **Step 5: Run the worker merge tests and verify they pass**

Run:

```powershell
cargo test --manifest-path backend/Cargo.toml workers::user_unifier::tests::merge_user_properties
```

Expected: all three merge tests pass.

- [ ] **Step 6: Commit the worker change**

Run:

```powershell
git add backend/src/workers/user_unifier.rs
git commit -m "feat(users): merge identify properties"
```

### Task 5: Full Backend Verification

**Files:**
- Modify: no source files unless verification reveals a compile error in the touched code.

- [ ] **Step 1: Format the backend**

Run:

```powershell
cargo fmt --manifest-path backend/Cargo.toml
```

Expected: command exits 0.

- [ ] **Step 2: Run focused tests**

Run:

```powershell
cargo test --manifest-path backend/Cargo.toml storage::models::tests::cohort_definition_
cargo test --manifest-path backend/Cargo.toml api::cohorts::tests
cargo test --manifest-path backend/Cargo.toml workers::user_unifier::tests
```

Expected: all focused tests pass.

- [ ] **Step 3: Run the backend test suite**

Run:

```powershell
cargo test --manifest-path backend/Cargo.toml
```

Expected: all backend tests pass. If integration tests require a local ClickHouse and fail with connection errors, record the exact failure and run the focused unit tests from Step 2 as the verified fallback.

- [ ] **Step 4: Inspect the final diff**

Run:

```powershell
git diff -- backend/src/storage/models.rs backend/src/api/cohorts.rs backend/src/workers/user_unifier.rs
git status --short
```

Expected: only intentional source changes remain unstaged. Pre-existing unrelated workspace changes may still appear in `git status`; do not revert them.

- [ ] **Step 5: Commit verification-only formatting if needed**

If `cargo fmt` changed any touched file after the task commits, run:

```powershell
git add backend/src/storage/models.rs backend/src/api/cohorts.rs backend/src/workers/user_unifier.rs
git commit -m "chore: format user properties enrichment"
```

Expected: commit is created only if formatting changed tracked touched files.
