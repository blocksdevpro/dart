use crate::instance::Instance;
use crate::runtime::FabricVersion;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MODRINTH_API_URL: &str = "https://api.modrinth.com/v2/";
const MODS_DIRECTORY: &str = "mods";
const DART_DIRECTORY: &str = ".dart";
const MANIFEST_FILE: &str = "mods.toml";
const MANIFEST_FORMAT_VERSION: u32 = 1;
const MAX_SEARCH_QUERY_LENGTH: usize = 120;
const MAX_MOD_FILE_BYTES: usize = 128 * 1024 * 1024;
const MAX_DEPENDENCY_PROJECTS: usize = 64;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModrinthProjectId(String);

impl ModrinthProjectId {
    pub(crate) fn parse(value: impl AsRef<str>) -> Result<Self, ModError> {
        let value = value.as_ref();
        if value.is_empty()
            || value.len() > 64
            || !value.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(ModError::InvalidMetadata(format!(
                "invalid Modrinth project ID '{value}'"
            )));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModrinthProjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModrinthVersionId(String);

impl ModrinthVersionId {
    pub(crate) fn parse(value: impl AsRef<str>) -> Result<Self, ModError> {
        let value = value.as_ref();
        if value.is_empty()
            || value.len() > 64
            || !value.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(ModError::InvalidMetadata(format!(
                "invalid Modrinth version ID '{value}'"
            )));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModrinthVersionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModFileName(String);

impl ModFileName {
    fn parse(value: impl AsRef<str>) -> Result<Self, ModError> {
        let value = value.as_ref();
        let path = Path::new(value);
        if value.is_empty()
            || value.len() > 255
            || !value.ends_with(".jar")
            || value.contains(['/', '\\', '\n', '\r'])
            || path.file_name().and_then(|name| name.to_str()) != Some(value)
        {
            return Err(ModError::InvalidMetadata(format!(
                "invalid mod JAR filename '{value}'"
            )));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModFileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModProject {
    id: ModrinthProjectId,
    slug: Option<String>,
    title: String,
}

impl ModProject {
    pub fn id(&self) -> &ModrinthProjectId {
        &self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModSearchHit {
    project: ModProject,
    description: String,
    author: String,
    downloads: u64,
}

impl ModSearchHit {
    pub fn project(&self) -> &ModProject {
        &self.project
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn author(&self) -> &str {
        &self.author
    }

    pub fn downloads(&self) -> u64 {
        self.downloads
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseChannel {
    Release,
    Beta,
    Alpha,
}

impl ReleaseChannel {
    fn parse(value: &str) -> Result<Self, ModError> {
        match value {
            "release" => Ok(Self::Release),
            "beta" => Ok(Self::Beta),
            "alpha" => Ok(Self::Alpha),
            other => Err(ModError::InvalidMetadata(format!(
                "unknown Modrinth version type '{other}'"
            ))),
        }
    }

    fn priority(self) -> u8 {
        match self {
            Self::Release => 0,
            Self::Beta => 1,
            Self::Alpha => 2,
        }
    }
}

impl fmt::Display for ReleaseChannel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Release => "release",
            Self::Beta => "beta",
            Self::Alpha => "alpha",
        })
    }
}

#[derive(Clone, Debug)]
pub struct ModRelease {
    project: ModProject,
    version_id: ModrinthVersionId,
    version_number: String,
    channel: ReleaseChannel,
    filename: ModFileName,
    sha512: String,
    download_url: reqwest::Url,
    required_dependencies: Vec<RequiredDependency>,
    published_at: String,
}

impl ModRelease {
    pub fn project(&self) -> &ModProject {
        &self.project
    }

    pub fn version_id(&self) -> &ModrinthVersionId {
        &self.version_id
    }

    pub fn version_number(&self) -> &str {
        &self.version_number
    }

    pub fn channel(&self) -> ReleaseChannel {
        self.channel
    }

    pub fn filename(&self) -> &ModFileName {
        &self.filename
    }

    pub fn required_dependencies(&self) -> &[RequiredDependency] {
        &self.required_dependencies
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequiredDependency {
    Project(ModrinthProjectId),
    Version {
        version_id: ModrinthVersionId,
        expected_project: Option<ModrinthProjectId>,
    },
    ExternalFile(String),
}

#[derive(Clone, Debug)]
pub struct ModInstallPlan {
    root: ModrinthProjectId,
    releases: Vec<ModRelease>,
}

impl ModInstallPlan {
    pub fn root(&self) -> &ModRelease {
        self.releases
            .iter()
            .find(|release| release.project.id == self.root)
            .expect("a mod install plan always contains its root release")
    }

    pub fn releases(&self) -> &[ModRelease] {
        &self.releases
    }

    pub fn dependency_titles(&self) -> Vec<&str> {
        self.releases
            .iter()
            .filter(|release| release.project.id != self.root)
            .map(|release| release.project.title())
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct ModInstallReport {
    root: ModRelease,
    root_outcome: ModInstallOutcome,
    dependency_titles: Vec<String>,
    changed_dependencies: usize,
}

impl ModInstallReport {
    pub fn root(&self) -> &ModRelease {
        &self.root
    }

    pub fn root_outcome(&self) -> ModInstallOutcome {
        self.root_outcome
    }

    pub fn dependency_titles(&self) -> &[String] {
        &self.dependency_titles
    }

    pub fn changed_dependencies(&self) -> usize {
        self.changed_dependencies
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManagedMod {
    pub project_id: ModrinthProjectId,
    pub project_slug: Option<String>,
    pub title: String,
    pub version_id: ModrinthVersionId,
    pub version_number: String,
    pub filename: ModFileName,
    pub sha512: String,
}

impl ManagedMod {
    fn from_release(release: &ModRelease) -> Self {
        Self {
            project_id: release.project.id.clone(),
            project_slug: release.project.slug.clone(),
            title: release.project.title.clone(),
            version_id: release.version_id.clone(),
            version_number: release.version_number.clone(),
            filename: release.filename.clone(),
            sha512: release.sha512.clone(),
        }
    }

    pub fn project(&self) -> ModProject {
        ModProject {
            id: self.project_id.clone(),
            slug: self.project_slug.clone(),
            title: self.title.clone(),
        }
    }

    fn validate(&self) -> Result<(), ModError> {
        ModrinthProjectId::parse(&self.project_id.0)?;
        ModrinthVersionId::parse(&self.version_id.0)?;
        ModFileName::parse(&self.filename.0)?;
        validate_text(&self.title, "mod title", 120)?;
        validate_text(&self.version_number, "mod version", 120)?;
        validate_sha512(&self.sha512)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstalledMod {
    Managed(ManagedMod),
    External { filename: ModFileName },
}

impl InstalledMod {
    pub fn filename(&self) -> &ModFileName {
        match self {
            Self::Managed(modification) => &modification.filename,
            Self::External { filename } => filename,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::Managed(modification) => &modification.title,
            Self::External { filename } => filename.as_str(),
        }
    }

    pub fn managed(&self) -> Option<&ManagedMod> {
        match self {
            Self::Managed(modification) => Some(modification),
            Self::External { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModInstallOutcome {
    Added,
    Updated,
    AlreadyInstalled,
}

#[derive(Clone)]
pub struct ModrinthClient {
    client: reqwest::Client,
    base_url: reqwest::Url,
}

impl ModrinthClient {
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

    pub async fn newest_compatible(
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

    async fn project(&self, project_id: &ModrinthProjectId) -> Result<ModProject, ModError> {
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

    async fn exact_compatible(
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

    pub async fn download(&self, release: &ModRelease) -> Result<Vec<u8>, ModError> {
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

    fn search_url(&self, query: &str, minecraft: &FabricVersion) -> Result<reqwest::Url, ModError> {
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

#[derive(Clone)]
pub struct ModManager {
    client: ModrinthClient,
    store: ModStore,
}

impl ModManager {
    pub fn new(client: ModrinthClient, store: ModStore) -> Self {
        Self { client, store }
    }

    pub async fn search(
        &self,
        query: &str,
        minecraft: &FabricVersion,
    ) -> Result<Vec<ModSearchHit>, ModError> {
        self.client.search(query, minecraft).await
    }

    pub fn list(&self, instance: &Instance) -> Result<Vec<InstalledMod>, ModError> {
        self.store.list(instance)
    }

    pub fn remove(&self, instance: &Instance, modification: &ManagedMod) -> Result<(), ModError> {
        self.store.remove(instance, modification)
    }

    pub async fn prepare_install(
        &self,
        root_project: ModProject,
        minecraft: &FabricVersion,
    ) -> Result<ModInstallPlan, ModError> {
        let root_release = self
            .client
            .newest_compatible(root_project, minecraft)
            .await?;
        let root = root_release.project.id.clone();
        let mut releases = BTreeMap::from([(root.clone(), root_release.clone())]);
        let mut edges = BTreeMap::<ModrinthProjectId, BTreeSet<ModrinthProjectId>>::new();
        edges.entry(root.clone()).or_default();
        let mut pending = root_release
            .required_dependencies
            .iter()
            .cloned()
            .map(|dependency| (root.clone(), dependency))
            .collect::<VecDeque<_>>();

        while let Some((parent, dependency)) = pending.pop_front() {
            let parent_title = releases
                .get(&parent)
                .expect("dependency parents are always resolved")
                .project
                .title
                .clone();
            let release = match dependency {
                RequiredDependency::Project(project_id) => {
                    if releases.contains_key(&project_id) {
                        edges.entry(parent).or_default().insert(project_id);
                        continue;
                    }
                    let project = self.client.project(&project_id).await?;
                    self.client.newest_compatible(project, minecraft).await?
                }
                RequiredDependency::Version {
                    version_id,
                    expected_project,
                } => {
                    if let Some(expected_project) = expected_project.as_ref()
                        && let Some(existing) = releases.get(expected_project)
                    {
                        if existing.version_id != version_id {
                            return Err(ModError::DependencyVersionConflict {
                                project: existing.project.title.clone(),
                                first: existing.version_id.clone(),
                                second: version_id,
                            });
                        }
                        edges
                            .entry(parent)
                            .or_default()
                            .insert(expected_project.clone());
                        continue;
                    }
                    self.client
                        .exact_compatible(&version_id, expected_project.as_ref(), minecraft)
                        .await?
                }
                RequiredDependency::ExternalFile(filename) => {
                    return Err(ModError::UnresolvableRequiredDependency {
                        requested_by: parent_title,
                        dependency: filename,
                    });
                }
            };

            let dependency_id = release.project.id.clone();
            edges
                .entry(parent)
                .or_default()
                .insert(dependency_id.clone());
            if let Some(existing) = releases.get(&dependency_id) {
                if existing.version_id != release.version_id {
                    return Err(ModError::DependencyVersionConflict {
                        project: existing.project.title.clone(),
                        first: existing.version_id.clone(),
                        second: release.version_id,
                    });
                }
                continue;
            }
            if releases.len() >= MAX_DEPENDENCY_PROJECTS {
                return Err(ModError::DependencyLimitExceeded {
                    limit: MAX_DEPENDENCY_PROJECTS,
                });
            }
            edges.entry(dependency_id.clone()).or_default();
            pending.extend(
                release
                    .required_dependencies
                    .iter()
                    .cloned()
                    .map(|dependency| (dependency_id.clone(), dependency)),
            );
            releases.insert(dependency_id, release);
        }

        Ok(ModInstallPlan {
            root: root.clone(),
            releases: order_releases(&root, &releases, &edges)?,
        })
    }

    pub async fn apply_plan(
        &self,
        instance: &Instance,
        plan: &ModInstallPlan,
    ) -> Result<ModInstallReport, ModError> {
        let mut root_outcome = None;
        let mut changed_dependencies = 0;
        for release in &plan.releases {
            let outcome = if self.store.release_is_intact(instance, release)? {
                ModInstallOutcome::AlreadyInstalled
            } else {
                let bytes = self.client.download(release).await?;
                self.store.install(instance, release, &bytes)?
            };
            if release.project.id == plan.root {
                root_outcome = Some(outcome);
            } else if outcome != ModInstallOutcome::AlreadyInstalled {
                changed_dependencies += 1;
            }
        }

        Ok(ModInstallReport {
            root: plan.root().clone(),
            root_outcome: root_outcome.expect("a mod install plan always applies its root"),
            dependency_titles: plan
                .dependency_titles()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            changed_dependencies,
        })
    }
}

fn order_releases(
    root: &ModrinthProjectId,
    releases: &BTreeMap<ModrinthProjectId, ModRelease>,
    edges: &BTreeMap<ModrinthProjectId, BTreeSet<ModrinthProjectId>>,
) -> Result<Vec<ModRelease>, ModError> {
    fn visit(
        project: &ModrinthProjectId,
        releases: &BTreeMap<ModrinthProjectId, ModRelease>,
        edges: &BTreeMap<ModrinthProjectId, BTreeSet<ModrinthProjectId>>,
        visiting: &mut BTreeSet<ModrinthProjectId>,
        visited: &mut BTreeSet<ModrinthProjectId>,
        ordered: &mut Vec<ModRelease>,
    ) -> Result<(), ModError> {
        if visited.contains(project) {
            return Ok(());
        }
        if !visiting.insert(project.clone()) {
            let title = releases.get(project).map_or_else(
                || project.to_string(),
                |release| release.project.title.clone(),
            );
            return Err(ModError::DependencyCycle { project: title });
        }
        if let Some(dependencies) = edges.get(project) {
            for dependency in dependencies {
                visit(dependency, releases, edges, visiting, visited, ordered)?;
            }
        }
        visiting.remove(project);
        visited.insert(project.clone());
        ordered.push(
            releases
                .get(project)
                .expect("dependency graph edges reference resolved projects")
                .clone(),
        );
        Ok(())
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut ordered = Vec::with_capacity(releases.len());
    visit(
        root,
        releases,
        edges,
        &mut visiting,
        &mut visited,
        &mut ordered,
    )?;
    Ok(ordered)
}

#[derive(Clone, Debug, Default)]
pub struct ModStore;

impl ModStore {
    pub fn new() -> Self {
        Self
    }

    pub fn mods_dir(&self, instance: &Instance) -> PathBuf {
        instance.root().join(MODS_DIRECTORY)
    }

    pub fn manifest_path(&self, instance: &Instance) -> PathBuf {
        instance.root().join(DART_DIRECTORY).join(MANIFEST_FILE)
    }

    pub fn list(&self, instance: &Instance) -> Result<Vec<InstalledMod>, ModError> {
        let manifest = self.load_manifest(instance)?;
        let files = self.jar_files(instance)?;
        let managed_by_filename = manifest
            .mods
            .into_iter()
            .map(|modification| (modification.filename.clone(), modification))
            .collect::<BTreeMap<_, _>>();

        let mut installed = files
            .into_iter()
            .map(|filename| match managed_by_filename.get(&filename) {
                Some(modification) => InstalledMod::Managed(modification.clone()),
                None => InstalledMod::External { filename },
            })
            .collect::<Vec<_>>();
        installed.sort_by(|left, right| {
            left.title()
                .to_lowercase()
                .cmp(&right.title().to_lowercase())
        });
        Ok(installed)
    }

    pub fn install(
        &self,
        instance: &Instance,
        release: &ModRelease,
        bytes: &[u8],
    ) -> Result<ModInstallOutcome, ModError> {
        validate_mod_jar(bytes)?;
        let actual_hash = sha512(bytes);
        if !actual_hash.eq_ignore_ascii_case(&release.sha512) {
            return Err(ModError::HashMismatch {
                expected: release.sha512.clone(),
                actual: actual_hash,
            });
        }

        let mut manifest = self.load_manifest(instance)?;
        let new_entry = ManagedMod::from_release(release);
        let existing = manifest
            .mods
            .iter()
            .find(|modification| modification.project_id == new_entry.project_id)
            .cloned();
        let target = self.mods_dir(instance).join(new_entry.filename.as_str());

        if existing.as_ref().is_some_and(|current| {
            current.version_id == new_entry.version_id
                && current.filename == new_entry.filename
                && current.sha512.eq_ignore_ascii_case(&new_entry.sha512)
                && file_matches_hash(&target, &current.sha512).unwrap_or(false)
        }) {
            return Ok(ModInstallOutcome::AlreadyInstalled);
        }

        fs::create_dir_all(self.mods_dir(instance)).map_err(|source| {
            ModError::io(
                "create instance mods directory",
                self.mods_dir(instance),
                source,
            )
        })?;
        match file_matches_hash(&target, &new_entry.sha512) {
            Ok(true) => {}
            Ok(false) if target.exists() => {
                let is_current_managed_file = existing.as_ref().is_some_and(|current| {
                    current.filename == new_entry.filename
                        && file_matches_hash(&target, &current.sha512).unwrap_or(false)
                });
                if !is_current_managed_file {
                    return Err(ModError::FileConflict { path: target });
                }
                atomic_write(&target, bytes, true)?;
            }
            Ok(false) => atomic_write(&target, bytes, false)?,
            Err(error) => return Err(error),
        }

        manifest
            .mods
            .retain(|modification| modification.project_id != new_entry.project_id);
        manifest.mods.push(new_entry.clone());
        manifest.sort_and_validate()?;
        self.save_manifest(instance, &manifest)?;

        let outcome = if existing.is_some() {
            ModInstallOutcome::Updated
        } else {
            ModInstallOutcome::Added
        };
        if let Some(previous) = &existing
            && previous.filename != new_entry.filename
        {
            let previous_path = self.mods_dir(instance).join(previous.filename.as_str());
            if file_matches_hash(&previous_path, &previous.sha512)? {
                fs::remove_file(&previous_path).map_err(|source| {
                    ModError::io("remove replaced managed mod", previous_path, source)
                })?;
            }
        }

        Ok(outcome)
    }

    pub fn remove(&self, instance: &Instance, modification: &ManagedMod) -> Result<(), ModError> {
        let mut manifest = self.load_manifest(instance)?;
        let Some(existing) = manifest
            .mods
            .iter()
            .find(|entry| entry.project_id == modification.project_id)
            .cloned()
        else {
            return Ok(());
        };
        let path = self.mods_dir(instance).join(existing.filename.as_str());
        if path.exists() && !file_matches_hash(&path, &existing.sha512)? {
            return Err(ModError::ManagedFileChanged { path });
        }

        manifest
            .mods
            .retain(|entry| entry.project_id != existing.project_id);
        self.save_manifest(instance, &manifest)?;
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|source| ModError::io("remove managed mod", path, source))?;
        }
        Ok(())
    }

    fn release_is_intact(
        &self,
        instance: &Instance,
        release: &ModRelease,
    ) -> Result<bool, ModError> {
        let manifest = self.load_manifest(instance)?;
        let Some(current) = manifest
            .mods
            .iter()
            .find(|modification| modification.project_id == release.project.id)
        else {
            return Ok(false);
        };
        if current.version_id != release.version_id
            || current.filename != release.filename
            || !current.sha512.eq_ignore_ascii_case(&release.sha512)
        {
            return Ok(false);
        }
        file_matches_hash(
            &self.mods_dir(instance).join(current.filename.as_str()),
            &current.sha512,
        )
    }

    fn load_manifest(&self, instance: &Instance) -> Result<ModManifest, ModError> {
        let path = self.manifest_path(instance);
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(ModManifest::default());
            }
            Err(source) => return Err(ModError::io("read mod manifest", path, source)),
        };
        let mut manifest: ModManifest =
            toml::from_str(&contents).map_err(|source| ModError::ParseManifest {
                path: path.clone(),
                source,
            })?;
        manifest.sort_and_validate().map_err(|error| match error {
            ModError::InvalidMetadata(message) => ModError::InvalidManifest { path, message },
            other => other,
        })?;
        Ok(manifest)
    }

    fn save_manifest(&self, instance: &Instance, manifest: &ModManifest) -> Result<(), ModError> {
        let path = self.manifest_path(instance);
        let parent = path
            .parent()
            .expect("the mod manifest path always has a parent")
            .to_owned();
        fs::create_dir_all(&parent).map_err(|source| {
            ModError::io("create Dart state directory", parent.clone(), source)
        })?;
        let contents = toml::to_string_pretty(manifest).map_err(ModError::SerializeManifest)?;
        atomic_write(&path, contents.as_bytes(), true)
    }

    fn jar_files(&self, instance: &Instance) -> Result<BTreeSet<ModFileName>, ModError> {
        let directory = self.mods_dir(instance);
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
            Err(source) => {
                return Err(ModError::io(
                    "read instance mods directory",
                    directory,
                    source,
                ));
            }
        };
        let mut files = BTreeSet::new();
        for entry in entries {
            let entry = entry.map_err(|source| {
                ModError::io(
                    "read instance mod directory entry",
                    directory.clone(),
                    source,
                )
            })?;
            if !entry
                .file_type()
                .map_err(|source| ModError::io("inspect instance mod file", entry.path(), source))?
                .is_file()
            {
                continue;
            }
            let Some(filename) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !filename.ends_with(".jar") {
                continue;
            }
            files.insert(ModFileName::parse(filename)?);
        }
        Ok(files)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ModManifest {
    format_version: u32,
    #[serde(default)]
    mods: Vec<ManagedMod>,
}

impl Default for ModManifest {
    fn default() -> Self {
        Self {
            format_version: MANIFEST_FORMAT_VERSION,
            mods: Vec::new(),
        }
    }
}

impl ModManifest {
    fn sort_and_validate(&mut self) -> Result<(), ModError> {
        if self.format_version != MANIFEST_FORMAT_VERSION {
            return Err(ModError::InvalidMetadata(format!(
                "unsupported mod manifest format version {}",
                self.format_version
            )));
        }
        let mut projects = BTreeSet::new();
        let mut filenames = BTreeSet::new();
        for modification in &self.mods {
            modification.validate()?;
            if !projects.insert(modification.project_id.clone()) {
                return Err(ModError::InvalidMetadata(format!(
                    "duplicate Modrinth project {} in the mod manifest",
                    modification.project_id
                )));
            }
            if !filenames.insert(modification.filename.clone()) {
                return Err(ModError::InvalidMetadata(format!(
                    "duplicate mod file {} in the mod manifest",
                    modification.filename
                )));
            }
        }
        self.mods.sort_by_key(|entry| entry.title.to_lowercase());
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    hits: Vec<SearchHitResponse>,
}

#[derive(Debug, Deserialize)]
struct SearchHitResponse {
    project_id: String,
    slug: Option<String>,
    title: String,
    description: String,
    author: String,
    downloads: u64,
}

#[derive(Debug, Deserialize)]
struct ProjectResponse {
    id: String,
    slug: Option<String>,
    title: String,
    project_type: String,
    status: String,
}

impl TryFrom<ProjectResponse> for ModProject {
    type Error = ModError;

    fn try_from(value: ProjectResponse) -> Result<Self, Self::Error> {
        if value.project_type != "mod" {
            return Err(ModError::InvalidMetadata(format!(
                "Modrinth project '{}' is not a mod",
                value.id
            )));
        }
        if !matches!(value.status.as_str(), "approved" | "archived" | "unlisted") {
            return Err(ModError::InvalidMetadata(format!(
                "Modrinth project '{}' is not installable",
                value.id
            )));
        }
        Ok(Self {
            id: ModrinthProjectId::parse(value.id)?,
            slug: value.slug.filter(|slug| !slug.trim().is_empty()),
            title: validate_text(value.title, "Modrinth project title", 120)?,
        })
    }
}

impl TryFrom<SearchHitResponse> for ModSearchHit {
    type Error = ModError;

    fn try_from(value: SearchHitResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            project: ModProject {
                id: ModrinthProjectId::parse(value.project_id)?,
                slug: value.slug.filter(|slug| !slug.trim().is_empty()),
                title: validate_text(value.title, "Modrinth project title", 120)?,
            },
            description: normalize_summary(value.description, 500),
            author: validate_text(value.author, "Modrinth project author", 120)?,
            downloads: value.downloads,
        })
    }
}

#[derive(Debug, Deserialize)]
struct VersionResponse {
    id: String,
    project_id: String,
    version_number: String,
    version_type: String,
    date_published: String,
    loaders: Vec<String>,
    game_versions: Vec<String>,
    status: String,
    environment: String,
    files: Vec<VersionFileResponse>,
    #[serde(default)]
    dependencies: Vec<DependencyResponse>,
}

#[derive(Debug, Deserialize)]
struct VersionFileResponse {
    hashes: BTreeMap<String, String>,
    url: String,
    filename: String,
    primary: bool,
}

#[derive(Debug, Deserialize)]
struct DependencyResponse {
    version_id: Option<String>,
    project_id: Option<String>,
    file_name: Option<String>,
    dependency_type: String,
}

impl TryFrom<&DependencyResponse> for RequiredDependency {
    type Error = ModError;

    fn try_from(value: &DependencyResponse) -> Result<Self, Self::Error> {
        if let Some(version_id) = value.version_id.as_deref() {
            return Ok(Self::Version {
                version_id: ModrinthVersionId::parse(version_id)?,
                expected_project: value
                    .project_id
                    .as_deref()
                    .map(ModrinthProjectId::parse)
                    .transpose()?,
            });
        }
        if let Some(project_id) = value.project_id.as_deref() {
            return Ok(Self::Project(ModrinthProjectId::parse(project_id)?));
        }
        if let Some(filename) = value.file_name.as_deref() {
            return Ok(Self::ExternalFile(validate_text(
                filename,
                "required dependency filename",
                255,
            )?));
        }
        Err(ModError::InvalidMetadata(
            "required dependency has no project, version, or filename".to_owned(),
        ))
    }
}

impl ModRelease {
    fn from_version(project: ModProject, version: VersionResponse) -> Result<Self, ModError> {
        let filename_is_jar = |file: &&VersionFileResponse| file.filename.ends_with(".jar");
        let file = version
            .files
            .iter()
            .find(|file| file.primary && filename_is_jar(file))
            .or_else(|| version.files.iter().find(filename_is_jar))
            .ok_or_else(|| {
                ModError::InvalidMetadata(format!(
                    "Modrinth version '{}' has no downloadable JAR",
                    version.id
                ))
            })?;
        let sha512 = file.hashes.get("sha512").cloned().ok_or_else(|| {
            ModError::InvalidMetadata(format!(
                "Modrinth version '{}' has no SHA-512 hash",
                version.id
            ))
        })?;
        validate_sha512(&sha512)?;
        let download_url = reqwest::Url::parse(&file.url).map_err(|_| {
            ModError::InvalidMetadata(format!(
                "Modrinth version '{}' has an invalid download URL",
                version.id
            ))
        })?;
        if download_url.scheme() != "https" {
            return Err(ModError::InvalidMetadata(format!(
                "Modrinth version '{}' does not use an HTTPS download URL",
                version.id
            )));
        }

        Ok(Self {
            project,
            version_id: ModrinthVersionId::parse(version.id)?,
            version_number: validate_text(version.version_number, "Modrinth version number", 120)?,
            channel: ReleaseChannel::parse(&version.version_type)?,
            filename: ModFileName::parse(file.filename.clone())?,
            sha512,
            download_url,
            required_dependencies: version
                .dependencies
                .iter()
                .filter(|dependency| dependency.dependency_type == "required")
                .map(RequiredDependency::try_from)
                .collect::<Result<Vec<_>, _>>()?,
            published_at: validate_text(version.date_published, "Modrinth publication date", 64)?,
        })
    }
}

fn validate_text(value: impl AsRef<str>, field: &str, maximum: usize) -> Result<String, ModError> {
    let value = value.as_ref().trim();
    if value.is_empty() || value.chars().count() > maximum || value.contains(['\n', '\r']) {
        return Err(ModError::InvalidMetadata(format!("invalid {field}")));
    }
    Ok(value.to_owned())
}

fn normalize_summary(value: impl AsRef<str>, maximum: usize) -> String {
    let sanitized = value
        .as_ref()
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let normalized = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return "No description provided.".to_owned();
    }
    if normalized.chars().count() <= maximum {
        return normalized;
    }
    let mut shortened = normalized
        .chars()
        .take(maximum.saturating_sub(3))
        .collect::<String>();
    shortened.push_str("...");
    shortened
}

fn supports_dedicated_server(environment: &str) -> bool {
    matches!(
        environment,
        "client_and_server"
            | "server_only"
            | "server_only_client_optional"
            | "dedicated_server_only"
            | "client_or_server"
            | "client_or_server_prefers_both"
    )
}

fn version_supports_instance(
    version: &VersionResponse,
    minecraft: &FabricVersion,
    pinned: bool,
) -> bool {
    version.loaders.iter().any(|loader| loader == "fabric")
        && version
            .game_versions
            .iter()
            .any(|version| version == minecraft.as_str())
        && supports_dedicated_server(&version.environment)
        && if pinned {
            matches!(version.status.as_str(), "listed" | "archived" | "unlisted")
        } else {
            version.status == "listed"
        }
}

fn validate_sha512(value: &str) -> Result<(), ModError> {
    if value.len() != 128 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ModError::InvalidMetadata(
            "invalid Modrinth SHA-512 hash".to_owned(),
        ));
    }
    Ok(())
}

fn validate_mod_jar(bytes: &[u8]) -> Result<(), ModError> {
    if bytes.len() < 4 || !bytes.starts_with(b"PK") {
        return Err(ModError::InvalidJar);
    }
    Ok(())
}

fn sha512(bytes: &[u8]) -> String {
    let digest = Sha512::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn file_matches_hash(path: &Path, expected: &str) -> Result<bool, ModError> {
    let mut file = match OpenOptions::new().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(source) => return Err(ModError::io("open managed mod", path.to_owned(), source)),
    };
    let mut digest = Sha512::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| ModError::io("read managed mod", path.to_owned(), source))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let actual = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(actual.eq_ignore_ascii_case(expected))
}

fn atomic_write(target: &Path, bytes: &[u8], replace: bool) -> Result<(), ModError> {
    let parent = target
        .parent()
        .expect("managed mod paths always have a parent");
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".dart-write.{}.{sequence}", std::process::id()));
    let write_result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|source| {
                ModError::io("create temporary mod file", temporary.clone(), source)
            })?;
        file.write_all(bytes).map_err(|source| {
            ModError::io("write temporary mod file", temporary.clone(), source)
        })?;
        file.sync_all()
            .map_err(|source| ModError::io("sync temporary mod file", temporary.clone(), source))?;
        if replace {
            if cfg!(windows) && target.exists() {
                fs::remove_file(target).map_err(|source| {
                    ModError::io("replace managed mod", target.to_owned(), source)
                })?;
            }
            fs::rename(&temporary, target)
                .map_err(|source| ModError::io("finish mod write", target.to_owned(), source))
        } else {
            fs::hard_link(&temporary, target).map_err(|source| {
                ModError::io(
                    "finish mod write without replacing a file",
                    target.to_owned(),
                    source,
                )
            })?;
            fs::remove_file(&temporary).map_err(|source| {
                ModError::io("remove temporary mod file", temporary.clone(), source)
            })
        }
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

#[derive(Debug)]
pub enum ModError {
    EmptySearch,
    SearchTooLong,
    NoCompatibleVersion {
        project: String,
        minecraft: String,
    },
    DependencyProjectMismatch {
        version: ModrinthVersionId,
        expected: ModrinthProjectId,
        actual: ModrinthProjectId,
    },
    IncompatibleDependencyVersion {
        project: String,
        version: ModrinthVersionId,
        minecraft: String,
    },
    DependencyVersionConflict {
        project: String,
        first: ModrinthVersionId,
        second: ModrinthVersionId,
    },
    UnresolvableRequiredDependency {
        requested_by: String,
        dependency: String,
    },
    DependencyLimitExceeded {
        limit: usize,
    },
    DependencyCycle {
        project: String,
    },
    InvalidMetadata(String),
    InvalidJar,
    DownloadTooLarge,
    HashMismatch {
        expected: String,
        actual: String,
    },
    FileConflict {
        path: PathBuf,
    },
    ManagedFileChanged {
        path: PathBuf,
    },
    Http(reqwest::Error),
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    ParseManifest {
        path: PathBuf,
        source: toml::de::Error,
    },
    SerializeManifest(toml::ser::Error),
    InvalidManifest {
        path: PathBuf,
        message: String,
    },
}

impl ModError {
    fn io(operation: &'static str, path: PathBuf, source: io::Error) -> Self {
        Self::Io {
            operation,
            path,
            source,
        }
    }
}

impl fmt::Display for ModError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySearch => formatter.write_str("enter a Modrinth search query"),
            Self::SearchTooLong => formatter.write_str("the Modrinth search query is too long"),
            Self::NoCompatibleVersion { project, minecraft } => write!(
                formatter,
                "Modrinth has no listed Fabric version of '{project}' for Minecraft {minecraft}"
            ),
            Self::DependencyProjectMismatch {
                version,
                expected,
                actual,
            } => write!(
                formatter,
                "required version {version} belongs to Modrinth project {actual}, not {expected}"
            ),
            Self::IncompatibleDependencyVersion {
                project,
                version,
                minecraft,
            } => write!(
                formatter,
                "required version {version} of '{project}' is not a Fabric server mod for Minecraft {minecraft}"
            ),
            Self::DependencyVersionConflict {
                project,
                first,
                second,
            } => write!(
                formatter,
                "required dependencies select conflicting versions {first} and {second} of '{project}'"
            ),
            Self::UnresolvableRequiredDependency {
                requested_by,
                dependency,
            } => write!(
                formatter,
                "'{requested_by}' requires external file '{dependency}', which Modrinth cannot resolve automatically"
            ),
            Self::DependencyLimitExceeded { limit } => write!(
                formatter,
                "required dependency graph exceeds Dart's safety limit of {limit} projects"
            ),
            Self::DependencyCycle { project } => write!(
                formatter,
                "required dependency graph contains a cycle at '{project}'"
            ),
            Self::InvalidMetadata(message) => {
                write!(formatter, "invalid Modrinth metadata: {message}")
            }
            Self::InvalidJar => formatter.write_str("downloaded mod is not a valid JAR archive"),
            Self::DownloadTooLarge => {
                formatter.write_str("Modrinth mod download exceeds Dart's 128 MiB limit")
            }
            Self::HashMismatch { expected, actual } => write!(
                formatter,
                "downloaded mod checksum did not match Modrinth (expected {expected}, got {actual})"
            ),
            Self::FileConflict { path } => write!(
                formatter,
                "refusing to overwrite existing mod file {}",
                path.display()
            ),
            Self::ManagedFileChanged { path } => write!(
                formatter,
                "refusing to remove {} because its contents changed outside Dart",
                path.display()
            ),
            Self::Http(source) => write!(formatter, "Modrinth request failed: {source}"),
            Self::Io {
                operation, path, ..
            } => write!(formatter, "cannot {operation} at {}", path.display()),
            Self::ParseManifest { path, source } => {
                write!(formatter, "cannot parse {}: {source}", path.display())
            }
            Self::SerializeManifest(source) => {
                write!(formatter, "cannot serialize mod manifest: {source}")
            }
            Self::InvalidManifest { path, message } => {
                write!(
                    formatter,
                    "invalid mod manifest at {}: {message}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ModError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Http(source) => Some(source),
            Self::Io { source, .. } => Some(source),
            Self::ParseManifest { source, .. } => Some(source),
            Self::SerializeManifest(source) => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DependencyResponse, ModError, ModFileName, ModInstallOutcome, ModProject, ModRelease,
        ModStore, ModrinthClient, ModrinthProjectId, ModrinthVersionId, ReleaseChannel,
        RequiredDependency, SearchHitResponse, order_releases, sha512,
    };
    use crate::instance::{FabricLaunch, Instance, InstanceConfig, InstanceId, InstanceName};
    use crate::runtime::FabricRuntime;
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("dart-mod-test-{}-{sequence}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn instance(directory: &TestDirectory) -> Instance {
        Instance::new(
            InstanceId::from_str("survival").unwrap(),
            directory.0.join("survival"),
            InstanceConfig::new(
                InstanceName::parse("Survival").unwrap(),
                FabricLaunch::default(),
                FabricRuntime::new("1.21.8", "0.17.2", "1.1.2").unwrap(),
            ),
        )
    }

    fn release(filename: &str, bytes: &[u8], version: &str) -> ModRelease {
        ModRelease {
            project: ModProject {
                id: ModrinthProjectId::parse("AABBCCDD").unwrap(),
                slug: Some("fabric-api".to_owned()),
                title: "Fabric API".to_owned(),
            },
            version_id: ModrinthVersionId::parse(version).unwrap(),
            version_number: "0.1.0".to_owned(),
            channel: ReleaseChannel::Release,
            filename: ModFileName::parse(filename).unwrap(),
            sha512: sha512(bytes),
            download_url: reqwest::Url::parse("https://cdn.modrinth.com/mod.jar").unwrap(),
            required_dependencies: Vec::new(),
            published_at: "2026-01-01T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn stores_managed_mods_without_touching_external_jars() {
        let directory = TestDirectory::new();
        let instance = instance(&directory);
        let store = ModStore::new();
        fs::create_dir_all(store.mods_dir(&instance)).unwrap();
        fs::write(
            store.mods_dir(&instance).join("other-launcher.jar"),
            b"PK\x03\x04external",
        )
        .unwrap();
        let bytes = b"PK\x03\x04fabric api";
        let version = release("fabric-api.jar", bytes, "EEFFGGHH");

        assert_eq!(
            store.install(&instance, &version, bytes).unwrap(),
            ModInstallOutcome::Added
        );
        assert_eq!(
            store.install(&instance, &version, bytes).unwrap(),
            ModInstallOutcome::AlreadyInstalled
        );
        assert!(store.release_is_intact(&instance, &version).unwrap());
        let installed = store.list(&instance).unwrap();
        assert_eq!(installed.len(), 2);
        assert!(installed.iter().any(|entry| entry.managed().is_some()));
        assert!(
            installed
                .iter()
                .any(|entry| entry.filename().as_str() == "other-launcher.jar")
        );
        assert!(store.manifest_path(&instance).is_file());

        let managed = installed
            .iter()
            .find_map(|entry| entry.managed())
            .unwrap()
            .clone();
        store.remove(&instance, &managed).unwrap();
        assert!(!store.mods_dir(&instance).join("fabric-api.jar").exists());
        assert!(
            store
                .mods_dir(&instance)
                .join("other-launcher.jar")
                .is_file()
        );
    }

    #[test]
    fn rejects_a_checksum_mismatch_before_writing_the_mod() {
        let directory = TestDirectory::new();
        let instance = instance(&directory);
        let store = ModStore::new();
        let bytes = b"PK\x03\x04good";
        let mut version = release("fabric-api.jar", bytes, "EEFFGGHH");
        version.sha512 = "0".repeat(128);

        assert!(store.install(&instance, &version, bytes).is_err());
        assert!(!store.mods_dir(&instance).join("fabric-api.jar").exists());
    }

    #[test]
    fn updates_a_managed_mod_in_place() {
        let directory = TestDirectory::new();
        let instance = instance(&directory);
        let store = ModStore::new();
        let first_bytes = b"PK\x03\x04fabric api v1";
        let second_bytes = b"PK\x03\x04fabric api v2";
        let first = release("fabric-api.jar", first_bytes, "EEFFGGHH");
        let mut second = release("fabric-api.jar", second_bytes, "IIJJKKLL");
        second.version_number = "0.2.0".to_owned();

        store.install(&instance, &first, first_bytes).unwrap();
        assert_eq!(
            store.install(&instance, &second, second_bytes).unwrap(),
            ModInstallOutcome::Updated
        );
        assert_eq!(
            fs::read(store.mods_dir(&instance).join("fabric-api.jar")).unwrap(),
            second_bytes
        );
        let installed = store.list(&instance).unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(
            installed[0].managed().unwrap().version_id.as_str(),
            "IIJJKKLL"
        );
    }

    #[test]
    fn refuses_to_remove_a_managed_file_changed_outside_dart() {
        let directory = TestDirectory::new();
        let instance = instance(&directory);
        let store = ModStore::new();
        let bytes = b"PK\x03\x04fabric api";
        let version = release("fabric-api.jar", bytes, "EEFFGGHH");
        store.install(&instance, &version, bytes).unwrap();
        let managed = store.list(&instance).unwrap()[0].managed().unwrap().clone();
        fs::write(
            store.mods_dir(&instance).join("fabric-api.jar"),
            b"PK\x03\x04changed outside Dart",
        )
        .unwrap();

        assert!(!store.release_is_intact(&instance, &version).unwrap());
        assert!(store.remove(&instance, &managed).is_err());
        assert!(store.mods_dir(&instance).join("fabric-api.jar").is_file());
        assert!(
            store
                .list(&instance)
                .unwrap()
                .iter()
                .any(|entry| entry.managed().is_some())
        );
    }

    #[test]
    fn rejects_client_only_mod_versions_for_a_server() {
        assert!(super::supports_dedicated_server("client_and_server"));
        assert!(super::supports_dedicated_server("server_only"));
        assert!(!super::supports_dedicated_server("client_only"));
        assert!(!super::supports_dedicated_server("singleplayer_only"));
    }

    #[test]
    fn searches_with_fabric_and_minecraft_facets() {
        let client = ModrinthClient::new().unwrap();
        let minecraft = crate::runtime::FabricVersion::parse("1.21.8").unwrap();
        let url = client.search_url("fabric api", &minecraft).unwrap();
        let query = url.query_pairs().collect::<Vec<_>>();
        assert!(query.iter().any(|(key, value)| key == "facets"
            && value.contains("project_type:mod")
            && value.contains("categories:fabric")
            && value.contains("versions:1.21.8")
            && value.contains("environment:server_only")));
    }

    #[test]
    fn normalizes_valid_multiline_modrinth_descriptions() {
        let result = super::ModSearchHit::try_from(SearchHitResponse {
            project_id: "AABBCCDD".to_owned(),
            slug: Some("chunk-pregenerator".to_owned()),
            title: "Chunk Pregenerator".to_owned(),
            description: "No more lag while exploring!\nLoad chunks beforehand.".to_owned(),
            author: "author".to_owned(),
            downloads: 42,
        })
        .unwrap();

        assert_eq!(
            result.description(),
            "No more lag while exploring! Load chunks beforehand."
        );
    }

    #[test]
    fn parses_every_supported_required_dependency_reference() {
        let exact = RequiredDependency::try_from(&DependencyResponse {
            version_id: Some("Version123".to_owned()),
            project_id: Some("Project123".to_owned()),
            file_name: None,
            dependency_type: "required".to_owned(),
        })
        .unwrap();
        assert_eq!(
            exact,
            RequiredDependency::Version {
                version_id: ModrinthVersionId::parse("Version123").unwrap(),
                expected_project: Some(ModrinthProjectId::parse("Project123").unwrap()),
            }
        );

        let project = RequiredDependency::try_from(&DependencyResponse {
            version_id: None,
            project_id: Some("Project123".to_owned()),
            file_name: None,
            dependency_type: "required".to_owned(),
        })
        .unwrap();
        assert_eq!(
            project,
            RequiredDependency::Project(ModrinthProjectId::parse("Project123").unwrap())
        );

        let external = RequiredDependency::try_from(&DependencyResponse {
            version_id: None,
            project_id: None,
            file_name: Some("library.jar".to_owned()),
            dependency_type: "required".to_owned(),
        })
        .unwrap();
        assert_eq!(
            external,
            RequiredDependency::ExternalFile("library.jar".to_owned())
        );
    }

    #[test]
    fn orders_transitive_dependencies_before_the_requested_mod() {
        let root = graph_release("RootProject", "Root", "RootVersion");
        let middle = graph_release("MiddleProject", "Middle", "MiddleVersion");
        let leaf = graph_release("LeafProject", "Leaf", "LeafVersion");
        let root_id = root.project.id.clone();
        let middle_id = middle.project.id.clone();
        let leaf_id = leaf.project.id.clone();
        let releases = BTreeMap::from([
            (root_id.clone(), root),
            (middle_id.clone(), middle),
            (leaf_id.clone(), leaf),
        ]);
        let edges = BTreeMap::from([
            (root_id.clone(), BTreeSet::from([middle_id.clone()])),
            (middle_id, BTreeSet::from([leaf_id])),
        ]);

        let ordered = order_releases(&root_id, &releases, &edges).unwrap();
        assert_eq!(
            ordered
                .iter()
                .map(|release| release.project.title.as_str())
                .collect::<Vec<_>>(),
            ["Leaf", "Middle", "Root"]
        );
    }

    #[test]
    fn rejects_required_dependency_cycles() {
        let root = graph_release("RootProject", "Root", "RootVersion");
        let dependency = graph_release("DependencyProject", "Dependency", "DependencyVersion");
        let root_id = root.project.id.clone();
        let dependency_id = dependency.project.id.clone();
        let releases =
            BTreeMap::from([(root_id.clone(), root), (dependency_id.clone(), dependency)]);
        let edges = BTreeMap::from([
            (root_id.clone(), BTreeSet::from([dependency_id.clone()])),
            (dependency_id, BTreeSet::from([root_id.clone()])),
        ]);

        assert!(matches!(
            order_releases(&root_id, &releases, &edges),
            Err(ModError::DependencyCycle { .. })
        ));
    }

    fn graph_release(project_id: &str, title: &str, version_id: &str) -> ModRelease {
        let bytes = format!("PK\\x03\\x04{project_id}");
        ModRelease {
            project: ModProject {
                id: ModrinthProjectId::parse(project_id).unwrap(),
                slug: None,
                title: title.to_owned(),
            },
            version_id: ModrinthVersionId::parse(version_id).unwrap(),
            version_number: "1.0.0".to_owned(),
            channel: ReleaseChannel::Release,
            filename: ModFileName::parse(format!("{project_id}.jar")).unwrap(),
            sha512: sha512(bytes.as_bytes()),
            download_url: reqwest::Url::parse("https://cdn.modrinth.com/mod.jar").unwrap(),
            required_dependencies: Vec::new(),
            published_at: "2026-01-01T00:00:00Z".to_owned(),
        }
    }
}
