use axum::{
    Router,
    routing::{get, patch},
};

use super::users_handler;
use crate::http::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users", get(users_handler::list_users))
        .route("/users/{id}", patch(users_handler::update_user))
}
