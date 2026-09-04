//! Add-on content (mods, data packs, resource packs) management endpoints.

use crate::error::ServerError;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use dart_daemon::content::mods::ModrinthProjectId;
use dart_daemon::{ContentInstallOutcome, ContentKind, InstanceId, InstanceState};
use dart_protocol::content::{
    ContentInstallOutcomeDto, ContentInstallReportDto, ContentKindDto, ContentSearchHitDto,
    InstallContentRequest, InstalledContentDto, RemoveContentRequest,
};
use serde::Deserialize;
use std::str::FromStr;

#[derive(Debug, Deserialize)]
pub struct ContentListQuery {
    pub kind: Option<ContentKindDto>,
}

#[derive(Debug, Deserialize)]
pub struct ContentSearchQuery {
    pub query: String,
    pub kind: Option<ContentKindDto>,
}

fn map_kind_dto(dto: ContentKindDto) -> ContentKind {
    match dto {
        ContentKindDto::Mod => ContentKind::Mod,
        ContentKindDto::DataPack => ContentKind::DataPack,
        ContentKindDto::ResourcePack => ContentKind::ResourcePack,
    }
}

fn map_kind(kind: ContentKind) -> ContentKindDto {
    match kind {
        ContentKind::Mod => ContentKindDto::Mod,
        ContentKind::DataPack => ContentKindDto::DataPack,
        ContentKind::ResourcePack => ContentKindDto::ResourcePack,
    }
}

fn ensure_content_can_change(state: &AppState, id: &InstanceId) -> Result<(), ServerError> {
    match state.daemon().instance_state(id) {
        InstanceState::Stopped | InstanceState::Failed { .. } => Ok(()),
        InstanceState::Starting | InstanceState::Running { .. } | InstanceState::Stopping => Err(
            ServerError::Conflict("stop the server before changing add-ons".to_owned()),
        ),
    }
}

/// Handler for `GET /api/v1/instances/:id/content`.
pub async fn list_content(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
    Query(query): Query<ContentListQuery>,
) -> Result<Json<Vec<InstalledContentDto>>, ServerError> {
    let id = InstanceId::from_str(&id_str).map_err(|e| ServerError::BadRequest(e.to_string()))?;
    let instance = state
        .daemon()
        .get_instance(&id)
        .map_err(ServerError::from)?;
    let kind = map_kind_dto(query.kind.unwrap_or(ContentKindDto::Mod));

    let items = state
        .daemon()
        .list_content(&instance, kind)
        .map_err(ServerError::from)?;

    let dtos = items
        .into_iter()
        .map(|item| match item {
            dart_daemon::InstalledContent::Mod(m) => InstalledContentDto {
                kind: ContentKindDto::Mod,
                key: m.filename().to_string(),
                file_name: m.filename().to_string(),
                name: m.title().to_string(),
                version: m.managed().map(|managed| managed.version_number.clone()),
                managed: m.managed().is_some(),
                project_id: m.managed().map(|p| p.project_id.to_string()),
            },
            dart_daemon::InstalledContent::DataPack(p) => InstalledContentDto {
                kind: ContentKindDto::DataPack,
                key: p.selection_key(),
                file_name: p.managed().map_or_else(
                    || p.title().to_string(),
                    |managed| managed.filename.to_string(),
                ),
                name: p.title().to_string(),
                version: p.managed().map(|managed| managed.version_number.clone()),
                managed: p.managed().is_some(),
                project_id: p.managed().map(|m| m.project_id.to_string()),
            },
            dart_daemon::InstalledContent::ResourcePack(p) => InstalledContentDto {
                kind: ContentKindDto::ResourcePack,
                key: p.selection_key(),
                file_name: p.managed().map_or_else(
                    || p.title().to_string(),
                    |managed| managed.filename.to_string(),
                ),
                name: p.title().to_string(),
                version: p.managed().map(|managed| managed.version_number.clone()),
                managed: p.managed().is_some(),
                project_id: p.managed().map(|m| m.project_id.to_string()),
            },
        })
        .collect();

    Ok(Json(dtos))
}

