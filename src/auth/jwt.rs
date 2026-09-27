use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::config::Config;

const MAX_TOKEN_BYTES: usize = 4096;
const ACCESS_TOKEN_TTL_SECS: u64 = 600;

#[derive(Debug, Error)]
pub enum JwtError {
    #[error("token exceeds maximum size of 4 KiB")]
    TooLarge,
    #[error("unknown or missing key ID")]
    UnknownKeyId,
    #[error("invalid token: {0}")]
    InvalidToken(#[from] jsonwebtoken::errors::Error),
    #[error("token claims invalid")]
    InvalidClaims,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessClaims {
    pub iss: String,
    pub aud: String,
    pub sub: String,
    pub sid: String,
    pub jti: String,
    pub exp: usize,
    pub iat: usize,
    pub nbf: usize,
}

pub fn create_access_token(
    config: &Config,
    user_id: i64,
    session_id: &str,
    session_expires_at: i64,
) -> Result<String, JwtError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let exp = (now + ACCESS_TOKEN_TTL_SECS).min(session_expires_at as u64) as usize;
    let claims = AccessClaims {
        iss: config.jwt_issuer.clone(),
        aud: config.jwt_audience.clone(),
        sub: user_id.to_string(),
        sid: session_id.to_string(),
        jti: Uuid::new_v4().to_string(),
        exp,
        iat: now as usize,
        nbf: now as usize,
    };

    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some(config.jwt_active_key_id.clone());
    header.typ = Some("JWT".to_string());

    let secret = config
        .jwt_keys
        .get(&config.jwt_active_key_id)
        .ok_or(JwtError::UnknownKeyId)?;

    let token = encode(&header, &claims, &EncodingKey::from_secret(secret))?;
    if token.len() > MAX_TOKEN_BYTES {
        return Err(JwtError::TooLarge);
    }
    Ok(token)
}

pub fn verify_access_token(config: &Config, token: &str) -> Result<AccessClaims, JwtError> {
    if token.len() > MAX_TOKEN_BYTES {
        return Err(JwtError::TooLarge);
    }

    let header = jsonwebtoken::decode_header(token)?;
    if header.alg != Algorithm::HS256 {
        return Err(JwtError::InvalidClaims);
    }

    let kid = header.kid.as_deref().unwrap_or(&config.jwt_active_key_id);
    let secret = config.jwt_keys.get(kid).ok_or(JwtError::UnknownKeyId)?;

    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[&config.jwt_issuer]);
    validation.set_audience(&[&config.jwt_audience]);
    validation.leeway = 30;
    validation.validate_exp = true;
    validation.validate_nbf = true;

    let data = decode::<AccessClaims>(token, &DecodingKey::from_secret(secret), &validation)?;
    Ok(data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_token_encoding_and_verification() {
        let config = Config::from_lookup(|_| Ok(None)).unwrap();
        let session_expires = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 3600;

        let token = create_access_token(&config, 42, "session-123", session_expires).unwrap();
        let claims = verify_access_token(&config, &token).unwrap();

        assert_eq!(claims.sub, "42");
        assert_eq!(claims.sid, "session-123");
        assert_eq!(claims.iss, "my-blog-api");
        assert_eq!(claims.aud, "my-blog");
    }

    #[test]
    fn invalid_key_or_tampered_token_is_rejected() {
        let config = Config::from_lookup(|_| Ok(None)).unwrap();
        let session_expires = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 3600;

        let token = create_access_token(&config, 42, "session-123", session_expires).unwrap();
        let tampered = format!("{token}extra");
        assert!(verify_access_token(&config, &tampered).is_err());
    }
}
