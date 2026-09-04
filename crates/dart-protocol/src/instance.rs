//! Instance configuration, state, and management DTOs.

use crate::runtime::FabricRuntimeDto;
use serde::{Deserialize, Serialize};

/// Current lifecycle status of an instance process.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InstanceStateDto {
    /// Process is not running.
    Stopped,
    /// Process is starting up.
    Starting,
    /// Process is actively running with the given OS process ID.
    Running {
        /// The operating system process ID.
        pid: u32,
    },
    /// Process is shutting down gracefully.
    Stopping,
    /// Process crashed or failed to start.
    Failed {
        /// Explanation of why the process failed.
        message: String,
    },
}

/// Java launch parameters for an instance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FabricLaunchDto {
    /// Java command or absolute executable path.
    pub java: String,
    /// Minimum allocated heap memory in megabytes (`-Xms`).
    pub min_memory_mib: u32,
    /// Maximum allocated heap memory in megabytes (`-Xmx`).
    pub max_memory_mib: u32,
}

impl Default for FabricLaunchDto {
    fn default() -> Self {
        Self {
            java: "java".to_owned(),
            min_memory_mib: 1024,
            max_memory_mib: 4096,
        }
    }
}

/// Instance configuration stored in `dart.toml`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InstanceConfigDto {
    /// Format version of the config file.
    pub format_version: u32,
    /// Display name of the instance.
    pub name: String,
    /// Java launch configuration.
    #[serde(flatten)]
    pub launch: FabricLaunchDto,
    /// Fabric runtime coordinates.
    pub fabric: FabricRuntimeDto,
}

/// Complete instance metadata and live status representation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InstanceDto {
    /// Unique identifier for the instance (1-64 lowercase ASCII alphanumerics/hyphens).
    pub id: String,
    /// Human-friendly display name.
    pub name: String,
    /// Absolute filesystem path to the instance directory.
    pub root: String,
    /// Static instance configuration.
    pub config: InstanceConfigDto,
    /// Real-time lifecycle state.
    pub state: InstanceStateDto,
}

/// Parameters for creating a new server instance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CreateInstanceRequest {
    /// Unique instance identifier.
    pub id: String,
    /// Human-friendly display name.
    pub name: String,
    /// Target Minecraft version (e.g. `1.21.4`). Defaults to latest stable if omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft: Option<String>,
    /// Explicit Fabric loader version (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader: Option<String>,
    /// Explicit Fabric installer version (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installer: Option<String>,
    /// Whether the Minecraft EULA has been accepted by the user.
    #[serde(default)]
    pub accept_eula: bool,
    /// Minimum allocated heap memory in megabytes (optional, defaults to 1024).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_memory_mib: Option<u32>,
    /// Maximum allocated heap memory in megabytes (optional, defaults to 4096).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory_mib: Option<u32>,
    /// Java executable path (optional, defaults to "java").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java: Option<String>,
}

/// Request to update an existing instance's settings.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct UpdateInstanceRequest {
    /// Updated display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Updated minimum memory in megabytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_memory_mib: Option<u32>,
    /// Updated maximum memory in megabytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory_mib: Option<u32>,
    /// Updated Java executable path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_instance_state_dto() {
        let state = InstanceStateDto::Running { pid: 41829 };
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, r#"{"status":"running","pid":41829}"#);

        let decoded: InstanceStateDto = serde_json::from_str(&json).unwrap();
        assert_eq!(state, decoded);
    }

    #[test]
    fn serializes_create_instance_request() {
        let req = CreateInstanceRequest {
            id: "survival".to_owned(),
            name: "Survival SMP".to_owned(),
            minecraft: Some("1.21.4".to_owned()),
            loader: None,
            installer: None,
            accept_eula: true,
            min_memory_mib: Some(2048),
            max_memory_mib: Some(6144),
            java: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: CreateInstanceRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, decoded);
    }
}