/// Handler for `GET /api/v1/instances/:id/content/search`.
pub async fn search_content(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
    Query(query): Query<ContentSearchQuery>,
) -> Result<Json<Vec<ContentSearchHitDto>>, ServerError> {
    let id = InstanceId::from_str(&id_str).map_err(|e| ServerError::BadRequest(e.to_string()))?;
    let instance = state
        .daemon()
        .get_instance(&id)
        .map_err(ServerError::from)?;
    let kind = map_kind_dto(query.kind.unwrap_or(ContentKindDto::Mod));

    let hits = state
        .daemon()
        .search_content(kind, &query.query, &instance.config().fabric.minecraft)
        .await
        .map_err(ServerError::from)?;

    let dtos = hits
        .into_iter()
        .map(|hit| ContentSearchHitDto {
            id: match &hit {
                dart_daemon::ContentSearchHit::Mod(m) => m.project().id().to_string(),
                dart_daemon::ContentSearchHit::DataPack(p)
                | dart_daemon::ContentSearchHit::ResourcePack(p) => p.project().id().to_string(),
            },
            slug: match &hit {
                dart_daemon::ContentSearchHit::Mod(m) => {
                    m.project().slug().unwrap_or_default().to_string()
                }
                dart_daemon::ContentSearchHit::DataPack(p)
                | dart_daemon::ContentSearchHit::ResourcePack(p) => {
                    p.project().slug().unwrap_or_default().to_string()
                }
            },
            title: hit.title().to_string(),
            description: hit.description().to_string(),
            author: hit.author().to_string(),
            downloads: hit.downloads(),
            icon_url: None,
            kind: map_kind(hit.kind()),
        })
        .collect();

    Ok(Json(dtos))
}

/// Handler for `POST /api/v1/instances/:id/content/install`.
pub async fn install_content(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
    Json(request): Json<InstallContentRequest>,
) -> Result<Json<ContentInstallReportDto>, ServerError> {
    let id = InstanceId::from_str(&id_str).map_err(|e| ServerError::BadRequest(e.to_string()))?;
    ensure_content_can_change(&state, &id)?;
    let instance = state
        .daemon()
        .get_instance(&id)
        .map_err(ServerError::from)?;
    let project_id = ModrinthProjectId::parse(&request.project_id)
        .map_err(|error| ServerError::BadRequest(error.to_string()))?;
    let plan = state
        .daemon()
        .prepare_content_install_project(
            map_kind_dto(request.kind),
            &project_id,
            &instance.config().fabric.minecraft,
        )
        .await
        .map_err(ServerError::from)?;
    let report = state
        .daemon()
        .apply_content_plan(&instance, &plan)
        .await
        .map_err(ServerError::from)?;

    Ok(Json(ContentInstallReportDto {
        kind: map_kind(report.kind()),
        title: report.title().to_owned(),
        version: report.version().to_owned(),
        outcome: match report.outcome() {
            ContentInstallOutcome::Added => ContentInstallOutcomeDto::Added,
            ContentInstallOutcome::Updated => ContentInstallOutcomeDto::Updated,
            ContentInstallOutcome::AlreadyInstalled => ContentInstallOutcomeDto::AlreadyInstalled,
        },
        dependencies: report.dependency_titles().to_vec(),
    }))
}

/// Handler for `POST /api/v1/instances/:id/content/remove`.
pub async fn remove_content(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
    Json(request): Json<RemoveContentRequest>,
) -> Result<StatusCode, ServerError> {
    let id = InstanceId::from_str(&id_str).map_err(|e| ServerError::BadRequest(e.to_string()))?;
    ensure_content_can_change(&state, &id)?;
    let instance = state
        .daemon()
        .get_instance(&id)
        .map_err(ServerError::from)?;
    let installed = state
        .daemon()
        .list_content(&instance, map_kind_dto(request.kind))
        .map_err(ServerError::from)?;
    let managed = installed
        .into_iter()
        .find(|item| item.selection_key() == request.key)
        .and_then(|item| item.managed())
        .ok_or_else(|| {
            ServerError::NotFound(format!(
                "managed add-on '{}' was not found in instance '{id}'",
                request.key
            ))
        })?;

    state
        .daemon()
        .remove_content(&instance, &managed)
        .map_err(ServerError::from)?;
    Ok(StatusCode::NO_CONTENT)
}
