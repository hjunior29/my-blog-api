use my_blog_api::database;
use sqlx::SqlitePool;

async fn scalar(pool: &SqlitePool, query: &'static str) -> i64 {
    sqlx::query_scalar(query).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn file_database_persists_data_and_migrations_across_restarts() {
    let directory = tempfile::tempdir().unwrap();
    let url = format!("sqlite://{}", directory.path().join("blog.db").display());
    let pool = database::connect(&url, 2).await.unwrap();
    assert_eq!(
        scalar(
            &pool,
            "SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1"
        )
        .await,
        4
    );
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(mode, "wal");
    sqlx::query("CREATE TABLE test_entries (value TEXT NOT NULL)")
        .execute(&pool)
        .await
        .unwrap();
    let value = "a quote ' and a statement ; DROP TABLE test_entries;";
    sqlx::query("INSERT INTO test_entries (value) VALUES (?)")
        .bind(value)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let reopened = database::connect(&url, 2).await.unwrap();
    let stored: String = sqlx::query_scalar("SELECT value FROM test_entries")
        .fetch_one(&reopened)
        .await
        .unwrap();
    assert_eq!(stored, value);
    assert_eq!(
        scalar(&reopened, "SELECT COUNT(*) FROM _sqlx_migrations").await,
        4
    );
    reopened.close().await;
}

#[tokio::test]
async fn every_connection_enforces_integrity_and_busy_timeout() {
    let directory = tempfile::tempdir().unwrap();
    let url = format!("sqlite://{}", directory.path().join("blog.db").display());
    let pool = database::connect(&url, 2).await.unwrap();
    let mut first = pool.acquire().await.unwrap();
    let mut second = pool.acquire().await.unwrap();
    for connection in [&mut first, &mut second] {
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut **connection)
            .await
            .unwrap();
        let busy_timeout: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
            .fetch_one(&mut **connection)
            .await
            .unwrap();
        assert_eq!(foreign_keys, 1);
        assert_eq!(busy_timeout, 5000);
    }
    drop(first);
    drop(second);
    sqlx::query("CREATE TABLE test_parents (id INTEGER PRIMARY KEY)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE test_children (parent_id INTEGER REFERENCES test_parents(id))")
        .execute(&pool)
        .await
        .unwrap();
    let error = sqlx::query("INSERT INTO test_children VALUES (999)")
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(
        error
            .as_database_error()
            .unwrap()
            .is_foreign_key_violation()
    );
    pool.close().await;
}

#[tokio::test]
async fn invalid_database_configuration_fails() {
    assert!(database::connect("sqlite::memory:", 0).await.is_err());
    assert!(database::connect("sqlite::memory:", 17).await.is_err());
    assert!(
        database::connect("postgres://localhost/blog", 1)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn migration_checksum_mismatch_prevents_startup() {
    let directory = tempfile::tempdir().unwrap();
    let url = format!("sqlite://{}", directory.path().join("blog.db").display());
    let pool = database::connect(&url, 1).await.unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET checksum = X'00'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(matches!(
        database::connect(&url, 1).await,
        Err(database::DatabaseError::Migration(_))
    ));
}

#[tokio::test]
async fn bundled_sqlite_includes_the_wal_reset_fix() {
    let pool = database::connect("sqlite::memory:", 1).await.unwrap();
    let version: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&pool)
        .await
        .unwrap();
    let parts: Vec<u32> = version
        .split('.')
        .map(|part| part.parse().unwrap())
        .collect();
    assert!(
        parts.as_slice() >= [3, 51, 3].as_slice(),
        "SQLite {version} lacks the WAL-reset fix"
    );
    pool.close().await;
}
