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
    pub owner_only: bool,
    pub jwt_issuer: String,
    pub jwt_audience: String,
    pub jwt_active_key_id: String,
    pub jwt_keys: Arc<HashMap<String, Vec<u8>>>,
    pub media_root: PathBuf,
    pub s3_endpoint: String,
    pub s3_bucket: Option<String>,
    pub s3_access_key: Option<String>,
    pub s3_secret_key: Option<String>,
    pub s3_region: String,
    pub s3_public_url_prefix: Option<String>,
    pub smtp_host: Option<String>,
    pub smtp_port: u16,
    pub smtp_username: Option<String>,
    pub smtp_password: Option<String>,
    pub smtp_from_email: String,
    pub smtp_from_name: String,
    pub two_factor_enabled: bool,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut map = HashMap::new();
        if let Ok(content) = fs::read_to_string(".env") {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = trimmed.split_once('=') {
                    let key = k.trim().to_string();
                    let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
                    map.insert(key, val);
                }
            }
        }
        Self::from_lookup(move |key| match env::var(key) {
            Ok(value) => Ok(Some(value)),
            Err(env::VarError::NotPresent) => Ok(map.get(key).cloned()),
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

        let app_origin = lookup("APP_ORIGIN")?.unwrap_or_else(|| "http://localhost:5173".into());
        if !app_origin.starts_with("http://") && !app_origin.starts_with("https://") {
            return Err(ConfigError("APP_ORIGIN"));
        }
        let app_origin = app_origin.trim_end_matches('/').to_string();

        let owner_only = match lookup("OWNER_ONLY")?.as_deref() {
            None | Some("true") => true,
            Some("false") => false,
            _ => return Err(ConfigError("OWNER_ONLY")),
        };

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

        let s3_endpoint = lookup("AWS_ENDPOINT_URL_S3")?
            .unwrap_or_else(|| "https://fly.storage.tigris.dev".into());
        let s3_bucket = lookup("BUCKET_NAME")?.or(lookup("TIGRIS_BUCKET")?);
        let s3_access_key = lookup("AWS_ACCESS_KEY_ID")?;
        let s3_secret_key = lookup("AWS_SECRET_ACCESS_KEY")?;
        let s3_region = lookup("AWS_REGION")?.unwrap_or_else(|| "auto".into());
        let s3_public_url_prefix = lookup("S3_PUBLIC_URL_PREFIX")?;

        let smtp_host = lookup("SMTP_HOST")?;
        let smtp_port = lookup("SMTP_PORT")?
            .unwrap_or_else(|| "587".into())
            .parse::<u16>()
            .map_err(|_| ConfigError("SMTP_PORT"))?;
        let smtp_username = lookup("SMTP_USERNAME")?;
        let smtp_password = lookup("SMTP_PASSWORD")?;
        let smtp_from_email = lookup("SMTP_FROM_EMAIL")?
            .unwrap_or_else(|| "noreply@vertices.me".into());
        let smtp_from_name = lookup("SMTP_FROM_NAME")?
            .unwrap_or_else(|| "Helder / Blog".into());
        let two_factor_enabled = match lookup("TWO_FACTOR_ENABLED")?.as_deref() {
            Some("true") | Some("1") => true,
            Some("false") | Some("0") => false,
            _ => env != AppEnv::Test,
        };

        Ok(Self {
            env,
            bind_address,
            database_url,
            database_max_connections,
            app_origin,
            secure_cookies,
            owner_only,
            jwt_issuer,
            jwt_audience,
            jwt_active_key_id,
            jwt_keys: Arc::new(jwt_keys),
            media_root,
            s3_endpoint,
            s3_bucket,
            s3_access_key,
            s3_secret_key,
            s3_region,
            s3_public_url_prefix,
            smtp_host,
            smtp_port,
            smtp_username,
            smtp_password,
            smtp_from_email,
            smtp_from_name,
            two_factor_enabled,
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
        assert_eq!(config.app_origin, "http://localhost:5173");
        assert!(!config.secure_cookies);
        assert_eq!(config.jwt_active_key_id, "default");
        assert!(config.jwt_keys.contains_key("default"));
        assert!(config.two_factor_enabled);
        assert_eq!(config.smtp_port, 587);
        assert_eq!(config.smtp_from_email, "noreply@vertices.me");
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
            ("SMTP_PORT", "not-a-port"),
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
