use axum::{
    Router,
    routing::{delete, get, post},
};

use super::{handler, two_factor_handler};
use crate::http::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", post(handler::login))
        .route("/2fa/verify", post(two_factor_handler::verify_two_factor))
        .route("/2fa/resend", post(two_factor_handler::resend_two_factor))
        .route("/csrf", get(handler::csrf))
        .route("/refresh", post(handler::refresh))
        .route("/logout", post(handler::logout))
        .route("/logout-all", post(handler::logout_all))
        .route("/sessions", get(handler::list_sessions))
        .route("/sessions/{id}", delete(handler::delete_session))
}
