//! Modrinth Fabric mods domain model, verification, and manifest management.

use crate::runtime::FabricVersion;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

mod client;
mod manager;
mod store;

pub use client::ModrinthClient;
pub use manager::ModManager;
pub use store::ModStore;

#[cfg(test)]
pub(crate) use manager::order_releases;

const MODRINTH_API_URL: &str = "https://api.modrinth.com/v2/";
const MODS_DIRECTORY: &str = "mods";
const DART_DIRECTORY: &str = ".dart";
const MANIFEST_FILE: &str = "mods.toml";
const MANIFEST_FORMAT_VERSION: u32 = 1;
const MAX_SEARCH_QUERY_LENGTH: usize = 120;
const MAX_MOD_FILE_BYTES: usize = 128 * 1024 * 1024;
const MAX_DEPENDENCY_PROJECTS: usize = 64;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A validated Modrinth project identifier (base64 or slug).
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModrinthProjectId(String);

impl ModrinthProjectId {
    /// Validates and parses a Modrinth project ID.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, ModError> {
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

    /// Returns the project ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModrinthProjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A validated Modrinth version identifier.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModrinthVersionId(String);

impl ModrinthVersionId {
    /// Validates and parses a Modrinth version ID.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, ModError> {
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

    /// Returns the version ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModrinthVersionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A sanitized JAR filename for a mod.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModFileName(String);

impl ModFileName {
    /// Validates and parses a mod filename.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, ModError> {
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

    /// Returns the filename as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModFileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Mod project identity metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModProject {
    pub(crate) id: ModrinthProjectId,
    pub(crate) slug: Option<String>,
    pub(crate) title: String,
}

impl ModProject {
    /// Returns the project ID.
    pub fn id(&self) -> &ModrinthProjectId {
        &self.id
    }

    /// Returns the project title.
    pub fn title(&self) -> &str {
        &self.title
    }
}

/// A single hit in Modrinth mod search results.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModSearchHit {
    project: ModProject,
    description: String,
    author: String,
    downloads: u64,
}

impl ModSearchHit {
    /// Returns the matched project.
    pub fn project(&self) -> &ModProject {
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

/// The release stability channel for a mod version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseChannel {
    /// Stable release.
    Release,
    /// Beta release.
    Beta,
    /// Alpha / snapshot release.
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

/// Downloadable release metadata for a specific mod version.
#[derive(Clone, Debug)]
pub struct ModRelease {
    pub(crate) project: ModProject,
    pub(crate) version_id: ModrinthVersionId,
    pub(crate) version_number: String,
    pub(crate) channel: ReleaseChannel,
    pub(crate) filename: ModFileName,
    pub(crate) sha512: String,
    pub(crate) download_url: reqwest::Url,
    pub(crate) required_dependencies: Vec<RequiredDependency>,
    pub(crate) published_at: String,
}

impl ModRelease {
    /// Returns the project identity for this release.
    pub fn project(&self) -> &ModProject {
        &self.project
    }

    /// Returns the Modrinth version ID.
    pub fn version_id(&self) -> &ModrinthVersionId {
        &self.version_id
    }

    /// Returns the version number string.
    pub fn version_number(&self) -> &str {
        &self.version_number
    }

    /// Returns the release channel.
    pub fn channel(&self) -> ReleaseChannel {
        self.channel
    }

    /// Returns the target JAR filename.
    pub fn filename(&self) -> &ModFileName {
        &self.filename
    }

    /// Returns the required dependencies for this release.
    pub fn required_dependencies(&self) -> &[RequiredDependency] {
        &self.required_dependencies
    }
}

/// A required dependency referenced by a mod release.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequiredDependency {
    /// Dependency points to any compatible version of a Modrinth project.
    Project(ModrinthProjectId),
    /// Dependency points to a specific Modrinth version.
    Version {
        /// Version ID.
        version_id: ModrinthVersionId,
        /// Expected project ID if known.
        expected_project: Option<ModrinthProjectId>,
    },
    /// Dependency points to an external file not hosted on Modrinth.
    ExternalFile(String),
}

/// An execution plan containing a mod and all its topologically sorted dependencies.
#[derive(Clone, Debug)]
pub struct ModInstallPlan {
    root: ModrinthProjectId,
    releases: Vec<ModRelease>,
}

impl ModInstallPlan {
    /// Returns the root mod being installed.
    pub fn root(&self) -> &ModRelease {
        self.releases
            .iter()
            .find(|release| release.project.id == self.root)
            .expect("a mod install plan always contains its root release")
    }

