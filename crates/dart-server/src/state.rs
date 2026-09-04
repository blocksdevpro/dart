//! Shared application state for API route handlers.

use dart_daemon::Daemon;
use std::sync::Arc;
use std::time::Instant;

/// Shared application state injected into Axum route handlers.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

struct AppStateInner {
    daemon: Daemon,
    started_at: Instant,
    shutdown_notify: Arc<tokio::sync::Notify>,
}

impl AppState {
    /// Creates a new application state wrapping the daemon.
    pub fn new(daemon: Daemon) -> Self {
        Self {
            inner: Arc::new(AppStateInner {
                daemon,
                started_at: Instant::now(),
                shutdown_notify: Arc::new(tokio::sync::Notify::new()),
            }),
        }
    }

    /// Returns a reference to the daemon engine.
    pub fn daemon(&self) -> &Daemon {
        &self.inner.daemon
    }

    /// Returns the duration the server has been running in seconds.
    pub fn uptime_seconds(&self) -> u64 {
        self.inner.started_at.elapsed().as_secs()
    }

    /// Returns the shutdown notification handle.
    pub fn shutdown_notify(&self) -> Arc<tokio::sync::Notify> {
        self.inner.shutdown_notify.clone()
    }
}
