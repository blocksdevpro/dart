//! Persistence and directory management for local server instances.

use crate::instance::{
    EulaAcceptance, Instance, InstanceConfig, InstanceId, InstanceIdError, InstanceValidationError,
};
use crate::runtime::FABRIC_LAUNCHER_FILE;
use crate::storage::DartPaths;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

const CONFIG_FILE: &str = "dart.toml";
const EULA_FILE: &str = "eula.txt";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Filesystem store for discovery, creation, and loading of server instances.
#[derive(Clone, Debug)]
pub struct InstanceStore {
    paths: DartPaths,
}

impl InstanceStore {
    /// Creates a new instance store for the given storage layout.
    pub fn new(paths: DartPaths) -> Self {
        Self { paths }
    }

    /// Returns the root data directory.
    pub fn dart_home(&self) -> &Path {
        self.paths.home()
    }

    /// Returns the directory containing instances (`$DART_HOME/instances`).
    pub fn instances_dir(&self) -> PathBuf {
        self.paths.instances_dir()
    }

    /// Derives an unused instance ID from a display name.
    pub fn available_id(&self, name: &crate::instance::InstanceName) -> InstanceId {
        let base = InstanceId::from_name(name);
        if !self.instances_dir().join(base.as_str()).exists() {
            return base;
        }

        for number in 2_u64.. {
            let suffix = format!("-{number}");
            let maximum_base_length = 64 - suffix.len();
            let shortened =
                base.as_str()[..base.as_str().len().min(maximum_base_length)].trim_end_matches('-');
            let candidate = InstanceId::from_str(&format!("{shortened}{suffix}"))
                .expect("a generated instance ID is always valid");
            if !self.instances_dir().join(candidate.as_str()).exists() {
                return candidate;
            }
        }

        unreachable!("the instance ID suffix space cannot be exhausted")
    }

    /// Discovers and loads all valid managed instances in the instances directory.
    pub fn list(&self) -> Result<Vec<Instance>, StoreError> {
        let instances_dir = self.instances_dir();
        let entries = match fs::read_dir(&instances_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(StoreError::io(
                    "read instance directory",
                    instances_dir,
                    source,
                ));
            }
        };

        let mut instances = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| {
                StoreError::io(
                    "read instance directory entry",
                    instances_dir.clone(),
                    source,
                )
            })?;
            let file_type = entry.file_type().map_err(|source| {
                StoreError::io("inspect instance directory entry", entry.path(), source)
            })?;
            if !file_type.is_dir() {
                continue;
            }

            let directory_name = entry.file_name();
            let Some(directory_name) = directory_name.to_str() else {
                continue;
            };
            if directory_name.starts_with('.') {
                continue;
            }

            let config_path = entry.path().join(CONFIG_FILE);
            if !config_path.is_file() {
                continue;
            }

            let id = InstanceId::from_str(directory_name).map_err(|source| {
                StoreError::InvalidInstanceId {
                    path: entry.path(),
                    source,
                }
            })?;
            instances.push(self.load_at(id, entry.path())?);
        }

        instances.sort_by(|left, right| left.id().cmp(right.id()));
        Ok(instances)
    }

    /// Loads a specific instance by its ID.
    pub fn get(&self, id: &InstanceId) -> Result<Instance, StoreError> {
        let root = self.instances_dir().join(id.as_str());
        if !root.is_dir() {
            return Err(StoreError::InstanceNotFound { id: id.clone() });
        }
        self.load_at(id.clone(), root)
    }

    /// Creates a new instance with the specified configuration, launcher JAR, and EULA acceptance.
    pub fn create(
        &self,
        id: InstanceId,
        config: InstanceConfig,
        cached_launcher: &Path,
        eula: EulaAcceptance,
    ) -> Result<Instance, StoreError> {
        config
            .validate()
            .map_err(|source| StoreError::InvalidConfig {
                path: self.instances_dir().join(id.as_str()).join(CONFIG_FILE),
                source,
            })?;
        if !cached_launcher.is_file() {
            return Err(StoreError::CachedLauncherMissing {
                path: cached_launcher.to_owned(),
            });
        }

        let instances_dir = self.instances_dir();
        fs::create_dir_all(&instances_dir).map_err(|source| {
            StoreError::io("create instance directory", instances_dir.clone(), source)
        })?;

        let root = instances_dir.join(id.as_str());
        if root.exists() {
            return self.reconcile_existing(id, root, &config, cached_launcher, eula);
        }

        let temporary_root = unique_path(&instances_dir, id.as_str(), "creating");
        fs::create_dir(&temporary_root).map_err(|source| {
            StoreError::io(
                "create temporary instance directory",
                temporary_root.clone(),
                source,
            )
        })?;

        let create_result = (|| {
            write_config(&temporary_root.join(CONFIG_FILE), &config)?;
            copy_launcher(cached_launcher, &temporary_root)?;
            write_eula_if_accepted(&temporary_root, eula)?;
            fs::rename(&temporary_root, &root).map_err(|source| {
                StoreError::io("finish instance creation", root.clone(), source)
            })?;
            Ok(())
        })();

        if let Err(error) = create_result {
            let _ = fs::remove_dir_all(&temporary_root);
            if root.exists() {
                return self.reconcile_existing(id, root, &config, cached_launcher, eula);
            }
            return Err(error);
        }

        Ok(Instance::new(id, root, config))
    }

    fn reconcile_existing(
        &self,
        id: InstanceId,
        root: PathBuf,
        requested: &InstanceConfig,
        cached_launcher: &Path,
        eula: EulaAcceptance,
    ) -> Result<Instance, StoreError> {
        let config_path = root.join(CONFIG_FILE);
        if !config_path.is_file() {
            return Err(StoreError::UnmanagedDirectory { root });
        }

        let existing = self.load_at(id.clone(), root.clone())?;
        if existing.config() == requested {
            copy_launcher(cached_launcher, &root)?;
            write_eula_if_accepted(&root, eula)?;
            return Ok(existing);
        }

        Err(StoreError::InstanceAlreadyExists { id, root })
    }

    fn load_at(&self, id: InstanceId, root: PathBuf) -> Result<Instance, StoreError> {
        let config_path = root.join(CONFIG_FILE);
        let text = fs::read_to_string(&config_path).map_err(|source| {
            StoreError::io("read instance configuration", config_path.clone(), source)
        })?;
        let config: InstanceConfig =
            toml::from_str(&text).map_err(|source| StoreError::ParseConfig {
                path: config_path.clone(),
                source,
            })?;
        config
            .validate()
            .map_err(|source| StoreError::InvalidConfig {
                path: config_path,
                source,
            })?;

        Ok(Instance::new(id, root, config))
    }
}

