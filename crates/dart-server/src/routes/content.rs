//! Add-on content (mods, data packs, resource packs) management endpoints.

use crate::error::ServerError;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::Json;
use dart_daemon::{ContentKind, InstanceId};
use dart_protocol::content::{
    ContentKindDto, ContentSearchHitDto, InstalledContentDto,
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

/// Handler for `GET /api/v1/instances/:id/content`.
pub async fn list_content(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
    Query(query): Query<ContentListQuery>,
) -> Result<Json<Vec<InstalledContentDto>>, ServerError> {
    let id = InstanceId::from_str(&id_str).map_err(|e| ServerError::BadRequest(e.to_string()))?;
    let instance = state.daemon().get_instance(&id).map_err(ServerError::from)?;
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
                file_name: m.filename().to_string(),
                name: m.title().to_string(),
                version: None,
                managed: m.managed().is_some(),
                project_id: m.managed().map(|p| p.project_id.to_string()),
            },
            dart_daemon::InstalledContent::DataPack(p) => InstalledContentDto {
                kind: ContentKindDto::DataPack,
                file_name: p.title().to_string(),
                name: p.title().to_string(),
                version: None,
                managed: p.managed().is_some(),
                project_id: p.managed().map(|m| m.project_id.to_string()),
            },
            dart_daemon::InstalledContent::ResourcePack(p) => InstalledContentDto {
                kind: ContentKindDto::ResourcePack,
                file_name: p.title().to_string(),
                name: p.title().to_string(),
                version: None,
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
    let instance = state.daemon().get_instance(&id).map_err(ServerError::from)?;
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
