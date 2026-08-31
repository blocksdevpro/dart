use crate::paths::DartPaths;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const FABRIC_LAUNCHER_FILE: &str = "fabric-server-launch.jar";
const META_BASE_URL: &str = "https://meta.fabricmc.net/";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FabricVersion(String);

impl FabricVersion {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, RuntimeError> {
        let value = value.as_ref().trim();
        if value.is_empty() || value.len() > 96 {
            return Err(RuntimeError::InvalidVersion(value.to_owned()));
        }
        if !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | '+' | ' ')
        }) {
            return Err(RuntimeError::InvalidVersion(value.to_owned()));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(&self) -> Result<(), RuntimeError> {
        Self::parse(&self.0).map(|_| ())
    }
}

impl fmt::Display for FabricVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct FabricRuntime {
    pub minecraft: FabricVersion,
    pub loader: FabricVersion,
    pub installer: FabricVersion,
}

impl FabricRuntime {
    pub fn new(
        minecraft: impl AsRef<str>,
        loader: impl AsRef<str>,
        installer: impl AsRef<str>,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            minecraft: FabricVersion::parse(minecraft)?,
            loader: FabricVersion::parse(loader)?,
            installer: FabricVersion::parse(installer)?,
        })
    }

    pub fn validate(&self) -> Result<(), RuntimeError> {
        self.minecraft.validate()?;
        self.loader.validate()?;
        self.installer.validate()
    }

    pub fn label(&self) -> String {
        format!(
            "Minecraft {} · loader {} · installer {}",
            self.minecraft, self.loader, self.installer
        )
    }
}

impl fmt::Display for FabricRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}/{}/{}",
            self.minecraft, self.loader, self.installer
        )
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeStore {
    paths: DartPaths,
}

impl RuntimeStore {
    pub fn new(paths: DartPaths) -> Self {
        Self { paths }
    }

    pub fn fabric_dir(&self) -> PathBuf {
        self.paths.fabric_runtimes_dir()
    }

    pub fn launcher_path(&self, runtime: &FabricRuntime) -> PathBuf {
        self.fabric_dir()
            .join(runtime.minecraft.as_str())
            .join(runtime.loader.as_str())
            .join(runtime.installer.as_str())
            .join(FABRIC_LAUNCHER_FILE)
    }

    pub fn is_installed(&self, runtime: &FabricRuntime) -> bool {
        self.launcher_path(runtime).is_file()
    }

    pub fn list(&self) -> Result<Vec<FabricRuntime>, RuntimeError> {
        let mut runtimes = Vec::new();
        let root = self.fabric_dir();
        let games = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(runtimes),
            Err(source) => return Err(RuntimeError::io("read runtime cache", root, source)),
        };

        for game in directories(games, "read Minecraft runtime versions")? {
            let loaders = fs::read_dir(game.path()).map_err(|source| {
                RuntimeError::io("read Fabric loader versions", game.path(), source)
            })?;
            for loader in directories(loaders, "read Fabric loader version")? {
                let installers = fs::read_dir(loader.path()).map_err(|source| {
                    RuntimeError::io("read Fabric installer versions", loader.path(), source)
                })?;
                for installer in directories(installers, "read Fabric installer version")? {
                    if !installer.path().join(FABRIC_LAUNCHER_FILE).is_file() {
                        continue;
                    }
                    let Some(game) = game.file_name().to_str().map(str::to_owned) else {
                        continue;
                    };
                    let Some(loader) = loader.file_name().to_str().map(str::to_owned) else {
                        continue;
                    };
                    let Some(installer) = installer.file_name().to_str().map(str::to_owned) else {
                        continue;
                    };
                    if let Ok(runtime) = FabricRuntime::new(game, loader, installer) {
                        runtimes.push(runtime);
                    }
                }
            }
        }
        runtimes.sort_by(|left, right| right.cmp(left));
        Ok(runtimes)
    }

    pub fn install_bytes(
        &self,
        runtime: &FabricRuntime,
        bytes: &[u8],
    ) -> Result<PathBuf, RuntimeError> {
        runtime.validate()?;
        validate_jar(bytes)?;
        let target = self.launcher_path(runtime);
        if target.is_file() {
            return Ok(target);
        }
        let parent = target.parent().expect("launcher path always has a parent");
        fs::create_dir_all(parent).map_err(|source| {
            RuntimeError::io("create Fabric runtime directory", parent.to_owned(), source)
        })?;
        atomic_write(&target, bytes)?;
        Ok(target)
    }
}

