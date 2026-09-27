use sqlx::SqlitePool;

use crate::http::error::ApiError;

pub async fn check_readiness(pool: &SqlitePool) -> Result<(), ApiError> {
    super::repository::check_connection(pool)
        .await
        .map_err(|_| {
            tracing::warn!("database readiness check failed");
            ApiError::unavailable()
        })
}
