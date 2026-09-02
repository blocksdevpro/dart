//! Local mod files and the per-instance managed-mod manifest.

use super::{
    DART_DIRECTORY, InstalledMod, MANIFEST_FILE, MANIFEST_FORMAT_VERSION, MODS_DIRECTORY,
    ManagedMod, ModError, ModFileName, ModInstallOutcome, ModRelease, atomic_write,
    file_matches_hash, sha512, validate_mod_jar,
};
use crate::instance::Instance;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::PathBuf;

/// Filesystem store for an instance's mods directory and manifest.
#[derive(Clone, Debug, Default)]
pub struct ModStore;

impl ModStore {
    /// Creates a new mod store handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the path to the mods directory inside an instance.
    pub fn mods_dir(&self, instance: &Instance) -> PathBuf {
        instance.root().join(MODS_DIRECTORY)
    }

    /// Returns the path to the mod manifest file (`.dart/mods.toml`).
    pub fn manifest_path(&self, instance: &Instance) -> PathBuf {
        instance.root().join(DART_DIRECTORY).join(MANIFEST_FILE)
    }

    /// Lists all installed mods (both Dart-managed and external) in the instance.
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

    /// Installs or updates a mod release into the instance's mods directory.
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

    /// Removes a managed mod from the instance and updates the manifest.
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

    pub(super) fn release_is_intact(
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