fn directories(
    entries: fs::ReadDir,
    operation: &'static str,
) -> Result<Vec<fs::DirEntry>, RuntimeError> {
    let mut directories = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| RuntimeError::io(operation, PathBuf::new(), source))?;
        let file_type = entry
            .file_type()
            .map_err(|source| RuntimeError::io(operation, entry.path(), source))?;
        if file_type.is_dir() {
            directories.push(entry);
        }
    }
    Ok(directories)
}

fn validate_jar(bytes: &[u8]) -> Result<(), RuntimeError> {
    if bytes.len() < 4 || !bytes.starts_with(b"PK") {
        return Err(RuntimeError::InvalidJar);
    }
    Ok(())
}

fn atomic_write(target: &Path, bytes: &[u8]) -> Result<(), RuntimeError> {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = target.with_file_name(format!(
        ".{}.download.{}.{}",
        FABRIC_LAUNCHER_FILE,
        std::process::id(),
        sequence
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|source| {
                RuntimeError::io("create temporary download", temporary.clone(), source)
            })?;
        file.write_all(bytes).map_err(|source| {
            RuntimeError::io("write Fabric launcher", temporary.clone(), source)
        })?;
        file.sync_all().map_err(|source| {
            RuntimeError::io("sync Fabric launcher", temporary.clone(), source)
        })?;
        match fs::rename(&temporary, target) {
            Ok(()) => Ok(()),
            Err(_) if target.is_file() => Ok(()),
            Err(source) => Err(RuntimeError::io(
                "finish Fabric launcher download",
                target.to_owned(),
                source,
            )),
        }
    })();
    if result.is_err() || target.is_file() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[derive(Clone)]
pub struct FabricClient {
    client: reqwest::Client,
    base_url: reqwest::Url,
}

impl FabricClient {
    pub fn new() -> Result<Self, RuntimeError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("dart/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(RuntimeError::Http)?;
        let base_url = reqwest::Url::parse(META_BASE_URL).map_err(|_| {
            RuntimeError::InvalidMetadata("invalid Fabric Meta base URL".to_owned())
        })?;
        Ok(Self { client, base_url })
    }

    pub async fn resolve(&self, minecraft: Option<&str>) -> Result<FabricRuntime, RuntimeError> {
        let minecraft = match minecraft {
            Some(version) => FabricVersion::parse(version)?,
            None => {
                let games: Vec<VersionEntry> = self.get_json(&["v2", "versions", "game"]).await?;
                preferred_version(&games, "Minecraft")?
            }
        };
        let loaders: Vec<LoaderCompatibility> = self
            .get_json(&["v2", "versions", "loader", minecraft.as_str()])
            .await?;
        let loader_entries = loaders
            .into_iter()
            .map(|compatibility| compatibility.loader)
            .collect::<Vec<_>>();
        let loader = preferred_version(&loader_entries, "Fabric loader")?;
        let installers: Vec<VersionEntry> = self.get_json(&["v2", "versions", "installer"]).await?;
        let installer = preferred_version(&installers, "Fabric installer")?;
        Ok(FabricRuntime {
            minecraft,
            loader,
            installer,
        })
    }

