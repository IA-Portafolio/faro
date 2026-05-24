mod common;

use chrono::Utc;
use faro::storage::FeatureFlagRow;

use common::TestApp;

#[tokio::test]
async fn ingest_feature_flags_returns_only_active_flags_for_token_project() {
    let app = TestApp::spawn().await;
    let now = Utc::now();
    let active = FeatureFlagRow {
        project_id: app.project_slug.clone(),
        key: "new-checkout".into(),
        rollout_percentage: 10,
        conditions: r#"{"properties":{"plan":"pro"}}"#.into(),
        active: 1,
        updated_at: now,
        version: now.timestamp_millis() as u64,
    };
    let inactive = FeatureFlagRow {
        project_id: app.project_slug.clone(),
        key: "old-checkout".into(),
        rollout_percentage: 100,
        conditions: String::new(),
        active: 0,
        updated_at: now,
        version: now.timestamp_millis() as u64,
    };
    app.ch
        .insert("faro.feature_flags", &[active, inactive])
        .await
        .expect("insert feature flags");
    app.state
        .feature_flags
        .reload(&app.ch)
        .await
        .expect("reload feature flags");

    let resp = app
        .http
        .get(format!("{}/api/v1/ingest/feature-flags", app.api_url))
        .bearer_auth(&app.project_token)
        .send()
        .await
        .expect("feature flags request");
    assert!(resp.status().is_success(), "status {}", resp.status());
    let body: serde_json::Value = resp.json().await.expect("json body");

    assert_eq!(body["project"].as_str(), Some(app.project_slug.as_str()));
    let flags = body["flags"].as_array().expect("flags array");
    assert_eq!(flags.len(), 1, "inactive flags must not be served");
    assert_eq!(flags[0]["key"], "new-checkout");
    assert_eq!(flags[0]["rollout_percentage"], 10);
    assert_eq!(flags[0]["conditions"]["properties"]["plan"], "pro");
}
