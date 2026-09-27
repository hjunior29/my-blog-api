use std::{collections::HashMap, env, fs, net::SocketAddr, path::PathBuf, sync::Arc};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEnv {
    Development,
    Test,
    Production,
}

#[derive(Debug, Error)]
#[error("invalid configuration for {0}")]
pub struct ConfigError(&'static str);

#[derive(Clone)]
pub struct Config {
    pub env: AppEnv,
    pub bind_address: SocketAddr,
    pub database_url: String,
    pub database_max_connections: u32,
    pub app_origin: String,
    pub secure_cookies: bool,
    pub jwt_issuer: String,
    pub jwt_audience: String,
    pub jwt_active_key_id: String,
    pub jwt_keys: Arc<HashMap<String, Vec<u8>>>,
    pub media_root: PathBuf,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| match env::var(key) {
            Ok(value) => Ok(Some(value)),
            Err(env::VarError::NotPresent) => Ok(None),
            Err(_) => Err(ConfigError(key)),
        })
    }

    pub fn from_lookup(
        lookup: impl Fn(&'static str) -> Result<Option<String>, ConfigError>,
    ) -> Result<Self, ConfigError> {
        let env = match lookup("APP_ENV")?.as_deref() {
            Some("production") => AppEnv::Production,
            Some("test") => AppEnv::Test,
            Some("development") | None => AppEnv::Development,
            _ => return Err(ConfigError("APP_ENV")),
        };

        let bind_address = lookup("BIND_ADDRESS")?
            .unwrap_or_else(|| "127.0.0.1:3000".into())
            .parse()
            .map_err(|_| ConfigError("BIND_ADDRESS"))?;

        let database_url = lookup("DATABASE_URL")?.unwrap_or_else(|| "sqlite://blog.db".into());
        if database_url.trim().is_empty() {
            return Err(ConfigError("DATABASE_URL"));
        }

        let database_max_connections = lookup("DATABASE_MAX_CONNECTIONS")?
            .unwrap_or_else(|| "4".into())
            .parse::<u32>()
            .map_err(|_| ConfigError("DATABASE_MAX_CONNECTIONS"))?;
        if !(1..=16).contains(&database_max_connections) {
            return Err(ConfigError("DATABASE_MAX_CONNECTIONS"));
        }

        let app_origin = lookup("APP_ORIGIN")?.unwrap_or_else(|| "http://localhost:3000".into());
        if !app_origin.starts_with("http://") && !app_origin.starts_with("https://") {
            return Err(ConfigError("APP_ORIGIN"));
        }
        let app_origin = app_origin.trim_end_matches('/').to_string();

        let secure_cookies = match lookup("SECURE_COOKIES")?.as_deref() {
            Some("true") | Some("1") => true,
            Some("false") | Some("0") => {
                if env == AppEnv::Production {
                    return Err(ConfigError("SECURE_COOKIES"));
                }
                false
            }
            None => env == AppEnv::Production,
            _ => return Err(ConfigError("SECURE_COOKIES")),
        };

        let jwt_issuer = lookup("JWT_ISSUER")?.unwrap_or_else(|| "my-blog-api".into());
        let jwt_audience = lookup("JWT_AUDIENCE")?.unwrap_or_else(|| "my-blog".into());
        let jwt_active_key_id = lookup("JWT_ACTIVE_KEY_ID")?.unwrap_or_else(|| "default".into());

        let mut jwt_keys = HashMap::new();
        if let Some(keys_path) = lookup("JWT_KEYS_FILE")? {
            let content =
                fs::read_to_string(keys_path).map_err(|_| ConfigError("JWT_KEYS_FILE"))?;
            let parsed: HashMap<String, String> =
                serde_json::from_str(&content).map_err(|_| ConfigError("JWT_KEYS_FILE"))?;
            for (key_id, secret) in parsed {
                let bytes = secret.into_bytes();
                if bytes.len() < 32 {
                    return Err(ConfigError("JWT_KEYS_FILE"));
                }
                jwt_keys.insert(key_id, bytes);
            }
            if !jwt_keys.contains_key(&jwt_active_key_id) {
                return Err(ConfigError("JWT_ACTIVE_KEY_ID"));
            }
        } else if let Some(secret) = lookup("JWT_SECRET")? {
            let bytes = secret.into_bytes();
            if bytes.len() < 32 {
                return Err(ConfigError("JWT_SECRET"));
            }
            jwt_keys.insert(jwt_active_key_id.clone(), bytes);
        } else if env == AppEnv::Production {
            return Err(ConfigError("JWT_KEYS_FILE"));
        } else {
            jwt_keys.insert(
                jwt_active_key_id.clone(),
                b"development-jwt-secret-key-must-be-32-bytes-long!".to_vec(),
            );
        }

        let media_root_str = lookup("MEDIA_ROOT")?.unwrap_or_else(|| "./uploads".into());
        if media_root_str.trim().is_empty() {
            return Err(ConfigError("MEDIA_ROOT"));
        }
        let media_root = PathBuf::from(media_root_str);

        Ok(Self {
            env,
            bind_address,
            database_url,
            database_max_connections,
            app_origin,
            secure_cookies,
            jwt_issuer,
            jwt_audience,
            jwt_active_key_id,
            jwt_keys: Arc::new(jwt_keys),
            media_root,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_local_and_bounded() {
        let config = Config::from_lookup(|_| Ok(None)).unwrap();
        assert!(config.bind_address.ip().is_loopback());
        assert_eq!(config.database_max_connections, 4);
        assert_eq!(config.database_url, "sqlite://blog.db");
        assert_eq!(config.app_origin, "http://localhost:3000");
        assert!(!config.secure_cookies);
        assert_eq!(config.jwt_active_key_id, "default");
        assert!(config.jwt_keys.contains_key("default"));
    }

    #[test]
    fn invalid_configuration_is_rejected_without_echoing_values() {
        for (key, value) in [
            ("BIND_ADDRESS", "invalid"),
            ("DATABASE_URL", " "),
            ("DATABASE_MAX_CONNECTIONS", "0"),
            ("DATABASE_MAX_CONNECTIONS", "17"),
            ("DATABASE_MAX_CONNECTIONS", "secret-value"),
            ("APP_ENV", "staging"),
            ("APP_ORIGIN", "ftp://invalid"),
            ("MEDIA_ROOT", " "),
            ("JWT_SECRET", "too-short"),
        ] {
            let error = Config::from_lookup(|name| Ok((name == key).then(|| value.to_owned())))
                .err()
                .unwrap();
            assert_eq!(
                error.to_string(),
                format!("invalid configuration for {key}")
            );
        }
    }

    #[test]
    fn production_requires_secure_cookies_and_keys() {
        let error = Config::from_lookup(|name| match name {
            "APP_ENV" => Ok(Some("production".into())),
            "SECURE_COOKIES" => Ok(Some("false".into())),
            _ => Ok(None),
        })
        .err()
        .unwrap();
        assert_eq!(
            error.to_string(),
            "invalid configuration for SECURE_COOKIES"
        );

        let error = Config::from_lookup(|name| match name {
            "APP_ENV" => Ok(Some("production".into())),
            _ => Ok(None),
        })
        .err()
        .unwrap();
        assert_eq!(error.to_string(), "invalid configuration for JWT_KEYS_FILE");
    }
}
