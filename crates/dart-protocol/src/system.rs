//! Health check and system diagnostic representations.

use serde::{Deserialize, Serialize};

/// Daemon health check response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Overall daemon status (e.g. "ok").
    pub status: String,
    /// Daemon version string.
    pub version: String,
    /// Number of seconds the daemon has been running.
    pub uptime_seconds: u64,
}

impl HealthResponse {
    /// Creates an "ok" health response.
    pub fn ok(version: impl Into<String>, uptime_seconds: u64) -> Self {
        Self {
            status: "ok".to_owned(),
            version: version.into(),
            uptime_seconds,
        }
    }
}

/// Detailed system and daemon runtime information.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SystemInfoResponse {
    /// Daemon version.
    pub version: String,
    /// Daemon process ID.
    #[serde(default)]
    pub pid: u32,
    /// Root data directory path (`$DART_HOME`).
    pub dart_home: String,
    /// Local Unix domain socket path.
    pub socket_path: String,
    /// Number of managed instances discovered on disk.
    pub instances_count: usize,
    /// Number of cached Fabric runtimes.
    pub runtimes_count: usize,
}
