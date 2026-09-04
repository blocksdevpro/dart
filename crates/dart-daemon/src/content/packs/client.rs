//! Modrinth HTTP adapter for data packs and resource packs.

use super::{
    MAX_PACK_FILE_BYTES, MAX_SEARCH_QUERY_LENGTH, MODRINTH_API_URL, PackError, PackKind,
    PackProject, PackRelease, PackSearchHit, ProjectResponse, SearchResponse, VersionResponse,
    channel_name_priority,
};
use crate::content::mods::ModrinthProjectId;
use crate::runtime::FabricVersion;

/// HTTP client for querying and downloading Modrinth data packs and resource packs.
#[derive(Clone)]
pub struct PackClient {
    client: reqwest::Client,
    base_url: reqwest::Url,
}

impl PackClient {
    /// Creates a new pack client.
    pub fn new() -> Result<Self, PackError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                "dart/",
                env!("CARGO_PKG_VERSION"),
                " (local Fabric server manager)"
            ))
            .build()
            .map_err(PackError::Http)?;
        let base_url = reqwest::Url::parse(MODRINTH_API_URL)
            .map_err(|_| PackError::InvalidMetadata("invalid Modrinth API URL".to_owned()))?;
        Ok(Self { client, base_url })
    }

    /// Searches Modrinth for data packs or resource packs.
    pub async fn search(
        &self,
        kind: PackKind,
        query: &str,
        minecraft: &FabricVersion,
    ) -> Result<Vec<PackSearchHit>, PackError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(PackError::EmptySearch);
        }
        if query.chars().count() > MAX_SEARCH_QUERY_LENGTH {
            return Err(PackError::SearchTooLong);
        }
        let facets = serde_json::to_string(&vec![
            vec![format!("project_type:{}", kind.project_type())],
            vec![format!("versions:{minecraft}")],
        ])
        .map_err(|error| PackError::InvalidMetadata(error.to_string()))?;
        let mut url = self.endpoint(&["search"])?;
        url.query_pairs_mut()
            .append_pair("query", query)
            .append_pair("facets", &facets)
            .append_pair("limit", "20")
            .append_pair("index", "relevance");
        let response: SearchResponse = self.get_json(url).await?;
        response
            .hits
            .into_iter()
            .filter(|hit| {
                hit.project_type == kind.project_type()
                    || hit
                        .all_project_types
                        .iter()
                        .any(|project_type| project_type == kind.project_type())
            })
            .map(PackSearchHit::try_from)
            .collect()
    }

    pub(super) async fn newest_compatible(
        &self,
        kind: PackKind,
        project: PackProject,
        minecraft: &FabricVersion,
    ) -> Result<PackRelease, PackError> {
        let mut url = self.endpoint(&["project", project.id.as_str(), "version"])?;
        url.query_pairs_mut()
            .append_pair("loaders", &format!("[\"{}\"]", kind.loader()))
            .append_pair("game_versions", &format!("[\"{minecraft}\"]"))
            .append_pair("include_changelog", "false");
        let response: Vec<VersionResponse> = self.get_json(url).await?;
        let mut versions = response
            .into_iter()
            .filter(|version| {
                version.project_id == project.id.as_str()
                    && version.status == "listed"
                    && version.loaders.iter().any(|loader| loader == kind.loader())
                    && version
                        .game_versions
                        .iter()
                        .any(|version| version == minecraft.as_str())
            })
            .collect::<Vec<_>>();
        versions.sort_by(|left, right| {
            channel_name_priority(&left.version_type)
                .cmp(&channel_name_priority(&right.version_type))
                .then_with(|| right.date_published.cmp(&left.date_published))
        });
        versions
            .into_iter()
            .next()
            .map(|version| PackRelease::from_version(kind, project.clone(), version))
            .transpose()?
            .ok_or_else(|| PackError::NoCompatibleVersion {
                kind,
                project: project.title,
                minecraft: minecraft.to_string(),
            })
    }

    pub(super) async fn project(
        &self,
        kind: PackKind,
        project_id: &ModrinthProjectId,
    ) -> Result<PackProject, PackError> {
        let response: ProjectResponse = self
            .get_json(self.endpoint(&["project", project_id.as_str()])?)
            .await?;
        let project = response.into_project(kind)?;
        if project.id != *project_id {
            return Err(PackError::InvalidMetadata(format!(
                "Modrinth returned project {} for requested project {project_id}",
                project.id
            )));
        }
        Ok(project)
    }

    pub(super) async fn download(&self, release: &PackRelease) -> Result<Vec<u8>, PackError> {
        let response = self
            .client
            .get(release.download_url.clone())
            .send()
            .await
            .map_err(PackError::Http)?
            .error_for_status()
            .map_err(PackError::Http)?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_PACK_FILE_BYTES as u64)
        {
            return Err(PackError::DownloadTooLarge);
        }
        let bytes = response.bytes().await.map_err(PackError::Http)?;
        if bytes.len() > MAX_PACK_FILE_BYTES {
            return Err(PackError::DownloadTooLarge);
        }
        Ok(bytes.to_vec())
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        url: reqwest::Url,
    ) -> Result<T, PackError> {
        self.client
            .get(url)
            .send()
            .await
            .map_err(PackError::Http)?
            .error_for_status()
            .map_err(PackError::Http)?
            .json()
            .await
            .map_err(PackError::Http)
    }

    fn endpoint(&self, segments: &[&str]) -> Result<reqwest::Url, PackError> {
        let mut url = self.base_url.clone();
        let mut path = url.path_segments_mut().map_err(|_| {
            PackError::InvalidMetadata("Modrinth API URL cannot contain paths".to_owned())
        })?;
        path.clear();
        path.extend(["v2"]);
        path.extend(segments.iter().copied());
        drop(path);
        Ok(url)
    }
}
