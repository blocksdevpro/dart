//! Unified add-on content management: mods, data packs, and resource packs.

pub mod mods;
pub mod packs;

mod manager;

pub use manager::ContentManager;
pub use mods::{
    InstalledMod, ManagedMod, ModError, ModInstallPlan, ModManager, ModSearchHit, ModStore,
    ModrinthClient,
};
pub use packs::{
    InstalledPack, ManagedPack, PackClient, PackError, PackInstallPlan, PackManager, PackSearchHit,
    PackStore,
};

use std::fmt;

/// The category of installable server content.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ContentKind {
    /// Fabric server mod (`mods/`).
    #[default]
    Mod,
    /// World data pack (`<world>/datapacks/`).
    DataPack,
    /// Server resource pack (`server.properties`).
    ResourcePack,
}

impl ContentKind {
    /// Returns the plural label for the content kind.
    pub fn label(self) -> &'static str {
        match self {
            Self::Mod => "Mods",
            Self::DataPack => "Data packs",
            Self::ResourcePack => "Resource pack",
        }
    }

    /// Returns the singular noun for the content kind.
    pub fn singular(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::DataPack => "data pack",
            Self::ResourcePack => "resource pack",
        }
    }

    /// Cycles to the previous content kind in UI tabs.
    pub fn previous(self) -> Self {
        match self {
            Self::Mod => Self::ResourcePack,
            Self::DataPack => Self::Mod,
            Self::ResourcePack => Self::DataPack,
        }
    }

    /// Cycles to the next content kind in UI tabs.
    pub fn next(self) -> Self {
        match self {
            Self::Mod => Self::DataPack,
            Self::DataPack => Self::ResourcePack,
            Self::ResourcePack => Self::Mod,
        }
    }
}

/// A search hit from Modrinth across any content kind.
#[derive(Clone, Debug)]
pub enum ContentSearchHit {
    /// A mod search hit.
    Mod(ModSearchHit),
    /// A data pack search hit.
    DataPack(PackSearchHit),
    /// A resource pack search hit.
    ResourcePack(PackSearchHit),
}

impl ContentSearchHit {
    /// Returns the content kind of this hit.
    pub fn kind(&self) -> ContentKind {
        match self {
            Self::Mod(_) => ContentKind::Mod,
            Self::DataPack(_) => ContentKind::DataPack,
            Self::ResourcePack(_) => ContentKind::ResourcePack,
        }
    }

    /// Returns the title of the project.
    pub fn title(&self) -> &str {
        match self {
            Self::Mod(hit) => hit.project().title(),
            Self::DataPack(hit) | Self::ResourcePack(hit) => hit.project().title(),
        }
    }

    /// Returns the project description.
    pub fn description(&self) -> &str {
        match self {
            Self::Mod(hit) => hit.description(),
            Self::DataPack(hit) | Self::ResourcePack(hit) => hit.description(),
        }
    }

    /// Returns the author of the project.
    pub fn author(&self) -> &str {
        match self {
            Self::Mod(hit) => hit.author(),
            Self::DataPack(hit) | Self::ResourcePack(hit) => hit.author(),
        }
    }

    /// Returns the total download count.
    pub fn downloads(&self) -> u64 {
        match self {
            Self::Mod(hit) => hit.downloads(),
            Self::DataPack(hit) | Self::ResourcePack(hit) => hit.downloads(),
        }
    }
}

/// An installed content item in an instance (mod, data pack, or resource pack).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstalledContent {
    /// An installed mod.
    Mod(InstalledMod),
    /// An installed data pack.
    DataPack(InstalledPack),
    /// An installed resource pack.
    ResourcePack(InstalledPack),
}

impl InstalledContent {
    /// Returns the kind of content.
    pub fn kind(&self) -> ContentKind {
        match self {
            Self::Mod(_) => ContentKind::Mod,
            Self::DataPack(_) => ContentKind::DataPack,
            Self::ResourcePack(_) => ContentKind::ResourcePack,
        }
    }

    /// Returns the display title.
    pub fn title(&self) -> &str {
        match self {
            Self::Mod(modification) => modification.title(),
            Self::DataPack(pack) | Self::ResourcePack(pack) => pack.title(),
        }
    }

    /// Returns a selection key for UI state preservation.
    pub fn selection_key(&self) -> String {
        match self {
            Self::Mod(modification) => modification.filename().to_string(),
            Self::DataPack(pack) | Self::ResourcePack(pack) => pack.selection_key(),
        }
    }

