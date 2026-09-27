use sqlx::SqlitePool;

pub async fn check_connection(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT version FROM _sqlx_migrations WHERE success = 1 ORDER BY version DESC LIMIT 1",
    )
    .fetch_one(pool)
    .await?;
    Ok(())
}
