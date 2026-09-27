use std::{str::FromStr, time::Duration};

use sqlx::{
    SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use thiserror::Error;

static MIGRATOR: Migrator = sqlx::migrate!();

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("invalid database configuration")]
    Configuration,
    #[error("database connection failed")]
    Connection(#[source] sqlx::Error),
    #[error("database migration failed")]
    Migration(#[source] sqlx::migrate::MigrateError),
}

pub async fn connect(url: &str, max_connections: u32) -> Result<SqlitePool, DatabaseError> {
    if !(1..=16).contains(&max_connections) {
        return Err(DatabaseError::Configuration);
    }
    let options = SqliteConnectOptions::from_str(url)
        .map_err(|_| DatabaseError::Configuration)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(3))
        .connect_with(options)
        .await
        .map_err(DatabaseError::Connection)?;
    if let Err(error) = MIGRATOR.run(&pool).await {
        pool.close().await;
        return Err(DatabaseError::Migration(error));
    }
    Ok(pool)
}