fn write_eula_if_accepted(
    instance_root: &Path,
    acceptance: EulaAcceptance,
) -> Result<(), StoreError> {
    if !acceptance.is_accepted() {
        return Ok(());
    }
    let target = instance_root.join(EULA_FILE);
    if fs::read_to_string(&target)
        .is_ok_and(|contents| contents.lines().any(|line| line.trim() == "eula=true"))
    {
        return Ok(());
    }
    let temporary = unique_path(instance_root, EULA_FILE, "accepting");
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|source| {
                StoreError::io("create temporary EULA file", temporary.clone(), source)
            })?;
        file.write_all(b"eula=true\n").map_err(|source| {
            StoreError::io("write Minecraft EULA acceptance", temporary.clone(), source)
        })?;
        file.sync_all().map_err(|source| {
            StoreError::io("sync Minecraft EULA acceptance", temporary.clone(), source)
        })?;
        replace_file(&temporary, &target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn copy_launcher(source: &Path, instance_root: &Path) -> Result<(), StoreError> {
    let target = instance_root.join(FABRIC_LAUNCHER_FILE);
    if target.is_file() {
        return Ok(());
    }
    let temporary = unique_path(instance_root, FABRIC_LAUNCHER_FILE, "copying");
    fs::copy(source, &temporary)
        .map_err(|error| StoreError::io("copy cached Fabric launcher", temporary.clone(), error))?;
    let file = OpenOptions::new()
        .read(true)
        .open(&temporary)
        .map_err(|error| StoreError::io("open copied Fabric launcher", temporary.clone(), error))?;
    file.sync_all()
        .map_err(|error| StoreError::io("sync copied Fabric launcher", temporary.clone(), error))?;
    match fs::rename(&temporary, &target) {
        Ok(()) => Ok(()),
        Err(_) if target.is_file() => {
            let _ = fs::remove_file(&temporary);
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(StoreError::io(
                "finish copying Fabric launcher",
                target,
                error,
            ))
        }
    }
}

fn write_config(path: &Path, config: &InstanceConfig) -> Result<(), StoreError> {
    let contents =
        toml::to_string_pretty(config).map_err(|source| StoreError::SerializeConfig {
            path: path.to_owned(),
            source,
        })?;
    let parent = path
        .parent()
        .ok_or_else(|| StoreError::MissingConfigParent {
            path: path.to_owned(),
        })?;
    let temporary_path = unique_path(parent, CONFIG_FILE, "tmp");

    let write_result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary_path)
            .map_err(|source| {
                StoreError::io(
                    "create temporary configuration",
                    temporary_path.clone(),
                    source,
                )
            })?;
        file.write_all(contents.as_bytes()).map_err(|source| {
            StoreError::io(
                "write temporary configuration",
                temporary_path.clone(),
                source,
            )
        })?;
        file.sync_all().map_err(|source| {
            StoreError::io(
                "sync temporary configuration",
                temporary_path.clone(),
                source,
            )
        })?;
        replace_file(&temporary_path, path)
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    write_result
}

