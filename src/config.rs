use std::{env, net::SocketAddr};

use thiserror::Error;

#[derive(Debug, Error)]
#[error("invalid configuration for {0}")]
pub struct ConfigError(&'static str);

pub struct Config {
    pub bind_address: SocketAddr,
    pub database_url: String,
    pub database_max_connections: u32,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| match env::var(key) {
            Ok(value) => Ok(Some(value)),
            Err(env::VarError::NotPresent) => Ok(None),
            Err(_) => Err(ConfigError(key)),
        })
    }

    fn from_lookup(
        lookup: impl Fn(&'static str) -> Result<Option<String>, ConfigError>,
    ) -> Result<Self, ConfigError> {
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
        Ok(Self {
            bind_address,
            database_url,
            database_max_connections,
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
    }

    #[test]
    fn invalid_configuration_is_rejected_without_echoing_values() {
        for (key, value) in [
            ("BIND_ADDRESS", "invalid"),
            ("DATABASE_URL", " "),
            ("DATABASE_MAX_CONNECTIONS", "0"),
            ("DATABASE_MAX_CONNECTIONS", "17"),
            ("DATABASE_MAX_CONNECTIONS", "secret-value"),
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
}
