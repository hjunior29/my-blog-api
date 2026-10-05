use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use axum::http::HeaderMap;

pub const LOGIN_RATE_LIMIT_WINDOW_SECS: i64 = 60;
pub const LOGIN_RATE_LIMIT_MAX_ATTEMPTS: usize = 10;
pub const LOGIN_RATE_LIMIT_MAX_TRACKED_IPS: usize = 5000;

#[derive(Clone, Default)]
pub struct LoginRateLimiter {
    attempts: Arc<Mutex<HashMap<String, Vec<i64>>>>,
}

impl LoginRateLimiter {
    pub fn new() -> Self {
        Self {
            attempts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn check_and_record(&self, ip: &str, now: i64) -> Result<(), i64> {
        let mut map = self.attempts.lock().map_err(|_| 60)?;

        if map.len() > LOGIN_RATE_LIMIT_MAX_TRACKED_IPS {
            map.retain(|_, timestamps| {
                timestamps.retain(|&ts| now - ts < LOGIN_RATE_LIMIT_WINDOW_SECS);
                !timestamps.is_empty()
            });
        }

        let timestamps = map.entry(ip.to_string()).or_default();
        timestamps.retain(|&ts| now - ts < LOGIN_RATE_LIMIT_WINDOW_SECS);

        if timestamps.len() >= LOGIN_RATE_LIMIT_MAX_ATTEMPTS {
            let oldest = timestamps[0];
            let retry_after = (LOGIN_RATE_LIMIT_WINDOW_SECS - (now - oldest)).max(1);
            return Err(retry_after);
        }

        timestamps.push(now);
        Ok(())
    }
}

pub struct ClientIp(pub Option<std::net::SocketAddr>);

impl<S> axum::extract::FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let addr = parts
            .extensions
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|ci| ci.0);
        Ok(ClientIp(addr))
    }
}

pub fn extract_client_ip(headers: &HeaderMap, peer_addr: Option<std::net::SocketAddr>) -> String {
    if let Some(fly_ip) = headers.get("fly-client-ip").and_then(|v| v.to_str().ok()) {
        let trimmed = fly_ip.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(cf_ip) = headers.get("cf-connecting-ip").and_then(|v| v.to_str().ok()) {
        let trimmed = cf_ip.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(real_ip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        let trimmed = real_ip.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        && let Some(last) = forwarded.rsplit(',').next()
    {
        let trimmed = last.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(addr) = peer_addr {
        return addr.ip().to_string();
    }
    "127.0.0.1".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn allows_up_to_max_attempts_and_enforces_retry_after() {
        let limiter = LoginRateLimiter::new();
        let ip = "192.0.2.1";
        let now = 1000;

        for _ in 0..10 {
            assert!(limiter.check_and_record(ip, now).is_ok());
        }

        let err = limiter.check_and_record(ip, now);
        assert_eq!(err, Err(60));

        let err_later = limiter.check_and_record(ip, now + 10);
        assert_eq!(err_later, Err(50));

        let ok_after_window = limiter.check_and_record(ip, now + 61);
        assert!(ok_after_window.is_ok());
    }

    #[test]
    fn isolates_different_ips() {
        let limiter = LoginRateLimiter::new();
        let ip1 = "198.51.100.1";
        let ip2 = "198.51.100.2";
        let now = 2000;

        for _ in 0..10 {
            assert!(limiter.check_and_record(ip1, now).is_ok());
        }
        assert!(limiter.check_and_record(ip1, now).is_err());
        assert!(limiter.check_and_record(ip2, now).is_ok());
    }

    #[test]
    fn extracts_ip_from_various_headers() {
        let mut headers = HeaderMap::new();
        assert_eq!(extract_client_ip(&headers, None), "127.0.0.1");

        let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 88)), 4321);
        assert_eq!(extract_client_ip(&headers, Some(peer)), "192.0.2.88");

        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("spoofed_attacker_ip, 203.0.113.195"),
        );
        assert_eq!(extract_client_ip(&headers, None), "203.0.113.195");

        let mut real_headers = HeaderMap::new();
        real_headers.insert("x-real-ip", HeaderValue::from_static("198.51.100.42"));
        assert_eq!(extract_client_ip(&real_headers, None), "198.51.100.42");

        let mut fly_headers = HeaderMap::new();
        fly_headers.insert("fly-client-ip", HeaderValue::from_static("198.51.100.77"));
        assert_eq!(extract_client_ip(&fly_headers, None), "198.51.100.77");

        let mut cf_headers = HeaderMap::new();
        cf_headers.insert("cf-connecting-ip", HeaderValue::from_static("198.51.100.99"));
        assert_eq!(extract_client_ip(&cf_headers, None), "198.51.100.99");
    }
}
