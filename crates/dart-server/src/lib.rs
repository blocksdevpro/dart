//! Dart Server: HTTP REST and WebSocket API server for Dart daemon.
//!
//! Provides dual-transport serving over both local Unix domain sockets (for CLI/TUI)
//! and TCP listeners (for Web/Desktop Panel).

pub mod error;
pub mod routes;
pub mod state;

use axum::Router;
use dart_daemon::Daemon;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
pub use routes::build_router;
pub use state::AppState;
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tokio::net::{TcpListener, UnixListener};

/// Serves the API router over a local Unix domain socket.
pub async fn serve_unix(router: Router, socket_path: impl AsRef<Path>) -> io::Result<()> {
    let path = socket_path.as_ref();
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let listener = UnixListener::bind(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let tower_service = router.clone();

        tokio::spawn(async move {
            let hyper_service = hyper_util::service::TowerToHyperService::new(tower_service);
            let _ = Builder::new(TokioExecutor::new())
                .serve_connection_with_upgrades(io, hyper_service)
                .await;
        });
    }
}

/// Serves the API router over a TCP socket listener.
pub async fn serve_tcp(router: Router, addr: SocketAddr) -> io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, router).await
}

/// Serves the API router over either or both Unix Domain Socket and TCP.
pub async fn serve(
    daemon: Daemon,
    socket_path: Option<PathBuf>,
    tcp_addr: Option<SocketAddr>,
) -> io::Result<()> {
    let state = AppState::new(daemon);
    let router = build_router(state.clone());
    let shutdown = state.shutdown_notify();

    let server_fut = async {
        match (socket_path, tcp_addr) {
            (Some(sock), Some(addr)) => {
                let r = router.clone();
                tokio::spawn(async move {
                    if let Err(err) = serve_tcp(r, addr).await {
                        eprintln!("[dartd] warning: TCP listener on {addr} failed: {err}");
                    }
                });
                serve_unix(router, sock).await
            }
            (Some(sock), None) => serve_unix(router, sock).await,
            (None, Some(addr)) => serve_tcp(router, addr).await,
            (None, None) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "must specify at least a socket path or TCP address to serve",
            )),
        }
    };

    tokio::select! {
        res = server_fut => res,
        _ = shutdown.notified() => Ok(()),
    }
}
