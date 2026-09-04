//! Dart Protocol: wire schemas, DTOs, and event models for Dart daemon clients.
//!
//! This crate contains only data structures and serialization rules. It has zero
//! runtime dependencies beyond `serde` and `serde_json`, allowing it to be used by
//! the daemon, CLI, TUI, client SDKs, and code-generation tools.

pub mod console;
pub mod content;
pub mod error;
pub mod event;
pub mod instance;
pub mod runtime;
pub mod system;

// Re-export primary types for convenience
pub use console::{
    ConsoleCommandRequest, ConsoleLineDto, ConsoleWsClientMessage, ConsoleWsServerMessage,
    OutputStreamDto,
};
pub use content::{
    ContentInstallOutcomeDto, ContentInstallPlanDto, ContentInstallReportDto, ContentKindDto,
    ContentSearchHitDto, InstallContentRequest, InstalledContentDto, RemoveContentRequest,
};
pub use error::{ApiErrorDetail, ApiErrorResponse, codes as error_codes};
pub use event::DaemonEvent;
pub use instance::{
    CreateInstanceOptionsResponse, CreateInstanceRequest, FabricLaunchDto, InstanceConfigDto,
    InstanceDto, InstanceSizeDto, InstanceSizeOptionDto, InstanceStateDto,
    MinecraftVersionOptionDto, UpdateInstanceRequest,
};
pub use runtime::{DownloadRuntimeRequest, FabricRuntimeDto, ResolveRuntimeResponse};
pub use system::{HealthResponse, SystemInfoResponse};
