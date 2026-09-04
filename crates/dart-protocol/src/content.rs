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
    /// List of file names installed or updated.
    pub installed_files: Vec<String>,
    /// Summary message.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_installed_content() {
        let item = InstalledContentDto {
            kind: ContentKindDto::Mod,
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
