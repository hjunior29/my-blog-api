use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use axum::http::HeaderMap;

pub const LOGIN_RATE_LIMIT_WINDOW_SECS: i64 = 60;
pub const LOGIN_RATE_LIMIT_MAX_ATTEMPTS: usize = 10;
pub const LOGIN_RATE_LIMIT_MAX_TRACKED_IPS: usize = 5000;

pub const ACCOUNT_LOCKOUT_WINDOW_SECS: i64 = 900;
pub const ACCOUNT_LOCKOUT_MAX_FAILURES: usize = 5;
pub const ACCOUNT_LOCKOUT_DURATION_SECS: i64 = 900;
pub const ACCOUNT_LOCKOUT_MAX_TRACKED: usize = 1000;

#[derive(Default)]
struct AccountLockState {
    failures: Vec<i64>,
    locked_until: Option<i64>,
}

#[derive(Clone, Default)]
pub struct LoginRateLimiter {
    attempts: Arc<Mutex<HashMap<String, Vec<i64>>>>,
    account_lockouts: Arc<Mutex<HashMap<String, AccountLockState>>>,
    active_hashes: Arc<Mutex<HashSet<String>>>,
}

pub struct HashConcurrencyGuard {
    limiter: LoginRateLimiter,
    ip: String,
}

impl Drop for HashConcurrencyGuard {
    fn drop(&mut self) {
        self.limiter.release_active_hash(&self.ip);
    }
}

impl LoginRateLimiter {
    pub fn new() -> Self {
        Self {
            attempts: Arc::new(Mutex::new(HashMap::new())),
            account_lockouts: Arc::new(Mutex::new(HashMap::new())),
            active_hashes: Arc::new(Mutex::new(HashSet::new())),
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

    pub fn check_account_lockout(&self, email: &str, now: i64) -> Result<(), i64> {
        let mut map = self.account_lockouts.lock().map_err(|_| 60)?;
        if let Some(state) = map.get_mut(email) {
            if let Some(until) = state.locked_until {
                if until > now {
                    return Err((until - now).max(1));
                }
                state.locked_until = None;
            }
            state.failures.retain(|&ts| now - ts < ACCOUNT_LOCKOUT_WINDOW_SECS);
            if state.failures.len() >= ACCOUNT_LOCKOUT_MAX_FAILURES {
                state.locked_until = Some(now + ACCOUNT_LOCKOUT_DURATION_SECS);
                return Err(ACCOUNT_LOCKOUT_DURATION_SECS);
            }
        }
        Ok(())
    }

    pub fn record_account_failure(&self, email: &str, now: i64) -> usize {
        if let Ok(mut map) = self.account_lockouts.lock() {
            if map.len() > ACCOUNT_LOCKOUT_MAX_TRACKED {
                map.retain(|_, state| {
                    state.failures.retain(|&ts| now - ts < ACCOUNT_LOCKOUT_WINDOW_SECS);
                    !state.failures.is_empty() || state.locked_until.map(|u| u > now).unwrap_or(false)
                });
            }
            let state = map.entry(email.to_string()).or_default();
            state.failures.retain(|&ts| now - ts < ACCOUNT_LOCKOUT_WINDOW_SECS);
            state.failures.push(now);
            state.failures.len()
        } else {
            1
        }
    }

    pub fn record_account_success(&self, email: &str) {
        if let Ok(mut map) = self.account_lockouts.lock() {
            map.remove(email);
        }
    }

    pub fn acquire_active_hash(&self, ip: &str) -> Result<HashConcurrencyGuard, i64> {
        let mut set = self.active_hashes.lock().map_err(|_| 1)?;
        if !set.insert(ip.to_string()) {
            return Err(1);
        }
        Ok(HashConcurrencyGuard {
            limiter: self.clone(),
            ip: ip.to_string(),
        })
    }

    pub fn release_active_hash(&self, ip: &str) {
        if let Ok(mut set) = self.active_hashes.lock() {
            set.remove(ip);
        }
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

fn is_trusted_peer(addr: std::net::IpAddr) -> bool {
    match addr {
        std::net::IpAddr::V4(ipv4) => {
            ipv4.is_loopback() || ipv4.is_private() || ipv4.is_link_local()
        }
        std::net::IpAddr::V6(ipv6) => {
            if ipv6.is_loopback() || ipv6.is_unicast_link_local() {
                return true;
            }
            (ipv6.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

pub fn extract_client_ip(headers: &HeaderMap, peer_addr: Option<std::net::SocketAddr>) -> String {
    if let Some(peer) = peer_addr {
        if !is_trusted_peer(peer.ip()) {
            return peer.ip().to_string();
        }
    }

    if let Some(cf_ip) = headers.get("cf-connecting-ip").and_then(|v| v.to_str().ok()) {
        let trimmed = cf_ip.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(fly_ip) = headers.get("fly-client-ip").and_then(|v| v.to_str().ok()) {
        let trimmed = fly_ip.trim();
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
    fn account_lockout_enforces_threshold_and_resets_on_success() {
        let limiter = LoginRateLimiter::new();
        let email = "owner@example.com";
        let now = 5000;

        for _ in 0..4 {
            limiter.record_account_failure(email, now);
            assert!(limiter.check_account_lockout(email, now).is_ok());
        }

        limiter.record_account_failure(email, now);
        assert_eq!(limiter.check_account_lockout(email, now), Err(900));

        limiter.record_account_success(email);
        assert!(limiter.check_account_lockout(email, now).is_ok());
    }

    #[test]
    fn limits_concurrent_active_hashes_per_ip() {
        let limiter = LoginRateLimiter::new();
        let ip = "203.0.113.10";

        let guard = limiter.acquire_active_hash(ip).unwrap();
        assert_eq!(limiter.acquire_active_hash(ip).err(), Some(1));
        drop(guard);
        assert!(limiter.acquire_active_hash(ip).is_ok());
    }

    #[test]
    fn untrusted_peer_ignores_forwarding_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("1.1.1.1"));
        headers.insert("fly-client-ip", HeaderValue::from_static("2.2.2.2"));

        let untrusted = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 88)), 54321);
        assert_eq!(extract_client_ip(&headers, Some(untrusted)), "203.0.113.88");
    }

    #[test]
    fn trusted_peer_reads_forwarding_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("fly-client-ip", HeaderValue::from_static("198.51.100.77"));

        let trusted = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        assert_eq!(extract_client_ip(&headers, Some(trusted)), "198.51.100.77");
    }

    #[test]
    fn cloudflare_ip_takes_precedence_over_fly_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("cf-connecting-ip", HeaderValue::from_static("203.0.113.1"));
        headers.insert("fly-client-ip", HeaderValue::from_static("198.51.100.2"));

        let trusted = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        assert_eq!(extract_client_ip(&headers, Some(trusted)), "203.0.113.1");
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
