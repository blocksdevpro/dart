//! Low-level dual-transport HTTP engine.

use crate::error::ClientError;
use dart_protocol::error::ApiErrorResponse;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::net::{TcpStream, UnixStream};

/// Underlying transport medium to communicate with `dartd`.
#[derive(Clone, Debug)]
pub enum Transport {
    /// Local Unix domain socket.
    Unix(PathBuf),
    /// Local or remote TCP listener.
    Tcp(SocketAddr),
}

impl Transport {
    /// Executes an HTTP request over the configured transport and returns the raw response body bytes.
    pub async fn request(
        &self,
        method: http::Method,
        path_and_query: &str,
        body: Option<Vec<u8>>,
    ) -> Result<Vec<u8>, ClientError> {
        let (mut sender, conn_handle) = match self {
            Self::Unix(sock) => {
                let stream = UnixStream::connect(sock).await.map_err(ClientError::Connect)?;
                let io = TokioIo::new(stream);
                let (sender, conn) = hyper::client::conn::http1::handshake(io)
                    .await
                    .map_err(|e| ClientError::Http(e.to_string()))?;
                let handle = tokio::spawn(async move {
                    let _ = conn.await;
                });
                (sender, handle)
            }
            Self::Tcp(addr) => {
                let stream = TcpStream::connect(addr).await.map_err(ClientError::Connect)?;
                let io = TokioIo::new(stream);
                let (sender, conn) = hyper::client::conn::http1::handshake(io)
                    .await
                    .map_err(|e| ClientError::Http(e.to_string()))?;
                let handle = tokio::spawn(async move {
                    let _ = conn.await;
                });
                (sender, handle)
            }
        };

        let mut req_builder = http::Request::builder()
            .method(method)
            .uri(path_and_query)
            .header("Host", "localhost")
            .header("Connection", "close");

        if body.is_some() {
            req_builder = req_builder.header("Content-Type", "application/json");
        }

        let body_data = body.unwrap_or_default();
        let req = req_builder
            .body(Full::new(Bytes::from(body_data)))
            .map_err(|e| ClientError::Http(e.to_string()))?;

        let response = sender
            .send_request(req)
            .await
            .map_err(|e| ClientError::Http(e.to_string()))?;

        let status = response.status();
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|e| ClientError::Http(e.to_string()))?
            .to_bytes()
            .to_vec();

        let _ = conn_handle;

        if status.is_success() {
            Ok(body_bytes)
        } else if let Ok(api_err) = serde_json::from_slice::<ApiErrorResponse>(&body_bytes) {
            Err(ClientError::Api(api_err))
        } else {
            Err(ClientError::Http(format!(
                "server returned HTTP status {}: {}",
                status,
                String::from_utf8_lossy(&body_bytes)
            )))
        }
    }
}
