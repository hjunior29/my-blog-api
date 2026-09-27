use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    Owner,
    Author,
}

impl fmt::Display for UserRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Owner => write!(f, "owner"),
            Self::Author => write!(f, "author"),
        }
    }
}

impl std::str::FromStr for UserRole {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "owner" => Ok(Self::Owner),
            "author" => Ok(Self::Author),
            _ => Err("invalid user role"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum UserStatus {
    Active,
    Inactive,
}

impl fmt::Display for UserStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Inactive => write!(f, "inactive"),
        }
    }
}

impl std::str::FromStr for UserStatus {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "inactive" => Ok(Self::Inactive),
            _ => Err("invalid user status"),
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub normalized_email: String,
    pub password_hash: String,
    pub display_name: String,
    pub bio: String,
    pub avatar_media_id: Option<String>,
    pub role: UserRole,
    pub status: UserStatus,
    pub created_at: i64,
    pub updated_at: i64,
    pub password_changed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicUser {
    pub id: i64,
    pub display_name: String,
    pub bio: String,
    pub avatar_media_id: Option<String>,
}

impl From<&User> for PublicUser {
    fn from(user: &User) -> Self {
        Self {
            id: user.id,
            display_name: user.display_name.clone(),
            bio: user.bio.clone(),
            avatar_media_id: user.avatar_media_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub id: i64,
    pub email: String,
    pub display_name: String,
    pub bio: String,
    pub avatar_media_id: Option<String>,
    pub role: UserRole,
    pub status: UserStatus,
    pub created_at: i64,
}

impl From<&User> for UserProfile {
    fn from(user: &User) -> Self {
        Self {
            id: user.id,
            email: user.email.clone(),
            display_name: user.display_name.clone(),
            bio: user.bio.clone(),
            avatar_media_id: user.avatar_media_id.clone(),
            role: user.role,
            status: user.status,
            created_at: user.created_at,
        }
    }
}

pub struct NewUser<'a> {
    pub email: &'a str,
    pub normalized_email: &'a str,
    pub password_hash: &'a str,
    pub display_name: &'a str,
    pub bio: &'a str,
    pub role: UserRole,
    pub status: UserStatus,
    pub now: i64,
}
