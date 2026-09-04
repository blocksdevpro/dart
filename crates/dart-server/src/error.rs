//! Error conversion and HTTP response mapping.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use dart_daemon::DaemonError;
use dart_protocol::error::{codes, ApiErrorResponse};

/// API server error wrapper mapping errors to HTTP status codes and JSON error bodies.
#[derive(Debug)]
pub enum ServerError {
    NotFound(String),
    BadRequest(String),
    Conflict(String),
    Internal(String),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            ServerError::NotFound(msg) => (StatusCode::NOT_FOUND, codes::INSTANCE_NOT_FOUND, msg),
            ServerError::BadRequest(msg) => (StatusCode::BAD_REQUEST, codes::BAD_REQUEST, msg),
            ServerError::Conflict(msg) => (StatusCode::CONFLICT, codes::INSTANCE_ALREADY_EXISTS, msg),
            ServerError::Internal(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                codes::INTERNAL_ERROR,
                msg,
            ),
        };
        (status, Json(ApiErrorResponse::new(code, message))).into_response()
    }
}

impl From<DaemonError> for ServerError {
    fn from(error: DaemonError) -> Self {
        match error {
            DaemonError::Instance(instance_err) => {
                let msg = instance_err.to_string();
                if msg.contains("not found") {
                    ServerError::NotFound(msg)
                } else if msg.contains("already exists") {
                    ServerError::Conflict(msg)
                } else {
                    ServerError::BadRequest(msg)
                }
            }
            DaemonError::Store(store_err) => {
                let msg = store_err.to_string();
                if msg.contains("not found") {
                    ServerError::NotFound(msg)
                } else {
                    ServerError::BadRequest(msg)
                }
            }
            DaemonError::Validation(val_err) => ServerError::BadRequest(val_err.to_string()),
            DaemonError::Id(id_err) => ServerError::BadRequest(id_err.to_string()),
            DaemonError::Supervisor(sup_err) => ServerError::Internal(sup_err.to_string()),
            DaemonError::Runtime(rt_err) => ServerError::BadRequest(rt_err.to_string()),
            DaemonError::Content(content_err) => ServerError::BadRequest(content_err.to_string()),
        }
    }
}