    pub async fn download(
        &self,
        runtime: &FabricRuntime,
        store: &RuntimeStore,
    ) -> Result<PathBuf, RuntimeError> {
        if store.is_installed(runtime) {
            return Ok(store.launcher_path(runtime));
        }
        let url = self.endpoint(&[
            "v2",
            "versions",
            "loader",
            runtime.minecraft.as_str(),
            runtime.loader.as_str(),
            runtime.installer.as_str(),
            "server",
            "jar",
        ])?;
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(RuntimeError::Http)?
            .error_for_status()
            .map_err(RuntimeError::Http)?;
        let bytes = response.bytes().await.map_err(RuntimeError::Http)?;
        store.install_bytes(runtime, &bytes)
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        segments: &[&str],
    ) -> Result<T, RuntimeError> {
        self.client
            .get(self.endpoint(segments)?)
            .send()
            .await
            .map_err(RuntimeError::Http)?
            .error_for_status()
            .map_err(RuntimeError::Http)?
            .json()
            .await
            .map_err(RuntimeError::Http)
    }

    fn endpoint(&self, segments: &[&str]) -> Result<reqwest::Url, RuntimeError> {
        let mut url = self.base_url.clone();
        let mut path = url.path_segments_mut().map_err(|_| {
            RuntimeError::InvalidMetadata("Fabric Meta base URL cannot contain paths".to_owned())
        })?;
        path.clear();
        path.extend(segments.iter().copied());
        drop(path);
        Ok(url)
    }
}

#[derive(Debug, Deserialize)]
struct VersionEntry {
    version: String,
    stable: bool,
}

#[derive(Debug, Deserialize)]
struct LoaderCompatibility {
    loader: VersionEntry,
}

fn preferred_version(
    versions: &[VersionEntry],
    component: &'static str,
) -> Result<FabricVersion, RuntimeError> {
    let entry = versions
        .iter()
        .find(|version| version.stable)
        .or_else(|| versions.first())
        .ok_or(RuntimeError::NoCompatibleVersion(component))?;
    FabricVersion::parse(&entry.version)
}

#[derive(Debug)]
pub enum RuntimeError {
    InvalidVersion(String),
    InvalidJar,
    NoCompatibleVersion(&'static str),
    InvalidMetadata(String),
    Http(reqwest::Error),
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl RuntimeError {
    fn io(operation: &'static str, path: PathBuf, source: io::Error) -> Self {
        Self::Io {
            operation,
            path,
            source,
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidVersion(version) => {
                write!(formatter, "invalid Fabric version '{version}'")
            }
            Self::InvalidJar => formatter.write_str("downloaded file is not a valid JAR archive"),
            Self::NoCompatibleVersion(component) => {
                write!(
                    formatter,
                    "Fabric Meta returned no compatible {component} version"
                )
            }
            Self::InvalidMetadata(message) => formatter.write_str(message),
            Self::Http(source) => write!(formatter, "Fabric download failed: {source}"),
            Self::Io {
                operation, path, ..
            } => {
                write!(formatter, "cannot {operation} at {}", path.display())
            }
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Http(source) => Some(source),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FabricRuntime, RuntimeStore};
    use crate::paths::DartPaths;
    use crate::test_support::TestDirectory;
    use std::fs;

    #[test]
    fn stores_and_discovers_multiple_runtime_versions() {
        let directory = TestDirectory::new("runtime");
        let store = RuntimeStore::new(DartPaths::new(directory.path().to_owned()));
        let first = FabricRuntime::new("1.21.8", "0.17.2", "1.1.2").unwrap();
        let second = FabricRuntime::new("1.20.1", "0.16.14", "1.0.3").unwrap();

        store.install_bytes(&first, b"PK\x03\x04first").unwrap();
        store.install_bytes(&second, b"PK\x03\x04second").unwrap();

        let installed = store.list().unwrap();
        assert!(installed.contains(&first));
        assert!(installed.contains(&second));
        assert_eq!(
            fs::read(store.launcher_path(&first)).unwrap(),
            b"PK\x03\x04first"
        );
    }

    #[test]
    fn rejects_path_traversal_versions_and_non_jars() {
        assert!(FabricRuntime::new("../1.21", "loader", "installer").is_err());
        let directory = TestDirectory::new("runtime");
        let store = RuntimeStore::new(DartPaths::new(directory.path().to_owned()));
        let runtime = FabricRuntime::new("1.21.8", "0.17.2", "1.1.2").unwrap();
        assert!(store.install_bytes(&runtime, b"not a jar").is_err());
    }
}
