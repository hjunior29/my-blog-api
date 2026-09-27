use sqlx::{SqliteConnection, SqlitePool};

use super::model::{NewUser, User, UserRole, UserStatus};

pub async fn find_by_id(pool: &SqlitePool, id: i64) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "SELECT id, email, normalized_email, password_hash, display_name, bio, avatar_media_id, role, status, created_at, updated_at, password_changed_at
         FROM users WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn find_by_normalized_email(
    pool: &SqlitePool,
    normalized_email: &str,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "SELECT id, email, normalized_email, password_hash, display_name, bio, avatar_media_id, role, status, created_at, updated_at, password_changed_at
         FROM users WHERE normalized_email = ?",
    )
    .bind(normalized_email)
    .fetch_optional(pool)
    .await
}

pub async fn count_active_owners(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'owner' AND status = 'active'")
        .fetch_one(pool)
        .await
}

pub async fn count_active_owners_conn(conn: &mut SqliteConnection) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'owner' AND status = 'active'")
        .fetch_one(conn)
        .await
}

pub async fn insert_user(
    conn: &mut SqliteConnection,
    user: &NewUser<'_>,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO users (email, normalized_email, password_hash, display_name, bio, role, status, created_at, updated_at, password_changed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(user.email)
    .bind(user.normalized_email)
    .bind(user.password_hash)
    .bind(user.display_name)
    .bind(user.bio)
    .bind(user.role.to_string())
    .bind(user.status.to_string())
    .bind(user.now)
    .bind(user.now)
    .bind(user.now)
    .execute(conn)
    .await?;

    Ok(result.last_insert_rowid())
}

pub async fn update_password(
    conn: &mut SqliteConnection,
    id: i64,
    password_hash: &str,
    now: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE users SET password_hash = ?, password_changed_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(password_hash)
    .bind(now)
    .bind(now)
    .bind(id)
    .execute(conn)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn set_status(
    conn: &mut SqliteConnection,
    id: i64,
    status: UserStatus,
    now: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE users SET status = ?, updated_at = ? WHERE id = ?")
        .bind(status.to_string())
        .bind(now)
        .bind(id)
        .execute(conn)
        .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn set_role(
    conn: &mut SqliteConnection,
    id: i64,
    role: UserRole,
    now: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE users SET role = ?, updated_at = ? WHERE id = ?")
        .bind(role.to_string())
        .bind(now)
        .bind(id)
        .execute(conn)
        .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn list(pool: &SqlitePool, limit: i64, offset: i64) -> Result<Vec<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "SELECT id, email, normalized_email, password_hash, display_name, bio, avatar_media_id, role, status, created_at, updated_at, password_changed_at
         FROM users ORDER BY id ASC LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}
