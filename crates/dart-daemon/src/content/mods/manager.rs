//! Mod installation planning and dependency resolution.

use super::{
    InstalledMod, MAX_DEPENDENCY_PROJECTS, ManagedMod, ModError, ModInstallOutcome, ModInstallPlan,
    ModInstallReport, ModProject, ModRelease, ModStore, ModrinthClient, ModrinthProjectId,
    RequiredDependency,
};
use crate::instance::Instance;
use crate::runtime::FabricVersion;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Coordinates mod searching, dependency resolution, and atomic installation.
#[derive(Clone)]
pub struct ModManager {
    client: ModrinthClient,
    store: ModStore,
}

impl ModManager {
    /// Creates a new mod manager using the provided Modrinth client and local store.
    pub fn new(client: ModrinthClient, store: ModStore) -> Self {
        Self { client, store }
    }

    /// Searches Modrinth for Fabric mods compatible with the instance's Minecraft version.
    pub async fn search(
        &self,
        query: &str,
        minecraft: &FabricVersion,
    ) -> Result<Vec<super::ModSearchHit>, ModError> {
        self.client.search(query, minecraft).await
    }

    /// Lists all installed mods in the instance.
    pub fn list(&self, instance: &Instance) -> Result<Vec<InstalledMod>, ModError> {
        self.store.list(instance)
    }

    /// Removes a managed mod from the instance.
    pub fn remove(&self, instance: &Instance, modification: &ManagedMod) -> Result<(), ModError> {
        self.store.remove(instance, modification)
    }

    /// Resolves the full transitive dependency graph for a mod and generates an install plan.
    pub async fn prepare_install(
        &self,
        root_project: ModProject,
        minecraft: &FabricVersion,
    ) -> Result<ModInstallPlan, ModError> {
        let root_release = self
            .client
            .newest_compatible(root_project, minecraft)
            .await?;
        let root = root_release.project.id.clone();
        let mut releases = BTreeMap::from([(root.clone(), root_release.clone())]);
        let mut edges = BTreeMap::<ModrinthProjectId, BTreeSet<ModrinthProjectId>>::new();
        edges.entry(root.clone()).or_default();
        let mut pending = root_release
            .required_dependencies
            .iter()
            .cloned()
            .map(|dependency| (root.clone(), dependency))
            .collect::<VecDeque<_>>();

        while let Some((parent, dependency)) = pending.pop_front() {
            let parent_title = releases
                .get(&parent)
                .expect("dependency parents are always resolved")
                .project
                .title
                .clone();
            let release = match dependency {
                RequiredDependency::Project(project_id) => {
                    if releases.contains_key(&project_id) {
                        edges.entry(parent).or_default().insert(project_id);
                        continue;
                    }
                    let project = self.client.project(&project_id).await?;
                    self.client.newest_compatible(project, minecraft).await?
                }
                RequiredDependency::Version {
                    version_id,
                    expected_project,
                } => {
                    if let Some(expected_project) = expected_project.as_ref()
                        && let Some(existing) = releases.get(expected_project)
                    {
                        if existing.version_id != version_id {
                            return Err(ModError::DependencyVersionConflict {
                                project: existing.project.title.clone(),
                                first: existing.version_id.clone(),
                                second: version_id,
                            });
                        }
                        edges
                            .entry(parent)
                            .or_default()
                            .insert(expected_project.clone());
                        continue;
                    }
                    self.client
                        .exact_compatible(&version_id, expected_project.as_ref(), minecraft)
                        .await?
                }
                RequiredDependency::ExternalFile(filename) => {
                    return Err(ModError::UnresolvableRequiredDependency {
                        requested_by: parent_title,
                        dependency: filename,
                    });
                }
            };

            let dependency_id = release.project.id.clone();
            edges
                .entry(parent)
                .or_default()
                .insert(dependency_id.clone());
            if let Some(existing) = releases.get(&dependency_id) {
                if existing.version_id != release.version_id {
                    return Err(ModError::DependencyVersionConflict {
                        project: existing.project.title.clone(),
                        first: existing.version_id.clone(),
                        second: release.version_id,
                    });
                }
                continue;
            }
            if releases.len() >= MAX_DEPENDENCY_PROJECTS {
                return Err(ModError::DependencyLimitExceeded {
                    limit: MAX_DEPENDENCY_PROJECTS,
                });
            }
            edges.entry(dependency_id.clone()).or_default();
            pending.extend(
                release
                    .required_dependencies
                    .iter()
                    .cloned()
                    .map(|dependency| (dependency_id.clone(), dependency)),
            );
            releases.insert(dependency_id, release);
        }

        Ok(ModInstallPlan {
            root: root.clone(),
            releases: order_releases(&root, &releases, &edges)?,
        })
    }

