//! The central control layer of Dart.
//!
//! [`Daemon`] is the single source of truth for instance state, runtime caching,
//! process supervision, and content management. It provides a clean, cohesive
//! API surface intended for consumers such as the CLI, TUI, and future API layers.

use crate::content::mods::{ModManager, ModStore, ModrinthClient};
use crate::content::packs::PackManager;
use crate::content::{
    ContentError, ContentInstallPlan, ContentInstallReport, ContentKind, ContentManager,
    ContentSearchHit, InstalledContent, ManagedContent,
};
use crate::error::DaemonError;
use crate::instance::{
    CreateInstance, EulaAcceptance, Instance, InstanceId, InstanceName, InstanceService,
    InstanceState, InstanceStore,
};
use crate::process::{ServerEvent, ServerSupervisor};
use crate::runtime::{FabricClient, FabricRuntime, FabricVersion, RuntimeStore};
use crate::storage::DartPaths;
use tokio::sync::broadcast;

/// The central daemon control layer owning all core state and subsystems.
#[derive(Clone)]
pub struct Daemon {
    paths: DartPaths,
    instances: InstanceStore,
    runtimes: RuntimeStore,
    fabric: FabricClient,
    instance_service: InstanceService,
    supervisor: ServerSupervisor,
    content: ContentManager,
}

impl Daemon {
    /// Bootstraps the daemon with the specified storage paths.
    pub fn new(paths: DartPaths) -> Result<(Self, broadcast::Receiver<ServerEvent>), DaemonError> {
        let fabric = FabricClient::new().map_err(DaemonError::Runtime)?;
        let instances = InstanceStore::new(paths.clone());
        let runtimes = RuntimeStore::new(paths.clone());
        let instance_service =
            InstanceService::new(instances.clone(), runtimes.clone(), fabric.clone());
        let (supervisor, event_rx) = ServerSupervisor::spawn();
        let content = ContentManager::new(
            ModManager::new(
                ModrinthClient::new().map_err(ContentError::Mod)?,
                ModStore::new(),
            ),
            PackManager::new().map_err(ContentError::Pack)?,
        );

        let daemon = Self {
            paths,
            instances,
            runtimes,
            fabric,
            instance_service,
            supervisor,
            content,
        };

        Ok((daemon, event_rx))
    }

    /// Constructs a daemon instance with already initialized components.
    pub fn from_components(
        paths: DartPaths,
        instance_service: InstanceService,
        supervisor: ServerSupervisor,
        content: ContentManager,
    ) -> Self {
        let instances = instance_service.instance_store().clone();
        let runtimes = instance_service.runtime_store().clone();
        let fabric = instance_service.fabric_client().clone();
        Self {
            paths,
            instances,
            runtimes,
            fabric,
            instance_service,
            supervisor,
            content,
        }
    }

    // --- Path Layout ---

    /// Returns the storage paths layout for the daemon.
    pub fn paths(&self) -> &DartPaths {
        &self.paths
    }

    // --- Subsystem Accessors ---

    /// Returns a reference to the instance store.
    pub fn instance_store(&self) -> &InstanceStore {
        &self.instances
    }

    /// Returns a reference to the runtime store.
    pub fn runtime_store(&self) -> &RuntimeStore {
        &self.runtimes
    }

    /// Returns a reference to the Fabric API client.
    pub fn fabric_client(&self) -> &FabricClient {
        &self.fabric
    }

    /// Returns a reference to the instance service.
    pub fn instance_service(&self) -> &InstanceService {
        &self.instance_service
    }

    /// Returns a reference to the server process supervisor.
    pub fn supervisor(&self) -> &ServerSupervisor {
        &self.supervisor
    }

    /// Returns a reference to the content manager.
    pub fn content_manager(&self) -> &ContentManager {
        &self.content
    }

    // --- Event Stream ---

    /// Subscribes to the broadcast stream of server events.
    pub fn subscribe_events(&self) -> broadcast::Receiver<ServerEvent> {
        self.supervisor.subscribe()
    }

