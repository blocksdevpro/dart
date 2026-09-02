//! Local Fabric runtime cache and launcher storage.

use super::{FABRIC_LAUNCHER_FILE, FabricRuntime, RuntimeError};
use crate::storage::DartPaths;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Local on-disk cache for downloaded Fabric server launcher JARs.
#[derive(Clone, Debug)]
pub struct RuntimeStore {
    paths: DartPaths,
}

impl RuntimeStore {
    /// Creates a new runtime store for the given storage layout.
    pub fn new(paths: DartPaths) -> Self {
        Self { paths }
    }

    /// Returns the directory where Fabric runtimes are stored.
    pub fn fabric_dir(&self) -> PathBuf {
        self.paths.fabric_runtimes_dir()
    }

    /// Returns the path to the launcher JAR for the given Fabric runtime.
    pub fn launcher_path(&self, runtime: &FabricRuntime) -> PathBuf {
        self.fabric_dir()
            .join(runtime.minecraft.as_str())
            .join(runtime.loader.as_str())
            .join(runtime.installer.as_str())
            .join(FABRIC_LAUNCHER_FILE)
    }

    /// Returns `true` if the launcher JAR for the runtime is already cached.
    pub fn is_installed(&self, runtime: &FabricRuntime) -> bool {
        self.launcher_path(runtime).is_file()
    }

    /// Scans the runtime cache directory and returns all installed Fabric runtimes.
    pub fn list(&self) -> Result<Vec<FabricRuntime>, RuntimeError> {
        let mut runtimes = Vec::new();
        let root = self.fabric_dir();
        let games = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(runtimes),
            Err(source) => return Err(RuntimeError::io("read runtime cache", root, source)),
        };
        for game in directories(games, "read Minecraft runtime versions", &root)? {
            let loaders = fs::read_dir(game.path()).map_err(|source| {
                RuntimeError::io("read Fabric loader versions", game.path(), source)
            })?;
            for loader in directories(loaders, "read Fabric loader version", &game.path())? {
                let installers = fs::read_dir(loader.path()).map_err(|source| {
                    RuntimeError::io("read Fabric installer versions", loader.path(), source)
                })?;
                for installer in
                    directories(installers, "read Fabric installer version", &loader.path())?
                {
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

    /// Validates and writes launcher JAR bytes atomically into the runtime cache.
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
    parent: &Path,
) -> Result<Vec<fs::DirEntry>, RuntimeError> {
    let mut directories = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|source| RuntimeError::io(operation, parent.to_owned(), source))?;
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
