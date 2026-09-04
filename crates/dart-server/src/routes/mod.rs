//! Route definitions and top-level Axum router builder.

pub mod console;
pub mod content;
pub mod events;
pub mod instances;
pub mod runtimes;
pub mod system;

use crate::state::AppState;
use axum::Router;
use axum::routing::{get, post};
use tower_http::cors::{Any, CorsLayer};

/// Builds the complete API router for the Dart daemon server.
pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // System endpoints
        .route("/api/v1/health", get(system::health))
        .route("/api/v1/system", get(system::system_info))
        .route("/api/v1/system/shutdown", post(system::shutdown))
        // Global events stream
        .route("/api/v1/events", get(events::ws_events))
        // Instance management
        .route(
            "/api/v1/instances",
            get(instances::list_instances).post(instances::create_instance),
        )
        .route(
            "/api/v1/instances/options",
            get(instances::create_instance_options),
        )
        .route("/api/v1/instances/{id}", get(instances::get_instance))
        .route(
            "/api/v1/instances/{id}/start",
            post(instances::start_instance),
        )
        .route(
            "/api/v1/instances/{id}/stop",
            post(instances::stop_instance),
        )
        .route(
            "/api/v1/instances/{id}/restart",
            post(instances::restart_instance),
        )
        .route(
            "/api/v1/instances/{id}/kill",
            post(instances::kill_instance),
        )
        .route("/api/v1/instances/{id}/state", get(instances::get_state))
        // Console & I/O
        .route(
            "/api/v1/instances/{id}/command",
            post(console::send_command),
        )
        .route("/api/v1/instances/{id}/logs", get(console::get_logs))
        .route("/api/v1/instances/{id}/console", get(console::ws_console))
        // Fabric runtimes
        .route("/api/v1/runtimes", get(runtimes::list_runtimes))
        .route("/api/v1/runtimes/resolve", get(runtimes::resolve_runtime))
        .route(
            "/api/v1/runtimes/download",
            post(runtimes::download_runtime),
        )
        // Content (mods / data packs / resource packs)
        .route("/api/v1/instances/{id}/content", get(content::list_content))
        .route(
            "/api/v1/instances/{id}/content/search",
            get(content::search_content),
        )
        .route(
            "/api/v1/instances/{id}/content/install",
            post(content::install_content),
        )
        .route(
            "/api/v1/instances/{id}/content/remove",
            post(content::remove_content),
        )
        .layer(cors)
        .with_state(state)
}
