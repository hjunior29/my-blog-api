use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, patch, post},
};

use super::admin_handler;
use crate::http::AppState;

pub const MAX_POST_BODY_BYTES: usize = 384 * 1024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/posts", get(admin_handler::list_posts))
        .route(
            "/posts",
            post(admin_handler::create_post).layer(DefaultBodyLimit::max(MAX_POST_BODY_BYTES)),
        )
        .route("/posts/{id}", get(admin_handler::get_post))
        .route(
            "/posts/{id}",
            patch(admin_handler::update_post)
                .delete(admin_handler::delete_post)
                .layer(DefaultBodyLimit::max(MAX_POST_BODY_BYTES)),
        )
        .route("/posts/{id}/publish", post(admin_handler::publish_post))
        .route("/posts/{id}/unpublish", post(admin_handler::unpublish_post))
        .route(
            "/posts/preview",
            post(admin_handler::preview_post).layer(DefaultBodyLimit::max(MAX_POST_BODY_BYTES)),
        )
}
