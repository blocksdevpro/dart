//! Fabric runtime resolution and caching endpoints.

use crate::error::ServerError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::Json;
use dart_daemon::FabricRuntime;
use dart_protocol::runtime::{DownloadRuntimeRequest, FabricRuntimeDto, ResolveRuntimeResponse};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ResolveQuery {
    pub minecraft: Option<String>,
}

/// Handler for `GET /api/v1/runtimes`.
pub async fn list_runtimes(
    State(state): State<AppState>,
) -> Result<Json<Vec<FabricRuntimeDto>>, ServerError> {
    let runtimes = state.daemon().list_runtimes().map_err(ServerError::from)?;
    let dtos = runtimes.iter().map(|rt| rt.into()).collect();
    Ok(Json(dtos))
}

/// Handler for `GET /api/v1/runtimes/resolve`.
pub async fn resolve_runtime(
    State(state): State<AppState>,
    Query(query): Query<ResolveQuery>,
) -> Result<Json<ResolveRuntimeResponse>, ServerError> {
    let resolved = state
        .daemon()
        .resolve_runtime(query.minecraft.as_deref())
        .await
        .map_err(ServerError::from)?;
    Ok(Json(ResolveRuntimeResponse {
        runtime: (&resolved).into(),
    }))
}

/// Handler for `POST /api/v1/runtimes/download`.
pub async fn download_runtime(
    State(state): State<AppState>,
    Json(request): Json<DownloadRuntimeRequest>,
) -> Result<Json<FabricRuntimeDto>, ServerError> {
    let runtime = match (&request.minecraft, &request.loader, &request.installer) {
        (Some(mc), Some(ldr), Some(inst)) => {
            FabricRuntime::new(mc, ldr, inst).map_err(|e| ServerError::BadRequest(e.to_string()))?
        }
        _ => {
            state
                .daemon()
                .resolve_runtime(request.minecraft.as_deref())
                .await
                .map_err(ServerError::from)?
        }
    };

    state
        .daemon()
        .cache_runtime(&runtime)
        .await
        .map_err(ServerError::from)?;
    Ok(Json((&runtime).into()))
}
