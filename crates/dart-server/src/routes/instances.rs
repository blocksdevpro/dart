//! Instance lifecycle and management endpoints.

use crate::error::ServerError;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use dart_daemon::{
    CreateInstance, EulaAcceptance, FabricRuntime, FabricVersion, InstanceId, InstanceName,
    InstanceSize, RuntimeRequest,
};
use dart_protocol::instance::{
    CreateInstanceOptionsResponse, CreateInstanceRequest, InstanceDto, InstanceSizeDto,
    InstanceSizeOptionDto, InstanceStateDto, MinecraftVersionOptionDto,
};
use std::str::FromStr;

fn map_size_dto(size: InstanceSizeDto) -> InstanceSize {
    match size {
        InstanceSizeDto::Personal => InstanceSize::Personal,
        InstanceSizeDto::Friends => InstanceSize::Friends,
        InstanceSizeDto::Community => InstanceSize::Community,
    }
}

fn size_option(
    value: InstanceSizeDto,
    label: &str,
    description: &str,
    recommended: bool,
) -> InstanceSizeOptionDto {
    let memory_mib = map_size_dto(value).launch().max_memory_mib;
    InstanceSizeOptionDto {
        value,
        label: label.to_owned(),
        description: description.to_owned(),
        memory_mib,
        recommended,
    }
}

/// Handler for `GET /api/v1/instances`.
pub async fn list_instances(
    State(state): State<AppState>,
) -> Result<Json<Vec<InstanceDto>>, ServerError> {
    let daemon = state.daemon();
    let instances = daemon.list_instances().map_err(ServerError::from)?;
    let dtos = instances
        .into_iter()
        .map(|inst| {
            let s = daemon.instance_state(inst.id());
            inst.to_dto(&s)
        })
        .collect();
    Ok(Json(dtos))
}

/// Handler for `GET /api/v1/instances/options`.
pub async fn create_instance_options(
    State(state): State<AppState>,
) -> Result<Json<CreateInstanceOptionsResponse>, ServerError> {
    let versions = state
        .daemon()
        .list_minecraft_versions()
        .await
        .map_err(ServerError::from)?;
    let minecraft_versions = versions
        .into_iter()
        .take(24)
        .enumerate()
        .map(|(index, version)| MinecraftVersionOptionDto {
            value: version.to_string(),
            label: format!("Minecraft {version}"),
            recommended: index == 0,
        })
        .collect();

    Ok(Json(CreateInstanceOptionsResponse {
        minecraft_versions,
        sizes: vec![
            size_option(
                InstanceSizeDto::Personal,
                "Personal",
                "A simple world for one or two people.",
                false,
            ),
            size_option(
                InstanceSizeDto::Friends,
                "Friends",
                "A balanced server for a regular group.",
                true,
            ),
            size_option(
                InstanceSizeDto::Community,
                "Community",
                "More room for players and add-ons.",
                false,
            ),
        ],
    }))
}

/// Handler for `POST /api/v1/instances`.
pub async fn create_instance(
    State(state): State<AppState>,
    Json(request): Json<CreateInstanceRequest>,
) -> Result<(StatusCode, Json<InstanceDto>), ServerError> {
    let name =
        InstanceName::parse(&request.name).map_err(|e| ServerError::BadRequest(e.to_string()))?;

    let runtime = match (&request.minecraft, &request.loader, &request.installer) {
        (Some(mc), Some(ldr), Some(inst)) => {
            let rt = FabricRuntime::new(mc, ldr, inst)
                .map_err(|e| ServerError::BadRequest(e.to_string()))?;
            RuntimeRequest::Exact(rt)
        }
        (Some(mc), _, _) => {
            let ver =
                FabricVersion::parse(mc).map_err(|e| ServerError::BadRequest(e.to_string()))?;
            RuntimeRequest::Minecraft(ver)
        }
        _ => RuntimeRequest::Latest,
    };

    let eula = if request.accept_eula {
        EulaAcceptance::Accepted
    } else {
        EulaAcceptance::NotAccepted
    };

    let size = map_size_dto(request.size);
    let cmd = CreateInstance::new(name, runtime, eula).with_launch(size.launch());
    let instance = state
        .daemon()
        .create_instance(cmd)
        .await
        .map_err(ServerError::from)?;
    let live_state = state.daemon().instance_state(instance.id());

    Ok((StatusCode::CREATED, Json(instance.to_dto(&live_state))))
}

/// Handler for `GET /api/v1/instances/:id`.
pub async fn get_instance(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
) -> Result<Json<InstanceDto>, ServerError> {
    let id = InstanceId::from_str(&id_str).map_err(|e| ServerError::BadRequest(e.to_string()))?;
    let instance = state
        .daemon()
        .get_instance(&id)
        .map_err(ServerError::from)?;
    let live_state = state.daemon().instance_state(&id);
    Ok(Json(instance.to_dto(&live_state)))
}

/// Handler for `POST /api/v1/instances/:id/start`.
pub async fn start_instance(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
) -> Result<StatusCode, ServerError> {
    let id = InstanceId::from_str(&id_str).map_err(|e| ServerError::BadRequest(e.to_string()))?;
    state
        .daemon()
        .start_instance(&id)
        .await
        .map_err(ServerError::from)?;
    Ok(StatusCode::OK)
}

/// Handler for `POST /api/v1/instances/:id/stop`.
pub async fn stop_instance(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
) -> Result<StatusCode, ServerError> {
    let id = InstanceId::from_str(&id_str).map_err(|e| ServerError::BadRequest(e.to_string()))?;
    state
        .daemon()
        .stop_instance(&id)
        .await
        .map_err(ServerError::from)?;
    Ok(StatusCode::OK)
}

/// Handler for `POST /api/v1/instances/:id/restart`.
pub async fn restart_instance(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
) -> Result<StatusCode, ServerError> {
    let id = InstanceId::from_str(&id_str).map_err(|e| ServerError::BadRequest(e.to_string()))?;
    let _ = state.daemon().stop_instance(&id).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    state
        .daemon()
        .start_instance(&id)
        .await
        .map_err(ServerError::from)?;
    Ok(StatusCode::OK)
}

/// Handler for `POST /api/v1/instances/:id/kill`.
pub async fn kill_instance(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
) -> Result<StatusCode, ServerError> {
    let _id = InstanceId::from_str(&id_str).map_err(|e| ServerError::BadRequest(e.to_string()))?;
    state
        .daemon()
        .supervisor()
        .kill_all()
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(StatusCode::OK)
}

/// Handler for `GET /api/v1/instances/:id/state`.
pub async fn get_state(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
) -> Result<Json<InstanceStateDto>, ServerError> {
    let id = InstanceId::from_str(&id_str).map_err(|e| ServerError::BadRequest(e.to_string()))?;
    let live_state = state.daemon().instance_state(&id);
    Ok(Json((&live_state).into()))
}
