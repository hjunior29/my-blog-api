use std::fmt::Write;
use rand::RngExt;
use sha2::{Digest, Sha256};
use sqlx::{SqliteConnection, sqlite::SqliteRow, Row};
use subtle::ConstantTimeEq;
use thiserror::Error;

pub const CHALLENGE_EXPIRY_SECS: i64 = 300;
pub const MAX_ATTEMPTS: i64 = 5;
pub const RESEND_COOLDOWN_SECS: i64 = 30;
pub const MAX_RESENDS: i64 = 3;

#[derive(Debug, Error)]
pub enum TwoFactorError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TwoFactorVerifyError {
    #[error("challenge not found or already used")]
    InvalidChallenge,
    #[error("challenge expired")]
    Expired,
    #[error("maximum verification attempts exceeded")]
    TooManyAttempts,
    #[error("invalid verification code; {remaining_attempts} attempts remaining")]
    InvalidCode { remaining_attempts: i64 },
    #[error("database error")]
    Database,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TwoFactorResendError {
    #[error("challenge not found or already used")]
    InvalidChallenge,
    #[error("resend cooldown active; retry in {retry_after} seconds")]
    CooldownActive { retry_after: i64 },
    #[error("maximum resends exceeded")]
    MaxResendsExceeded,
    #[error("database error")]
    Database,
}

pub fn generate_otp() -> String {
    let mut rng = rand::rng();
    let code: u32 = rng.random_range(100_000..=999_999);
    format!("{code:06}")
}

pub fn hash_code(code: &str) -> String {
    let hash = Sha256::digest(code.as_bytes());
    let mut hex = String::with_capacity(64);
    for b in hash {
        let _ = write!(&mut hex, "{:02x}", b);
    }
    hex
}

pub fn mask_email(email: &str) -> String {
    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() != 2 {
        return "***".to_string();
    }
    let username = parts[0];
    let domain = parts[1];
    let masked_user = if username.len() <= 2 {
        format!("{}***", &username[..1])
    } else {
        let first = &username[..1];
        let last = &username[username.len() - 1..];
        format!("{first}***{last}")
    };
    format!("{masked_user}@{domain}")
}

#[derive(Debug, Clone)]
pub struct TwoFactorVerifyResult {
    pub user_id: i64,
    pub remember_me: bool,
    pub created_at: i64,
}

pub async fn create_challenge(
    conn: &mut SqliteConnection,
    user_id: i64,
    remember_me: bool,
    now: i64,
) -> Result<(String, String), TwoFactorError> {
    sqlx::query("UPDATE auth_two_factor_challenges SET consumed_at = ? WHERE user_id = ? AND consumed_at IS NULL")
        .bind(now)
        .bind(user_id)
        .execute(&mut *conn)
        .await?;

    let challenge_token = uuid::Uuid::new_v4().to_string();
    let code = generate_otp();
    let code_hash = hash_code(&code);
    let expires_at = now + CHALLENGE_EXPIRY_SECS;

    sqlx::query(
        "INSERT INTO auth_two_factor_challenges (
            id, user_id, code_hash, attempts, max_attempts, resend_count, last_resend_at, created_at, expires_at, consumed_at, remember_me
        ) VALUES (?, ?, ?, 0, ?, 0, ?, ?, ?, NULL, ?)",
    )
    .bind(&challenge_token)
    .bind(user_id)
    .bind(&code_hash)
    .bind(MAX_ATTEMPTS)
    .bind(now)
    .bind(now)
    .bind(expires_at)
    .bind(if remember_me { 1 } else { 0 })
    .execute(&mut *conn)
    .await?;

    Ok((challenge_token, code))
}

