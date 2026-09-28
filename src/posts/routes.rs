use axum::{Router, routing::get};

use super::handler;
use crate::http::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/posts", get(handler::list_posts))
        .route("/posts/search", get(handler::search_posts))
        .route("/posts/{slug}", get(handler::get_post_by_slug))
        .route("/tags", get(handler::list_tags))
}
