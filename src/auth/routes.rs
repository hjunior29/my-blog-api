use axum::{
    Router,
    routing::{get, post},
};

use super::handler;
use crate::http::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", post(handler::login))
        .route("/csrf", get(handler::csrf))
        .route("/refresh", post(handler::refresh))
        .route("/logout", post(handler::logout))
        .route("/logout-all", post(handler::logout_all))
}
