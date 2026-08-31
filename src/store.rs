use crate::instance::{
    EulaAcceptance, Instance, InstanceConfig, InstanceId, InstanceIdError, InstanceValidationError,
};
use crate::paths::DartPaths;
use crate::runtime::FABRIC_LAUNCHER_FILE;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

const CONFIG_FILE: &str = "dart.toml";
const EULA_FILE: &str = "eula.txt";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct InstanceStore {
    paths: DartPaths,
}

impl InstanceStore {
    pub fn new(paths: DartPaths) -> Self {
        Self { paths }
    }

    pub fn dart_home(&self) -> &Path {
        self.paths.home()
    }

    pub fn instances_dir(&self) -> PathBuf {
        self.paths.instances_dir()
    }

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
                    self.instances_dir(),
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

#[derive(Debug)]
pub enum StoreError {
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    InvalidInstanceId {
        path: PathBuf,
        source: InstanceIdError,
    },
    ParseConfig {
        path: PathBuf,
        source: toml::de::Error,
    },
    SerializeConfig {
        path: PathBuf,
        source: toml::ser::Error,
    },
    InvalidConfig {
        path: PathBuf,
        source: InstanceValidationError,
    },
    MissingConfigParent {
        path: PathBuf,
    },
    InstanceAlreadyExists {
        id: InstanceId,
        root: PathBuf,
    },
    UnmanagedDirectory {
        root: PathBuf,
    },
    CachedLauncherMissing {
        path: PathBuf,
    },
}

impl StoreError {
    fn io(operation: &'static str, path: PathBuf, source: io::Error) -> Self {
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
            | Self::UnmanagedDirectory { .. }
            | Self::CachedLauncherMissing { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::InstanceStore;
    use crate::instance::{EulaAcceptance, FabricLaunch, InstanceConfig, InstanceId, InstanceName};
    use crate::paths::DartPaths;
    use crate::runtime::{FABRIC_LAUNCHER_FILE, FabricRuntime};
    use crate::test_support::TestDirectory;
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
