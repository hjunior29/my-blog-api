use axum::{
    Router,
    routing::{get, put},
};

use super::handler;
use crate::http::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me", get(handler::me).patch(handler::update_me))
        .route("/me/password", put(handler::change_password))
}
