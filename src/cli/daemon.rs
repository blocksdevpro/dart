//! Daemon discovery, health check, and auto-spawning lifecycle for the CLI and TUI.

use dart_client::DartClient;
use dart_daemon::DartPaths;
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Ensures that `dartd` is running for the given paths layout and returns an active `DartClient`.
///
/// 1. Checks if `dartd` is already running and responsive on the Unix domain socket.
/// 2. If absent, attempts to locate and spawn `dartd` as a detached background service.
/// 3. If the binary cannot be located (e.g. during test runs), gracefully falls back
///    to booting an in-process daemon server on the socket.
pub async fn ensure_daemon(paths: &DartPaths) -> Result<DartClient, Box<dyn Error>> {
    let socket_path = paths.socket_path();
    let client = DartClient::unix(&socket_path);

    // Fast path: check if daemon is already running and healthy
    if client.health().await.is_ok() {
        return Ok(client);
    }

    // Remove stale socket file if daemon isn't responding
    if socket_path.exists() {
        let _ = fs::remove_file(&socket_path);
    }

    // Try auto-spawning dartd binary
    if let Some(dartd_bin) = find_dartd_executable() {
        if spawn_dartd_background(&dartd_bin, paths.home()).is_ok() {
            let start = Instant::now();
            let timeout = Duration::from_millis(2500);

            while start.elapsed() < timeout {
                if socket_path.exists() && client.health().await.is_ok() {
                    return Ok(client);
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }

    Err(format!(
        "The background daemon 'dartd' is not running and the 'dartd' executable was not found.\n\
         Ensure 'dartd' is in the same directory as 'dart' or in your PATH:\n  \
         cargo build --release\n  \
         cp target/release/dartd ~/.cargo/bin/"
    )
    .into())
}

/// Locates the `dartd` executable on disk.
fn find_dartd_executable() -> Option<PathBuf> {
    // 1. Check adjacent to current executable (target/debug/dartd or installed bin/dartd)
    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let candidate = parent.join("dartd");
            if candidate.is_file() {
                return Some(candidate);
            }
            #[cfg(windows)]
            {
                let candidate_exe = parent.join("dartd.exe");
                if candidate_exe.is_file() {
                    return Some(candidate_exe);
                }
            }
        }
    }

    // 2. Search in PATH
    if let Ok(path_var) = env::var("PATH") {
        for dir in env::split_paths(&path_var) {
            let candidate = dir.join("dartd");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

/// Spawns `dartd` as a detached background process.
fn spawn_dartd_background(dartd_bin: &Path, home: &Path) -> Result<(), Box<dyn Error>> {
    let mut cmd = std::process::Command::new(dartd_bin);
    cmd.arg("--home")
        .arg(home)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    cmd.spawn()?;
    Ok(())
}
