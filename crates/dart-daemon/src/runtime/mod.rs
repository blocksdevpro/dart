//! Fabric runtime versioning, discovery, and installation.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;
use std::path::PathBuf;

mod client;
mod store;

pub use client::FabricClient;
pub use store::RuntimeStore;

/// Standard filename for the compiled Fabric server launcher.
pub const FABRIC_LAUNCHER_FILE: &str = "fabric-server-launch.jar";
const META_BASE_URL: &str = "https://meta.fabricmc.net/";

/// A validated version string for Minecraft, Fabric loader, or installer.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FabricVersion(String);

impl FabricVersion {
    /// Validates and parses a version string.
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

    /// Returns the version as a string slice.
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

/// A complete Fabric runtime specification consisting of Minecraft, Loader, and Installer versions.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct FabricRuntime {
    /// The Minecraft game version.
    pub minecraft: FabricVersion,
    /// The Fabric loader version.
    pub loader: FabricVersion,
    /// The Fabric installer version used to package the server.
    pub installer: FabricVersion,
}

impl FabricRuntime {
    /// Parses and constructs a new `FabricRuntime`.
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

    /// Validates all version components in the runtime.
    pub fn validate(&self) -> Result<(), RuntimeError> {
        self.minecraft.validate()?;
        self.loader.validate()?;
        self.installer.validate()
    }

    /// Returns a human-friendly label describing this runtime.
    pub fn label(&self) -> String {
        format!(
            "Minecraft {} · loader {} · installer {}",
            self.minecraft, self.loader, self.installer
        )
    }
}

impl From<&FabricRuntime> for dart_protocol::runtime::FabricRuntimeDto {
    fn from(runtime: &FabricRuntime) -> Self {
        Self {
            minecraft: runtime.minecraft.to_string(),
            loader: runtime.loader.to_string(),
            installer: runtime.installer.to_string(),
        }
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

/// Errors that can occur during Fabric runtime resolution or caching.
#[derive(Debug)]
pub enum RuntimeError {
    /// The version string failed validation.
    InvalidVersion(String),
    /// The downloaded or cached file is not a valid ZIP/JAR archive.
    InvalidJar,
    /// Upstream returned no compatible version for a component.
    NoCompatibleVersion(&'static str),
    /// Upstream returned invalid or unexpected metadata.
    InvalidMetadata(String),
    /// An HTTP request failed.
    Http(reqwest::Error),
    /// An I/O error occurred on the filesystem.
    Io {
        /// The operation that failed.
        operation: &'static str,
        /// The path involved.
        path: PathBuf,
        /// The underlying I/O error.
        source: io::Error,
    },
}

impl RuntimeError {
    pub(crate) fn io(operation: &'static str, path: PathBuf, source: io::Error) -> Self {
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
    use crate::storage::DartPaths;
    use crate::testing::TestDirectory;
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