    /// Resolves a project ID and prepares its newest compatible install plan.
    pub async fn prepare_install_project(
        &self,
        project_id: &ModrinthProjectId,
        minecraft: &FabricVersion,
    ) -> Result<ModInstallPlan, ModError> {
        let project = self.client.project(project_id).await?;
        self.prepare_install(project, minecraft).await
    }

    /// Downloads and installs all releases in the install plan in topological order.
    pub async fn apply_plan(
        &self,
        instance: &Instance,
        plan: &ModInstallPlan,
    ) -> Result<ModInstallReport, ModError> {
        let mut root_outcome = None;
        let mut changed_dependencies = 0;
        for release in &plan.releases {
            let outcome = if self.store.release_is_intact(instance, release)? {
                ModInstallOutcome::AlreadyInstalled
            } else {
                let bytes = self.client.download(release).await?;
                self.store.install(instance, release, &bytes)?
            };
            if release.project.id == plan.root {
                root_outcome = Some(outcome);
            } else if outcome != ModInstallOutcome::AlreadyInstalled {
                changed_dependencies += 1;
            }
        }

        Ok(ModInstallReport {
            root: plan.root().clone(),
            root_outcome: root_outcome.expect("a mod install plan always applies its root"),
            dependency_titles: plan
                .dependency_titles()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            changed_dependencies,
        })
    }
}

pub(crate) fn order_releases(
    root: &ModrinthProjectId,
    releases: &BTreeMap<ModrinthProjectId, ModRelease>,
    edges: &BTreeMap<ModrinthProjectId, BTreeSet<ModrinthProjectId>>,
) -> Result<Vec<ModRelease>, ModError> {
    fn visit(
        project: &ModrinthProjectId,
        releases: &BTreeMap<ModrinthProjectId, ModRelease>,
        edges: &BTreeMap<ModrinthProjectId, BTreeSet<ModrinthProjectId>>,
        visiting: &mut BTreeSet<ModrinthProjectId>,
        visited: &mut BTreeSet<ModrinthProjectId>,
        ordered: &mut Vec<ModRelease>,
    ) -> Result<(), ModError> {
        if visited.contains(project) {
            return Ok(());
        }
        if !visiting.insert(project.clone()) {
            let title = releases.get(project).map_or_else(
                || project.to_string(),
                |release| release.project.title.clone(),
            );
            return Err(ModError::DependencyCycle { project: title });
        }
        if let Some(dependencies) = edges.get(project) {
            for dependency in dependencies {
                visit(dependency, releases, edges, visiting, visited, ordered)?;
            }
        }
        visiting.remove(project);
        visited.insert(project.clone());
        ordered.push(
            releases
                .get(project)
                .expect("dependency graph edges reference resolved projects")
                .clone(),
        );
        Ok(())
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut ordered = Vec::with_capacity(releases.len());
    visit(
        root,
        releases,
        edges,
        &mut visiting,
        &mut visited,
        &mut ordered,
    )?;
    Ok(ordered)
}
