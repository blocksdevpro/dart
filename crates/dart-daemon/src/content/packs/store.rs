//! Local pack files, server.properties synchronization, and pack manifests.

use super::{
    DART_DIRECTORY, DATAPACK_MANIFEST_FILE, InstalledPack, MANIFEST_FORMAT_VERSION, ManagedPack,
    PackError, PackFileName, PackInstallOutcome, PackKind, PackRelease, RESOURCE_PACK_DIRECTORY,
    RESOURCE_PACK_MANIFEST_FILE, SERVER_PROPERTIES_FILE, file_matches_hash, sha512,
    validate_pack_zip,
};
use crate::instance::Instance;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Filesystem store for data packs, resource packs, and server.properties configuration.
#[derive(Clone, Debug, Default)]
pub struct PackStore;

impl PackStore {
    /// Lists all installed packs of the given kind.
    pub fn list(
        &self,
        instance: &Instance,
        kind: PackKind,
    ) -> Result<Vec<InstalledPack>, PackError> {
        match kind {
            PackKind::DataPack => self.list_datapacks(instance),
            PackKind::ResourcePack => self.list_resource_pack(instance),
        }
    }

    /// Installs a downloaded pack release into the instance.
    pub fn install(
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

    /// Removes a managed pack from the instance.
    pub fn remove(
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

    /// Returns `true` if the pack release is already installed and unchanged.
    pub fn release_is_intact(
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
