//! Endpoints de servicios:
//!   GET /services     → lista de servicios con conteos de logs y errores
//!   GET /services/map → grafo de dependencias entre servicios (service map)

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::api::params::Range;
use crate::error::ApiResult;
use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/services", get(list_services))
        .route("/services/map", get(service_map))
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Service {
    pub service_name: String,
    pub log_count: u64,
    pub error_count: u64,
    pub last_seen: String,
}

async fn list_services(
    State(state): State<SharedState>,
    Query(range): Query<Range>,
) -> ApiResult<Json<Vec<Service>>> {
    let (from, to) = range.resolve();
    let sql = format!(
        "SELECT service_name, \
                toUInt64(count()) AS log_count, \
                toUInt64(countIf(severity_number >= 17)) AS error_count, \
                toString(max(timestamp)) AS last_seen \
         FROM faro.logs WHERE timestamp >= toDateTime64('{from}', 9) AND timestamp <= toDateTime64('{to}', 9) \
         GROUP BY service_name ORDER BY last_seen DESC LIMIT 200",
        from = crate::api::params::ch_dt(from),
        to = crate::api::params::ch_dt(to),
    );
    let rows: Vec<Service> = state.ch.select(&sql).await?;
    Ok(Json(rows))
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ServiceMapNode {
    pub service: String,
    pub calls: u64,
    pub errors: u64,
    pub p95_ms: u64,
    pub is_root: u8,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ServiceMapEdge {
    pub source: String,
    pub target: String,
    pub calls: u64,
    pub errors: u64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub p99_ms: u64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ServiceMap {
    pub nodes: Vec<ServiceMapNode>,
    pub edges: Vec<ServiceMapEdge>,
}

/// Grafo de llamadas servicio→servicio inferido del self-join de `faro.spans`
/// sobre `parent_span_id`. La estadística por arista (calls, errors, p50/p95/p99)
/// se calcula del lado del span hijo, que es donde caen la latencia y el status
/// del RPC. Para los nodos hacemos un agregado independiente sobre la tabla, así
/// también aparecen servicios sin llamadas entrantes (e.g. el frontend que abre
/// todas las trazas) con sus métricas globales.
async fn service_map(
    State(state): State<SharedState>,
    Query(range): Query<Range>,
) -> ApiResult<Json<ServiceMap>> {
    let (from, to) = range.resolve();
    let from_s = crate::api::params::ch_dt(from);
    let to_s = crate::api::params::ch_dt(to);

    let (proj_clause, proj_val) = range.project_clause_named("", "project");
    let proj_clause_c = proj_clause.replace("project_id", "c.project_id");
    let proj_clause_p = proj_clause.replace("project_id", "p.project_id");

    // Edges: filtramos los timestamps en ambos lados del JOIN para que ClickHouse
    // pode particiones por fecha en parent y child. Sin esto, la build-side de la
    // hash join escanea toda la tabla.
    let edges_sql = format!(
        "SELECT p.service_name AS source, c.service_name AS target, \
                toUInt64(count()) AS calls, \
                toUInt64(countIf(c.status_code = 'ERROR')) AS errors, \
                toUInt64(quantileExact(0.50)(c.duration_ns) / 1000000) AS p50_ms, \
                toUInt64(quantileExact(0.95)(c.duration_ns) / 1000000) AS p95_ms, \
                toUInt64(quantileExact(0.99)(c.duration_ns) / 1000000) AS p99_ms \
         FROM faro.spans AS c \
         INNER JOIN faro.spans AS p \
           ON c.trace_id = p.trace_id AND c.parent_span_id = p.span_id \
         WHERE c.timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
           AND c.timestamp <= toDateTime64({{to:DateTime64(9)}}, 9) \
           AND p.timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
           AND p.timestamp <= toDateTime64({{to:DateTime64(9)}}, 9) \
           AND c.parent_span_id != '' \
           AND c.service_name != p.service_name \
           {proj_clause_c}{proj_clause_p} \
         GROUP BY source, target \
         HAVING calls > 0 \
         ORDER BY calls DESC \
         LIMIT 500"
    );

    // Nodes: agregado sobre todos los spans del rango. Incluye servicios que solo
    // *originan* trazas (parent_span_id vacío) sin recibir llamadas entrantes —
    // ejemplo típico: el navegador / frontend.
    let nodes_sql = format!(
        "SELECT service_name AS service, \
                toUInt64(count()) AS calls, \
                toUInt64(countIf(status_code = 'ERROR')) AS errors, \
                toUInt64(quantileExact(0.95)(duration_ns) / 1000000) AS p95_ms, \
                toUInt8(countIf(parent_span_id = '' OR parent_span_id = '0000000000000000') > 0) AS is_root \
         FROM faro.spans \
         WHERE timestamp >= toDateTime64({{from:DateTime64(9)}}, 9) \
           AND timestamp <= toDateTime64({{to:DateTime64(9)}}, 9) \
           {proj_clause_top} \
         GROUP BY service \
         HAVING calls > 0 \
         ORDER BY calls DESC \
         LIMIT 200",
        proj_clause_top = proj_clause
    );

    let mut params: Vec<(&str, &str)> = vec![("from", &from_s), ("to", &to_s)];
    if let Some(p) = proj_val {
        params.push(("project", p));
    }

    let edges: Vec<ServiceMapEdge> = state.ch.select_with_params(&edges_sql, &params).await?;
    let nodes: Vec<ServiceMapNode> = state.ch.select_with_params(&nodes_sql, &params).await?;

    Ok(Json(ServiceMap { nodes, edges }))
}