    // --- Instance Operations ---

    /// Lists all managed server instances.
    pub fn list_instances(&self) -> Result<Vec<Instance>, DaemonError> {
        self.instance_service
            .list_instances()
            .map_err(DaemonError::Instance)
    }

    /// Gets a specific managed server instance by ID.
    pub fn get_instance(&self, id: &InstanceId) -> Result<Instance, DaemonError> {
        self.instance_service
            .get_instance(id)
            .map_err(DaemonError::Instance)
    }

    /// Returns the current lifecycle state of an instance.
    pub fn instance_state(&self, id: &InstanceId) -> InstanceState {
        self.supervisor.state(id)
    }

    /// Creates a new server instance, resolving and downloading the runtime if needed.
    pub async fn create_instance(&self, request: CreateInstance) -> Result<Instance, DaemonError> {
        self.instance_service
            .create(request)
            .await
            .map_err(DaemonError::Instance)
    }

    /// Creates an instance using an already cached runtime launcher.
    pub fn create_instance_with_cached_runtime(
        &self,
        id: InstanceId,
        name: InstanceName,
        runtime: FabricRuntime,
        eula: EulaAcceptance,
    ) -> Result<Instance, DaemonError> {
        self.instance_service
            .create_with_cached_runtime(id, name, runtime, eula)
            .map_err(DaemonError::Instance)
    }

    // --- Process Lifecycle ---

    /// Starts a managed instance by ID.
    pub async fn start_instance(&self, id: &InstanceId) -> Result<(), DaemonError> {
        let instance = self.get_instance(id)?;
        self.supervisor
            .start(instance)
            .await
            .map_err(DaemonError::Supervisor)
    }

    /// Starts an instance using an existing instance handle.
    pub async fn start(&self, instance: Instance) -> Result<(), DaemonError> {
        self.supervisor
            .start(instance)
            .await
            .map_err(DaemonError::Supervisor)
    }

    /// Gracefully stops a running instance.
    pub async fn stop_instance(&self, id: &InstanceId) -> Result<(), DaemonError> {
        self.supervisor
            .stop(id.clone())
            .await
            .map_err(DaemonError::Supervisor)
    }

    /// Gracefully stops all running instances.
    pub async fn stop_all(&self) -> Result<(), DaemonError> {
        self.supervisor
            .stop_all()
            .await
            .map_err(DaemonError::Supervisor)
    }

    /// Forcefully kills all running instances.
    pub async fn kill_all(&self) -> Result<(), DaemonError> {
        self.supervisor
            .kill_all()
            .await
            .map_err(DaemonError::Supervisor)
    }

    /// Sends a console command into a running instance's standard input.
    pub async fn send_console(&self, id: &InstanceId, command: &str) -> Result<(), DaemonError> {
        self.supervisor
            .send_console(id.clone(), command.to_owned())
            .await
            .map_err(DaemonError::Supervisor)
    }

    /// Returns the most recent in-memory console log entries for an instance.
    pub fn recent_logs(
        &self,
        id: &InstanceId,
        count: Option<usize>,
    ) -> Vec<crate::process::ConsoleLineRecord> {
        self.supervisor.recent_logs(id, count)
    }

    // --- Runtime Operations ---

    /// Lists all cached Fabric runtimes on disk.
    pub fn list_runtimes(&self) -> Result<Vec<FabricRuntime>, DaemonError> {
        self.instance_service
            .list_runtimes()
            .map_err(DaemonError::Instance)
    }

    /// Queries Fabric Meta to resolve compatible loader and installer versions.
    pub async fn resolve_runtime(
        &self,
        minecraft: Option<&str>,
    ) -> Result<FabricRuntime, DaemonError> {
        self.instance_service
            .resolve_runtime(minecraft)
            .await
            .map_err(DaemonError::Instance)
    }

    /// Downloads and caches the launcher JAR for the given Fabric runtime.
    pub async fn cache_runtime(&self, runtime: &FabricRuntime) -> Result<(), DaemonError> {
        self.instance_service
            .cache_runtime(runtime)
            .await
            .map_err(DaemonError::Instance)
    }

