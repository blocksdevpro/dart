//! Fabric runtime representations and requests.

use serde::{Deserialize, Serialize};

/// Version coordinates identifying a specific Fabric server runtime.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct FabricRuntimeDto {
    /// Minecraft vanilla version (e.g. `1.21.4`).
    pub minecraft: String,
    /// Fabric loader version (e.g. `0.16.10`).
    pub loader: String,
    /// Fabric installer version (e.g. `1.0.1`).
    pub installer: String,
}

impl FabricRuntimeDto {
    /// Creates a new Fabric runtime DTO.
    pub fn new(
        minecraft: impl Into<String>,
        loader: impl Into<String>,
        installer: impl Into<String>,
    ) -> Self {
        Self {
            minecraft: minecraft.into(),
            loader: loader.into(),
            installer: installer.into(),
        }
    }
}

/// Request payload for downloading and caching a Fabric runtime.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadRuntimeRequest {
    /// Optional target Minecraft version. Defaults to latest stable if omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft: Option<String>,
    /// Optional explicit loader version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader: Option<String>,
    /// Optional explicit installer version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installer: Option<String>,
}

/// Response payload from resolving runtime compatibility with Fabric Meta.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResolveRuntimeResponse {
    /// The resolved compatible runtime.
    pub runtime: FabricRuntimeDto,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_fabric_runtime_dto() {
        let runtime = FabricRuntimeDto::new("1.21.4", "0.16.10", "1.0.1");
        let json = serde_json::to_string(&runtime).unwrap();
        let decoded: FabricRuntimeDto = serde_json::from_str(&json).unwrap();
        assert_eq!(runtime, decoded);
    }
}
