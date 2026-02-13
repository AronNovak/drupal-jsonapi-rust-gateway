use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum AppError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, title, detail) = match &self {
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, "Not Found", msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "Bad Request", msg.clone()),
            AppError::Database(e) => {
                tracing::error!("Database error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error",
                    "A database error occurred.".to_string(),
                )
            }
            AppError::Internal(msg) => {
                tracing::error!("Internal error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error",
                    msg.clone(),
                )
            }
        };

        let body = json!({
            "jsonapi": { "version": "1.1" },
            "errors": [
                {
                    "status": status.as_str(),
                    "title": title,
                    "detail": detail,
                }
            ]
        });

        (
            status,
            [(
                axum::http::header::CONTENT_TYPE,
                "application/vnd.api+json",
            )],
            Json(body),
        )
            .into_response()
    }
}
