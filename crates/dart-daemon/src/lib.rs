//! Dart Daemon: the core control layer and single source of truth for Dart.
//!
//! This crate owns all instance configuration and persistence, Fabric runtime
//! resolution and caching, background server process supervision, and Modrinth
//! add-on content management (mods, data packs, resource packs).

pub mod content;
pub mod daemon;
pub mod error;
pub mod instance;
pub mod process;
pub mod runtime;
pub mod storage;

#[cfg(test)]
pub(crate) mod testing;

// Alias process as supervisor for backward compatibility and domain clarity
pub use process as supervisor;

// Top-level re-exports of primary types
pub use content::{
    ContentError, ContentInstallOutcome, ContentInstallPlan, ContentInstallReport, ContentKind,
    ContentManager, ContentSearchHit, InstalledContent, InstalledMod, InstalledPack,
    ManagedContent, ManagedMod, ManagedPack, ModManager, ModrinthClient, PackManager,
};
pub use daemon::Daemon;
pub use error::DaemonError;
pub use instance::{
    CreateInstance, EulaAcceptance, FabricLaunch, Instance, InstanceConfig, InstanceId,
    InstanceIdError, InstanceName, InstanceService, InstanceSize, InstanceState, InstanceStore,
    InstanceValidationError, RuntimeRequest,
};
pub use process::{
    ConsoleLineRecord, OutputStream, ServerEvent, ServerSupervisor, SupervisorCommand,
    SupervisorUnavailable,
};
pub use runtime::{FabricClient, FabricRuntime, FabricVersion, RuntimeError, RuntimeStore};
pub use storage::DartPaths;
