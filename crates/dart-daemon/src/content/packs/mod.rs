//! Modrinth data pack and resource pack domain model, verification, and manifest storage.

use crate::content::mods::{ModrinthProjectId, ModrinthVersionId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::OpenOptions;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

mod client;
mod manager;
mod store;

pub use client::PackClient;
pub use manager::PackManager;
pub use store::PackStore;

const MODRINTH_API_URL: &str = "https://api.modrinth.com/v2/";
const DART_DIRECTORY: &str = ".dart";
const DATAPACK_MANIFEST_FILE: &str = "datapacks.toml";
const RESOURCE_PACK_MANIFEST_FILE: &str = "resource-pack.toml";
const RESOURCE_PACK_DIRECTORY: &str = "resource-packs";
const SERVER_PROPERTIES_FILE: &str = "server.properties";
const MANIFEST_FORMAT_VERSION: u32 = 1;
const MAX_SEARCH_QUERY_LENGTH: usize = 120;
const MAX_PACK_FILE_BYTES: usize = 256 * 1024 * 1024;

/// Classification of pack content (data pack or server resource pack).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackKind {
    /// Server-side world data pack (`<world>/datapacks/`).
    DataPack,
    /// Client-downloaded server resource pack (`server.properties`).
    ResourcePack,
}

impl PackKind {
    /// Returns the display label for the pack kind.
    pub fn label(self) -> &'static str {
        match self {
            Self::DataPack => "Data pack",
            Self::ResourcePack => "Resource pack",
        }
    }

    pub(crate) fn project_type(self) -> &'static str {
        match self {
            Self::DataPack => "datapack",
            Self::ResourcePack => "resourcepack",
        }
    }

    pub(crate) fn loader(self) -> &'static str {
        match self {
            Self::DataPack => "datapack",
            Self::ResourcePack => "minecraft",
        }
    }
}

/// Pack project identity metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackProject {
    pub(crate) id: ModrinthProjectId,
    pub(crate) slug: Option<String>,
    pub(crate) title: String,
}

impl PackProject {
    /// Returns the Modrinth project ID.
    pub fn id(&self) -> &ModrinthProjectId {
        &self.id
    }

    /// Returns the project title.
    pub fn title(&self) -> &str {
        &self.title
    }
}

/// A single hit in Modrinth pack search results.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackSearchHit {
    project: PackProject,
    description: String,
    author: String,
    downloads: u64,
}

impl PackSearchHit {
    /// Returns the matched project.
    pub fn project(&self) -> &PackProject {
        &self.project
    }

    /// Returns the project description.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns the project author.
    pub fn author(&self) -> &str {
        &self.author
    }

    /// Returns the total download count.
    pub fn downloads(&self) -> u64 {
        self.downloads
    }
}

/// A sanitized ZIP filename for a data pack or resource pack.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PackFileName(String);

impl PackFileName {
    /// Validates and parses a pack filename.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, PackError> {
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

    /// Returns the filename as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackFileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Downloadable release metadata for a specific pack version.
#[derive(Clone, Debug)]
pub struct PackRelease {
    pub(crate) kind: PackKind,
    pub(crate) project: PackProject,
    pub(crate) version_id: ModrinthVersionId,
    pub(crate) version_number: String,
    pub(crate) filename: PackFileName,
    pub(crate) sha1: String,
    pub(crate) sha512: String,
    pub(crate) download_url: reqwest::Url,
}

impl PackRelease {
    /// Returns the kind of pack.
    pub fn kind(&self) -> PackKind {
        self.kind
    }

    /// Returns the pack project metadata.
    pub fn project(&self) -> &PackProject {
        &self.project
    }

    /// Returns the version number string.
    pub fn version_number(&self) -> &str {
        &self.version_number
    }
}

/// An execution plan for installing a pack release.
#[derive(Clone, Debug)]
pub struct PackInstallPlan {
    pub(crate) release: PackRelease,
}

impl PackInstallPlan {
    /// Returns the release being installed.
    pub fn release(&self) -> &PackRelease {
        &self.release
    }
}

/// The outcome of a pack install operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackInstallOutcome {
    /// Added as a new pack.
    Added,
    /// Upgraded an existing pack version.
    Updated,
    /// Already present with matching hash and version.
    AlreadyInstalled,
}

/// The result report after applying a pack install plan.
#[derive(Clone, Debug)]
pub struct PackInstallReport {
    release: PackRelease,
    outcome: PackInstallOutcome,
}

impl PackInstallReport {
    /// Returns the release installed.
    pub fn release(&self) -> &PackRelease {
        &self.release
    }

    /// Returns the installation outcome.
    pub fn outcome(&self) -> PackInstallOutcome {
        self.outcome
    }
}

/// An entry in `.dart/datapacks.toml` or `.dart/resource-pack.toml`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManagedPack {
    /// Modrinth project ID.
    pub project_id: ModrinthProjectId,
    /// Modrinth slug.
    pub project_slug: Option<String>,
    /// Display title.
    pub title: String,
    /// Version ID.
    pub version_id: ModrinthVersionId,
    /// Version number.
    pub version_number: String,
    /// Target filename.
    pub filename: PackFileName,
    /// SHA-1 hash (required by Minecraft for resource packs).
    pub sha1: String,
    /// SHA-512 hash.
    pub sha512: String,
    /// Download URL.
    pub download_url: String,
}