    /// Returns all releases in installation order.
    pub fn releases(&self) -> &[ModRelease] {
        &self.releases
    }

    /// Returns the titles of all dependencies included in the plan.
    pub fn dependency_titles(&self) -> Vec<&str> {
        self.releases
            .iter()
            .filter(|release| release.project.id != self.root)
            .map(|release| release.project.title())
            .collect()
    }
}

/// The result report after applying a mod install plan.
#[derive(Clone, Debug)]
pub struct ModInstallReport {
    root: ModRelease,
    root_outcome: ModInstallOutcome,
    dependency_titles: Vec<String>,
    changed_dependencies: usize,
}

impl ModInstallReport {
    /// Returns the root release installed.
    pub fn root(&self) -> &ModRelease {
        &self.root
    }

    /// Returns whether the root mod was added, updated, or already present.
    pub fn root_outcome(&self) -> ModInstallOutcome {
        self.root_outcome
    }

    /// Returns the titles of installed dependencies.
    pub fn dependency_titles(&self) -> &[String] {
        &self.dependency_titles
    }

    /// Returns how many dependencies were actually downloaded/changed.
    pub fn changed_dependencies(&self) -> usize {
        self.changed_dependencies
    }
}

/// An entry in `.dart/mods.toml` tracking a Dart-managed mod.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManagedMod {
    /// Modrinth project ID.
    pub project_id: ModrinthProjectId,
    /// Modrinth slug.
    pub project_slug: Option<String>,
    /// Display title.
    pub title: String,
    /// Installed version ID.
    pub version_id: ModrinthVersionId,
    /// Installed version number.
    pub version_number: String,
    /// Target filename.
    pub filename: ModFileName,
    /// Expected SHA-512 digest.
    pub sha512: String,
}

impl ManagedMod {
    pub(crate) fn from_release(release: &ModRelease) -> Self {
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

    /// Returns the mod project identity.
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

/// An installed mod discovered in the instance `mods/` directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstalledMod {
    /// Mod tracked and managed in `.dart/mods.toml`.
    Managed(ManagedMod),
    /// External mod file placed directly in `mods/`.
    External {
        /// The JAR filename.
        filename: ModFileName,
    },
}

impl InstalledMod {
    /// Returns the JAR filename of the mod.
    pub fn filename(&self) -> &ModFileName {
        match self {
            Self::Managed(modification) => &modification.filename,
            Self::External { filename } => filename,
        }
    }

    /// Returns the title of the mod (or filename if external).
    pub fn title(&self) -> &str {
        match self {
            Self::Managed(modification) => &modification.title,
            Self::External { filename } => filename.as_str(),
        }
    }

