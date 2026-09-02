//! Unified error definitions for the daemon control layer.

use crate::content::ContentError;
use crate::instance::{InstanceValidationError, ServiceError, StoreError};
use crate::process::SupervisorUnavailable;
use crate::runtime::RuntimeError;
use std::fmt;

/// Top-level error type representing any operational failure in the daemon.
#[derive(Debug)]
pub enum DaemonError {
    /// Instance service or creation error.
    Instance(ServiceError),
    /// Instance store or filesystem error.
    Store(StoreError),
    /// Instance configuration validation error.
    Validation(InstanceValidationError),
    /// Instance identifier format error.
    Id(crate::instance::InstanceIdError),
    /// Runtime resolution or caching error.
    Runtime(RuntimeError),
    /// Background process supervisor error.
    Supervisor(SupervisorUnavailable),
    /// Content (mod, data pack, resource pack) management error.
    Content(ContentError),
}

impl fmt::Display for DaemonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Instance(error) => error.fmt(formatter),
            Self::Store(error) => error.fmt(formatter),
            Self::Validation(error) => error.fmt(formatter),
            Self::Id(error) => error.fmt(formatter),
            Self::Runtime(error) => error.fmt(formatter),
            Self::Supervisor(error) => error.fmt(formatter),
            Self::Content(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for DaemonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Instance(error) => Some(error),
            Self::Store(error) => Some(error),
            Self::Validation(error) => Some(error),
            Self::Id(error) => Some(error),
            Self::Runtime(error) => Some(error),
            Self::Supervisor(error) => Some(error),
            Self::Content(error) => Some(error),
        }
    }
}

impl From<ServiceError> for DaemonError {
    fn from(error: ServiceError) -> Self {
        Self::Instance(error)
    }
}

impl From<StoreError> for DaemonError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<InstanceValidationError> for DaemonError {
    fn from(error: InstanceValidationError) -> Self {
        Self::Validation(error)
    }
}

impl From<RuntimeError> for DaemonError {
    fn from(error: RuntimeError) -> Self {
        Self::Runtime(error)
    }
}

impl From<SupervisorUnavailable> for DaemonError {
    fn from(error: SupervisorUnavailable) -> Self {
        Self::Supervisor(error)
    }
}

impl From<ContentError> for DaemonError {
    fn from(error: ContentError) -> Self {
        Self::Content(error)
    }
}

impl From<crate::content::mods::ModError> for DaemonError {
    fn from(error: crate::content::mods::ModError) -> Self {
        Self::Content(ContentError::Mod(error))
    }
}

impl From<crate::content::packs::PackError> for DaemonError {
    fn from(error: crate::content::packs::PackError) -> Self {
        Self::Content(ContentError::Pack(error))
    }
}

impl From<crate::instance::InstanceIdError> for DaemonError {
    fn from(error: crate::instance::InstanceIdError) -> Self {
        Self::Id(error)
    }
}
