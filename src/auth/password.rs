use argon2::{
    Argon2, Params,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use thiserror::Error;
use tokio::sync::{Semaphore, SemaphorePermit};

const MEMORY_COST_KIB: u32 = 19456;
const TIME_COST: u32 = 2;
const PARALLELISM: u32 = 1;
const MIN_PASSWORD_CHARS: usize = 15;
const MAX_PASSWORD_CHARS: usize = 128;
const MAX_PASSWORD_BYTES: usize = 512;
const MAX_HASHING_CONCURRENCY: usize = 2;
const MAX_QUEUE_DEPTH: usize = 16;
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

static HASH_SEMAPHORE: Semaphore = Semaphore::const_new(MAX_HASHING_CONCURRENCY);
static ACTIVE_AND_QUEUED: AtomicUsize = AtomicUsize::new(0);

struct HashSlot {
    _permit: SemaphorePermit<'static>,
}

impl Drop for HashSlot {
    fn drop(&mut self) {
        ACTIVE_AND_QUEUED.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn acquire_hash_slot() -> Result<HashSlot, PasswordError> {
    let current = ACTIVE_AND_QUEUED.fetch_add(1, Ordering::SeqCst);
    if current >= MAX_HASHING_CONCURRENCY + MAX_QUEUE_DEPTH {
        ACTIVE_AND_QUEUED.fetch_sub(1, Ordering::SeqCst);
        return Err(PasswordError::Busy);
    }

    match tokio::time::timeout(ACQUIRE_TIMEOUT, HASH_SEMAPHORE.acquire()).await {
        Ok(Ok(permit)) => Ok(HashSlot { _permit: permit }),
        Ok(Err(_)) => {
            ACTIVE_AND_QUEUED.fetch_sub(1, Ordering::SeqCst);
            Err(PasswordError::HashingFailed)
        }
        Err(_) => {
            ACTIVE_AND_QUEUED.fetch_sub(1, Ordering::SeqCst);
            Err(PasswordError::Busy)
        }
    }
}

pub const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$YWJjZGVmZ2hpamtsbW5vcA$e6lJ4q9Z0+wI8Hq3fXjP3zU0y0mH9aW9j6iN3wQ7r1s";

#[derive(Debug, Error)]
pub enum PasswordError {
    #[error("password must be between 15 and 128 characters and at most 512 bytes")]
    InvalidPolicy,
    #[error("hashing service is busy")]
    Busy,
    #[error("failed to hash password")]
    HashingFailed,
    #[error("failed to verify password")]
    VerificationFailed,
}

pub fn validate_password(password: &str) -> Result<(), PasswordError> {
    let char_count = password.chars().count();
    if !(MIN_PASSWORD_CHARS..=MAX_PASSWORD_CHARS).contains(&char_count)
        || password.len() > MAX_PASSWORD_BYTES
    {
        return Err(PasswordError::InvalidPolicy);
    }
    Ok(())
}

fn create_argon2<'a>() -> Result<Argon2<'a>, PasswordError> {
    let params = Params::new(MEMORY_COST_KIB, TIME_COST, PARALLELISM, None)
        .map_err(|_| PasswordError::HashingFailed)?;
    Ok(Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        params,
    ))
}

pub async fn hash_password(password: String) -> Result<String, PasswordError> {
    validate_password(&password)?;
    let slot = acquire_hash_slot().await?;

    tokio::task::spawn_blocking(move || {
        let _slot = slot;
        let argon2 = create_argon2()?;
        let password_hash = argon2
            .hash_password(password.as_bytes())
            .map_err(|_| PasswordError::HashingFailed)?;
        Ok(password_hash.to_string())
    })
    .await
    .map_err(|_| PasswordError::HashingFailed)?
}

pub async fn verify_password(password: String, hash: String) -> Result<bool, PasswordError> {
    let slot = acquire_hash_slot().await?;

    tokio::task::spawn_blocking(move || {
        let _slot = slot;
        let parsed_hash =
            PasswordHash::new(&hash).map_err(|_| PasswordError::VerificationFailed)?;
        let argon2 = Argon2::default();
        Ok(argon2
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok())
    })
    .await
    .map_err(|_| PasswordError::VerificationFailed)?
}

pub async fn dummy_verify(password: String) -> Result<bool, PasswordError> {
    let _ = verify_password(password, DUMMY_HASH.to_string()).await?;
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_enforces_length_and_byte_bounds() {
        assert!(validate_password("short").is_err());
        assert!(validate_password(&"a".repeat(14)).is_err());
        assert!(validate_password(&"a".repeat(15)).is_ok());
        assert!(validate_password(&"a".repeat(128)).is_ok());
        assert!(validate_password(&"a".repeat(129)).is_err());
        assert!(validate_password("valid password with spaces 12345").is_ok());
        assert!(validate_password("senha válida com acentuação e unicode 🚀").is_ok());
    }

    #[tokio::test]
    async fn hash_and_verify_cycle_succeeds() {
        let secret = "correct horse battery staple 2026".to_string();
        let hash = hash_password(secret.clone()).await.unwrap();
        assert!(hash.starts_with("$argon2id$"));
        let valid = verify_password(secret, hash.clone()).await.unwrap();
        assert!(valid);
        let invalid = verify_password("wrong password long enough 12345".to_string(), hash)
            .await
            .unwrap();
        assert!(!invalid);
    }
}