    /// Returns the managed metadata if this mod is tracked by Dart.
    pub fn managed(&self) -> Option<&ManagedMod> {
        match self {
            Self::Managed(modification) => Some(modification),
            Self::External { .. } => None,
        }
    }
}

/// The outcome of an install operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModInstallOutcome {
    /// Added as a new mod.
    Added,
    /// Upgraded an existing mod version.
    Updated,
    /// Already present with matching hash and version.
    AlreadyInstalled,
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

/// Errors originating from Modrinth API calls, dependency resolution, or local mod storage.
#[derive(Debug)]
pub enum ModError {
    /// Search query was empty.
    EmptySearch,
    /// Search query was too long.
    SearchTooLong,
    /// No compatible version found on Modrinth for the project and Minecraft version.
    NoCompatibleVersion {
        /// Project name.
        project: String,
        /// Minecraft version.
        minecraft: String,
    },
    /// A dependency version belongs to a different project than expected.
    DependencyProjectMismatch {
        /// The version ID.
        version: ModrinthVersionId,
        /// Expected project ID.
        expected: ModrinthProjectId,
        /// Actual project ID returned.
        actual: ModrinthProjectId,
    },
    /// A required dependency version is incompatible with the instance.
    IncompatibleDependencyVersion {
        /// Project title.
        project: String,
        /// Incompatible version.
        version: ModrinthVersionId,
        /// Minecraft version.
        minecraft: String,
    },
    /// Two or more dependencies require conflicting versions of the same project.
    DependencyVersionConflict {
        /// The conflicting project.
        project: String,
        /// First requested version.
        first: ModrinthVersionId,
        /// Second requested version.
        second: ModrinthVersionId,
    },
    /// A required dependency references an external file not on Modrinth.
    UnresolvableRequiredDependency {
        /// Project requesting the dependency.
        requested_by: String,
        /// Filename required.
        dependency: String,
    },
    /// The dependency graph exceeds safety limits.
    DependencyLimitExceeded {
        /// Max allowed count.
        limit: usize,
    },
    /// A cycle was detected in the required dependency graph.
    DependencyCycle {
        /// Project involved in cycle.
        project: String,
    },
    /// Invalid or malformed Modrinth metadata received.
    InvalidMetadata(String),
    /// Downloaded file is not a valid JAR.
    InvalidJar,
    /// Download exceeds safety limits.
    DownloadTooLarge,
    /// SHA-512 digest did not match expected upstream hash.
    HashMismatch {
        /// Expected hash.
        expected: String,
        /// Actual computed hash.
        actual: String,
    },
    /// Refusing to overwrite an existing unmanaged file.
    FileConflict {
        /// Target path.
        path: PathBuf,
    },
    /// Refusing to remove a managed file that was modified externally.
    ManagedFileChanged {
        /// Path that changed.
        path: PathBuf,
    },
    /// Upstream HTTP request error.
    Http(reqwest::Error),
    /// File I/O error.
    Io {
        /// Operation.
        operation: &'static str,
        /// Path.
        path: PathBuf,
        /// Underlying error.
        source: io::Error,
    },
    /// Failed to parse mod manifest `mods.toml`.
    ParseManifest {
        /// Manifest path.
        path: PathBuf,
        /// Parsing error.
        source: toml::de::Error,
    },
    /// Failed to serialize mod manifest `mods.toml`.
    SerializeManifest(toml::ser::Error),
    /// Manifest contains invalid or inconsistent entries.
    InvalidManifest {
        /// Manifest path.
        path: PathBuf,
        /// Explanation.
        message: String,
    },
}

impl ModError {
    pub(crate) fn io(operation: &'static str, path: PathBuf, source: io::Error) -> Self {
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
        ModSearchHit, ModStore, ModrinthClient, ModrinthProjectId, ModrinthVersionId,
        ReleaseChannel, RequiredDependency, SearchHitResponse, order_releases, sha512,
    };
    use crate::instance::{FabricLaunch, Instance, InstanceConfig, InstanceId, InstanceName};
    use crate::runtime::FabricRuntime;
    use crate::runtime::FabricVersion;
    use crate::testing::TestDirectory;
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::str::FromStr;

    fn instance(directory: &TestDirectory) -> Instance {
        Instance::new(
            InstanceId::from_str("survival").unwrap(),
            directory.path().join("survival"),
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
        let directory = TestDirectory::new("mods");
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
        let directory = TestDirectory::new("mods");
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
        let directory = TestDirectory::new("mods");
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
        let directory = TestDirectory::new("mods");
        let instance = instance(&directory);
        let store = ModStore::new();
        let bytes = b"PK\x03\x04good";
        let version = release("fabric-api.jar", bytes, "EEFFGGHH");

        store.install(&instance, &version, bytes).unwrap();
        fs::write(
            store.mods_dir(&instance).join("fabric-api.jar"),
            b"PK\x03\x04modified",
        )
        .unwrap();

        let installed = store.list(&instance).unwrap();
        let managed = installed[0].managed().unwrap().clone();
        assert!(matches!(
            store.remove(&instance, &managed),
            Err(ModError::ManagedFileChanged { .. })
        ));
        assert!(store.mods_dir(&instance).join("fabric-api.jar").is_file());
    }

    #[test]
    fn normalizes_valid_multiline_modrinth_descriptions() {
        let hit = ModSearchHit::try_from(SearchHitResponse {
            project_id: "AABBCCDD".to_owned(),
            slug: Some("fabric-api".to_owned()),
            title: "Fabric API".to_owned(),
            description: "First line\nSecond line\r\n\tIndented".to_owned(),
            author: "Mod Author".to_owned(),
            downloads: 123,
        })
        .unwrap();

        assert_eq!(hit.description(), "First line Second line Indented");
    }

    #[test]
    fn parses_every_supported_required_dependency_reference() {
        let project = RequiredDependency::try_from(&DependencyResponse {
            version_id: None,
            project_id: Some("AABBCCDD".to_owned()),
            file_name: None,
            dependency_type: "required".to_owned(),
        })
        .unwrap();
        let version = RequiredDependency::try_from(&DependencyResponse {
            version_id: Some("EEFFGGHH".to_owned()),
            project_id: Some("AABBCCDD".to_owned()),
            file_name: None,
            dependency_type: "required".to_owned(),
        })
        .unwrap();
        let external = RequiredDependency::try_from(&DependencyResponse {
            version_id: None,
            project_id: None,
            file_name: Some("external-lib.jar".to_owned()),
            dependency_type: "required".to_owned(),
        })
        .unwrap();

        assert!(matches!(project, RequiredDependency::Project(_)));
        assert!(matches!(version, RequiredDependency::Version { .. }));
        assert!(matches!(external, RequiredDependency::ExternalFile(_)));
    }

    #[test]
    fn orders_transitive_dependencies_before_the_requested_mod() {
        let root = ModrinthProjectId::parse("ROOT").unwrap();
        let dep = ModrinthProjectId::parse("DEP").unwrap();
        let mut releases = BTreeMap::new();
        releases.insert(
            root.clone(),
            ModRelease {
                project: ModProject {
                    id: root.clone(),
                    slug: None,
                    title: "Root".to_owned(),
                },
                version_id: ModrinthVersionId::parse("v1").unwrap(),
                version_number: "1.0.0".to_owned(),
                channel: ReleaseChannel::Release,
                filename: ModFileName::parse("root.jar").unwrap(),
                sha512: "0".repeat(128),
                download_url: reqwest::Url::parse("https://cdn.modrinth.com/root.jar").unwrap(),
                required_dependencies: vec![RequiredDependency::Project(dep.clone())],
                published_at: "2026-01-01T00:00:00Z".to_owned(),
            },
        );
        releases.insert(
            dep.clone(),
            ModRelease {
                project: ModProject {
                    id: dep.clone(),
                    slug: None,
                    title: "Dep".to_owned(),
                },
                version_id: ModrinthVersionId::parse("v2").unwrap(),
                version_number: "1.0.0".to_owned(),
                channel: ReleaseChannel::Release,
                filename: ModFileName::parse("dep.jar").unwrap(),
                sha512: "0".repeat(128),
                download_url: reqwest::Url::parse("https://cdn.modrinth.com/dep.jar").unwrap(),
                required_dependencies: Vec::new(),
                published_at: "2026-01-01T00:00:00Z".to_owned(),
            },
        );

        let mut edges = BTreeMap::new();
        edges.insert(root.clone(), BTreeSet::from([dep.clone()]));
        edges.insert(dep.clone(), BTreeSet::new());

        let ordered = order_releases(&root, &releases, &edges).unwrap();
        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0].project.id, dep);
        assert_eq!(ordered[1].project.id, root);
    }