pub async fn verify_challenge(
    conn: &mut SqliteConnection,
    challenge_token: &str,
    submitted_code: &str,
    now: i64,
) -> Result<TwoFactorVerifyResult, TwoFactorVerifyError> {
    let row: Option<SqliteRow> = sqlx::query(
        "SELECT user_id, code_hash, attempts, max_attempts, expires_at, consumed_at, created_at, remember_me
         FROM auth_two_factor_challenges WHERE id = ?",
    )
    .bind(challenge_token)
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| TwoFactorVerifyError::Database)?;

    let row = match row {
        Some(r) => r,
        None => return Err(TwoFactorVerifyError::InvalidChallenge),
    };

    let consumed_at: Option<i64> = row.get("consumed_at");
    if consumed_at.is_some() {
        return Err(TwoFactorVerifyError::InvalidChallenge);
    }

    let expires_at: i64 = row.get("expires_at");
    if now > expires_at {
        return Err(TwoFactorVerifyError::Expired);
    }

    let attempts: i64 = row.get("attempts");
    let max_attempts: i64 = row.get("max_attempts");
    if attempts >= max_attempts {
        return Err(TwoFactorVerifyError::TooManyAttempts);
    }

    let expected_hash: String = row.get("code_hash");
    let submitted_hash = hash_code(submitted_code.trim());

    let is_match: bool = submitted_hash
        .as_bytes()
        .ct_eq(expected_hash.as_bytes())
        .into();

    if !is_match {
        let update_res = sqlx::query(
            "UPDATE auth_two_factor_challenges 
             SET attempts = attempts + 1 
             WHERE id = ? AND consumed_at IS NULL AND attempts < max_attempts",
        )
        .bind(challenge_token)
        .execute(&mut *conn)
        .await
        .map_err(|_| TwoFactorVerifyError::Database)?;

        if update_res.rows_affected() == 0 {
            return Err(TwoFactorVerifyError::TooManyAttempts);
        }

        let current_attempts: i64 = sqlx::query_scalar(
            "SELECT attempts FROM auth_two_factor_challenges WHERE id = ?",
        )
        .bind(challenge_token)
        .fetch_one(&mut *conn)
        .await
        .unwrap_or(attempts + 1);

        if current_attempts >= max_attempts {
            return Err(TwoFactorVerifyError::TooManyAttempts);
        }

        let remaining = (max_attempts - current_attempts).max(0);
        return Err(TwoFactorVerifyError::InvalidCode {
            remaining_attempts: remaining,
        });
    }

    let consume_res = sqlx::query(
        "UPDATE auth_two_factor_challenges 
         SET consumed_at = ? 
         WHERE id = ? AND consumed_at IS NULL AND attempts < max_attempts AND expires_at >= ?",
    )
    .bind(now)
    .bind(challenge_token)
    .bind(now)
    .execute(&mut *conn)
    .await
    .map_err(|_| TwoFactorVerifyError::Database)?;

    if consume_res.rows_affected() == 0 {
        return Err(TwoFactorVerifyError::InvalidChallenge);
    }

    let user_id: i64 = row.get("user_id");
    let created_at: i64 = row.get("created_at");
    let remember_me: bool = row.try_get::<i64, _>("remember_me").map(|v| v != 0).unwrap_or(false);

    Ok(TwoFactorVerifyResult {
        user_id,
        remember_me,
        created_at,
    })
}

pub async fn resend_challenge(
    conn: &mut SqliteConnection,
    challenge_token: &str,
    now: i64,
) -> Result<(String, String, i64), TwoFactorResendError> {
    let row: Option<SqliteRow> = sqlx::query(
        "SELECT user_id, resend_count, last_resend_at, consumed_at
         FROM auth_two_factor_challenges WHERE id = ?",
    )
    .bind(challenge_token)
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| TwoFactorResendError::Database)?;

    let row = match row {
        Some(r) => r,
        None => return Err(TwoFactorResendError::InvalidChallenge),
    };

    let consumed_at: Option<i64> = row.get("consumed_at");
    if consumed_at.is_some() {
        return Err(TwoFactorResendError::InvalidChallenge);
    }

    let last_resend_at: i64 = row.get("last_resend_at");
    let elapsed = now - last_resend_at;
    if elapsed < RESEND_COOLDOWN_SECS {
        return Err(TwoFactorResendError::CooldownActive {
            retry_after: RESEND_COOLDOWN_SECS - elapsed,
        });
    }

    let resend_count: i64 = row.get("resend_count");
    if resend_count >= MAX_RESENDS {
        return Err(TwoFactorResendError::MaxResendsExceeded);
    }

    let user_id: i64 = row.get("user_id");
    let new_code = generate_otp();
    let new_code_hash = hash_code(&new_code);
    let new_expires_at = now + CHALLENGE_EXPIRY_SECS;

    let update_res = sqlx::query(
        "UPDATE auth_two_factor_challenges
         SET code_hash = ?, resend_count = resend_count + 1, last_resend_at = ?, attempts = 0, expires_at = ?
         WHERE id = ? AND consumed_at IS NULL AND resend_count < ? AND ? - last_resend_at >= ?",
    )
    .bind(&new_code_hash)
    .bind(now)
    .bind(new_expires_at)
    .bind(challenge_token)
    .bind(MAX_RESENDS)
    .bind(now)
    .bind(RESEND_COOLDOWN_SECS)
    .execute(&mut *conn)
    .await
    .map_err(|_| TwoFactorResendError::Database)?;

    if update_res.rows_affected() == 0 {
        return Err(TwoFactorResendError::InvalidChallenge);
    }

    Ok((challenge_token.to_string(), new_code, user_id))
}
