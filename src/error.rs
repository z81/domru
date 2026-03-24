use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("HTTP error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Auth error: {0}")]
    Auth(String),

    #[error("API error: {status} {message}")]
    Api { status: u16, message: String },

    #[error("SIP error: {0}")]
    Sip(String),

    #[error("Not found: {0}")]
    NotFound(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Auth(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            AppError::Api { status, message } => {
                let code = StatusCode::from_u16(*status)
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                (code, message.clone())
            }
            AppError::Reqwest(err) => (StatusCode::BAD_GATEWAY, err.to_string()),
            AppError::Io(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
            AppError::Json(err) => (StatusCode::BAD_REQUEST, err.to_string()),
            AppError::Sip(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
        };

        let body = axum::Json(json!({ "error": message }));
        (status, body).into_response()
    }
}
