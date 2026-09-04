//! Fabric Meta HTTP adapter.

use super::{FabricRuntime, FabricVersion, META_BASE_URL, RuntimeError, RuntimeStore};
use serde::Deserialize;
use std::path::PathBuf;

/// HTTP client for interacting with the Fabric Meta API.
#[derive(Clone)]
pub struct FabricClient {
    client: reqwest::Client,
    base_url: reqwest::Url,
}

impl FabricClient {
    /// Creates a new Fabric client using the standard user-agent.
    pub fn new() -> Result<Self, RuntimeError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("dart/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(RuntimeError::Http)?;
        let base_url = reqwest::Url::parse(META_BASE_URL).map_err(|_| {
            RuntimeError::InvalidMetadata("invalid Fabric Meta base URL".to_owned())
        })?;
        Ok(Self { client, base_url })
    }

    /// Resolves the latest stable loader and installer versions for the specified
    /// Minecraft version (or newest stable Minecraft version if `None`).
    pub async fn resolve(&self, minecraft: Option<&str>) -> Result<FabricRuntime, RuntimeError> {
        let minecraft = match minecraft {
            Some(version) => FabricVersion::parse(version)?,
            None => {
                let games: Vec<VersionEntry> = self.get_json(&["v2", "versions", "game"]).await?;
                preferred_version(&games, "Minecraft")?
            }
        };
        let loaders: Vec<LoaderCompatibility> = self
            .get_json(&["v2", "versions", "loader", minecraft.as_str()])
            .await?;
        let loader_entries = loaders
            .into_iter()
            .map(|compatibility| compatibility.loader)
            .collect::<Vec<_>>();
        let loader = preferred_version(&loader_entries, "Fabric loader")?;
        let installers: Vec<VersionEntry> = self.get_json(&["v2", "versions", "installer"]).await?;
        let installer = preferred_version(&installers, "Fabric installer")?;
        Ok(FabricRuntime {
            minecraft,
            loader,
            installer,
        })
    }

    /// Lists stable Minecraft releases supported by Fabric, newest first.
    pub async fn minecraft_versions(&self) -> Result<Vec<FabricVersion>, RuntimeError> {
        let entries: Vec<VersionEntry> = self.get_json(&["v2", "versions", "game"]).await?;
        entries
            .into_iter()
            .filter(|entry| entry.stable)
            .map(|entry| FabricVersion::parse(entry.version))
            .collect()
    }

    /// Downloads the server launcher JAR for the given runtime into the store cache.
    pub async fn download(
        &self,
        runtime: &FabricRuntime,
        store: &RuntimeStore,
    ) -> Result<PathBuf, RuntimeError> {
        if store.is_installed(runtime) {
            return Ok(store.launcher_path(runtime));
        }
        let url = self.endpoint(&[
            "v2",
            "versions",
            "loader",
            runtime.minecraft.as_str(),
            runtime.loader.as_str(),
            runtime.installer.as_str(),
            "server",
            "jar",
        ])?;
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(RuntimeError::Http)?
            .error_for_status()
            .map_err(RuntimeError::Http)?;
        let bytes = response.bytes().await.map_err(RuntimeError::Http)?;
        store.install_bytes(runtime, &bytes)
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        segments: &[&str],
    ) -> Result<T, RuntimeError> {
        self.client
            .get(self.endpoint(segments)?)
            .send()
            .await
            .map_err(RuntimeError::Http)?
            .error_for_status()
            .map_err(RuntimeError::Http)?
            .json()
            .await
            .map_err(RuntimeError::Http)
    }

    fn endpoint(&self, segments: &[&str]) -> Result<reqwest::Url, RuntimeError> {
        let mut url = self.base_url.clone();
        let mut path = url.path_segments_mut().map_err(|_| {
            RuntimeError::InvalidMetadata("Fabric Meta base URL cannot contain paths".to_owned())
        })?;
        path.clear();
        path.extend(segments.iter().copied());
        drop(path);
        Ok(url)
    }
}

#[derive(Debug, Deserialize)]
struct VersionEntry {
    version: String,
    stable: bool,
}

#[derive(Debug, Deserialize)]
struct LoaderCompatibility {
    loader: VersionEntry,
}

fn preferred_version(
    versions: &[VersionEntry],
    component: &'static str,
) -> Result<FabricVersion, RuntimeError> {
    let entry = versions
        .iter()
        .find(|version| version.stable)
        .or_else(|| versions.first())
        .ok_or(RuntimeError::NoCompatibleVersion(component))?;
    FabricVersion::parse(&entry.version)
}