    #[test]
    fn rejects_required_dependency_cycles() {
        let first = ModrinthProjectId::parse("FIRST").unwrap();
        let second = ModrinthProjectId::parse("SECOND").unwrap();
        let mut releases = BTreeMap::new();
        for (id, name) in [(&first, "First"), (&second, "Second")] {
            releases.insert(
                (*id).clone(),
                ModRelease {
                    project: ModProject {
                        id: (*id).clone(),
                        slug: None,
                        title: (*name).to_owned(),
                    },
                    version_id: ModrinthVersionId::parse("v").unwrap(),
                    version_number: "1.0.0".to_owned(),
                    channel: ReleaseChannel::Release,
                    filename: ModFileName::parse("mod.jar").unwrap(),
                    sha512: "0".repeat(128),
                    download_url: reqwest::Url::parse("https://cdn.modrinth.com/mod.jar").unwrap(),
                    required_dependencies: Vec::new(),
                    published_at: "2026-01-01T00:00:00Z".to_owned(),
                },
            );
        }
        let mut edges = BTreeMap::new();
        edges.insert(first.clone(), BTreeSet::from([second.clone()]));
        edges.insert(second.clone(), BTreeSet::from([first.clone()]));

        assert!(matches!(
            order_releases(&first, &releases, &edges),
            Err(ModError::DependencyCycle { .. })
        ));
    }

    #[test]
    fn rejects_client_only_mod_versions_for_a_server() {
        use super::supports_dedicated_server;

        assert!(!supports_dedicated_server("client_only"));
        assert!(supports_dedicated_server("client_and_server"));
        assert!(supports_dedicated_server("server_only"));
        assert!(supports_dedicated_server("dedicated_server_only"));
    }

    #[test]
    fn searches_with_fabric_and_minecraft_facets() {
        let client = ModrinthClient::new().unwrap();
        let minecraft = FabricVersion::parse("1.21.8").unwrap();
        let url = client.search_url("fabric api", &minecraft).unwrap();
        let query = url.query_pairs().collect::<Vec<_>>();
        assert!(query.iter().any(|(key, value)| key == "facets"
            && value.contains("project_type:mod")
            && value.contains("categories:fabric")
            && value.contains("versions:1.21.8")
            && value.contains("environment:server_only")));
    }
}
