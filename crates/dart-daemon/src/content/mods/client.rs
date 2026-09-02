//! Modrinth HTTP adapter for Fabric server mods.

use super::{
    MAX_MOD_FILE_BYTES, MAX_SEARCH_QUERY_LENGTH, MODRINTH_API_URL, ModError, ModProject,
    ModRelease, ModSearchHit, ModrinthProjectId, ModrinthVersionId, ProjectResponse,
    SearchResponse, VersionResponse, version_supports_instance,
};
use crate::runtime::FabricVersion;

/// HTTP client for interacting with the Modrinth v2 API for Fabric mods.
#[derive(Clone)]
pub struct ModrinthClient {
    client: reqwest::Client,
    base_url: reqwest::Url,
}

impl ModrinthClient {
    /// Creates a new Modrinth client with appropriate user-agent.
    pub fn new() -> Result<Self, ModError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                "dart/",
                env!("CARGO_PKG_VERSION"),
                " (local Fabric server manager)"
            ))
            .build()
            .map_err(ModError::Http)?;
        let base_url = reqwest::Url::parse(MODRINTH_API_URL)
            .map_err(|_| ModError::InvalidMetadata("invalid Modrinth API URL".to_owned()))?;
        Ok(Self { client, base_url })
    }

    /// Searches for compatible Fabric server mods matching the query.
    pub async fn search(
        &self,
        query: &str,
        minecraft: &FabricVersion,
    ) -> Result<Vec<ModSearchHit>, ModError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(ModError::EmptySearch);
        }
        if query.chars().count() > MAX_SEARCH_QUERY_LENGTH {
            return Err(ModError::SearchTooLong);
        }
        let response: SearchResponse = self.get_json(self.search_url(query, minecraft)?).await?;
        response
            .hits
            .into_iter()
            .map(ModSearchHit::try_from)
            .collect()
    }

    pub(super) async fn newest_compatible(
        &self,
        project: ModProject,
        minecraft: &FabricVersion,
    ) -> Result<ModRelease, ModError> {
        let mut url = self.endpoint(&["project", project.id.as_str(), "version"])?;
        url.query_pairs_mut()
            .append_pair("loaders", "[\"fabric\"]")
            .append_pair("game_versions", &format!("[\"{minecraft}\"]"))
            .append_pair("include_changelog", "false");
        let response: Vec<VersionResponse> = self.get_json(url).await?;
        let mut releases = response
            .into_iter()
            .filter(|version| {
                version.project_id == project.id.as_str()
                    && version_supports_instance(version, minecraft, false)
            })
            .map(|version| ModRelease::from_version(project.clone(), version))
            .collect::<Result<Vec<_>, _>>()?;
        releases.sort_by(|left, right| {
            left.channel
                .priority()
                .cmp(&right.channel.priority())
                .then_with(|| right.published_at.cmp(&left.published_at))
        });
        releases
            .into_iter()
            .next()
            .ok_or_else(|| ModError::NoCompatibleVersion {
                project: project.title,
                minecraft: minecraft.to_string(),
            })
    }

    pub(super) async fn project(
        &self,
        project_id: &ModrinthProjectId,
    ) -> Result<ModProject, ModError> {
        let response: ProjectResponse = self
            .get_json(self.endpoint(&["project", project_id.as_str()])?)
            .await?;
        let project = ModProject::try_from(response)?;
        if project.id != *project_id {
            return Err(ModError::InvalidMetadata(format!(
                "Modrinth returned project {} for requested project {project_id}",
                project.id
            )));
        }
        Ok(project)
    }

    pub(super) async fn exact_compatible(
        &self,
        version_id: &ModrinthVersionId,
        expected_project: Option<&ModrinthProjectId>,
        minecraft: &FabricVersion,
    ) -> Result<ModRelease, ModError> {
        let response: VersionResponse = self
            .get_json(self.endpoint(&["version", version_id.as_str()])?)
            .await?;
        if response.id != version_id.as_str() {
            return Err(ModError::InvalidMetadata(format!(
                "Modrinth returned version '{}' for requested version {version_id}",
                response.id
            )));
        }
        let project_id = ModrinthProjectId::parse(&response.project_id)?;
        if let Some(expected_project) = expected_project
            && expected_project != &project_id
        {
            return Err(ModError::DependencyProjectMismatch {
                version: version_id.clone(),
                expected: expected_project.clone(),
                actual: project_id,
            });
        }
        let project = self.project(&project_id).await?;
        if !version_supports_instance(&response, minecraft, true) {
            return Err(ModError::IncompatibleDependencyVersion {
                project: project.title.clone(),
                version: version_id.clone(),
                minecraft: minecraft.to_string(),
            });
        }
        ModRelease::from_version(project, response)
    }

    pub(super) async fn download(&self, release: &ModRelease) -> Result<Vec<u8>, ModError> {
        let response = self
            .client
            .get(release.download_url.clone())
            .send()
            .await
            .map_err(ModError::Http)?
            .error_for_status()
            .map_err(ModError::Http)?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_MOD_FILE_BYTES as u64)
        {
            return Err(ModError::DownloadTooLarge);
        }
        let bytes = response.bytes().await.map_err(ModError::Http)?;
        if bytes.len() > MAX_MOD_FILE_BYTES {
            return Err(ModError::DownloadTooLarge);
        }
        Ok(bytes.to_vec())
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        url: reqwest::Url,
    ) -> Result<T, ModError> {
        self.client
            .get(url)
            .send()
            .await
            .map_err(ModError::Http)?
            .error_for_status()
            .map_err(ModError::Http)?
            .json()
            .await
            .map_err(ModError::Http)
    }

    pub(super) fn search_url(
        &self,
        query: &str,
        minecraft: &FabricVersion,
    ) -> Result<reqwest::Url, ModError> {
        let facets = serde_json::to_string(&vec![
            vec!["project_type:mod".to_owned()],
            vec!["categories:fabric".to_owned()],
            vec![format!("versions:{minecraft}")],
            vec![
                "environment:client_and_server".to_owned(),
                "environment:server_only".to_owned(),
                "environment:server_only_client_optional".to_owned(),
                "environment:dedicated_server_only".to_owned(),
                "environment:client_or_server".to_owned(),
                "environment:client_or_server_prefers_both".to_owned(),
            ],
        ])
        .map_err(|error| ModError::InvalidMetadata(error.to_string()))?;
        let mut url = self.endpoint(&["search"])?;
        url.query_pairs_mut()
            .append_pair("query", query)
            .append_pair("facets", &facets)
            .append_pair("limit", "20")
            .append_pair("index", "relevance");
        Ok(url)
    }

    fn endpoint(&self, segments: &[&str]) -> Result<reqwest::Url, ModError> {
        let mut url = self.base_url.clone();
        let mut path = url.path_segments_mut().map_err(|_| {
            ModError::InvalidMetadata("Modrinth API URL cannot contain paths".to_owned())
        })?;
        path.clear();
        path.extend(["v2"]);
        path.extend(segments.iter().copied());
        drop(path);
        Ok(url)
    }
}
