use axum::http::{HeaderMap, HeaderValue, header};

use crate::config::Config;

pub fn access_cookie_name(secure_cookies: bool) -> &'static str {
    if secure_cookies {
        "__Host-blog_access"
    } else {
        "blog_access"
    }
}

pub fn refresh_cookie_name(secure_cookies: bool) -> &'static str {
    if secure_cookies {
        "__Host-blog_refresh"
    } else {
        "blog_refresh"
    }
}

pub fn build_cookie_header(
    name: &str,
    value: &str,
    max_age_secs: i64,
    secure: bool,
) -> HeaderValue {
    let secure_flag = if secure { "; Secure" } else { "" };
    let cookie_str = format!(
        "{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure_flag}"
    );
    HeaderValue::from_str(&cookie_str).expect("cookie string must be valid header value")
}

pub fn build_clear_cookie_header(name: &str, secure: bool) -> HeaderValue {
    let secure_flag = if secure { "; Secure" } else { "" };
    let cookie_str = format!("{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure_flag}");
    HeaderValue::from_str(&cookie_str).expect("clear cookie string must be valid header value")
}

pub fn build_auth_cookies(
    config: &Config,
    access_token: &str,
    refresh_token: &str,
    refresh_max_age_secs: i64,
) -> (HeaderValue, HeaderValue) {
    let access_name = access_cookie_name(config.secure_cookies);
    let refresh_name = refresh_cookie_name(config.secure_cookies);

    let access_cookie = build_cookie_header(access_name, access_token, 600, config.secure_cookies);
    let refresh_cookie = build_cookie_header(
        refresh_name,
        refresh_token,
        refresh_max_age_secs,
        config.secure_cookies,
    );

    (access_cookie, refresh_cookie)
}

pub fn build_clear_auth_cookies(config: &Config) -> (HeaderValue, HeaderValue) {
    let access_name = access_cookie_name(config.secure_cookies);
    let refresh_name = refresh_cookie_name(config.secure_cookies);

    let access_cookie = build_clear_cookie_header(access_name, config.secure_cookies);
    let refresh_cookie = build_clear_cookie_header(refresh_name, config.secure_cookies);

    (access_cookie, refresh_cookie)
}

pub fn extract_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    for cookie_header in headers.get_all(header::COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for pair in cookie_str.split(';') {
                let trimmed = pair.trim();
                if let Some((k, v)) = trimmed.split_once('=')
                    && k == name
                {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}
