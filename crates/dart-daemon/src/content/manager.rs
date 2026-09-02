//! Application service unifying mods, data packs, and resource packs.

use super::mods::{ModInstallOutcome, ModManager};
use super::packs::{PackInstallOutcome, PackInstallPlan, PackKind, PackManager};
use super::{
    ContentError, ContentInstallOutcome, ContentInstallPlan, ContentInstallReport, ContentKind,
    ContentSearchHit, InstalledContent, ManagedContent,
};
use crate::instance::Instance;
use crate::runtime::FabricVersion;

/// Coordinates mods, data packs, and resource packs behind a single content management interface.
#[derive(Clone)]
pub struct ContentManager {
    mods: ModManager,
    packs: PackManager,
}

impl ContentManager {
    /// Creates a new content manager from mod and pack managers.
    pub fn new(mods: ModManager, packs: PackManager) -> Self {
        Self { mods, packs }
    }

    /// Lists all installed content of the specified kind in an instance.
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

    /// Searches Modrinth for content of the specified kind.
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

    /// Prepares an installation plan for a search result hit.
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

    /// Prepares an update plan for currently installed managed content.
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

    /// Executes an installation or update plan against an instance.
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

    /// Removes managed content from an instance.
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

    /// Returns a reference to the underlying mod manager.
    pub fn mods(&self) -> &ModManager {
        &self.mods
    }

    /// Returns a reference to the underlying pack manager.
    pub fn packs(&self) -> &PackManager {
        &self.packs
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
