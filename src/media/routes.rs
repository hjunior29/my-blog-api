use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{delete, get, post},
};

use super::handler::{
    UPLOAD_LIMIT_BYTES, delete_media_handler, get_media_handler,
    get_media_with_filename_handler, list_media_handler, upload_media_handler,
};
use crate::http::AppState;

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/media",
            post(upload_media_handler).layer(DefaultBodyLimit::max(UPLOAD_LIMIT_BYTES)),
        )
        .route("/media", get(list_media_handler))
        .route("/media/{id}", delete(delete_media_handler))
}

pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/media/{id}", get(get_media_handler))
        .route("/media/{id}/{*filename}", get(get_media_with_filename_handler))
}
