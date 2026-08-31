# Dart architecture

Dart manages local Fabric servers. An instance is a self-contained Minecraft
working directory; a runtime is a reusable, immutable Fabric launcher in
Dart's cache.

## Supported server type

The first release supports Fabric only. Fabric is a mod loader, and mods belong
in each instance's `mods/` directory. Paper is a different server platform
whose extensions are plugins. Dart does not mix those models.

Dart obtains version metadata and executable server launchers from the official
[Fabric Meta API](https://github.com/FabricMC/fabric-meta). Fabric's
[server download page](https://fabricmc.net/use/server/) documents the same
launcher endpoint used by Dart.

Dart searches and downloads mods through the official
[Modrinth API](https://docs.modrinth.com/api/). It sends a distinct user agent,
uses Modrinth's `mod`, `fabric`, dedicated-server environment, and exact
Minecraft-version filters. It checks the compatible version endpoint before it
downloads a JAR. Data-pack searches use the `datapack` project type and loader.
Resource-pack searches use the `resourcepack` project type and the `minecraft`
loader.

## Core model

```rust
struct FabricRuntime {
    minecraft: FabricVersion,
    loader: FabricVersion,
    installer: FabricVersion,
}

struct InstanceConfig {
    format_version: u32,
    name: InstanceName,
    launch: FabricLaunch,
    fabric: FabricRuntime,
}

struct Instance {
    id: InstanceId,
    root: PathBuf,
    config: InstanceConfig,
}

enum InstanceState {
    Stopped,
    Starting,
    Running { pid: u32 },
    Stopping,
    Failed { message: String },
}

struct ManagedMod {
    project_id: ModrinthProjectId,
    version_id: ModrinthVersionId,
    filename: ModFileName,
    sha512: String,
}

enum InstalledMod {
    Managed(ManagedMod),
    External { filename: ModFileName },
}

enum ContentKind {
    Mod,
    DataPack,
    ResourcePack,
}

enum InstalledContent {
    Mod(InstalledMod),
    DataPack(InstalledPack),
    ResourcePack(InstalledPack),
}

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
    releases: Vec<ModRelease>, // dependency-first order
}
```

`InstanceConfig`, `FabricRuntime`, and `ManagedMod` are persistent. Process
handles, state, and recent console lines exist only in memory. Dart does not
persist a PID as proof that a server survived a Dart restart.

## Runtime cache and instance ownership

```text
$DART_HOME/
├─ runtimes/fabric/
│  └─ <minecraft>/<loader>/<installer>/fabric-server-launch.jar
└─ instances/
   └─ <instance-id>/
      ├─ dart.toml
      ├─ fabric-server-launch.jar
      ├─ server.properties
      ├─ mods/
      │  └─ <mod>.jar
      ├─ <level-name>/
      │  └─ datapacks/
      │     └─ <data-pack>.zip
      └─ .dart/
         ├─ mods.toml
         ├─ datapacks.toml
         ├─ resource-pack.toml
         └─ resource-packs/
            └─ <resource-pack>.zip
```

The cache path is derived from validated version values. Version values cannot
contain path separators or traversal components.

Creation follows this sequence:

1. Choose an installed runtime or resolve compatible versions through Fabric
   Meta.
2. Download a missing launcher to a temporary file, verify the ZIP/JAR marker,
   sync it, and rename it into the versioned cache.
3. Create a temporary instance directory.
4. Write `dart.toml` and copy the cached launcher into that directory.
5. If the user explicitly accepted the EULA, write `eula=true`.
6. Rename the completed directory to its final instance ID.

Steps are idempotent. Repeating a completed runtime download returns the cached
path. Repeating creation with the same ID and configuration converges on the
existing instance and repairs a missing launcher copy. Dart refuses to adopt an
unmanaged directory or replace an instance with different settings.

The cache and the instance copy deliberately have different ownership. A
cached launcher can seed many instances, but starting an instance reads only
its own launcher. Cache changes therefore cannot silently change an existing
server.

`mods/` is Minecraft-owned data. Dart treats `.dart/mods.toml` as its own
record of Modrinth installs. The manifest keeps immutable project and version
IDs, plus the selected file name and SHA-512 hash. Dart lists JAR files that
are not in the manifest as external. It never overwrites, updates, or removes
an external JAR.

For an installation, `ModManager` resolves the requested mod and recursively
follows its required Modrinth dependencies. Project-only edges choose the
newest compatible version; version-pinned edges preserve the exact version.
Every release must support Fabric, the instance's exact Minecraft version, and
a dedicated server. The manager rejects cycles, conflicting version pins,
unresolvable external-file dependencies, and graphs over 64 projects before it
writes a JAR. Optional, incompatible, and embedded dependency edges are not
installed.

The completed plan orders dependencies before their dependents. For each
release, Dart downloads over HTTPS, checks the SHA-512 hash, writes the JAR,
then writes the manifest. Applying a plan is idempotent: an intact managed
release is skipped, so retrying after a partial download converges. An update
uses the same path and removes the replaced JAR only after the new manifest is
durable. Removal checks the managed JAR's checksum before it deletes anything.
A mismatch means another tool changed the file, and Dart refuses the removal.

Data packs follow the world selected by `level-name` in `server.properties`.
Dart uses `world` when the property does not exist. Dart accepts only a safe
single directory name, then writes verified ZIP files to
`<level-name>/datapacks/`. `.dart/datapacks.toml` records the files that Dart
owns. Other ZIP files remain external.

Resource packs are client content. A dedicated server distributes one through
the `resource-pack` URL and verifies it with `resource-pack-sha1`. Dart stores a
verified copy in `.dart/resource-packs/`, records it in
`.dart/resource-pack.toml`, and updates those two properties without changing
the other server properties. Dart refuses to replace an external resource-pack
configuration.

## Process ownership

The supervisor is the only component that owns child-process handles. The UI
sends commands and renders events.

```text
TUI ── actions ──> app ── commands ──> supervisor ──> Java/Fabric
 ^                  ^                       |
 └── renders state ─┴──── state/log events ─┘

app <──> instance store <──> instances/*/dart.toml
app <──> runtime store  <──> runtimes/fabric/*
             ^
             └── Fabric client <── HTTPS ── Fabric Meta API

app <──> content manager ──> mod manager  ──> mod store
              │          └─> pack manager ──> pack store
              │              │               instances/*/{mods,world,.dart}
              └──────────────┴─ HTTPS ─────> Modrinth API
```

The supervisor validates the instance launcher before it starts Java. It uses
structured process arguments, sets the instance root as the working directory,
and pipes standard input, output, and error. Stop writes Minecraft's `stop`
command. A second quit request terminates processes that do not stop.

If the operating system cannot find or execute the configured Java command,
the supervisor reports the command and the `dart.toml` path. For Minecraft
26.x, the error also states the Java 25 requirement.

## UI flow

The dashboard keeps instances in a persistent navigator and renders the
selected instance in an inspector. Status markers, a transient notice line,
contextual key hints, focused borders, editable cursors, and spinners provide
feedback without separate confirmation screens.

The Content workspace is tied to the selected instance. It switches among
mods, data packs, and the server resource pack. Its Installed tab separates
Dart-managed content from external files or settings. Its Discover tab searches
only for Modrinth projects compatible with the selected content kind and exact
Minecraft version. Network work runs in Tokio tasks, so search, downloads, and
updates never block rendering. During a mod installation, the workspace names
the required dependencies in the resolved plan. Removal has a confirmation
state before Dart starts the filesystem operation.

Creating an instance is a state machine. Review requires explicit EULA
acceptance:

```text
Identity ──> Runtime ───────────────> Review + EULA ──> Create
                │                       ^
                ├─ cached runtime ──────┘
                ├─ latest ─> Download ─┘
                └─ version > Download ─┘
```

Network work runs in a Tokio task and reports completion through a channel. The
render loop never waits on HTTP. It redraws immediately after keyboard input so
the resolving state reaches the terminal before a fast completion event. A
download failure remains visible until the user returns to the runtime picker.

## Rust modules

```text
src/
├─ main.rs        CLI parsing and dependency wiring
├─ app.rs         screen, wizard, selection, notice, and log state
├─ content.rs     typed content coordinator for the TUI
├─ instance.rs    validated instance and launch types
├─ mods.rs         Modrinth client, dependency plans, and mod ownership
├─ packs.rs        Modrinth packs, world paths, and server resource packs
├─ runtime.rs     Fabric Meta client and versioned runtime cache
├─ store.rs       instance discovery and transactional creation
├─ supervisor.rs  Fabric process and console ownership
└─ ui.rs          keyboard handling and Ratatui rendering
```

Boundary modules validate untrusted input: CLI arguments, `dart.toml`, mod
manifests, directory names, Fabric metadata, Modrinth metadata, and downloaded
bytes. Internal code operates on validated types.

## Current scope

Dart currently provides instance creation, Fabric launcher downloads, runtime
reuse, list, start, stop, status, live console access, and Modrinth installation,
required-mod-dependency resolution, data packs, one server resource pack,
update, and removal. It intentionally does not accept Minecraft's EULA on the
user's behalf, keep servers alive after Dart exits, install optional or pack
dependencies, import modpacks, or support non-Fabric loaders.
