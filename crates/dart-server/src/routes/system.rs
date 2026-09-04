//! System diagnostics and health endpoints.

use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use dart_protocol::system::{HealthResponse, SystemInfoResponse};
use std::time::Duration;

/// Handler for `GET /api/v1/health`.
pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse::ok(
        env!("CARGO_PKG_VERSION"),
        state.uptime_seconds(),
    ))
}

/// Handler for `GET /api/v1/system`.
pub async fn system_info(State(state): State<AppState>) -> Json<SystemInfoResponse> {
    let daemon = state.daemon();
    let instances_count = daemon.list_instances().map(|i| i.len()).unwrap_or(0);
    let runtimes_count = daemon.list_runtimes().map(|r| r.len()).unwrap_or(0);

    Json(SystemInfoResponse {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        pid: std::process::id(),
        dart_home: daemon.paths().home().display().to_string(),
        socket_path: daemon.paths().socket_path().display().to_string(),
        instances_count,
        runtimes_count,
    })
}

/// Handler for `POST /api/v1/system/shutdown`.
pub async fn shutdown(State(state): State<AppState>) -> StatusCode {
    let daemon = state.daemon().clone();
    let notify = state.shutdown_notify();
    tokio::spawn(async move {
        let _ = daemon.stop_all().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        notify.notify_waiters();
    });
    StatusCode::ACCEPTED
}
