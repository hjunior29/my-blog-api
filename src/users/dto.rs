use std::fmt;

use serde::{Deserialize, Serialize};

use super::model::{User, UserRole, UserStatus};

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateProfileRequest {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub avatar_media_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

impl fmt::Debug for ChangePasswordRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChangePasswordRequest")
            .field("current_password", &"[REDACTED]")
            .field("new_password", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminUpdateUserRequest {
    pub role: Option<UserRole>,
    pub status: Option<UserStatus>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminUserResponse {
    pub id: i64,
    pub email: String,
    pub display_name: String,
    pub bio: String,
    pub avatar_media_id: Option<String>,
    pub role: UserRole,
    pub status: UserStatus,
    pub created_at: i64,
    pub updated_at: i64,
    pub password_changed_at: i64,
}

impl From<&User> for AdminUserResponse {
    fn from(u: &User) -> Self {
        Self {
            id: u.id,
            email: u.email.clone(),
            display_name: u.display_name.clone(),
            bio: u.bio.clone(),
            avatar_media_id: u.avatar_media_id.clone(),
            role: u.role,
            status: u.status,
            created_at: u.created_at,
            updated_at: u.updated_at,
            password_changed_at: u.password_changed_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserListResponse {
    pub items: Vec<AdminUserResponse>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
