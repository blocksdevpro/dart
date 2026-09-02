//! Pack installation application service.

use super::client::PackClient;
use super::store::PackStore;
use super::{
    InstalledPack, ManagedPack, PackError, PackInstallOutcome, PackInstallPlan, PackInstallReport,
    PackKind, PackProject, PackSearchHit,
};
use crate::instance::Instance;
use crate::runtime::FabricVersion;

/// Coordinates data pack and resource pack discovery, installation, and removal.
#[derive(Clone)]
pub struct PackManager {
    client: PackClient,
    store: PackStore,
}

impl PackManager {
    /// Creates a new pack manager.
    pub fn new() -> Result<Self, PackError> {
        Ok(Self {
            client: PackClient::new()?,
            store: PackStore,
        })
    }

    /// Searches Modrinth for data packs or resource packs.
    pub async fn search(
        &self,
        kind: PackKind,
        query: &str,
        minecraft: &FabricVersion,
    ) -> Result<Vec<PackSearchHit>, PackError> {
        self.client.search(kind, query, minecraft).await
    }

    /// Lists installed packs of the given kind in the instance.
    pub fn list(
        &self,
        instance: &Instance,
        kind: PackKind,
    ) -> Result<Vec<InstalledPack>, PackError> {
        self.store.list(instance, kind)
    }

    /// Prepares an installation plan for a pack project.
    pub async fn prepare_install(
        &self,
        kind: PackKind,
        project: PackProject,
        minecraft: &FabricVersion,
    ) -> Result<PackInstallPlan, PackError> {
        Ok(PackInstallPlan {
            release: self
                .client
                .newest_compatible(kind, project, minecraft)
                .await?,
        })
    }

    /// Applies a pack installation plan to the instance.
    pub async fn apply_plan(
        &self,
        instance: &Instance,
        plan: &PackInstallPlan,
    ) -> Result<PackInstallReport, PackError> {
        let outcome = if self.store.release_is_intact(instance, &plan.release)? {
            PackInstallOutcome::AlreadyInstalled
        } else {
            let bytes = self.client.download(&plan.release).await?;
            self.store.install(instance, &plan.release, &bytes)?
        };
        Ok(PackInstallReport {
            release: plan.release.clone(),
            outcome,
        })
    }

    /// Removes a managed pack from the instance.
    pub fn remove(
        &self,
        instance: &Instance,
        kind: PackKind,
        pack: &ManagedPack,
    ) -> Result<(), PackError> {
        self.store.remove(instance, kind, pack)
    }
}