impl ManagedPack {
    pub(crate) fn from_release(release: &PackRelease) -> Self {
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

    /// Returns the pack project metadata.
    pub fn project(&self) -> PackProject {
        PackProject {
            id: self.project_id.clone(),
            slug: self.project_slug.clone(),
            title: self.title.clone(),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), PackError> {
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

/// An installed pack discovered on disk or in configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstalledPack {
    /// A Dart-managed pack tracked in a manifest.
    Managed(ManagedPack),
    /// An external ZIP file present in the data packs directory.
    ExternalFile {
        /// The ZIP filename.
        filename: PackFileName,
    },
    /// An external resource pack configured directly in `server.properties`.
    ExternalResource {
        /// The configured download URL.
        url: String,
    },
}

impl InstalledPack {
    /// Returns the display title for the pack.
    pub fn title(&self) -> &str {
        match self {
            Self::Managed(pack) => &pack.title,
            Self::ExternalFile { filename } => filename.as_str(),
            Self::ExternalResource { url } => url,
        }
    }

    /// Returns the managed metadata if this pack is tracked by Dart.
    pub fn managed(&self) -> Option<&ManagedPack> {
        match self {
            Self::Managed(pack) => Some(pack),
            Self::ExternalFile { .. } | Self::ExternalResource { .. } => None,
        }
    }

    /// Returns a unique key for UI selection retention.
    pub fn selection_key(&self) -> String {
        match self {
            Self::Managed(pack) => pack.project_id.to_string(),
            Self::ExternalFile { filename } => filename.to_string(),
            Self::ExternalResource { url } => url.clone(),
        }
    }
}

pub(crate) fn channel_name_priority(channel: &str) -> u8 {
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

pub(crate) fn validate_hash(value: &str, length: usize, name: &str) -> Result<(), PackError> {
    if value.len() != length || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(PackError::InvalidMetadata(format!(
            "invalid Modrinth {name} hash"
        )));
    }
    Ok(())
}

pub(crate) fn validate_pack_zip(bytes: &[u8]) -> Result<(), PackError> {
    if bytes.len() < 4 || !bytes.starts_with(b"PK") {
        return Err(PackError::InvalidZip);
    }
    Ok(())
}

pub(crate) fn sha512(bytes: &[u8]) -> String {
    let digest = Sha512::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn file_matches_hash(path: &Path, expected: &str) -> Result<bool, PackError> {
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

/// Errors originating from pack operations, server.properties parsing, or downloads.
#[derive(Debug)]
pub enum PackError {
    /// Search query was empty.
    EmptySearch,
    /// Search query was too long.
    SearchTooLong,
    /// No compatible version found for this pack kind.
    NoCompatibleVersion {
        /// Pack kind.
        kind: PackKind,
        /// Project name.
        project: String,
        /// Minecraft version.
        minecraft: String,
    },
    /// Pack requires dependencies that cannot be installed safely.
    RequiredDependencies {
        /// Project title.
        project: String,
        /// Number of required dependencies.
        count: usize,
    },
    /// Invalid Modrinth metadata.
    InvalidMetadata(String),
    /// Downloaded file is not a valid ZIP archive.
    InvalidZip,
    /// Pack download exceeds size limits.
    DownloadTooLarge,
    /// SHA-512 digest did not match expected upstream hash.
    HashMismatch {
        /// Expected hash.
        expected: String,
        /// Actual hash.
        actual: String,
    },
    /// World level name escapes instance root.
    UnsafeLevelName(String),
    /// Refusing to overwrite existing unmanaged pack file.
    FileConflict {
        /// Conflicting path.
        path: PathBuf,
    },
    /// Refusing to remove a managed file that changed outside Dart.
    ManagedFileChanged {
        /// Changed path.
        path: PathBuf,
    },
    /// An external resource pack is already configured.
    ExternalResourcePack {
        /// Configured URL.
        url: String,
    },
    /// Refusing to modify resource pack because `server.properties` was modified externally.
    ManagedConfigurationChanged,
    /// Upstream HTTP request failed.
    Http(reqwest::Error),
    /// Filesystem I/O error.
    Io {
        /// Operation.
        operation: &'static str,
        /// Path.
        path: PathBuf,
        /// Source error.
        source: io::Error,
    },
    /// Failed to parse pack manifest.
    ParseManifest {
        /// Manifest path.
        path: PathBuf,
        /// Source error.
        source: toml::de::Error,
    },
    /// Failed to serialize pack manifest.
    SerializeManifest(toml::ser::Error),
    /// Manifest contains invalid or inconsistent entries.
    InvalidManifest {
        /// Manifest path.
        path: PathBuf,
        /// Message.
        message: String,
    },
}

impl PackError {
    pub(crate) fn io(operation: &'static str, path: PathBuf, source: io::Error) -> Self {
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
    use crate::content::mods::{ModrinthProjectId, ModrinthVersionId};
    use crate::instance::{FabricLaunch, Instance, InstanceConfig, InstanceId, InstanceName};
    use crate::runtime::FabricRuntime;
    use crate::testing::TestDirectory;
    use std::fs;
    use std::str::FromStr;

    fn instance(directory: &TestDirectory) -> Instance {
        let root = directory.path().join("survival");
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
        let directory = TestDirectory::new("pack");
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
        let directory = TestDirectory::new("pack");
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
        assert!(
            !directory
                .path()
                .join("outside/datapacks/managed.zip")
                .exists()
        );
    }

    #[test]
    fn configures_and_removes_a_verified_server_resource_pack() {
        let directory = TestDirectory::new("pack");
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
        let directory = TestDirectory::new("pack");
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
