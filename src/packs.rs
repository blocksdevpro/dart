use crate::instance::Instance;
use crate::mods::{ModrinthProjectId, ModrinthVersionId};
use crate::runtime::FabricVersion;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MODRINTH_API_URL: &str = "https://api.modrinth.com/v2/";
const DART_DIRECTORY: &str = ".dart";
const DATAPACK_MANIFEST_FILE: &str = "datapacks.toml";
const RESOURCE_PACK_MANIFEST_FILE: &str = "resource-pack.toml";
const RESOURCE_PACK_DIRECTORY: &str = "resource-packs";
const SERVER_PROPERTIES_FILE: &str = "server.properties";
const MANIFEST_FORMAT_VERSION: u32 = 1;
const MAX_SEARCH_QUERY_LENGTH: usize = 120;
const MAX_PACK_FILE_BYTES: usize = 256 * 1024 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackKind {
    DataPack,
    ResourcePack,
}

impl PackKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::DataPack => "Data pack",
            Self::ResourcePack => "Resource pack",
        }
    }

    fn project_type(self) -> &'static str {
        match self {
            Self::DataPack => "datapack",
            Self::ResourcePack => "resourcepack",
        }
    }

    fn loader(self) -> &'static str {
        match self {
            Self::DataPack => "datapack",
            Self::ResourcePack => "minecraft",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackProject {
    id: ModrinthProjectId,
    slug: Option<String>,
    title: String,
}

impl PackProject {
    pub fn id(&self) -> &ModrinthProjectId {
        &self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackSearchHit {
    project: PackProject,
    description: String,
    author: String,
    downloads: u64,
}

impl PackSearchHit {
    pub fn project(&self) -> &PackProject {
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

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PackFileName(String);

impl PackFileName {
    fn parse(value: impl AsRef<str>) -> Result<Self, PackError> {
        let value = value.as_ref();
        let path = Path::new(value);
        if value.is_empty()
            || value.len() > 255
            || !value.to_ascii_lowercase().ends_with(".zip")
            || value.contains(['/', '\\', '\n', '\r'])
            || path.file_name().and_then(|name| name.to_str()) != Some(value)
        {
            return Err(PackError::InvalidMetadata(format!(
                "invalid pack ZIP filename '{value}'"
            )));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackFileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug)]
pub struct PackRelease {
    kind: PackKind,
    project: PackProject,
    version_id: ModrinthVersionId,
    version_number: String,
    filename: PackFileName,
    sha1: String,
    sha512: String,
    download_url: reqwest::Url,
}

impl PackRelease {
    pub fn kind(&self) -> PackKind {
        self.kind
    }

    pub fn project(&self) -> &PackProject {
        &self.project
    }

    pub fn version_number(&self) -> &str {
        &self.version_number
    }
}

#[derive(Clone, Debug)]
pub struct PackInstallPlan {
    release: PackRelease,
}

impl PackInstallPlan {
    pub fn release(&self) -> &PackRelease {
        &self.release
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackInstallOutcome {
    Added,
    Updated,
    AlreadyInstalled,
}

#[derive(Clone, Debug)]
pub struct PackInstallReport {
    release: PackRelease,
    outcome: PackInstallOutcome,
}

impl PackInstallReport {
    pub fn release(&self) -> &PackRelease {
        &self.release
    }

    pub fn outcome(&self) -> PackInstallOutcome {
        self.outcome
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManagedPack {
    pub project_id: ModrinthProjectId,
    pub project_slug: Option<String>,
    pub title: String,
    pub version_id: ModrinthVersionId,
    pub version_number: String,
    pub filename: PackFileName,
    pub sha1: String,
    pub sha512: String,
    pub download_url: String,
}

impl ManagedPack {
    fn from_release(release: &PackRelease) -> Self {
        Self {
            project_id: release.project.id.clone(),
            project_slug: release.project.slug.clone(),
            title: release.project.title.clone(),
            version_id: release.version_id.clone(),
            version_number: release.version_number.clone(),
            filename: release.filename.clone(),
            sha1: release.sha1.clone(),
            sha512: release.sha512.clone(),
            download_url: release.download_url.to_string(),
        }
    }

    pub fn project(&self) -> PackProject {
        PackProject {
            id: self.project_id.clone(),
            slug: self.project_slug.clone(),
            title: self.title.clone(),
        }
    }

    fn validate(&self) -> Result<(), PackError> {
        parse_project_id(self.project_id.as_str())?;
        parse_version_id(self.version_id.as_str())?;
        validate_text(&self.title, "pack title", 120)?;
        validate_text(&self.version_number, "pack version", 120)?;
        PackFileName::parse(&self.filename.0)?;
        validate_hash(&self.sha1, 40, "SHA-1")?;
        validate_hash(&self.sha512, 128, "SHA-512")?;
        let url = reqwest::Url::parse(&self.download_url)
            .map_err(|_| PackError::InvalidMetadata("invalid pack download URL".to_owned()))?;
        if url.scheme() != "https" {
            return Err(PackError::InvalidMetadata(
                "pack download URL does not use HTTPS".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstalledPack {
    Managed(ManagedPack),
    ExternalFile { filename: PackFileName },
    ExternalResource { url: String },
}

impl InstalledPack {
    pub fn title(&self) -> &str {
        match self {
            Self::Managed(pack) => &pack.title,
            Self::ExternalFile { filename } => filename.as_str(),
            Self::ExternalResource { url } => url,
        }
    }

    pub fn managed(&self) -> Option<&ManagedPack> {
        match self {
            Self::Managed(pack) => Some(pack),
            Self::ExternalFile { .. } | Self::ExternalResource { .. } => None,
        }
    }

    pub fn selection_key(&self) -> String {
        match self {
            Self::Managed(pack) => pack.project_id.to_string(),
            Self::ExternalFile { filename } => filename.to_string(),
            Self::ExternalResource { url } => url.clone(),
        }
    }
}

#[derive(Clone)]
pub struct PackManager {
    client: PackClient,
    store: PackStore,
}

impl PackManager {
    pub fn new() -> Result<Self, PackError> {
        Ok(Self {
            client: PackClient::new()?,
            store: PackStore,
        })
    }

    pub async fn search(
        &self,
        kind: PackKind,
        query: &str,
        minecraft: &FabricVersion,
    ) -> Result<Vec<PackSearchHit>, PackError> {
        self.client.search(kind, query, minecraft).await
    }

    pub fn list(
        &self,
        instance: &Instance,
        kind: PackKind,
    ) -> Result<Vec<InstalledPack>, PackError> {
        self.store.list(instance, kind)
    }

    pub async fn prepare_install(
        &self,
        kind: PackKind,
        project: PackProject,
        minecraft: &FabricVersion,
    ) -> Result<PackInstallPlan, PackError> {
        Ok(PackInstallPlan {
            release: self
                .client
                .newest_compatible(kind, project, minecraft)
                .await?,
        })
    }

    pub async fn apply_plan(
        &self,
        instance: &Instance,
        plan: &PackInstallPlan,
    ) -> Result<PackInstallReport, PackError> {
        let outcome = if self.store.release_is_intact(instance, &plan.release)? {
            PackInstallOutcome::AlreadyInstalled
        } else {
            let bytes = self.client.download(&plan.release).await?;
            self.store.install(instance, &plan.release, &bytes)?
        };
        Ok(PackInstallReport {
            release: plan.release.clone(),
            outcome,
        })
    }

    pub fn remove(
        &self,
        instance: &Instance,
        kind: PackKind,
        pack: &ManagedPack,
    ) -> Result<(), PackError> {
        self.store.remove(instance, kind, pack)
    }
}

#[derive(Clone)]
struct PackClient {
    client: reqwest::Client,
    base_url: reqwest::Url,
}

impl PackClient {
    fn new() -> Result<Self, PackError> {
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

    async fn search(
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

    async fn newest_compatible(
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

    async fn download(&self, release: &PackRelease) -> Result<Vec<u8>, PackError> {
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

fn channel_name_priority(channel: &str) -> u8 {
    match channel {
        "release" => 0,
        "beta" => 1,
        "alpha" => 2,
        _ => 3,
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    hits: Vec<SearchHitResponse>,
}

#[derive(Debug, Deserialize)]
struct SearchHitResponse {
    project_id: String,
    project_type: String,
    #[serde(default)]
    all_project_types: Vec<String>,
    slug: Option<String>,
    title: String,
    description: String,
    author: String,
    downloads: u64,
}

impl TryFrom<SearchHitResponse> for PackSearchHit {
    type Error = PackError;

    fn try_from(value: SearchHitResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            project: PackProject {
                id: parse_project_id(value.project_id)?,
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
    dependency_type: String,
}

impl PackRelease {
    fn from_version(
        kind: PackKind,
        project: PackProject,
        version: VersionResponse,
    ) -> Result<Self, PackError> {
        if channel_name_priority(&version.version_type) == 3 {
            return Err(PackError::InvalidMetadata(format!(
                "unknown Modrinth version type '{}'",
                version.version_type
            )));
        }
        let required_dependencies = version
            .dependencies
            .iter()
            .filter(|dependency| dependency.dependency_type == "required")
            .count();
        if required_dependencies > 0 {
            return Err(PackError::RequiredDependencies {
                project: project.title.clone(),
                count: required_dependencies,
            });
        }
        let file = version
            .files
            .iter()
            .find(|file| file.primary && file.filename.to_ascii_lowercase().ends_with(".zip"))
            .or_else(|| {
                version
                    .files
                    .iter()
                    .find(|file| file.filename.to_ascii_lowercase().ends_with(".zip"))
            })
            .ok_or_else(|| {
                PackError::InvalidMetadata(format!(
                    "Modrinth version '{}' has no downloadable ZIP",
                    version.id
                ))
            })?;
        let sha1 = file.hashes.get("sha1").cloned().ok_or_else(|| {
            PackError::InvalidMetadata(format!(
                "Modrinth version '{}' has no SHA-1 hash",
                version.id
            ))
        })?;
        let sha512 = file.hashes.get("sha512").cloned().ok_or_else(|| {
            PackError::InvalidMetadata(format!(
                "Modrinth version '{}' has no SHA-512 hash",
                version.id
            ))
        })?;
        validate_hash(&sha1, 40, "SHA-1")?;
        validate_hash(&sha512, 128, "SHA-512")?;
        let download_url = reqwest::Url::parse(&file.url).map_err(|_| {
            PackError::InvalidMetadata(format!(
                "Modrinth version '{}' has an invalid download URL",
                version.id
            ))
        })?;
        if download_url.scheme() != "https" {
            return Err(PackError::InvalidMetadata(format!(
                "Modrinth version '{}' does not use an HTTPS download URL",
                version.id
            )));
        }
        Ok(Self {
            kind,
            project,
            version_id: parse_version_id(version.id)?,
            version_number: validate_text(version.version_number, "Modrinth version number", 120)?,
            filename: PackFileName::parse(&file.filename)?,
            sha1,
            sha512,
            download_url,
        })
    }
}

#[derive(Clone, Debug, Default)]
struct PackStore;

impl PackStore {
    fn list(&self, instance: &Instance, kind: PackKind) -> Result<Vec<InstalledPack>, PackError> {
        match kind {
            PackKind::DataPack => self.list_datapacks(instance),
            PackKind::ResourcePack => self.list_resource_pack(instance),
        }
    }

    fn install(
        &self,
        instance: &Instance,
        release: &PackRelease,
        bytes: &[u8],
    ) -> Result<PackInstallOutcome, PackError> {
        validate_pack_zip(bytes)?;
        let actual_hash = sha512(bytes);
        if !actual_hash.eq_ignore_ascii_case(&release.sha512) {
            return Err(PackError::HashMismatch {
                expected: release.sha512.clone(),
                actual: actual_hash,
            });
        }
        match release.kind {
            PackKind::DataPack => self.install_datapack(instance, release, bytes),
            PackKind::ResourcePack => self.install_resource_pack(instance, release, bytes),
        }
    }

    fn remove(
        &self,
        instance: &Instance,
        kind: PackKind,
        pack: &ManagedPack,
    ) -> Result<(), PackError> {
        match kind {
            PackKind::DataPack => self.remove_datapack(instance, pack),
            PackKind::ResourcePack => self.remove_resource_pack(instance, pack),
        }
    }

    fn release_is_intact(
        &self,
        instance: &Instance,
        release: &PackRelease,
    ) -> Result<bool, PackError> {
        let manifest = self.load_manifest(instance, release.kind)?;
        let Some(current) = manifest
            .packs
            .iter()
            .find(|pack| pack.project_id == release.project.id)
        else {
            return Ok(false);
        };
        if current.version_id != release.version_id
            || current.filename != release.filename
            || !current.sha512.eq_ignore_ascii_case(&release.sha512)
        {
            return Ok(false);
        }
        let path = self.pack_path(instance, release.kind, current)?;
        if !file_matches_hash(&path, &current.sha512)? {
            return Ok(false);
        }
        if release.kind == PackKind::ResourcePack {
            let properties = self.load_properties(instance)?;
            return Ok(property_value(&properties, "resource-pack")
                == Some(current.download_url.as_str())
                && property_value(&properties, "resource-pack-sha1")
                    .is_some_and(|hash| hash.eq_ignore_ascii_case(&current.sha1)));
        }
        Ok(true)
    }

    fn list_datapacks(&self, instance: &Instance) -> Result<Vec<InstalledPack>, PackError> {
        let manifest = self.load_manifest(instance, PackKind::DataPack)?;
        let directory = self.datapacks_dir(instance)?;
        let files = zip_files(&directory)?;
        let managed = manifest
            .packs
            .into_iter()
            .map(|pack| (pack.filename.clone(), pack))
            .collect::<BTreeMap<_, _>>();
        let mut installed = files
            .into_iter()
            .map(|filename| match managed.get(&filename) {
                Some(pack) => InstalledPack::Managed(pack.clone()),
                None => InstalledPack::ExternalFile { filename },
            })
            .collect::<Vec<_>>();
        installed.sort_by_key(|pack| pack.title().to_lowercase());
        Ok(installed)
    }

    fn list_resource_pack(&self, instance: &Instance) -> Result<Vec<InstalledPack>, PackError> {
        let manifest = self.load_manifest(instance, PackKind::ResourcePack)?;
        let properties = self.load_properties(instance)?;
        let configured_url = property_value(&properties, "resource-pack")
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned);
        if let Some(pack) = manifest.packs.into_iter().next()
            && configured_url.as_deref() == Some(pack.download_url.as_str())
        {
            return Ok(vec![InstalledPack::Managed(pack)]);
        }
        Ok(configured_url
            .map(|url| vec![InstalledPack::ExternalResource { url }])
            .unwrap_or_default())
    }

    fn install_datapack(
        &self,
        instance: &Instance,
        release: &PackRelease,
        bytes: &[u8],
    ) -> Result<PackInstallOutcome, PackError> {
        let mut manifest = self.load_manifest(instance, PackKind::DataPack)?;
        let new_entry = ManagedPack::from_release(release);
        let existing = manifest
            .packs
            .iter()
            .find(|pack| pack.project_id == new_entry.project_id)
            .cloned();
        let directory = self.datapacks_dir(instance)?;
        fs::create_dir_all(&directory).map_err(|source| {
            PackError::io(
                "create world datapacks directory",
                directory.clone(),
                source,
            )
        })?;
        let target = directory.join(new_entry.filename.as_str());
        write_managed_file(&target, bytes, existing.as_ref(), &new_entry)?;
        manifest
            .packs
            .retain(|pack| pack.project_id != new_entry.project_id);
        manifest.packs.push(new_entry.clone());
        manifest.sort_and_validate()?;
        self.save_manifest(instance, PackKind::DataPack, &manifest)?;
        remove_replaced_file(&directory, existing.as_ref(), &new_entry)?;
        Ok(if existing.is_some() {
            PackInstallOutcome::Updated
        } else {
            PackInstallOutcome::Added
        })
    }

    fn install_resource_pack(
        &self,
        instance: &Instance,
        release: &PackRelease,
        bytes: &[u8],
    ) -> Result<PackInstallOutcome, PackError> {
        let mut manifest = self.load_manifest(instance, PackKind::ResourcePack)?;
        let properties = self.load_properties(instance)?;
        let configured_url =
            property_value(&properties, "resource-pack").filter(|value| !value.trim().is_empty());
        let existing = manifest.packs.first().cloned();
        if let Some(url) = configured_url
            && existing.as_ref().map(|pack| pack.download_url.as_str()) != Some(url)
        {
            return Err(PackError::ExternalResourcePack {
                url: url.to_owned(),
            });
        }
        if let Some(existing) = existing.as_ref()
            && configured_url == Some(existing.download_url.as_str())
            && property_value(&properties, "resource-pack-sha1")
                .is_some_and(|hash| !hash.eq_ignore_ascii_case(&existing.sha1))
        {
            return Err(PackError::ManagedConfigurationChanged);
        }
        let new_entry = ManagedPack::from_release(release);
        let directory = self.resource_pack_dir(instance);
        fs::create_dir_all(&directory).map_err(|source| {
            PackError::io(
                "create Dart resource-pack directory",
                directory.clone(),
                source,
            )
        })?;
        let target = directory.join(new_entry.filename.as_str());
        write_managed_file(&target, bytes, existing.as_ref(), &new_entry)?;
        manifest.packs.clear();
        manifest.packs.push(new_entry.clone());
        manifest.sort_and_validate()?;
        self.save_manifest(instance, PackKind::ResourcePack, &manifest)?;
        self.save_resource_pack_properties(instance, &new_entry)?;
        remove_replaced_file(&directory, existing.as_ref(), &new_entry)?;
        Ok(if existing.is_some() {
            PackInstallOutcome::Updated
        } else {
            PackInstallOutcome::Added
        })
    }

    fn remove_datapack(&self, instance: &Instance, pack: &ManagedPack) -> Result<(), PackError> {
        let mut manifest = self.load_manifest(instance, PackKind::DataPack)?;
        let Some(existing) = manifest
            .packs
            .iter()
            .find(|entry| entry.project_id == pack.project_id)
            .cloned()
        else {
            return Ok(());
        };
        let path = self
            .datapacks_dir(instance)?
            .join(existing.filename.as_str());
        ensure_managed_file_unchanged(&path, &existing.sha512)?;
        manifest
            .packs
            .retain(|entry| entry.project_id != existing.project_id);
        self.save_manifest(instance, PackKind::DataPack, &manifest)?;
        remove_existing_file(&path)
    }

    fn remove_resource_pack(
        &self,
        instance: &Instance,
        pack: &ManagedPack,
    ) -> Result<(), PackError> {
        let mut manifest = self.load_manifest(instance, PackKind::ResourcePack)?;
        let Some(existing) = manifest
            .packs
            .iter()
            .find(|entry| entry.project_id == pack.project_id)
            .cloned()
        else {
            return Ok(());
        };
        let properties = self.load_properties(instance)?;
        if property_value(&properties, "resource-pack") != Some(existing.download_url.as_str())
            || !property_value(&properties, "resource-pack-sha1")
                .is_some_and(|hash| hash.eq_ignore_ascii_case(&existing.sha1))
        {
            return Err(PackError::ManagedConfigurationChanged);
        }
        let path = self
            .resource_pack_dir(instance)
            .join(existing.filename.as_str());
        ensure_managed_file_unchanged(&path, &existing.sha512)?;
        manifest.packs.clear();
        self.save_manifest(instance, PackKind::ResourcePack, &manifest)?;
        self.clear_resource_pack_properties(instance)?;
        remove_existing_file(&path)
    }

    fn datapacks_dir(&self, instance: &Instance) -> Result<PathBuf, PackError> {
        Ok(instance
            .root()
            .join(self.level_name(instance)?)
            .join("datapacks"))
    }

    fn resource_pack_dir(&self, instance: &Instance) -> PathBuf {
        instance
            .root()
            .join(DART_DIRECTORY)
            .join(RESOURCE_PACK_DIRECTORY)
    }

    fn level_name(&self, instance: &Instance) -> Result<String, PackError> {
        let properties = self.load_properties(instance)?;
        let name = property_value(&properties, "level-name").unwrap_or("world");
        validate_level_name(name)
    }

    fn pack_path(
        &self,
        instance: &Instance,
        kind: PackKind,
        pack: &ManagedPack,
    ) -> Result<PathBuf, PackError> {
        Ok(match kind {
            PackKind::DataPack => self.datapacks_dir(instance)?.join(pack.filename.as_str()),
            PackKind::ResourcePack => self
                .resource_pack_dir(instance)
                .join(pack.filename.as_str()),
        })
    }

    fn manifest_path(&self, instance: &Instance, kind: PackKind) -> PathBuf {
        instance.root().join(DART_DIRECTORY).join(match kind {
            PackKind::DataPack => DATAPACK_MANIFEST_FILE,
            PackKind::ResourcePack => RESOURCE_PACK_MANIFEST_FILE,
        })
    }

    fn load_manifest(
        &self,
        instance: &Instance,
        kind: PackKind,
    ) -> Result<PackManifest, PackError> {
        let path = self.manifest_path(instance, kind);
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(PackManifest::default());
            }
            Err(source) => return Err(PackError::io("read pack manifest", path, source)),
        };
        let mut manifest: PackManifest =
            toml::from_str(&contents).map_err(|source| PackError::ParseManifest {
                path: path.clone(),
                source,
            })?;
        manifest.sort_and_validate().map_err(|error| match error {
            PackError::InvalidMetadata(message) => PackError::InvalidManifest {
                path: path.clone(),
                message,
            },
            other => other,
        })?;
        if kind == PackKind::ResourcePack && manifest.packs.len() > 1 {
            return Err(PackError::InvalidManifest {
                path,
                message: "resource-pack manifest contains more than one pack".to_owned(),
            });
        }
        Ok(manifest)
    }

    fn save_manifest(
        &self,
        instance: &Instance,
        kind: PackKind,
        manifest: &PackManifest,
    ) -> Result<(), PackError> {
        let path = self.manifest_path(instance, kind);
        let parent = path
            .parent()
            .expect("pack manifest paths always have a parent")
            .to_owned();
        fs::create_dir_all(&parent)
            .map_err(|source| PackError::io("create Dart state directory", parent, source))?;
        let contents = toml::to_string_pretty(manifest).map_err(PackError::SerializeManifest)?;
        atomic_write(&path, contents.as_bytes(), true)
    }

    fn load_properties(&self, instance: &Instance) -> Result<String, PackError> {
        let path = instance.root().join(SERVER_PROPERTIES_FILE);
        match fs::read_to_string(&path) {
            Ok(contents) => Ok(contents),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            Err(source) => Err(PackError::io("read server properties", path, source)),
        }
    }

    fn save_resource_pack_properties(
        &self,
        instance: &Instance,
        pack: &ManagedPack,
    ) -> Result<(), PackError> {
        let contents = self.load_properties(instance)?;
        let updated = update_properties(
            &contents,
            &[
                ("resource-pack", pack.download_url.as_str()),
                ("resource-pack-sha1", pack.sha1.as_str()),
            ],
        );
        atomic_write(
            &instance.root().join(SERVER_PROPERTIES_FILE),
            updated.as_bytes(),
            true,
        )
    }

    fn clear_resource_pack_properties(&self, instance: &Instance) -> Result<(), PackError> {
        let contents = self.load_properties(instance)?;
        let updated = update_properties(
            &contents,
            &[("resource-pack", ""), ("resource-pack-sha1", "")],
        );
        atomic_write(
            &instance.root().join(SERVER_PROPERTIES_FILE),
            updated.as_bytes(),
            true,
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PackManifest {
    format_version: u32,
    #[serde(default)]
    packs: Vec<ManagedPack>,
}

impl Default for PackManifest {
    fn default() -> Self {
        Self {
            format_version: MANIFEST_FORMAT_VERSION,
            packs: Vec::new(),
        }
    }
}

impl PackManifest {
    fn sort_and_validate(&mut self) -> Result<(), PackError> {
        if self.format_version != MANIFEST_FORMAT_VERSION {
            return Err(PackError::InvalidMetadata(format!(
                "unsupported pack manifest format version {}",
                self.format_version
            )));
        }
        let mut projects = BTreeSet::new();
        let mut filenames = BTreeSet::new();
        for pack in &self.packs {
            pack.validate()?;
            if !projects.insert(pack.project_id.clone()) {
                return Err(PackError::InvalidMetadata(format!(
                    "duplicate Modrinth project {} in the pack manifest",
                    pack.project_id
                )));
            }
            if !filenames.insert(pack.filename.clone()) {
                return Err(PackError::InvalidMetadata(format!(
                    "duplicate pack file {} in the pack manifest",
                    pack.filename
                )));
            }
        }
        self.packs.sort_by_key(|pack| pack.title.to_lowercase());
        Ok(())
    }
}

fn write_managed_file(
    target: &Path,
    bytes: &[u8],
    existing: Option<&ManagedPack>,
    new_entry: &ManagedPack,
) -> Result<(), PackError> {
    match file_matches_hash(target, &new_entry.sha512) {
        Ok(true) => Ok(()),
        Ok(false) if target.exists() => {
            let current_is_managed = existing.is_some_and(|pack| {
                pack.filename == new_entry.filename
                    && file_matches_hash(target, &pack.sha512).unwrap_or(false)
            });
            if !current_is_managed {
                return Err(PackError::FileConflict {
                    path: target.to_owned(),
                });
            }
            atomic_write(target, bytes, true)
        }
        Ok(false) => atomic_write(target, bytes, false),
        Err(error) => Err(error),
    }
}

fn remove_replaced_file(
    directory: &Path,
    existing: Option<&ManagedPack>,
    new_entry: &ManagedPack,
) -> Result<(), PackError> {
    if let Some(previous) = existing
        && previous.filename != new_entry.filename
    {
        let path = directory.join(previous.filename.as_str());
        if file_matches_hash(&path, &previous.sha512)? {
            fs::remove_file(&path)
                .map_err(|source| PackError::io("remove replaced managed pack", path, source))?;
        }
    }
    Ok(())
}

fn ensure_managed_file_unchanged(path: &Path, expected_hash: &str) -> Result<(), PackError> {
    if path.exists() && !file_matches_hash(path, expected_hash)? {
        return Err(PackError::ManagedFileChanged {
            path: path.to_owned(),
        });
    }
    Ok(())
}

fn remove_existing_file(path: &Path) -> Result<(), PackError> {
    if path.exists() {
        fs::remove_file(path)
            .map_err(|source| PackError::io("remove managed pack", path.to_owned(), source))?;
    }
    Ok(())
}

fn zip_files(directory: &Path) -> Result<BTreeSet<PackFileName>, PackError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(source) => {
            return Err(PackError::io(
                "read pack directory",
                directory.to_owned(),
                source,
            ));
        }
    };
    let mut files = BTreeSet::new();
    for entry in entries {
        let entry = entry.map_err(|source| {
            PackError::io("read pack directory entry", directory.to_owned(), source)
        })?;
        if !entry
            .file_type()
            .map_err(|source| PackError::io("inspect pack file", entry.path(), source))?
            .is_file()
        {
            continue;
        }
        let Some(filename) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if filename.to_ascii_lowercase().ends_with(".zip") {
            files.insert(PackFileName::parse(filename)?);
        }
    }
    Ok(files)
}

fn property_value<'a>(contents: &'a str, key: &str) -> Option<&'a str> {
    contents.lines().find_map(|line| {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with('!') {
            return None;
        }
        let (candidate, value) = split_property(line)?;
        (candidate.trim() == key).then_some(value.trim())
    })
}

fn split_property(line: &str) -> Option<(&str, &str)> {
    let separator = line
        .char_indices()
        .find(|(_, character)| matches!(character, '=' | ':'))?
        .0;
    Some((&line[..separator], &line[separator + 1..]))
}

fn update_properties(contents: &str, values: &[(&str, &str)]) -> String {
    let keys = values.iter().map(|(key, _)| *key).collect::<BTreeSet<_>>();
    let mut lines = contents
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.starts_with('!') {
                return true;
            }
            split_property(trimmed).is_none_or(|(key, _)| !keys.contains(key.trim()))
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for (key, value) in values {
        lines.push(format!("{key}={value}"));
    }
    let mut updated = lines.join("\n");
    updated.push('\n');
    updated
}

fn validate_level_name(value: &str) -> Result<String, PackError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 128
        || matches!(value, "." | "..")
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | '_' | '.')
        })
    {
        return Err(PackError::UnsafeLevelName(value.to_owned()));
    }
    Ok(value.to_owned())
}

fn validate_text(value: impl AsRef<str>, field: &str, maximum: usize) -> Result<String, PackError> {
    let value = value.as_ref().trim();
    if value.is_empty() || value.chars().count() > maximum || value.contains(['\n', '\r']) {
        return Err(PackError::InvalidMetadata(format!("invalid {field}")));
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

fn parse_project_id(value: impl AsRef<str>) -> Result<ModrinthProjectId, PackError> {
    ModrinthProjectId::parse(value.as_ref()).map_err(|_| {
        PackError::InvalidMetadata(format!("invalid Modrinth project ID '{}'", value.as_ref()))
    })
}

fn parse_version_id(value: impl AsRef<str>) -> Result<ModrinthVersionId, PackError> {
    ModrinthVersionId::parse(value.as_ref()).map_err(|_| {
        PackError::InvalidMetadata(format!("invalid Modrinth version ID '{}'", value.as_ref()))
    })
}

fn validate_hash(value: &str, length: usize, name: &str) -> Result<(), PackError> {
    if value.len() != length || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(PackError::InvalidMetadata(format!(
            "invalid Modrinth {name} hash"
        )));
    }
    Ok(())
}

fn validate_pack_zip(bytes: &[u8]) -> Result<(), PackError> {
    if bytes.len() < 4 || !bytes.starts_with(b"PK") {
        return Err(PackError::InvalidZip);
    }
    Ok(())
}

fn sha512(bytes: &[u8]) -> String {
    let digest = Sha512::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn file_matches_hash(path: &Path, expected: &str) -> Result<bool, PackError> {
    let mut file = match OpenOptions::new().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(source) => return Err(PackError::io("open managed pack", path.to_owned(), source)),
    };
    let mut digest = Sha512::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| PackError::io("read managed pack", path.to_owned(), source))?;
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

fn atomic_write(target: &Path, bytes: &[u8], replace: bool) -> Result<(), PackError> {
    let parent = target
        .parent()
        .expect("managed pack paths always have a parent");
    fs::create_dir_all(parent)
        .map_err(|source| PackError::io("create managed pack parent", parent.to_owned(), source))?;
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".dart-pack-write.{}.{sequence}",
        std::process::id()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|source| {
                PackError::io("create temporary pack file", temporary.clone(), source)
            })?;
        file.write_all(bytes).map_err(|source| {
            PackError::io("write temporary pack file", temporary.clone(), source)
        })?;
        file.sync_all().map_err(|source| {
            PackError::io("sync temporary pack file", temporary.clone(), source)
        })?;
        if replace {
            if cfg!(windows) && target.exists() {
                fs::remove_file(target).map_err(|source| {
                    PackError::io("replace managed pack", target.to_owned(), source)
                })?;
            }
            fs::rename(&temporary, target)
                .map_err(|source| PackError::io("finish pack write", target.to_owned(), source))
        } else {
            fs::hard_link(&temporary, target)
                .map_err(|source| PackError::io("finish pack write", target.to_owned(), source))?;
            fs::remove_file(&temporary).map_err(|source| {
                PackError::io("remove temporary pack file", temporary.clone(), source)
            })
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[derive(Debug)]
pub enum PackError {
    EmptySearch,
    SearchTooLong,
    NoCompatibleVersion {
        kind: PackKind,
        project: String,
        minecraft: String,
    },
    RequiredDependencies {
        project: String,
        count: usize,
    },
    InvalidMetadata(String),
    InvalidZip,
    DownloadTooLarge,
    HashMismatch {
        expected: String,
        actual: String,
    },
    UnsafeLevelName(String),
    FileConflict {
        path: PathBuf,
    },
    ManagedFileChanged {
        path: PathBuf,
    },
    ExternalResourcePack {
        url: String,
    },
    ManagedConfigurationChanged,
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

impl PackError {
    fn io(operation: &'static str, path: PathBuf, source: io::Error) -> Self {
        Self::Io {
            operation,
            path,
            source,
        }
    }
}

impl fmt::Display for PackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySearch => formatter.write_str("enter a Modrinth search query"),
            Self::SearchTooLong => formatter.write_str("the Modrinth search query is too long"),
            Self::NoCompatibleVersion {
                kind,
                project,
                minecraft,
            } => write!(
                formatter,
                "Modrinth has no listed {} version of '{project}' for Minecraft {minecraft}",
                kind.label().to_lowercase()
            ),
            Self::RequiredDependencies { project, count } => write!(
                formatter,
                "'{project}' has {count} required pack dependencies that Dart cannot place safely yet"
            ),
            Self::InvalidMetadata(message) => write!(formatter, "invalid Modrinth metadata: {message}"),
            Self::InvalidZip => formatter.write_str("downloaded pack is not a valid ZIP archive"),
            Self::DownloadTooLarge => formatter.write_str("Modrinth pack download exceeds Dart's 256 MiB limit"),
            Self::HashMismatch { expected, actual } => write!(
                formatter,
                "downloaded pack checksum did not match Modrinth (expected {expected}, got {actual})"
            ),
            Self::UnsafeLevelName(name) => write!(
                formatter,
                "refusing to install a data pack because level-name '{name}' is not a safe instance directory"
            ),
            Self::FileConflict { path } => write!(
                formatter,
                "refusing to overwrite existing pack file {}",
                path.display()
            ),
            Self::ManagedFileChanged { path } => write!(
                formatter,
                "refusing to remove {} because its contents changed outside Dart",
                path.display()
            ),
            Self::ExternalResourcePack { url } => write!(
                formatter,
                "server.properties already configures an external resource pack at {url}"
            ),
            Self::ManagedConfigurationChanged => formatter.write_str(
                "refusing to change the resource pack because server.properties changed outside Dart",
            ),
            Self::Http(source) => write!(formatter, "Modrinth request failed: {source}"),
            Self::Io { operation, path, .. } => write!(formatter, "cannot {operation} at {}", path.display()),
            Self::ParseManifest { path, source } => write!(formatter, "cannot parse {}: {source}", path.display()),
            Self::SerializeManifest(source) => write!(formatter, "cannot serialize pack manifest: {source}"),
            Self::InvalidManifest { path, message } => write!(
                formatter,
                "invalid pack manifest at {}: {message}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for PackError {
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
        InstalledPack, PackError, PackFileName, PackInstallOutcome, PackKind, PackProject,
        PackRelease, PackStore, sha512,
    };
    use crate::instance::{FabricLaunch, Instance, InstanceConfig, InstanceId, InstanceName};
    use crate::mods::{ModrinthProjectId, ModrinthVersionId};
    use crate::runtime::FabricRuntime;
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
                .join(format!("dart-pack-test-{}-{sequence}", std::process::id()));
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
        let root = directory.0.join("survival");
        fs::create_dir(&root).unwrap();
        Instance::new(
            InstanceId::from_str("survival").unwrap(),
            root,
            InstanceConfig::new(
                InstanceName::parse("Survival").unwrap(),
                FabricLaunch::default(),
                FabricRuntime::new("26.1", "0.19.3", "1.1.2").unwrap(),
            ),
        )
    }

    fn release(kind: PackKind, project: &str, filename: &str, bytes: &[u8]) -> PackRelease {
        PackRelease {
            kind,
            project: PackProject {
                id: ModrinthProjectId::parse(project).unwrap(),
                slug: None,
                title: format!("{project} title"),
            },
            version_id: ModrinthVersionId::parse(format!("{project}Version")).unwrap(),
            version_number: "1.0.0".to_owned(),
            filename: PackFileName::parse(filename).unwrap(),
            sha1: "a".repeat(40),
            sha512: sha512(bytes),
            download_url: reqwest::Url::parse(&format!(
                "https://cdn.modrinth.com/data/{project}/{filename}"
            ))
            .unwrap(),
        }
    }

    #[test]
    fn installs_datapacks_into_the_configured_world_and_preserves_external_zips() {
        let directory = TestDirectory::new();
        let instance = instance(&directory);
        fs::write(
            instance.root().join("server.properties"),
            "motd=Hello\nlevel-name=custom-world\n",
        )
        .unwrap();
        let store = PackStore;
        let datapacks = instance.root().join("custom-world/datapacks");
        fs::create_dir_all(&datapacks).unwrap();
        fs::write(datapacks.join("external.zip"), b"PK\x03\x04external").unwrap();
        let bytes = b"PK\x03\x04managed datapack";
        let release = release(PackKind::DataPack, "DataPack1", "managed.zip", bytes);

        assert_eq!(
            store.install(&instance, &release, bytes).unwrap(),
            PackInstallOutcome::Added
        );
        assert!(datapacks.join("managed.zip").is_file());
        assert!(store.release_is_intact(&instance, &release).unwrap());
        let installed = store.list(&instance, PackKind::DataPack).unwrap();
        assert_eq!(installed.len(), 2);
        assert!(
            installed
                .iter()
                .any(|pack| matches!(pack, InstalledPack::ExternalFile { .. }))
        );

        let managed = installed
            .iter()
            .find_map(InstalledPack::managed)
            .unwrap()
            .clone();
        store
            .remove(&instance, PackKind::DataPack, &managed)
            .unwrap();
        assert!(!datapacks.join("managed.zip").exists());
        assert!(datapacks.join("external.zip").is_file());
    }

    #[test]
    fn rejects_a_level_name_that_can_escape_the_instance() {
        let directory = TestDirectory::new();
        let instance = instance(&directory);
        fs::write(
            instance.root().join("server.properties"),
            "level-name=../outside\n",
        )
        .unwrap();
        let bytes = b"PK\x03\x04managed datapack";
        let release = release(PackKind::DataPack, "DataPack1", "managed.zip", bytes);

        assert!(matches!(
            PackStore.install(&instance, &release, bytes),
            Err(PackError::UnsafeLevelName(_))
        ));
        assert!(!directory.0.join("outside/datapacks/managed.zip").exists());
    }

    #[test]
    fn configures_and_removes_a_verified_server_resource_pack() {
        let directory = TestDirectory::new();
        let instance = instance(&directory);
        fs::write(
            instance.root().join("server.properties"),
            "# Minecraft properties\nmotd=Keep me\nresource-pack=\nresource-pack-sha1=\n",
        )
        .unwrap();
        let store = PackStore;
        let bytes = b"PK\x03\x04managed resource pack";
        let release = release(PackKind::ResourcePack, "Resource1", "resources.zip", bytes);

        store.install(&instance, &release, bytes).unwrap();
        let properties = fs::read_to_string(instance.root().join("server.properties")).unwrap();
        assert!(properties.contains("motd=Keep me"));
        assert!(properties.contains(&format!("resource-pack={}", release.download_url)));
        assert!(properties.contains(&format!("resource-pack-sha1={}", release.sha1)));
        assert!(store.release_is_intact(&instance, &release).unwrap());

        let installed = store.list(&instance, PackKind::ResourcePack).unwrap();
        let managed = installed[0].managed().unwrap().clone();
        store
            .remove(&instance, PackKind::ResourcePack, &managed)
            .unwrap();
        let properties = fs::read_to_string(instance.root().join("server.properties")).unwrap();
        assert!(properties.contains("motd=Keep me"));
        assert!(properties.contains("resource-pack=\n"));
        assert!(properties.contains("resource-pack-sha1=\n"));
        assert!(
            store
                .list(&instance, PackKind::ResourcePack)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn refuses_to_replace_an_external_server_resource_pack() {
        let directory = TestDirectory::new();
        let instance = instance(&directory);
        fs::write(
            instance.root().join("server.properties"),
            "resource-pack:https://example.com/external.zip\nresource-pack-sha1:abc\n",
        )
        .unwrap();
        let bytes = b"PK\x03\x04managed resource pack";
        let release = release(PackKind::ResourcePack, "Resource1", "resources.zip", bytes);

        assert!(matches!(
            PackStore.install(&instance, &release, bytes),
            Err(PackError::ExternalResourcePack { .. })
        ));
        assert!(!instance.root().join(".dart/resource-pack.toml").exists());
    }
}
