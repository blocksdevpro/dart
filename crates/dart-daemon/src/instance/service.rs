//! Instance management workflows and lifecycle orchestration.

use super::{InstanceStore, StoreError};
use crate::instance::{EulaAcceptance, Instance, InstanceId, InstanceName};
use crate::runtime::{FabricClient, FabricRuntime, FabricVersion, RuntimeError, RuntimeStore};
use std::fmt;

/// A request specifying which Fabric runtime version to use when creating an instance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeRequest {
    /// Reuse an exact cached runtime or download that exact launcher.
    Exact(FabricRuntime),
    /// Resolve and cache the newest stable Fabric runtime.
    Latest,
    /// Resolve and cache the newest stable Fabric runtime for one Minecraft version.
    Minecraft(FabricVersion),
}

/// Parameters for creating a new server instance.
#[derive(Clone, Debug)]
pub struct CreateInstance {
    /// Unique identifier for the instance.
    pub id: InstanceId,
    /// Display name for the instance.
    pub name: InstanceName,
    /// Runtime version request.
    pub runtime: RuntimeRequest,
    /// EULA acceptance status.
    pub eula: EulaAcceptance,
}

impl CreateInstance {
    /// Creates a new instance creation request.
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

/// Service that coordinates instance configuration, runtime caching, and creation.
#[derive(Clone)]
pub struct InstanceService {
    instances: InstanceStore,
    runtimes: RuntimeStore,
    fabric: FabricClient,
}

impl InstanceService {
    /// Creates a new instance service.
    pub fn new(instances: InstanceStore, runtimes: RuntimeStore, fabric: FabricClient) -> Self {
        Self {
            instances,
            runtimes,
            fabric,
        }
    }

    /// Lists all managed instances on disk.
    pub fn list_instances(&self) -> Result<Vec<Instance>, ServiceError> {
        self.instances.list().map_err(ServiceError::Store)
    }

    /// Gets a specific instance by its ID.
    pub fn get_instance(&self, id: &InstanceId) -> Result<Instance, ServiceError> {
        self.instances.get(id).map_err(ServiceError::Store)
    }

    /// Lists all cached Fabric runtimes.
    pub fn list_runtimes(&self) -> Result<Vec<FabricRuntime>, ServiceError> {
        self.runtimes.list().map_err(ServiceError::Runtime)
    }

    /// Returns a reference to the underlying instance store.
    pub fn instance_store(&self) -> &InstanceStore {
        &self.instances
    }

    /// Returns a reference to the underlying runtime store.
    pub fn runtime_store(&self) -> &RuntimeStore {
        &self.runtimes
    }

    /// Returns a reference to the underlying Fabric API client.
    pub fn fabric_client(&self) -> &FabricClient {
        &self.fabric
    }

    /// Resolves Fabric version coordinates from upstream without downloading.
    pub async fn resolve_runtime(
        &self,
        minecraft: Option<&str>,
    ) -> Result<FabricRuntime, ServiceError> {
        self.fabric
            .resolve(minecraft)
            .await
            .map_err(ServiceError::Runtime)
    }

    /// Caches a launcher JAR for already validated Fabric coordinates.
    pub async fn cache_runtime(&self, runtime: &FabricRuntime) -> Result<(), ServiceError> {
        self.fabric
            .download(runtime, &self.runtimes)
            .await
            .map(|_| ())
            .map_err(ServiceError::Runtime)
    }

    /// Resolves/caches the requested runtime and creates the instance.
    pub async fn create(&self, request: CreateInstance) -> Result<Instance, ServiceError> {
        let runtime = self.runtime_for(request.runtime).await?;
        self.create_with_cached_runtime(request.id, request.name, runtime, request.eula)
    }

    /// Creates an instance using an already cached runtime launcher.
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

/// Errors originating from the instance service layer.
#[derive(Debug)]
pub enum ServiceError {
    /// An error occurred during runtime resolution or download.
    Runtime(RuntimeError),
    /// An error occurred in filesystem persistence.
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

impl From<RuntimeError> for ServiceError {
    fn from(error: RuntimeError) -> Self {
        Self::Runtime(error)
    }
}

impl From<StoreError> for ServiceError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

#[cfg(test)]
mod tests {
    use super::InstanceStore;
    use super::{CreateInstance, InstanceService, RuntimeRequest};
    use crate::instance::{EulaAcceptance, InstanceId, InstanceName};
    use crate::runtime::{FabricClient, FabricRuntime, RuntimeStore};
    use crate::storage::DartPaths;
    use crate::testing::TestDirectory;
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