    /// Returns the managed content metadata if managed by Dart.
    pub fn managed(&self) -> Option<ManagedContent> {
        match self {
            Self::Mod(modification) => modification.managed().cloned().map(ManagedContent::Mod),
            Self::DataPack(pack) => pack.managed().cloned().map(ManagedContent::DataPack),
            Self::ResourcePack(pack) => pack.managed().cloned().map(ManagedContent::ResourcePack),
        }
    }
}

/// Managed metadata for an item tracked in a Dart manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedContent {
    /// Managed mod.
    Mod(ManagedMod),
    /// Managed data pack.
    DataPack(ManagedPack),
    /// Managed resource pack.
    ResourcePack(ManagedPack),
}

impl ManagedContent {
    /// Returns the kind of content.
    pub fn kind(&self) -> ContentKind {
        match self {
            Self::Mod(_) => ContentKind::Mod,
            Self::DataPack(_) => ContentKind::DataPack,
            Self::ResourcePack(_) => ContentKind::ResourcePack,
        }
    }

    /// Returns the title of the managed item.
    pub fn title(&self) -> &str {
        match self {
            Self::Mod(modification) => &modification.title,
            Self::DataPack(pack) | Self::ResourcePack(pack) => &pack.title,
        }
    }
}

/// An install plan for adding or updating content.
#[derive(Clone, Debug)]
pub enum ContentInstallPlan {
    /// Mod install plan.
    Mod(ModInstallPlan),
    /// Data pack install plan.
    DataPack(Box<PackInstallPlan>),
    /// Resource pack install plan.
    ResourcePack(Box<PackInstallPlan>),
}

impl ContentInstallPlan {
    /// Returns the kind of content targeted by this plan.
    pub fn kind(&self) -> ContentKind {
        match self {
            Self::Mod(_) => ContentKind::Mod,
            Self::DataPack(_) => ContentKind::DataPack,
            Self::ResourcePack(_) => ContentKind::ResourcePack,
        }
    }

    /// Returns the title of the root project being installed.
    pub fn title(&self) -> &str {
        match self {
            Self::Mod(plan) => plan.root().project().title(),
            Self::DataPack(plan) | Self::ResourcePack(plan) => plan.release().project().title(),
        }
    }

    /// Returns the titles of any dependencies included in the plan.
    pub fn dependency_titles(&self) -> Vec<&str> {
        match self {
            Self::Mod(plan) => plan.dependency_titles(),
            Self::DataPack(_) | Self::ResourcePack(_) => Vec::new(),
        }
    }
}

/// Outcome of applying a content install plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentInstallOutcome {
    /// Content was added.
    Added,
    /// Content was updated to a newer version.
    Updated,
    /// Content was already installed with matching version and hash.
    AlreadyInstalled,
}

/// Report summarizing the result of a content installation.
#[derive(Clone, Debug)]
pub struct ContentInstallReport {
    kind: ContentKind,
    title: String,
    version: String,
    outcome: ContentInstallOutcome,
    dependency_titles: Vec<String>,
    changed_dependencies: usize,
}

impl ContentInstallReport {
    /// Returns the kind of content installed.
    pub fn kind(&self) -> ContentKind {
        self.kind
    }

    /// Returns the title of the installed item.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the installed version number.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Returns the installation outcome.
    pub fn outcome(&self) -> ContentInstallOutcome {
        self.outcome
    }

    /// Returns the titles of dependencies that were installed or updated.
    pub fn dependency_titles(&self) -> &[String] {
        &self.dependency_titles
    }

    /// Returns the count of dependencies that changed.
    pub fn changed_dependencies(&self) -> usize {
        self.changed_dependencies
    }
}

/// Errors occurring across mod or pack operations.
#[derive(Debug)]
pub enum ContentError {
    /// Mod-specific error.
    Mod(ModError),
    /// Pack-specific error.
    Pack(PackError),
}

impl fmt::Display for ContentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mod(error) => error.fmt(formatter),
            Self::Pack(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ContentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Mod(error) => Some(error),
            Self::Pack(error) => Some(error),
        }
    }
}

impl From<ModError> for ContentError {
    fn from(error: ModError) -> Self {
        Self::Mod(error)
    }
}

impl From<PackError> for ContentError {
    fn from(error: PackError) -> Self {
        Self::Pack(error)
    }
}
