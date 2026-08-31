//! Shared instance workflows for the CLI and TUI.
//!
//! This module owns the policy that joins a Fabric runtime cache to instance
//! creation. Front ends validate their raw input, choose a `RuntimeRequest`,
//! and present the result. They do not build configuration files or copy
//! launchers themselves.

use crate::instance::{EulaAcceptance, Instance, InstanceId, InstanceName};
use crate::runtime::{FabricClient, FabricRuntime, FabricVersion, RuntimeError, RuntimeStore};
use crate::store::{InstanceStore, StoreError};
use std::fmt;

#[derive(Clone, Debug)]
pub enum RuntimeRequest {
    /// Reuse an exact cached runtime or download that exact launcher.
    Exact(FabricRuntime),
    /// Resolve and cache the newest stable Fabric runtime.
    Latest,
    /// Resolve and cache the newest stable Fabric runtime for one Minecraft version.
    Minecraft(FabricVersion),
}

#[derive(Clone, Debug)]
pub struct CreateInstance {
    id: InstanceId,
    name: InstanceName,
    runtime: RuntimeRequest,
    eula: EulaAcceptance,
}

impl CreateInstance {
    pub fn new(
        id: InstanceId,
        name: InstanceName,
        runtime: RuntimeRequest,
        eula: EulaAcceptance,
    ) -> Self {
        Self {
            id,
            name,
            runtime,
            eula,
        }
    }
}

#[derive(Clone)]
pub struct InstanceService {
    instances: InstanceStore,
    runtimes: RuntimeStore,
    fabric: FabricClient,
}

impl InstanceService {
    pub fn new(instances: InstanceStore, runtimes: RuntimeStore, fabric: FabricClient) -> Self {
        Self {
            instances,
            runtimes,
            fabric,
        }
    }

    pub fn list_instances(&self) -> Result<Vec<Instance>, ServiceError> {
        self.instances.list().map_err(ServiceError::Store)
    }

    pub fn list_runtimes(&self) -> Result<Vec<FabricRuntime>, ServiceError> {
        self.runtimes.list().map_err(ServiceError::Runtime)
    }

    pub fn instance_store(&self) -> &InstanceStore {
        &self.instances
    }

    /// Resolve only the version coordinates. The TUI uses this before it shows
    /// the download phase to the user.
    pub async fn resolve_runtime(
        &self,
        minecraft: Option<&str>,
    ) -> Result<FabricRuntime, ServiceError> {
        self.fabric
            .resolve(minecraft)
            .await
            .map_err(ServiceError::Runtime)
    }

    /// Store a launcher for already validated Fabric coordinates. Repeated
    /// calls reuse the cache without downloading again.
    pub async fn cache_runtime(&self, runtime: &FabricRuntime) -> Result<(), ServiceError> {
        self.fabric
            .download(runtime, &self.runtimes)
            .await
            .map(|_| ())
            .map_err(ServiceError::Runtime)
    }

    pub async fn create(&self, request: CreateInstance) -> Result<Instance, ServiceError> {
        let runtime = self.runtime_for(request.runtime).await?;
        self.create_with_cached_runtime(request.id, request.name, runtime, request.eula)
    }

    pub fn create_with_cached_runtime(
        &self,
        id: InstanceId,
        name: InstanceName,
        runtime: FabricRuntime,
        eula: EulaAcceptance,
    ) -> Result<Instance, ServiceError> {
        let config = crate::instance::InstanceConfig::new(
            name,
            crate::instance::FabricLaunch::default(),
            runtime.clone(),
        );
        self.instances
            .create(id, config, &self.runtimes.launcher_path(&runtime), eula)
            .map_err(ServiceError::Store)
    }

