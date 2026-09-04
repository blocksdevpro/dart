//! Standard error representations for the Dart API.

use serde::{Deserialize, Serialize};

/// Canonical error response envelope returned by all HTTP and WebSocket endpoints.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiErrorResponse {
    /// Detailed information about the error.
    pub error: ApiErrorDetail,
}

impl ApiErrorResponse {
    /// Creates a new API error response with the given code and human-readable message.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: ApiErrorDetail {
                code: code.into(),
                message: message.into(),
                details: None,
            },
        }
    }

    /// Creates an error response with additional structured context.
    pub fn with_details(
        code: impl Into<String>,
        message: impl Into<String>,
        details: serde_json::Value,
    ) -> Self {
        Self {
            error: ApiErrorDetail {
                code: code.into(),
                message: message.into(),
                details: Some(details),
            },
        }
    }
}

/// Structured error details describing what went wrong.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiErrorDetail {
    /// Machine-readable error code (e.g. `INSTANCE_NOT_FOUND`).
    pub code: String,
    /// Human-readable explanation of the error.
    pub message: String,
    /// Optional structured details (e.g. invalid fields, validation diagnostics).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// Standard machine-readable error codes.
pub mod codes {
    pub const INSTANCE_NOT_FOUND: &str = "INSTANCE_NOT_FOUND";
    pub const INSTANCE_ALREADY_EXISTS: &str = "INSTANCE_ALREADY_EXISTS";
    pub const INSTANCE_ALREADY_RUNNING: &str = "INSTANCE_ALREADY_RUNNING";
    pub const INSTANCE_NOT_RUNNING: &str = "INSTANCE_NOT_RUNNING";
    pub const INSTANCE_VALIDATION_FAILED: &str = "INSTANCE_VALIDATION_FAILED";
    pub const RUNTIME_NOT_FOUND: &str = "RUNTIME_NOT_FOUND";
    pub const RUNTIME_RESOLUTION_FAILED: &str = "RUNTIME_RESOLUTION_FAILED";
    pub const CONTENT_NOT_FOUND: &str = "CONTENT_NOT_FOUND";
    pub const CONTENT_SEARCH_FAILED: &str = "CONTENT_SEARCH_FAILED";
    pub const CONTENT_INSTALL_FAILED: &str = "CONTENT_INSTALL_FAILED";
    pub const BAD_REQUEST: &str = "BAD_REQUEST";
    pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_api_error_response() {
        let err = ApiErrorResponse::new(codes::INSTANCE_NOT_FOUND, "Instance 'survival' does not exist");
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("INSTANCE_NOT_FOUND"));
        assert!(json.contains("Instance 'survival' does not exist"));

        let deserialized: ApiErrorResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(err, deserialized);
    }
}
