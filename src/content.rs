use crate::instance::Instance;
use crate::mods::{
    InstalledMod, ManagedMod, ModError, ModInstallOutcome, ModInstallPlan, ModManager, ModSearchHit,
};
use crate::packs::{
    InstalledPack, ManagedPack, PackError, PackInstallOutcome, PackInstallPlan, PackKind,
    PackManager, PackSearchHit,
};
use crate::runtime::FabricVersion;
use std::fmt;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ContentKind {
    #[default]
    Mod,
    DataPack,
    ResourcePack,
}

impl ContentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mod => "Mods",
            Self::DataPack => "Data packs",
            Self::ResourcePack => "Resource pack",
        }
    }

    pub fn singular(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::DataPack => "data pack",
            Self::ResourcePack => "resource pack",
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Mod => Self::ResourcePack,
            Self::DataPack => Self::Mod,
            Self::ResourcePack => Self::DataPack,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Mod => Self::DataPack,
            Self::DataPack => Self::ResourcePack,
            Self::ResourcePack => Self::Mod,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ContentSearchHit {
    Mod(ModSearchHit),
    DataPack(PackSearchHit),
    ResourcePack(PackSearchHit),
}

impl ContentSearchHit {
    pub fn kind(&self) -> ContentKind {
        match self {
            Self::Mod(_) => ContentKind::Mod,
            Self::DataPack(_) => ContentKind::DataPack,
            Self::ResourcePack(_) => ContentKind::ResourcePack,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::Mod(hit) => hit.project().title(),
            Self::DataPack(hit) | Self::ResourcePack(hit) => hit.project().title(),
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Self::Mod(hit) => hit.description(),
            Self::DataPack(hit) | Self::ResourcePack(hit) => hit.description(),
        }
    }

    pub fn author(&self) -> &str {
        match self {
            Self::Mod(hit) => hit.author(),
            Self::DataPack(hit) | Self::ResourcePack(hit) => hit.author(),
        }
    }

    pub fn downloads(&self) -> u64 {
        match self {
            Self::Mod(hit) => hit.downloads(),
            Self::DataPack(hit) | Self::ResourcePack(hit) => hit.downloads(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstalledContent {
    Mod(InstalledMod),
    DataPack(InstalledPack),
    ResourcePack(InstalledPack),
}

impl InstalledContent {
    pub fn kind(&self) -> ContentKind {
        match self {
            Self::Mod(_) => ContentKind::Mod,
            Self::DataPack(_) => ContentKind::DataPack,
            Self::ResourcePack(_) => ContentKind::ResourcePack,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::Mod(modification) => modification.title(),
            Self::DataPack(pack) | Self::ResourcePack(pack) => pack.title(),
        }
    }

    pub fn selection_key(&self) -> String {
        match self {
            Self::Mod(modification) => modification.filename().to_string(),
            Self::DataPack(pack) | Self::ResourcePack(pack) => pack.selection_key(),
        }
    }

    pub fn managed(&self) -> Option<ManagedContent> {
        match self {
            Self::Mod(modification) => modification.managed().cloned().map(ManagedContent::Mod),
            Self::DataPack(pack) => pack.managed().cloned().map(ManagedContent::DataPack),
            Self::ResourcePack(pack) => pack.managed().cloned().map(ManagedContent::ResourcePack),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedContent {
    Mod(ManagedMod),
    DataPack(ManagedPack),
    ResourcePack(ManagedPack),
}

impl ManagedContent {
    pub fn kind(&self) -> ContentKind {
        match self {
            Self::Mod(_) => ContentKind::Mod,
            Self::DataPack(_) => ContentKind::DataPack,
            Self::ResourcePack(_) => ContentKind::ResourcePack,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::Mod(modification) => &modification.title,
            Self::DataPack(pack) | Self::ResourcePack(pack) => &pack.title,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ContentInstallPlan {
    Mod(ModInstallPlan),
    DataPack(Box<PackInstallPlan>),
    ResourcePack(Box<PackInstallPlan>),
}

impl ContentInstallPlan {
    pub fn kind(&self) -> ContentKind {
        match self {
            Self::Mod(_) => ContentKind::Mod,
            Self::DataPack(_) => ContentKind::DataPack,
            Self::ResourcePack(_) => ContentKind::ResourcePack,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::Mod(plan) => plan.root().project().title(),
            Self::DataPack(plan) | Self::ResourcePack(plan) => plan.release().project().title(),
        }
    }

    pub fn dependency_titles(&self) -> Vec<&str> {
        match self {
            Self::Mod(plan) => plan.dependency_titles(),
            Self::DataPack(_) | Self::ResourcePack(_) => Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentInstallOutcome {
    Added,
    Updated,
    AlreadyInstalled,
}

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
    pub fn kind(&self) -> ContentKind {
        self.kind
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn outcome(&self) -> ContentInstallOutcome {
        self.outcome
    }

    pub fn dependency_titles(&self) -> &[String] {
        &self.dependency_titles
    }

    pub fn changed_dependencies(&self) -> usize {
        self.changed_dependencies
    }
}

#[derive(Clone)]
pub struct ContentManager {
    mods: ModManager,
    packs: PackManager,
}

impl ContentManager {
    pub fn new(mods: ModManager, packs: PackManager) -> Self {
        Self { mods, packs }
    }

    pub fn list(
        &self,
        instance: &Instance,
        kind: ContentKind,
    ) -> Result<Vec<InstalledContent>, ContentError> {
        match kind {
            ContentKind::Mod => Ok(self
                .mods
                .list(instance)?
                .into_iter()
                .map(InstalledContent::Mod)
                .collect()),
            ContentKind::DataPack => Ok(self
                .packs
                .list(instance, PackKind::DataPack)?
                .into_iter()
                .map(InstalledContent::DataPack)
                .collect()),
            ContentKind::ResourcePack => Ok(self
                .packs
                .list(instance, PackKind::ResourcePack)?
                .into_iter()
                .map(InstalledContent::ResourcePack)
                .collect()),
        }
    }

    pub async fn search(
        &self,
        kind: ContentKind,
        query: &str,
        minecraft: &FabricVersion,
    ) -> Result<Vec<ContentSearchHit>, ContentError> {
        match kind {
            ContentKind::Mod => Ok(self
                .mods
                .search(query, minecraft)
                .await?
                .into_iter()
                .map(ContentSearchHit::Mod)
                .collect()),
            ContentKind::DataPack => Ok(self
                .packs
                .search(PackKind::DataPack, query, minecraft)
                .await?
                .into_iter()
                .map(ContentSearchHit::DataPack)
                .collect()),
            ContentKind::ResourcePack => Ok(self
                .packs
                .search(PackKind::ResourcePack, query, minecraft)
                .await?
                .into_iter()
                .map(ContentSearchHit::ResourcePack)
                .collect()),
        }
    }

    pub async fn prepare_install(
        &self,
        hit: ContentSearchHit,
        minecraft: &FabricVersion,
    ) -> Result<ContentInstallPlan, ContentError> {
        match hit {
            ContentSearchHit::Mod(hit) => Ok(ContentInstallPlan::Mod(
                self.mods
                    .prepare_install(hit.project().clone(), minecraft)
                    .await?,
            )),
            ContentSearchHit::DataPack(hit) => Ok(ContentInstallPlan::DataPack(Box::new(
                self.packs
                    .prepare_install(PackKind::DataPack, hit.project().clone(), minecraft)
                    .await?,
            ))),
            ContentSearchHit::ResourcePack(hit) => Ok(ContentInstallPlan::ResourcePack(Box::new(
                self.packs
                    .prepare_install(PackKind::ResourcePack, hit.project().clone(), minecraft)
                    .await?,
            ))),
        }
    }

    pub async fn prepare_update(
        &self,
        content: ManagedContent,
        minecraft: &FabricVersion,
    ) -> Result<ContentInstallPlan, ContentError> {
        match content {
            ManagedContent::Mod(modification) => Ok(ContentInstallPlan::Mod(
                self.mods
                    .prepare_install(modification.project(), minecraft)
                    .await?,
            )),
            ManagedContent::DataPack(pack) => Ok(ContentInstallPlan::DataPack(Box::new(
                self.packs
                    .prepare_install(PackKind::DataPack, pack.project(), minecraft)
                    .await?,
            ))),
            ManagedContent::ResourcePack(pack) => Ok(ContentInstallPlan::ResourcePack(Box::new(
                self.packs
                    .prepare_install(PackKind::ResourcePack, pack.project(), minecraft)
                    .await?,
            ))),
        }
    }

    pub async fn apply_plan(
        &self,
        instance: &Instance,
        plan: &ContentInstallPlan,
    ) -> Result<ContentInstallReport, ContentError> {
        match plan {
            ContentInstallPlan::Mod(plan) => {
                let report = self.mods.apply_plan(instance, plan).await?;
                Ok(ContentInstallReport {
                    kind: ContentKind::Mod,
                    title: report.root().project().title().to_owned(),
                    version: report.root().version_number().to_owned(),
                    outcome: map_mod_outcome(report.root_outcome()),
                    dependency_titles: report.dependency_titles().to_vec(),
                    changed_dependencies: report.changed_dependencies(),
                })
            }
            ContentInstallPlan::DataPack(plan) | ContentInstallPlan::ResourcePack(plan) => {
                let report = self.packs.apply_plan(instance, plan).await?;
                Ok(ContentInstallReport {
                    kind: plan_kind(plan),
                    title: report.release().project().title().to_owned(),
                    version: report.release().version_number().to_owned(),
                    outcome: map_pack_outcome(report.outcome()),
                    dependency_titles: Vec::new(),
                    changed_dependencies: 0,
                })
            }
        }
    }

    pub fn remove(
        &self,
        instance: &Instance,
        content: &ManagedContent,
    ) -> Result<(), ContentError> {
        match content {
            ManagedContent::Mod(modification) => Ok(self.mods.remove(instance, modification)?),
            ManagedContent::DataPack(pack) => {
                Ok(self.packs.remove(instance, PackKind::DataPack, pack)?)
            }
            ManagedContent::ResourcePack(pack) => {
                Ok(self.packs.remove(instance, PackKind::ResourcePack, pack)?)
            }
        }
    }
}

fn plan_kind(plan: &PackInstallPlan) -> ContentKind {
    match plan.release().kind() {
        PackKind::DataPack => ContentKind::DataPack,
        PackKind::ResourcePack => ContentKind::ResourcePack,
    }
}

fn map_mod_outcome(outcome: ModInstallOutcome) -> ContentInstallOutcome {
    match outcome {
        ModInstallOutcome::Added => ContentInstallOutcome::Added,
        ModInstallOutcome::Updated => ContentInstallOutcome::Updated,
        ModInstallOutcome::AlreadyInstalled => ContentInstallOutcome::AlreadyInstalled,
    }
}

fn map_pack_outcome(outcome: PackInstallOutcome) -> ContentInstallOutcome {
    match outcome {
        PackInstallOutcome::Added => ContentInstallOutcome::Added,
        PackInstallOutcome::Updated => ContentInstallOutcome::Updated,
        PackInstallOutcome::AlreadyInstalled => ContentInstallOutcome::AlreadyInstalled,
    }
}

#[derive(Debug)]
pub enum ContentError {
    Mod(ModError),
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