    // --- Content Operations ---

    /// Lists installed content (mods, data packs, or resource packs) in an instance.
    pub fn list_content(
        &self,
        instance: &Instance,
        kind: ContentKind,
    ) -> Result<Vec<InstalledContent>, DaemonError> {
        self.content
            .list(instance, kind)
            .map_err(DaemonError::Content)
    }

    /// Searches Modrinth for compatible content.
    pub async fn search_content(
        &self,
        kind: ContentKind,
        query: &str,
        minecraft: &FabricVersion,
    ) -> Result<Vec<ContentSearchHit>, DaemonError> {
        self.content
            .search(kind, query, minecraft)
            .await
            .map_err(DaemonError::Content)
    }

    /// Prepares an install plan for a search result hit.
    pub async fn prepare_content_install(
        &self,
        hit: ContentSearchHit,
        minecraft: &FabricVersion,
    ) -> Result<ContentInstallPlan, DaemonError> {
        self.content
            .prepare_install(hit, minecraft)
            .await
            .map_err(DaemonError::Content)
    }

    /// Prepares an update plan for currently installed managed content.
    pub async fn prepare_content_update(
        &self,
        content: ManagedContent,
        minecraft: &FabricVersion,
    ) -> Result<ContentInstallPlan, DaemonError> {
        self.content
            .prepare_update(content, minecraft)
            .await
            .map_err(DaemonError::Content)
    }

    /// Executes an install or update plan against an instance.
    pub async fn apply_content_plan(
        &self,
        instance: &Instance,
        plan: &ContentInstallPlan,
    ) -> Result<ContentInstallReport, DaemonError> {
        self.content
            .apply_plan(instance, plan)
            .await
            .map_err(DaemonError::Content)
    }

    /// Removes managed content from an instance.
    pub fn remove_content(
        &self,
        instance: &Instance,
        content: &ManagedContent,
    ) -> Result<(), DaemonError> {
        self.content
            .remove(instance, content)
            .map_err(DaemonError::Content)
    }
}

#[cfg(test)]
mod tests {
    use super::Daemon;
    use crate::instance::{EulaAcceptance, InstanceId, InstanceName, InstanceState};
    use crate::runtime::FabricRuntime;
    use crate::storage::DartPaths;
    use crate::testing::TestDirectory;
    use std::str::FromStr;

    #[tokio::test]
    async fn bootstrap_and_query_state() {
        let directory = TestDirectory::new("daemon");
        let paths = DartPaths::new(directory.path().to_owned());
        let (daemon, _events) = Daemon::new(paths).unwrap();

        assert_eq!(daemon.list_instances().unwrap(), vec![]);
        assert_eq!(daemon.list_runtimes().unwrap(), vec![]);

        let id = InstanceId::from_str("survival").unwrap();
        assert_eq!(daemon.instance_state(&id), InstanceState::Stopped);
    }

    #[tokio::test]
    async fn creates_and_tracks_instance() {
        let directory = TestDirectory::new("daemon");
        let paths = DartPaths::new(directory.path().to_owned());
        let (daemon, _events) = Daemon::new(paths).unwrap();

        let runtime = FabricRuntime::new("1.21.8", "0.17.2", "1.1.2").unwrap();
        daemon
            .runtime_store()
            .install_bytes(&runtime, b"PK\x03\x04launcher")
            .unwrap();

        let id = InstanceId::from_str("survival").unwrap();
        let name = InstanceName::parse("Survival").unwrap();
        let instance = daemon
            .create_instance_with_cached_runtime(
                id.clone(),
                name,
                runtime.clone(),
                EulaAcceptance::Accepted,
            )
            .unwrap();

        assert_eq!(instance.id(), &id);
        let listed = daemon.list_instances().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id(), &id);

        let fetched = daemon.get_instance(&id).unwrap();
        assert_eq!(fetched.id(), &id);
        assert_eq!(daemon.instance_state(&id), InstanceState::Stopped);
    }
}
