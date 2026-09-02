//! Dart: Local Fabric server instance manager CLI and TUI.

pub mod cli;
pub mod tui;
pub use tui as ui;

// Re-export the daemon control layer
pub use dart_daemon as daemon;
pub use dart_daemon::content::{mods, packs};
pub use dart_daemon::{
    ContentError, ContentInstallOutcome, ContentInstallPlan, ContentInstallReport, ContentKind,
    ContentManager, ContentSearchHit, Daemon, DaemonError, DartPaths, EulaAcceptance, FabricClient,
    FabricLaunch, FabricRuntime, FabricVersion, InstalledContent, InstalledMod, InstalledPack,
    Instance, InstanceConfig, InstanceId, InstanceName, InstanceService, InstanceState,
    InstanceStore, ManagedContent, ManagedMod, ManagedPack, ModManager, ModrinthClient,
    OutputStream, PackManager, RuntimeRequest, RuntimeStore, ServerEvent, ServerSupervisor,
    content, instance, process, runtime, storage, supervisor,
};
