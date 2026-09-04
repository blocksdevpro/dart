//! Client error definitions.

use dart_protocol::error::ApiErrorResponse;
use std::fmt;

/// Errors produced when communicating with the Dart daemon API.
#[derive(Debug)]
pub enum ClientError {
    /// Failed to connect to the daemon transport (Unix socket or TCP address).
    Connect(std::io::Error),
    /// The daemon API returned an error response.
    Api(ApiErrorResponse),
    /// HTTP protocol failure.
    Http(String),
    /// Payload serialization or deserialization failure.
    Serialization(String),
    /// WebSocket error.
    WebSocket(String),
}

impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(err) => write!(formatter, "cannot connect to dartd: {err}"),
            Self::Api(api_err) => write!(
                formatter,
                "dartd error [{}]: {}",
                api_err.error.code, api_err.error.message
            ),
            Self::Http(err) => write!(formatter, "HTTP error: {err}"),
            Self::Serialization(err) => write!(formatter, "serialization error: {err}"),
            Self::WebSocket(err) => write!(formatter, "websocket error: {err}"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<std::io::Error> for ClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Connect(error)
    }
}