    async fn runtime_for(&self, request: RuntimeRequest) -> Result<FabricRuntime, ServiceError> {
        match request {
            RuntimeRequest::Exact(runtime) => {
                self.cache_runtime(&runtime).await?;
                Ok(runtime)
            }
            RuntimeRequest::Latest => {
                let runtime = self.resolve_runtime(None).await?;
                self.cache_runtime(&runtime).await?;
                Ok(runtime)
            }
            RuntimeRequest::Minecraft(minecraft) => {
                let runtime = self.resolve_runtime(Some(minecraft.as_str())).await?;
                self.cache_runtime(&runtime).await?;
                Ok(runtime)
            }
        }
    }
}

#[derive(Debug)]
pub enum ServiceError {
    Runtime(RuntimeError),
    Store(StoreError),
}

impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Runtime(error) => error.fmt(formatter),
            Self::Store(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ServiceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Runtime(error) => Some(error),
            Self::Store(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CreateInstance, InstanceService, RuntimeRequest};
    use crate::instance::{EulaAcceptance, InstanceId, InstanceName};
    use crate::paths::DartPaths;
    use crate::runtime::{FabricClient, FabricRuntime, RuntimeStore};
    use crate::store::InstanceStore;
    use crate::test_support::TestDirectory;
    use std::fs;
    use std::str::FromStr;

    fn service(directory: &TestDirectory) -> (InstanceService, RuntimeStore) {
        let paths = DartPaths::new(directory.path().to_owned());
        let runtimes = RuntimeStore::new(paths.clone());
        let service = InstanceService::new(
            InstanceStore::new(paths),
            runtimes.clone(),
            FabricClient::new().unwrap(),
        );
        (service, runtimes)
    }

    #[test]
    fn creates_an_instance_from_a_cached_runtime() {
        let directory = TestDirectory::new("service");
        let (service, runtimes) = service(&directory);
        let runtime = FabricRuntime::new("1.21.8", "0.17.2", "1.1.2").unwrap();
        runtimes
            .install_bytes(&runtime, b"PK\x03\x04launcher")
            .unwrap();

        let instance = service
            .create_with_cached_runtime(
                InstanceId::from_str("survival").unwrap(),
                InstanceName::parse("Survival").unwrap(),
                runtime.clone(),
                EulaAcceptance::Accepted,
            )
            .unwrap();

        assert_eq!(instance.config().fabric, runtime);
        assert_eq!(
            fs::read(instance.root().join("fabric-server-launch.jar")).unwrap(),
            b"PK\x03\x04launcher"
        );
        assert_eq!(
            fs::read_to_string(instance.root().join("eula.txt")).unwrap(),
            "eula=true\n"
        );
    }

    #[test]
    fn refuses_a_runtime_that_the_cache_does_not_contain() {
        let directory = TestDirectory::new("service");
        let (service, _) = service(&directory);
        let runtime = FabricRuntime::new("1.21.8", "0.17.2", "1.1.2").unwrap();

        let error = service
            .create_with_cached_runtime(
                InstanceId::from_str("survival").unwrap(),
                InstanceName::parse("Survival").unwrap(),
                runtime,
                EulaAcceptance::NotAccepted,
            )
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("cached Fabric launcher is missing")
        );
    }

    #[tokio::test]
    async fn exact_runtime_request_reuses_the_cache_without_network_access() {
        let directory = TestDirectory::new("service");
        let (service, runtimes) = service(&directory);
        let runtime = FabricRuntime::new("1.21.8", "0.17.2", "1.1.2").unwrap();
        runtimes
            .install_bytes(&runtime, b"PK\x03\x04launcher")
            .unwrap();

        let instance = service
            .create(CreateInstance::new(
                InstanceId::from_str("survival").unwrap(),
                InstanceName::parse("Survival").unwrap(),
                RuntimeRequest::Exact(runtime.clone()),
                EulaAcceptance::NotAccepted,
            ))
            .await
            .unwrap();

        assert_eq!(instance.config().fabric, runtime);
        assert!(!instance.root().join("eula.txt").exists());
    }
}
