use std::fmt;

use serde::{Deserialize, Serialize};

use crate::users::model::UserProfile;

#[derive(Serialize, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub remember_me: Option<bool>,
}

impl fmt::Debug for LoginRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginRequest")
            .field("email", &self.email)
            .field("password", &"[REDACTED]")
            .field("remember_me", &self.remember_me)
            .finish()
    }
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub user: UserProfile,
    pub csrf_token: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum LoginResultResponse {
    TwoFactorRequired(TwoFactorChallengeResponse),
    Success(LoginResponse),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TwoFactorChallengeResponse {
    pub requires_2fa: bool,
    pub challenge_token: String,
    pub email_masked: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyTwoFactorRequest {
    pub challenge_token: String,
    pub code: String,
    #[serde(default)]
    pub remember_me: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResendTwoFactorRequest {
    pub challenge_token: String,
}

#[derive(Debug, Serialize)]
pub struct ResendTwoFactorResponse {
    pub challenge_token: String,
    pub email_masked: String,
}

#[derive(Debug, Serialize)]
pub struct CsrfResponse {
    pub csrf_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionItemResponse {
    pub id: String,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
    pub is_current: bool,
}
