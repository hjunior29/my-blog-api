use axum::{
    extract::{FromRequest, Request},
    http::StatusCode,
};
use serde::de::DeserializeOwned;

use super::error::ApiError;

pub struct Json<T>(pub T);

impl<S, T> FromRequest<S> for Json<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        axum::Json::<T>::from_request(request, state)
            .await
            .map(|axum::Json(value)| Self(value))
            .map_err(|rejection| {
                let (code, message) = match rejection.status() {
                    StatusCode::PAYLOAD_TOO_LARGE => {
                        ("payload_too_large", "Request body is too large")
                    }
                    StatusCode::UNSUPPORTED_MEDIA_TYPE => {
                        ("unsupported_media_type", "Expected application/json")
                    }
                    StatusCode::UNPROCESSABLE_ENTITY => {
                        ("invalid_payload", "JSON does not match the expected schema")
                    }
                    _ => ("invalid_json", "Invalid JSON body"),
                };
                ApiError::new(rejection.status(), code, message)
            })
    }
}

impl<T> axum::response::IntoResponse for Json<T>
where
    T: serde::Serialize,
{
    fn into_response(self) -> axum::response::Response {
        axum::Json(self.0).into_response()
    }
}
