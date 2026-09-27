use axum::{Json, extract::State};
use serde::Serialize;
use sqlx::SqlitePool;

use crate::http::error::ApiError;

#[derive(Serialize)]
pub struct HealthResponse {
    status: &'static str,
}

pub async fn live() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

pub async fn ready(State(pool): State<SqlitePool>) -> Result<Json<HealthResponse>, ApiError> {
    super::service::check_readiness(&pool).await?;
    Ok(Json(HealthResponse { status: "ready" }))
}
