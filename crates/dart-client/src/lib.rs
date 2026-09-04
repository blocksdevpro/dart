//! Dart Client: async client SDK for communicating with the Dart daemon API.
//!
//! Connects seamlessly over local Unix domain sockets or TCP networks.

pub mod console;
pub mod error;
pub mod transport;

pub use console::ConsoleSession;
pub use error::ClientError;
pub use transport::Transport;

use dart_protocol::console::{ConsoleCommandRequest, ConsoleLineDto};
use dart_protocol::content::{ContentKindDto, ContentSearchHitDto, InstalledContentDto};
use dart_protocol::instance::{CreateInstanceRequest, InstanceDto, InstanceStateDto};
use dart_protocol::runtime::{DownloadRuntimeRequest, FabricRuntimeDto, ResolveRuntimeResponse};
use dart_protocol::system::{HealthResponse, SystemInfoResponse};
use http::Method;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tokio::net::{TcpStream, UnixStream};

/// Primary client handle for interacting with `dartd`.
#[derive(Clone, Debug)]
pub struct DartClient {
    transport: Transport,
}

impl DartClient {
    /// Creates a client connecting to a local Unix domain socket.
    pub fn unix(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            transport: Transport::Unix(socket_path.into()),
        }
    }

    /// Creates a client connecting to a TCP address.
    pub fn tcp(addr: SocketAddr) -> Self {
        Self {
            transport: Transport::Tcp(addr),
        }
    }

    /// Returns a reference to the active transport.
    pub fn transport(&self) -> &Transport {
        &self.transport
    }

    // --- System & Health ---

    /// Checks daemon health and uptime.
    pub async fn health(&self) -> Result<HealthResponse, ClientError> {
        let bytes = self.transport.request(Method::GET, "/api/v1/health", None).await?;
        serde_json::from_slice(&bytes).map_err(|e| ClientError::Serialization(e.to_string()))
    }

    /// Fetches system information and metrics.
    pub async fn system_info(&self) -> Result<SystemInfoResponse, ClientError> {
        let bytes = self.transport.request(Method::GET, "/api/v1/system", None).await?;
        serde_json::from_slice(&bytes).map_err(|e| ClientError::Serialization(e.to_string()))
    }

    /// Requests graceful shutdown of the daemon service.
    pub async fn shutdown(&self) -> Result<(), ClientError> {
        self.transport.request(Method::POST, "/api/v1/system/shutdown", None).await?;
        Ok(())
    }

    // --- Instances ---

    /// Lists all managed instances with their live execution state.
    pub async fn list_instances(&self) -> Result<Vec<InstanceDto>, ClientError> {
        let bytes = self.transport.request(Method::GET, "/api/v1/instances", None).await?;
        serde_json::from_slice(&bytes).map_err(|e| ClientError::Serialization(e.to_string()))
    }

    /// Gets detailed information about a single instance by ID.
    pub async fn get_instance(&self, id: &str) -> Result<InstanceDto, ClientError> {
        let path = format!("/api/v1/instances/{id}");
        let bytes = self.transport.request(Method::GET, &path, None).await?;
        serde_json::from_slice(&bytes).map_err(|e| ClientError::Serialization(e.to_string()))
    }

    /// Creates a new managed server instance.
    pub async fn create_instance(
        &self,
        request: &CreateInstanceRequest,
    ) -> Result<InstanceDto, ClientError> {
        let body = serde_json::to_vec(request)
            .map_err(|e| ClientError::Serialization(e.to_string()))?;
        let bytes = self
            .transport
            .request(Method::POST, "/api/v1/instances", Some(body))
            .await?;
        serde_json::from_slice(&bytes).map_err(|e| ClientError::Serialization(e.to_string()))
    }

    /// Starts a managed instance process.
    pub async fn start_instance(&self, id: &str) -> Result<(), ClientError> {
        let path = format!("/api/v1/instances/{id}/start");
        self.transport.request(Method::POST, &path, None).await?;
        Ok(())
    }

    /// Gracefully stops a running instance.
    pub async fn stop_instance(&self, id: &str) -> Result<(), ClientError> {
        let path = format!("/api/v1/instances/{id}/stop");
        self.transport.request(Method::POST, &path, None).await?;
        Ok(())
    }

    /// Gracefully restarts an instance.
    pub async fn restart_instance(&self, id: &str) -> Result<(), ClientError> {
        let path = format!("/api/v1/instances/{id}/restart");
        self.transport.request(Method::POST, &path, None).await?;
        Ok(())
    }

    /// Immediately kills an instance process.
    pub async fn kill_instance(&self, id: &str) -> Result<(), ClientError> {
        let path = format!("/api/v1/instances/{id}/kill");
        self.transport.request(Method::POST, &path, None).await?;
        Ok(())
    }

    /// Gets the current lifecycle state of an instance.
    pub async fn get_state(&self, id: &str) -> Result<InstanceStateDto, ClientError> {
        let path = format!("/api/v1/instances/{id}/state");
        let bytes = self.transport.request(Method::GET, &path, None).await?;
        serde_json::from_slice(&bytes).map_err(|e| ClientError::Serialization(e.to_string()))
    }

    // --- Console & Logs ---

    /// Sends a command into a running instance's standard input.
    pub async fn send_command(&self, id: &str, command: &str) -> Result<(), ClientError> {
        let path = format!("/api/v1/instances/{id}/command");
        let body = serde_json::to_vec(&ConsoleCommandRequest {
            command: command.to_owned(),
        })
        .map_err(|e| ClientError::Serialization(e.to_string()))?;
        self.transport.request(Method::POST, &path, Some(body)).await?;
        Ok(())
    }

    /// Queries recent console logs from the daemon's in-memory buffer.
    pub async fn get_logs(
        &self,
        id: &str,
        tail: Option<usize>,
    ) -> Result<Vec<ConsoleLineDto>, ClientError> {
        let path = match tail {
            Some(count) => format!("/api/v1/instances/{id}/logs?tail={count}"),
            None => format!("/api/v1/instances/{id}/logs"),
        };
        let bytes = self.transport.request(Method::GET, &path, None).await?;
        serde_json::from_slice(&bytes).map_err(|e| ClientError::Serialization(e.to_string()))
    }

    // --- Runtimes ---

    /// Lists cached Fabric runtimes on disk.
    pub async fn list_runtimes(&self) -> Result<Vec<FabricRuntimeDto>, ClientError> {
        let bytes = self.transport.request(Method::GET, "/api/v1/runtimes", None).await?;
        serde_json::from_slice(&bytes).map_err(|e| ClientError::Serialization(e.to_string()))
    }

    /// Queries Fabric Meta for runtime version recommendations.
    pub async fn resolve_runtime(
        &self,
        minecraft: Option<&str>,
    ) -> Result<ResolveRuntimeResponse, ClientError> {
        let path = match minecraft {
            Some(mc) => format!("/api/v1/runtimes/resolve?minecraft={mc}"),
            None => "/api/v1/runtimes/resolve".to_owned(),
        };
        let bytes = self.transport.request(Method::GET, &path, None).await?;
        serde_json::from_slice(&bytes).map_err(|e| ClientError::Serialization(e.to_string()))
    }

    /// Downloads and caches a Fabric launcher runtime.
    pub async fn download_runtime(
        &self,
        request: &DownloadRuntimeRequest,
    ) -> Result<FabricRuntimeDto, ClientError> {
        let body = serde_json::to_vec(request)
            .map_err(|e| ClientError::Serialization(e.to_string()))?;
        let bytes = self
            .transport
            .request(Method::POST, "/api/v1/runtimes/download", Some(body))
            .await?;
        serde_json::from_slice(&bytes).map_err(|e| ClientError::Serialization(e.to_string()))
    }

    // --- Content ---

    /// Lists installed mods or packs in an instance.
    pub async fn list_content(
        &self,
        id: &str,
        kind: Option<ContentKindDto>,
    ) -> Result<Vec<InstalledContentDto>, ClientError> {
        let path = match kind {
            Some(ContentKindDto::Mod) => format!("/api/v1/instances/{id}/content?kind=mod"),
            Some(ContentKindDto::DataPack) => format!("/api/v1/instances/{id}/content?kind=data_pack"),
            Some(ContentKindDto::ResourcePack) => {
                format!("/api/v1/instances/{id}/content?kind=resource_pack")
            }
            None => format!("/api/v1/instances/{id}/content"),
        };
        let bytes = self.transport.request(Method::GET, &path, None).await?;
        serde_json::from_slice(&bytes).map_err(|e| ClientError::Serialization(e.to_string()))
    }

    /// Searches Modrinth for compatible content.
    pub async fn search_content(
        &self,
        id: &str,
        query: &str,
        kind: Option<ContentKindDto>,
    ) -> Result<Vec<ContentSearchHitDto>, ClientError> {
        let kind_str = match kind.unwrap_or(ContentKindDto::Mod) {
            ContentKindDto::Mod => "mod",
            ContentKindDto::DataPack => "data_pack",
            ContentKindDto::ResourcePack => "resource_pack",
        };
        let path = format!("/api/v1/instances/{id}/content/search?query={query}&kind={kind_str}");
        let bytes = self.transport.request(Method::GET, &path, None).await?;
        serde_json::from_slice(&bytes).map_err(|e| ClientError::Serialization(e.to_string()))
    }

    // --- Console WebSocket Attach ---

    /// Attaches an interactive console session over a Unix domain socket.
    pub async fn attach_console_unix(
        &self,
        socket_path: impl AsRef<Path>,
        id: &str,
    ) -> Result<ConsoleSession<UnixStream>, ClientError> {
        let stream = UnixStream::connect(socket_path.as_ref())
            .await
            .map_err(ClientError::Connect)?;
        let uri = format!("ws://localhost/api/v1/instances/{id}/console");
        let (ws_stream, _) = tokio_tungstenite::client_async(uri, stream)
            .await
            .map_err(|e| ClientError::WebSocket(e.to_string()))?;
        Ok(ConsoleSession::new(ws_stream))
    }

    /// Attaches an interactive console session over a TCP socket.
    pub async fn attach_console_tcp(
        &self,
        addr: SocketAddr,
        id: &str,
    ) -> Result<ConsoleSession<TcpStream>, ClientError> {
        let stream = TcpStream::connect(addr)
            .await
            .map_err(ClientError::Connect)?;
        let uri = format!("ws://{addr}/api/v1/instances/{id}/console");
        let (ws_stream, _) = tokio_tungstenite::client_async(uri, stream)
            .await
            .map_err(|e| ClientError::WebSocket(e.to_string()))?;
        Ok(ConsoleSession::new(ws_stream))
    }

    /// Subscribes to the daemon's global event stream.
    pub async fn subscribe_events(
        &self,
    ) -> Result<tokio::sync::broadcast::Receiver<dart_protocol::event::DaemonEvent>, ClientError> {
        let (tx, rx) = tokio::sync::broadcast::channel(1024);
        match &self.transport {
            Transport::Unix(socket_path) => {
                let stream = UnixStream::connect(socket_path)
                    .await
                    .map_err(ClientError::Connect)?;
                let uri = "ws://localhost/api/v1/events";
                let (mut ws_stream, _) = tokio_tungstenite::client_async(uri, stream)
                    .await
                    .map_err(|e| ClientError::WebSocket(e.to_string()))?;
                let tx_clone = tx.clone();
                tokio::spawn(async move {
                    use futures_util::StreamExt;
                    while let Some(msg_res) = ws_stream.next().await {
                        if let Ok(tokio_tungstenite::tungstenite::Message::Text(text)) = msg_res {
                            if let Ok(event) =
                                serde_json::from_str::<dart_protocol::event::DaemonEvent>(&text)
                            {
                                let _ = tx_clone.send(event);
                            }
                        }
                    }
                });
            }
            Transport::Tcp(addr) => {
                let stream = TcpStream::connect(addr)
                    .await
                    .map_err(ClientError::Connect)?;
                let uri = format!("ws://{addr}/api/v1/events");
                let (mut ws_stream, _) = tokio_tungstenite::client_async(uri, stream)
                    .await
                    .map_err(|e| ClientError::WebSocket(e.to_string()))?;
                let tx_clone = tx.clone();
                tokio::spawn(async move {
                    use futures_util::StreamExt;
                    while let Some(msg_res) = ws_stream.next().await {
                        if let Ok(tokio_tungstenite::tungstenite::Message::Text(text)) = msg_res {
                            if let Ok(event) =
                                serde_json::from_str::<dart_protocol::event::DaemonEvent>(&text)
                            {
                                let _ = tx_clone.send(event);
                            }
                        }
                    }
                });
            }
        }
        Ok(rx)
    }
}