fn replace_file(temporary_path: &Path, target_path: &Path) -> Result<(), StoreError> {
    match fs::rename(temporary_path, target_path) {
        Ok(()) => Ok(()),
        Err(_source) if cfg!(windows) && target_path.exists() => {
            fs::remove_file(target_path).map_err(|remove_source| {
                StoreError::io(
                    "replace instance configuration",
                    target_path.to_owned(),
                    remove_source,
                )
            })?;
            fs::rename(temporary_path, target_path).map_err(|rename_source| {
                StoreError::io(
                    "replace instance configuration",
                    target_path.to_owned(),
                    rename_source,
                )
            })
        }
        Err(source) => Err(StoreError::io(
            "replace instance configuration",
            target_path.to_owned(),
            source,
        )),
    }
}

fn unique_path(parent: &Path, name: &str, purpose: &str) -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".{name}.{purpose}.{}.{}",
        std::process::id(),
        sequence
    ))
}

/// Errors originating from instance filesystem persistence or configuration parsing.
#[derive(Debug)]
pub enum StoreError {
    /// An I/O error occurred on the filesystem.
    Io {
        /// The operation that failed.
        operation: &'static str,
        /// The path involved.
        path: PathBuf,
        /// The underlying I/O error.
        source: io::Error,
    },
    /// The instance ID derived from a directory name is invalid.
    InvalidInstanceId {
        /// The path of the directory.
        path: PathBuf,
        /// The parsing error.
        source: InstanceIdError,
    },
    /// Parsing `dart.toml` failed.
    ParseConfig {
        /// Path to `dart.toml`.
        path: PathBuf,
        /// Parsing error.
        source: toml::de::Error,
    },
    /// Serializing `dart.toml` failed.
    SerializeConfig {
        /// Path to `dart.toml`.
        path: PathBuf,
        /// Serialization error.
        source: toml::ser::Error,
    },
    /// The configuration in `dart.toml` failed semantic validation.
    InvalidConfig {
        /// Path to `dart.toml`.
        path: PathBuf,
        /// Validation error.
        source: InstanceValidationError,
    },
    /// The configuration path has no parent directory.
    MissingConfigParent {
        /// Path missing a parent.
        path: PathBuf,
    },
    /// An instance directory already exists with conflicting configuration.
    InstanceAlreadyExists {
        /// The colliding instance ID.
        id: InstanceId,
        /// The colliding directory root.
        root: PathBuf,
    },
    /// An instance was requested but does not exist on disk.
    InstanceNotFound {
        /// The missing instance ID.
        id: InstanceId,
    },
    /// The directory exists but does not contain a managed `dart.toml` file.
    UnmanagedDirectory {
        /// The unmanaged root.
        root: PathBuf,
    },
    /// The cached Fabric launcher JAR is missing.
    CachedLauncherMissing {
        /// Expected path to the launcher.
        path: PathBuf,
    },
}

