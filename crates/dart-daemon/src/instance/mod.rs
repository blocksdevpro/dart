//! Instance data structures, configuration validation, and identity.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::runtime::FabricRuntime;

mod service;
mod store;

pub use service::{CreateInstance, InstanceService, RuntimeRequest, ServiceError};
pub use store::{InstanceStore, StoreError};

/// The expected configuration format version for `dart.toml`.
pub const CONFIG_FORMAT_VERSION: u32 = 2;

/// Explicit acceptance or rejection of the Minecraft End User License Agreement.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EulaAcceptance {
    /// EULA is not yet accepted.
    #[default]
    NotAccepted,
    /// EULA has been accepted by the user.
    Accepted,
}

impl EulaAcceptance {
    /// Returns `true` if the EULA has been accepted.
    pub fn is_accepted(self) -> bool {
        self == Self::Accepted
    }
}

/// A validated unique identifier for a server instance.
///
/// Instance IDs must be between 1 and 64 ASCII lowercase alphanumeric characters
/// or hyphens, starting and ending with an alphanumeric character.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstanceId(String);

impl InstanceId {
    /// Returns the instance ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InstanceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for InstanceId {
    type Err = InstanceIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() || value.len() > 64 {
            return Err(InstanceIdError::Length);
        }

        let is_edge = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
        if !is_edge(value.as_bytes()[0]) || !is_edge(value.as_bytes()[value.len() - 1]) {
            return Err(InstanceIdError::Edge);
        }

        if !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(InstanceIdError::Character);
        }

        Ok(Self(value.to_owned()))
    }
}

/// Errors when parsing or validating an [`InstanceId`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstanceIdError {
    /// Must contain between 1 and 64 characters.
    Length,
    /// Must start and end with an alphanumeric character.
    Edge,
    /// Contains invalid characters (only lowercase, digits, hyphens allowed).
    Character,
}

impl fmt::Display for InstanceIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length => formatter.write_str("must contain between 1 and 64 characters"),
            Self::Edge => {
                formatter.write_str("must start and end with a lowercase letter or digit")
            }
            Self::Character => {
                formatter.write_str("may contain only lowercase letters, digits, and hyphens")
            }
        }
    }
}

impl std::error::Error for InstanceIdError {}

/// The human-readable display name for a server instance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstanceName(String);

impl InstanceName {
    /// Validates and constructs an [`InstanceName`].
    pub fn parse(value: impl AsRef<str>) -> Result<Self, InstanceValidationError> {
        let name = value.as_ref().trim();
        if name.is_empty() {
            return Err(InstanceValidationError::EmptyName);
        }
        if name.chars().count() > 64 {
            return Err(InstanceValidationError::NameTooLong);
        }
        if name.contains(['\n', '\r']) {
            return Err(InstanceValidationError::NameContainsNewline);
        }

        Ok(Self(name.to_owned()))
    }

    fn validate(&self) -> Result<(), InstanceValidationError> {
        Self::parse(&self.0).map(|_| ())
    }
}

impl fmt::Display for InstanceName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Process launch parameters for Java and Fabric.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FabricLaunch {
    /// Executable path for Java.
    pub java: PathBuf,
    /// Minimum memory allocated to Java heap in MiB (-Xms).
    pub min_memory_mib: u32,
    /// Maximum memory allocated to Java heap in MiB (-Xmx).
    pub max_memory_mib: u32,
}

impl FabricLaunch {
    fn validate(&self) -> Result<(), InstanceValidationError> {
        if self.java.as_os_str().is_empty() {
            return Err(InstanceValidationError::EmptyJavaCommand);
        }
        if self.min_memory_mib == 0 || self.max_memory_mib == 0 {
            return Err(InstanceValidationError::ZeroMemory);
        }
        if self.min_memory_mib > self.max_memory_mib {
            return Err(InstanceValidationError::InvalidMemoryRange);
        }

        Ok(())
    }
}

impl Default for FabricLaunch {
    fn default() -> Self {
        Self {
            java: PathBuf::from("java"),
            min_memory_mib: 1024,
            max_memory_mib: 4096,
        }
    }
}

/// The persistent configuration file schema stored in `dart.toml`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InstanceConfig {
    /// File format version.
    pub format_version: u32,
    /// Display name of the instance.
    pub name: InstanceName,
    /// Java launch configuration.
    #[serde(flatten)]
    pub launch: FabricLaunch,
    /// Fabric runtime coordinates.
    pub fabric: FabricRuntime,
}

impl InstanceConfig {
    /// Creates a new instance configuration.
    pub fn new(name: InstanceName, launch: FabricLaunch, fabric: FabricRuntime) -> Self {
        Self {
            format_version: CONFIG_FORMAT_VERSION,
            name,
            launch,
            fabric,
        }
    }

