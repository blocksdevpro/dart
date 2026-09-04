//! Add-on content (mods, data packs, resource packs) protocol schemas.

use serde::{Deserialize, Serialize};

/// The category of server content.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKindDto {
    /// Fabric server mod (`mods/`).
    Mod,
    /// World data pack (`<world>/datapacks/`).
    DataPack,
    /// Server resource pack (`server.properties`).
    ResourcePack,
}

impl ContentKindDto {
    /// Returns the user-facing label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Mod => "Mods",
            Self::DataPack => "Data packs",
            Self::ResourcePack => "Resource packs",
        }
    }
}

/// Information about an installed content file in an instance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InstalledContentDto {
    /// Content kind (`mod`, `data_pack`, `resource_pack`).
    pub kind: ContentKindDto,
    /// Stable key to use for update or removal operations.
    pub key: String,
    /// Filename on disk (e.g. `fabric-api-0.92.0.jar`).
    pub file_name: String,
    /// Display name or mod ID.
    pub name: String,
    /// Version string if discovered or managed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Whether this file is tracked and managed by Dart.
    pub managed: bool,
    /// Modrinth project ID if tracked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

/// Search hit returned from Modrinth query.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContentSearchHitDto {
    /// Modrinth project ID.
    pub id: String,
    /// URL slug.
    pub slug: String,
    /// Project title.
    pub title: String,
    /// Short description.
    pub description: String,
    /// Author / creator username.
    pub author: String,
    /// Total downloads.
    pub downloads: u64,
    /// Project icon URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    /// Content category.
    pub kind: ContentKindDto,
}

/// Request to install the newest compatible release of a Modrinth project.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InstallContentRequest {
    /// Type of add-on being installed.
    pub kind: ContentKindDto,
    /// Modrinth project identifier returned by search.
    pub project_id: String,
}

/// Request to remove content that Dart already manages.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoveContentRequest {
    /// Type of add-on being removed.
    pub kind: ContentKindDto,
    /// Stable selection key returned by the installed-content endpoint.
    pub key: String,
}

/// Result of installing or updating an add-on.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentInstallOutcomeDto {
    /// The add-on was newly installed.
    Added,
    /// An older managed release was replaced.
    Updated,
    /// The newest compatible release was already installed.
    AlreadyInstalled,
}

/// Resolved plan for installing or updating content.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContentInstallPlanDto {
    /// Content kind.
    pub kind: ContentKindDto,
    /// Human-readable title.
    pub title: String,
    /// Target file name on disk.
    pub file_name: String,
    /// Direct download URL.
    pub download_url: String,
    /// Expected SHA-512 checksum if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha512: Option<String>,
    /// Required dependency plans that will also be installed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<ContentInstallPlanDto>,
}

/// Outcome report returned after applying a content plan.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContentInstallReportDto {
    /// Type of add-on that was installed.
    pub kind: ContentKindDto,
    /// Human-readable project name.
    pub title: String,
    /// Installed project version.
    pub version: String,
    /// Whether the operation added, updated, or kept the current release.
    pub outcome: ContentInstallOutcomeDto,
    /// Required dependencies included in the plan.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_installed_content() {
        let item = InstalledContentDto {
            kind: ContentKindDto::Mod,
            key: "sodium-fabric-0.5.8.jar".to_owned(),
            file_name: "sodium-fabric-0.5.8.jar".to_owned(),
            name: "Sodium".to_owned(),
            version: Some("0.5.8".to_owned()),
            managed: true,
            project_id: Some("AANobbMI".to_owned()),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains(r#""kind":"mod""#));
        assert!(json.contains("sodium-fabric-0.5.8.jar"));

        let decoded: InstalledContentDto = serde_json::from_str(&json).unwrap();
        assert_eq!(item, decoded);
    }
}
