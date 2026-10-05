use std::fmt::Write;

use base64::prelude::*;
use rand::RngExt;
use sha2::{Digest, Sha256};
use sqlx::{SqliteConnection, SqlitePool};
use subtle::ConstantTimeEq;
use thiserror::Error;

pub const SESSION_TTL_SECS: i64 = 7 * 24 * 3600;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session expired or invalid")]
    InvalidSession,
    #[error("refresh token not found or invalid")]
    InvalidToken,
    #[error("token replay detected, session revoked")]
    ReplayDetected,
    #[error("invalid csrf token")]
    InvalidCsrf,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuthSession {
    pub id: String,
    pub user_id: i64,
    pub csrf_token: String,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RefreshTokenRow {
    pub token_hash: String,
    pub session_id: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub consumed_at: Option<i64>,
    pub replaced_by_hash: Option<String>,
}

pub fn generate_random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    BASE64_URL_SAFE_NO_PAD.encode(bytes)
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for b in digest {
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

pub fn constant_time_compare(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

pub const SHORT_SESSION_TTL_SECS: i64 = 24 * 3600;

pub async fn create_session(
    conn: &mut SqliteConnection,
    user_id: i64,
    user_agent: Option<&str>,
    ip_address: Option<&str>,
    remember_me: bool,
    now: i64,
) -> Result<(AuthSession, String), SessionError> {
    let session_id = generate_random_token();
    let csrf_token = generate_random_token();
    let ttl = if remember_me {
        SESSION_TTL_SECS
    } else {
        SHORT_SESSION_TTL_SECS
    };
    let expires_at = now + ttl;

    sqlx::query(
        "INSERT INTO auth_sessions (id, user_id, csrf_token, user_agent, ip_address, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&session_id)
    .bind(user_id)
    .bind(&csrf_token)
    .bind(user_agent)
    .bind(ip_address)
    .bind(now)
    .bind(expires_at)
    .execute(&mut *conn)
    .await?;

    let raw_refresh_token = generate_random_token();
    let token_hash = hash_token(&raw_refresh_token);

    sqlx::query(
        "INSERT INTO refresh_tokens (token_hash, session_id, created_at, expires_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&token_hash)
    .bind(&session_id)
    .bind(now)
    .bind(expires_at)
    .execute(&mut *conn)
    .await?;

    let session = AuthSession {
        id: session_id,
        user_id,
        csrf_token,
        user_agent: user_agent.map(str::to_string),
        ip_address: ip_address.map(str::to_string),
        created_at: now,
        expires_at,
        revoked_at: None,
    };

    Ok((session, raw_refresh_token))
}

pub async fn rotate_refresh_token(
    pool: &SqlitePool,
    raw_token: &str,
    csrf_token: Option<&str>,
    now: i64,
) -> Result<(AuthSession, String), SessionError> {
    let token_hash = hash_token(raw_token);
    let mut tx = pool.begin().await?;

    let row = sqlx::query_as::<_, RefreshTokenRow>(
        "SELECT token_hash, session_id, created_at, expires_at, consumed_at, replaced_by_hash
         FROM refresh_tokens WHERE token_hash = ?",
    )
    .bind(&token_hash)
    .fetch_optional(&mut *tx)
    .await?;

    let token_record = match row {
        Some(r) => r,
        None => return Err(SessionError::InvalidToken),
    };

    if token_record.consumed_at.is_some() {
        sqlx::query("UPDATE auth_sessions SET revoked_at = ? WHERE id = ?")
            .bind(now)
            .bind(&token_record.session_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Err(SessionError::ReplayDetected);
    }

    if token_record.expires_at <= now {
        return Err(SessionError::InvalidToken);
    }

    let session = sqlx::query_as::<_, AuthSession>(
        "SELECT id, user_id, csrf_token, user_agent, ip_address, created_at, expires_at, revoked_at
         FROM auth_sessions WHERE id = ?",
    )
    .bind(&token_record.session_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(SessionError::InvalidSession)?;

    if session.revoked_at.is_some() || session.expires_at <= now {
        return Err(SessionError::InvalidSession);
    }

    let candidate_csrf = csrf_token.ok_or(SessionError::InvalidCsrf)?;
    if !constant_time_compare(candidate_csrf, &session.csrf_token) {
        return Err(SessionError::InvalidCsrf);
    }

    let new_raw_token = generate_random_token();
    let new_token_hash = hash_token(&new_raw_token);

    let update_res = sqlx::query(
        "UPDATE refresh_tokens SET consumed_at = ?, replaced_by_hash = ?
         WHERE token_hash = ? AND consumed_at IS NULL",
    )
    .bind(now)
    .bind(&new_token_hash)
    .bind(&token_hash)
    .execute(&mut *tx)
    .await?;

    if update_res.rows_affected() == 0 {
        sqlx::query("UPDATE auth_sessions SET revoked_at = ? WHERE id = ?")
            .bind(now)
            .bind(&token_record.session_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Err(SessionError::ReplayDetected);
    }

    sqlx::query(
        "INSERT INTO refresh_tokens (token_hash, session_id, created_at, expires_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&new_token_hash)
    .bind(&session.id)
    .bind(now)
    .bind(session.expires_at)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok((session, new_raw_token))
}

pub async fn find_active_session(
    pool: &SqlitePool,
    session_id: &str,
    now: i64,
) -> Result<Option<AuthSession>, SessionError> {
    let session = sqlx::query_as::<_, AuthSession>(
        "SELECT id, user_id, csrf_token, user_agent, ip_address, created_at, expires_at, revoked_at
         FROM auth_sessions
         WHERE id = ? AND revoked_at IS NULL AND expires_at > ?",
    )
    .bind(session_id)
    .bind(now)
    .fetch_optional(pool)
    .await?;

    Ok(session)
}

pub async fn revoke_session(
    pool: &SqlitePool,
    session_id: &str,
    now: i64,
) -> Result<(), SessionError> {
    sqlx::query("UPDATE auth_sessions SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL")
        .bind(now)
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn revoke_all_user_sessions(
    pool: &SqlitePool,
    user_id: i64,
    now: i64,
) -> Result<(), SessionError> {
    sqlx::query("UPDATE auth_sessions SET revoked_at = ? WHERE user_id = ? AND revoked_at IS NULL")
        .bind(now)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_user_sessions(
    pool: &SqlitePool,
    user_id: i64,
    now: i64,
) -> Result<Vec<AuthSession>, SessionError> {
    let sessions = sqlx::query_as::<_, AuthSession>(
        "SELECT id, user_id, csrf_token, user_agent, ip_address, created_at, expires_at, revoked_at
         FROM auth_sessions
         WHERE user_id = ? AND revoked_at IS NULL AND expires_at > ?
         ORDER BY created_at DESC",
    )
    .bind(user_id)
    .bind(now)
    .fetch_all(pool)
    .await?;

    Ok(sessions)
}

pub async fn revoke_user_session(
    pool: &SqlitePool,
    user_id: i64,
    session_id: &str,
    now: i64,
) -> Result<bool, SessionError> {
    let res = sqlx::query(
        "UPDATE auth_sessions SET revoked_at = ?
         WHERE id = ? AND user_id = ? AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(session_id)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(res.rows_affected() > 0)
}

pub async fn revoke_other_user_sessions(
    pool: &SqlitePool,
    user_id: i64,
    current_session_id: &str,
    now: i64,
) -> Result<(), SessionError> {
    sqlx::query(
        "UPDATE auth_sessions SET revoked_at = ?
         WHERE user_id = ? AND id != ? AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(user_id)
    .bind(current_session_id)
    .execute(pool)
    .await?;

    Ok(())
}
