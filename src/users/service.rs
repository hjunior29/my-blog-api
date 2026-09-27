use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::SqlitePool;
use thiserror::Error;

use super::{
    model::{NewUser, UserRole, UserStatus},
    repository,
};
use crate::auth::password::{self, PasswordError};

#[derive(Debug, Error)]
pub enum UserServiceError {
    #[error("invalid email address")]
    InvalidEmail,
    #[error("display name must be between 1 and 80 characters")]
    InvalidDisplayName,
    #[error("bio must be at most 500 characters")]
    InvalidBio,
    #[error("owner already exists")]
    OwnerAlreadyExists,
    #[error("email is already registered")]
    EmailAlreadyRegistered,
    #[error("user not found")]
    UserNotFound,
    #[error("cannot disable or demote the last active owner")]
    LastOwnerProtection,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("password error: {0}")]
    Password(#[from] PasswordError),
}

#[derive(Debug, PartialEq, Eq)]
pub enum SeedResult {
    Created(i64),
    AlreadyExists(i64),
}

pub fn normalize_email(email: &str) -> Result<String, UserServiceError> {
    let trimmed = email.trim();
    if trimmed.is_empty() || trimmed.len() > 254 || !trimmed.is_ascii() {
        return Err(UserServiceError::InvalidEmail);
    }
    let (local, domain) = trimmed
        .split_once('@')
        .ok_or(UserServiceError::InvalidEmail)?;
    if local.is_empty() || domain.is_empty() || !domain.contains('.') {
        return Err(UserServiceError::InvalidEmail);
    }
    Ok(trimmed.to_ascii_lowercase())
}

pub fn validate_display_name(name: &str) -> Result<(), UserServiceError> {
    let count = name.trim().chars().count();
    if !(1..=80).contains(&count) {
        return Err(UserServiceError::InvalidDisplayName);
    }
    Ok(())
}

pub fn validate_bio(bio: &str) -> Result<(), UserServiceError> {
    if bio.chars().count() > 500 {
        return Err(UserServiceError::InvalidBio);
    }
    Ok(())
}

pub fn current_unix_time() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub async fn seed_owner(
    pool: &SqlitePool,
    email: &str,
    display_name: &str,
    password: &str,
) -> Result<SeedResult, UserServiceError> {
    let normalized = normalize_email(email)?;
    validate_display_name(display_name)?;
    password::validate_password(password)?;

    if let Some(existing_user) = repository::find_by_normalized_email(pool, &normalized).await? {
        if existing_user.role == UserRole::Owner {
            return Ok(SeedResult::AlreadyExists(existing_user.id));
        }
        return Err(UserServiceError::OwnerAlreadyExists);
    }

    let active_owners = repository::count_active_owners(pool).await?;
    if active_owners > 0 {
        return Err(UserServiceError::OwnerAlreadyExists);
    }

    let password_hash = password::hash_password(password.to_string()).await?;
    let mut tx = pool.begin().await?;
    let now = current_unix_time();

    let active_owners_in_tx = repository::count_active_owners_conn(&mut tx).await?;
    if active_owners_in_tx > 0 {
        return match repository::find_by_normalized_email(pool, &normalized).await? {
            Some(existing) if existing.role == UserRole::Owner => {
                Ok(SeedResult::AlreadyExists(existing.id))
            }
            _ => Err(UserServiceError::OwnerAlreadyExists),
        };
    }

    let user_id = match repository::insert_user(
        &mut tx,
        &NewUser {
            email: email.trim(),
            normalized_email: &normalized,
            password_hash: &password_hash,
            display_name: display_name.trim(),
            bio: "",
            role: UserRole::Owner,
            status: UserStatus::Active,
            now,
        },
    )
    .await
    {
        Ok(id) => id,
        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
            drop(tx);
            return match repository::find_by_normalized_email(pool, &normalized).await? {
                Some(u) if u.role == UserRole::Owner => Ok(SeedResult::AlreadyExists(u.id)),
                _ => Err(UserServiceError::OwnerAlreadyExists),
            };
        }
        Err(e) => return Err(UserServiceError::Database(e)),
    };

    tx.commit().await?;
    Ok(SeedResult::Created(user_id))
}

pub async fn create_user(
    pool: &SqlitePool,
    email: &str,
    display_name: &str,
    password: &str,
    role: UserRole,
) -> Result<i64, UserServiceError> {
    let normalized = normalize_email(email)?;
    validate_display_name(display_name)?;
    password::validate_password(password)?;

    if repository::find_by_normalized_email(pool, &normalized)
        .await?
        .is_some()
    {
        return Err(UserServiceError::EmailAlreadyRegistered);
    }

    let password_hash = password::hash_password(password.to_string()).await?;
    let mut tx = pool.begin().await?;
    let now = current_unix_time();

    let user_id = match repository::insert_user(
        &mut tx,
        &NewUser {
            email: email.trim(),
            normalized_email: &normalized,
            password_hash: &password_hash,
            display_name: display_name.trim(),
            bio: "",
            role,
            status: UserStatus::Active,
            now,
        },
    )
    .await
    {
        Ok(id) => id,
        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
            return Err(UserServiceError::EmailAlreadyRegistered);
        }
        Err(e) => return Err(UserServiceError::Database(e)),
    };

    tx.commit().await?;
    Ok(user_id)
}

pub async fn disable_user(pool: &SqlitePool, user_id: i64) -> Result<(), UserServiceError> {
    let user = repository::find_by_id(pool, user_id)
        .await?
        .ok_or(UserServiceError::UserNotFound)?;

    let mut tx = pool.begin().await?;
    if user.role == UserRole::Owner && user.status == UserStatus::Active {
        let owners_count = repository::count_active_owners_conn(&mut tx).await?;
        if owners_count <= 1 {
            return Err(UserServiceError::LastOwnerProtection);
        }
    }

    let now = current_unix_time();
    repository::set_status(&mut tx, user_id, UserStatus::Inactive, now).await?;
    sqlx::query("UPDATE auth_sessions SET revoked_at = ? WHERE user_id = ? AND revoked_at IS NULL")
        .bind(now)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(())
}

pub async fn reset_password(
    pool: &SqlitePool,
    user_id: i64,
    new_password: &str,
) -> Result<(), UserServiceError> {
    let _ = repository::find_by_id(pool, user_id)
        .await?
        .ok_or(UserServiceError::UserNotFound)?;

    let password_hash = password::hash_password(new_password.to_string()).await?;
    let mut tx = pool.begin().await?;
    let now = current_unix_time();

    repository::update_password(&mut tx, user_id, &password_hash, now).await?;
    sqlx::query("UPDATE auth_sessions SET revoked_at = ? WHERE user_id = ? AND revoked_at IS NULL")
        .bind(now)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(())
}

pub async fn set_user_role(
    pool: &SqlitePool,
    user_id: i64,
    new_role: UserRole,
) -> Result<(), UserServiceError> {
    let user = repository::find_by_id(pool, user_id)
        .await?
        .ok_or(UserServiceError::UserNotFound)?;

    let mut tx = pool.begin().await?;
    if user.role == UserRole::Owner
        && new_role != UserRole::Owner
        && user.status == UserStatus::Active
    {
        let owners_count = repository::count_active_owners_conn(&mut tx).await?;
        if owners_count <= 1 {
            return Err(UserServiceError::LastOwnerProtection);
        }
    }

    let now = current_unix_time();
    repository::set_role(&mut tx, user_id, new_role, now).await?;
    tx.commit().await?;
    Ok(())
}
