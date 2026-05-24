use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::error::ApiResult;
use crate::feature_flags::SdkFeatureFlag;
use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new().route("/feature-flags", get(get_feature_flags))
}

#[derive(Serialize)]
struct FeatureFlagsResponse {
    project: String,
    flags: Vec<SdkFeatureFlag>,
}

async fn get_feature_flags(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> ApiResult<Json<FeatureFlagsResponse>> {
    let project = crate::ingest::resolve_project(&state, &headers)?;
    crate::ingest::check_origin(&state, &project, &headers)?;
    let flags = state.feature_flags.flags_for_project(&project);
    Ok(Json(FeatureFlagsResponse { project, flags }))
}
