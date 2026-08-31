# Modrinth mods workspace

Dart needs a fast path from an instance to a compatible Fabric mod. The
interaction must keep the selected instance visible because Minecraft version
and loader compatibility are properties of that instance, not global settings.

## Interaction sketches

### A. Add mods in the dashboard inspector

```text
┌ INSTANCES ───────────────┬ OVERVIEW ──────────────────────────────┐
│ > survival               │ Minecraft 1.21.8 · Fabric             │
│                           │ Mods                                  │
│                           │ [Search Modrinth] [3 installed]       │
└──────────────────────────┴───────────────────────────────────────┘
```

This keeps the action close to the instance, but search results, installed
mods, and destructive actions would overcrowd the dashboard.

### B. Dedicated Mods workspace

```text
┌ MODS · survival · Minecraft 1.21.8 · Fabric ──────────────────────┐
│ [Installed]  Discover                                             │
│ ┌ Installed ─────────────────┐ ┌ Details ───────────────────────┐ │
│ │ > Fabric API 0.129.0       │ │ Managed by Dart                │ │
│ │   Lithium 0.18.0           │ │ Update available: check with u │ │
│ └────────────────────────────┘ └─────────────────────────────────┘ │
│ m dashboard  / search  u update  d remove  Esc back                │
└───────────────────────────────────────────────────────────────────┘
```

Discover replaces the left panel with a query and compatible Modrinth results.
The instance version stays in the title. Enter searches, `i` installs the
selected result, and Dart returns to the installed list after installation.

### C. Command palette action

```text
┌ Run an action ────────────────────────────────────────────────────┐
│ > Search compatible mods                                           │
│   List installed mods                                              │
│   Update installed mods                                            │
└───────────────────────────────────────────────────────────────────┘
```

This makes keyboard experts fast, but it hides the installed state and needs a
second surface for search results.

## Decision

Use the dedicated workspace. It gives Modrinth search and installed-mod
management enough room without making the dashboard a control panel. It also
makes the compatibility context impossible to miss.

The user selects one root Modrinth project at a time. Dart installs that project
and its transitive required dependencies. It filters both the search and every
resolved release by `fabric` and the exact Minecraft version in the instance's
`dart.toml`. It also excludes versions that do not support a dedicated server.
It prefers a release build, then a beta or alpha only when a project has no
release. Dart verifies every downloaded JAR with Modrinth's SHA-512 hash before
it reaches `mods/`.

## Local ownership

`mods/` remains a normal Minecraft directory. Dart writes its own install
record to `.dart/mods.toml` inside the instance:

```text
instances/<id>/
├─ dart.toml
├─ mods/
│  └─ fabric-api-…jar
└─ .dart/
   └─ mods.toml
```

The manifest records the immutable Modrinth project ID, version ID, selected
file name, and SHA-512 hash for every mod that Dart installs. It lets Dart
update or remove only files it owns. JAR files placed in `mods/` by the user or
another launcher remain visible as external files and Dart leaves them alone.

An install stages the verified JAR and writes the manifest atomically. An
interrupted operation can leave an untracked JAR, but it cannot leave a managed
manifest entry that claims a missing file. Retrying the install converges on a
single managed copy.

## Required dependency plan

Installing one mod also installs every required Modrinth dependency. The UI
uses this contract:

```text
plan = mod_manager.prepare_install(project, minecraft)
show(plan.dependency_titles())
report = mod_manager.apply_plan(instance, plan)
```

### Types

```rust
enum RequiredDependency {
    Project(ModrinthProjectId),
    Version {
        version_id: ModrinthVersionId,
        expected_project: Option<ModrinthProjectId>,
    },
    ExternalFile(String),
}

struct ModInstallPlan {
    root: ModrinthProjectId,
    releases: Vec<ModRelease>,
}
```

`releases` is ordered with dependencies before the mod that needs them. The
plan contains one release per project. A cycle, two required versions of the
same project, or a required external file produces an error before Dart starts
the install.

### Interfaces and ownership

`ModrinthClient` parses dependency records and resolves compatible releases.
It validates pinned versions against Fabric, the instance's exact Minecraft
version, and dedicated-server support. `ModManager` owns graph traversal,
cycle detection, the size limit, downloads, and application of the completed
plan. `ModStore` continues to own checksums, the manifest, and JAR files.

The TUI receives a typed plan. It shows the dependency names, then asks
`ModManager` to apply the plan in the background. The TUI does not traverse
dependency records or choose versions.

### Flow

1. Resolve the selected mod's newest compatible release.
2. Follow only `required` dependency edges. Ignore optional, incompatible, and
   embedded edges.
3. Use an exact dependency version when Modrinth supplies `version_id`.
   Otherwise, choose the newest compatible release for `project_id`.
4. Reject cycles, version conflicts, and dependencies that provide only an
   external filename.
5. Order required dependencies before their dependents.
6. Download and install each missing release. A retry skips intact files and
   converges after a partial failure.

### Alternatives

The first option was recursive dependency handling in `ui.rs`. It was rejected
because the TUI would own API rules, graph state, and installation order.

The second option was a single opaque `install(project)` call. It keeps the UI
small, but the UI cannot tell the user which dependencies Dart is installing.

The selected design exposes an immutable plan between resolution and install.
The plan gives the UI enough information for progress without exposing raw API
responses or graph traversal.

### Risks

Modrinth can describe a required dependency with only `file_name`. Dart cannot
download that dependency safely through Modrinth, so it stops and names the
unresolved file. Conflicting exact versions also stop the plan. Dart does not
guess which version might work.

Modpacks, non-Modrinth downloads, optional dependencies, and loader families
other than Fabric remain out of scope.