impl StoreError {
    pub(crate) fn io(operation: &'static str, path: PathBuf, source: io::Error) -> Self {
        Self::Io {
            operation,
            path,
            source,
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                operation, path, ..
            } => write!(formatter, "cannot {operation} at {}", path.display()),
            Self::InvalidInstanceId { path, source } => {
                write!(
                    formatter,
                    "invalid instance directory {}: {source}",
                    path.display()
                )
            }
            Self::ParseConfig { path, source } => {
                write!(formatter, "cannot parse {}: {source}", path.display())
            }
            Self::SerializeConfig { path, source } => {
                write!(formatter, "cannot serialize {}: {source}", path.display())
            }
            Self::InvalidConfig { path, source } => {
                write!(
                    formatter,
                    "invalid configuration at {}: {source}",
                    path.display()
                )
            }
            Self::MissingConfigParent { path } => {
                write!(
                    formatter,
                    "configuration path has no parent: {}",
                    path.display()
                )
            }
            Self::InstanceAlreadyExists { id, root } => write!(
                formatter,
                "instance '{id}' already exists with different settings at {}",
                root.display()
            ),
            Self::InstanceNotFound { id } => write!(formatter, "instance '{id}' not found"),
            Self::UnmanagedDirectory { root } => write!(
                formatter,
                "refusing to overwrite unmanaged directory {}",
                root.display()
            ),
            Self::CachedLauncherMissing { path } => write!(
                formatter,
                "cached Fabric launcher is missing at {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::InvalidInstanceId { source, .. } => Some(source),
            Self::ParseConfig { source, .. } => Some(source),
            Self::SerializeConfig { source, .. } => Some(source),
            Self::InvalidConfig { source, .. } => Some(source),
            Self::MissingConfigParent { .. }
            | Self::InstanceAlreadyExists { .. }
            | Self::InstanceNotFound { .. }
            | Self::UnmanagedDirectory { .. }
            | Self::CachedLauncherMissing { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::InstanceStore;
    use crate::instance::{EulaAcceptance, FabricLaunch, InstanceConfig, InstanceId, InstanceName};
    use crate::runtime::{FABRIC_LAUNCHER_FILE, FabricRuntime};
    use crate::storage::DartPaths;
    use crate::testing::TestDirectory;
    use std::fs;
    use std::path::PathBuf;
    use std::str::FromStr;

    fn config(name: &str) -> InstanceConfig {
        InstanceConfig::new(
            InstanceName::parse(name).unwrap(),
            FabricLaunch::default(),
            FabricRuntime::new("1.21.8", "0.17.2", "1.1.2").unwrap(),
        )
    }

    fn cached_launcher(directory: &TestDirectory) -> PathBuf {
        let path = directory.path().join("cached.jar");
        fs::write(&path, b"PK\x03\x04cached launcher").unwrap();
        path
    }

    #[test]
    fn creates_and_lists_self_contained_instances() {
        let directory = TestDirectory::new("store");
        let store = InstanceStore::new(DartPaths::new(directory.path().to_owned()));
        let id = InstanceId::from_str("survival").unwrap();
        let launcher = cached_launcher(&directory);

        let created = store
            .create(id, config("Survival"), &launcher, EulaAcceptance::Accepted)
            .unwrap();

        assert!(created.root().join("dart.toml").is_file());
        assert_eq!(
            fs::read(created.root().join(FABRIC_LAUNCHER_FILE)).unwrap(),
            b"PK\x03\x04cached launcher"
        );
        assert_eq!(
            fs::read_to_string(created.root().join("eula.txt")).unwrap(),
            "eula=true\n"
        );
        assert_eq!(store.list().unwrap(), vec![created]);
    }

    #[test]
    fn creating_the_same_instance_twice_converges() {
        let directory = TestDirectory::new("store");
        let store = InstanceStore::new(DartPaths::new(directory.path().to_owned()));
        let id = InstanceId::from_str("survival").unwrap();
        let requested = config("Survival");
        let launcher = cached_launcher(&directory);

        let first = store
            .create(
                id.clone(),
                requested.clone(),
                &launcher,
                EulaAcceptance::NotAccepted,
            )
            .unwrap();
        let second = store
            .create(id, requested, &launcher, EulaAcceptance::NotAccepted)
            .unwrap();

        assert_eq!(first, second);
        assert!(!first.root().join("eula.txt").exists());
    }

    #[test]
    fn generates_a_unique_id_without_asking_the_caller() {
        let directory = TestDirectory::new("generated-id");
        let store = InstanceStore::new(DartPaths::new(directory.path().to_owned()));
        let launcher = cached_launcher(&directory);
        let name = InstanceName::parse("Friends World").unwrap();

        let first_id = store.available_id(&name);
        store
            .create(
                first_id.clone(),
                config("Friends World"),
                &launcher,
                EulaAcceptance::NotAccepted,
            )
            .unwrap();
        let second_id = store.available_id(&name);

        assert_eq!(first_id.as_str(), "friends-world");
        assert_eq!(second_id.as_str(), "friends-world-2");
    }

    #[test]
    fn does_not_adopt_an_unmanaged_directory() {
        let directory = TestDirectory::new("store");
        let store = InstanceStore::new(DartPaths::new(directory.path().to_owned()));
        let instance_root = store.instances_dir().join("survival");
        let launcher = cached_launcher(&directory);
        fs::create_dir_all(&instance_root).unwrap();
        fs::write(instance_root.join("world-data"), "keep me").unwrap();

        let result = store.create(
            InstanceId::from_str("survival").unwrap(),
            config("Survival"),
            &launcher,
            EulaAcceptance::NotAccepted,
        );

        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(instance_root.join("world-data")).unwrap(),
            "keep me"
        );
    }
}