    /// Validates all configuration settings.
    pub fn validate(&self) -> Result<(), InstanceValidationError> {
        if self.format_version != CONFIG_FORMAT_VERSION {
            return Err(InstanceValidationError::UnsupportedFormatVersion(
                self.format_version,
            ));
        }
        self.name.validate()?;
        self.launch.validate()?;
        self.fabric
            .validate()
            .map_err(|error| InstanceValidationError::InvalidFabricRuntime(error.to_string()))
    }
}

/// A managed Minecraft server instance on disk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Instance {
    id: InstanceId,
    root: PathBuf,
    config: InstanceConfig,
}

impl Instance {
    /// Creates an instance handle.
    pub fn new(id: InstanceId, root: PathBuf, config: InstanceConfig) -> Self {
        Self { id, root, config }
    }

    /// Returns the unique identifier of the instance.
    pub fn id(&self) -> &InstanceId {
        &self.id
    }

    /// Returns the filesystem directory where the instance resides.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the configuration of the instance.
    pub fn config(&self) -> &InstanceConfig {
        &self.config
    }
}

/// Current lifecycle status of an instance process.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum InstanceState {
    /// Process is not running.
    Stopped,
    /// Process is starting up.
    Starting,
    /// Process is actively running with the given OS process ID.
    Running {
        /// The OS process ID.
        pid: u32,
    },
    /// Process is shutting down gracefully.
    Stopping,
    /// Process failed or crashed with an explanatory message.
    Failed {
        /// Explanation of failure.
        message: String,
    },
}

/// Errors that occur when validating instance properties or configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstanceValidationError {
    /// Instance name cannot be empty.
    EmptyName,
    /// Instance name exceeds 64 characters.
    NameTooLong,
    /// Instance name cannot contain newlines.
    NameContainsNewline,
    /// Java command path cannot be empty.
    EmptyJavaCommand,
    /// Memory limit must be greater than zero.
    ZeroMemory,
    /// Minimum memory cannot exceed maximum memory.
    InvalidMemoryRange,
    /// Fabric runtime coordinates are invalid.
    InvalidFabricRuntime(String),
    /// Format version in `dart.toml` is unsupported.
    UnsupportedFormatVersion(u32),
}

impl fmt::Display for InstanceValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => formatter.write_str("instance name cannot be empty"),
            Self::NameTooLong => formatter.write_str("instance name cannot exceed 64 characters"),
            Self::NameContainsNewline => {
                formatter.write_str("instance name cannot contain a newline")
            }
            Self::EmptyJavaCommand => formatter.write_str("Java command cannot be empty"),
            Self::ZeroMemory => formatter.write_str("memory limits must be greater than zero"),
            Self::InvalidMemoryRange => {
                formatter.write_str("minimum memory cannot exceed maximum memory")
            }
            Self::InvalidFabricRuntime(message) => {
                write!(formatter, "invalid Fabric runtime: {message}")
            }
            Self::UnsupportedFormatVersion(version) => {
                write!(formatter, "unsupported dart.toml format version {version}")
            }
        }
    }
}

impl std::error::Error for InstanceValidationError {}

#[cfg(test)]
mod tests {
    use super::{FabricLaunch, InstanceConfig, InstanceId, InstanceName, InstanceValidationError};
    use crate::runtime::FabricRuntime;
    use std::path::PathBuf;
    use std::str::FromStr;

    #[test]
    fn accepts_safe_instance_ids() {
        assert_eq!(
            InstanceId::from_str("survival-2026").unwrap().as_str(),
            "survival-2026"
        );
    }

    #[test]
    fn rejects_instance_ids_that_escape_the_instances_directory() {
        for id in [
            "",
            ".",
            "..",
            "survival/backup",
            "Survival",
            "-survival",
            "survival-",
        ] {
            assert!(InstanceId::from_str(id).is_err(), "{id} must be rejected");
        }
    }

    #[test]
    fn rejects_an_empty_persisted_java_command() {
        let config = InstanceConfig {
            format_version: super::CONFIG_FORMAT_VERSION,
            name: InstanceName::parse("Survival").unwrap(),
            launch: FabricLaunch {
                java: PathBuf::new(),
                min_memory_mib: 2048,
                max_memory_mib: 1024,
            },
            fabric: FabricRuntime::new("1.21.8", "0.17.2", "1.1.2").unwrap(),
        };

        assert!(matches!(
            config.validate(),
            Err(InstanceValidationError::EmptyJavaCommand)
        ));
    }

    #[test]
    fn rejects_an_inverted_persisted_memory_range() {
        let config = InstanceConfig {
            format_version: super::CONFIG_FORMAT_VERSION,
            name: InstanceName::parse("Survival").unwrap(),
            launch: FabricLaunch {
                java: PathBuf::from("java"),
                min_memory_mib: 2048,
                max_memory_mib: 1024,
            },
            fabric: FabricRuntime::new("1.21.8", "0.17.2", "1.1.2").unwrap(),
        };

        assert!(matches!(
            config.validate(),
            Err(InstanceValidationError::InvalidMemoryRange)
        ));
    }
}
