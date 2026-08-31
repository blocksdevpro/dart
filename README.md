# Dart

Dart is a lightweight terminal interface for local Fabric Minecraft servers.
It manages isolated instances, downloads and reuses versioned Fabric launchers,
starts and stops servers, reports live state, provides an interactive console,
and installs compatible mods, data packs, and server resource packs from
Modrinth.

## Run Dart

Build and open the TUI:

```sh
cargo run
```

Dart reads `DART_HOME` first. On Linux, it otherwise uses
`$XDG_DATA_HOME/dart` or `~/.local/share/dart`. To keep development data in the
repository, run:

```sh
DART_HOME="$PWD/dart-data" cargo run
```

Press `n` to create an instance. The wizard can:

- reuse any Fabric runtime already cached by Dart;
- download the latest stable Minecraft/Fabric combination; or
- download Fabric for a Minecraft version that you enter.

The Review step has a required checkbox: `I have read and accept the Minecraft
EULA.` Press Space to select it. Dart does not create the instance until you
select the checkbox.

Dart uses Fabric's official Meta API. It chooses the newest stable compatible
loader and installer, stores that launcher by its exact versions, and copies it
into the new instance.

The wizard shows separate resolving and downloading states. Download failures
stay on screen with the full error instead of returning silently to the runtime
picker.

## Data layout

```text
$DART_HOME/
├─ runtimes/
│  └─ fabric/
│     └─ <minecraft>/<loader>/<installer>/
│        └─ fabric-server-launch.jar
└─ instances/
   └─ survival/
      ├─ dart.toml
      ├─ fabric-server-launch.jar
      ├─ server.properties
      ├─ mods/
      │  └─ fabric-api-….jar
      ├─ world/
      │  └─ datapacks/
      │     └─ homes-….zip
      └─ .dart/
         ├─ mods.toml
         ├─ datapacks.toml
         ├─ resource-pack.toml
         └─ resource-packs/
            └─ faithful-….zip
```

The runtime cache avoids repeated downloads. The copy in each instance is the
file that the server executes, so instances do not depend on cache files after
creation.

An instance configuration records the exact runtime:

```toml
format_version = 2
name = "Survival"
java = "java"
min_memory_mib = 1024
max_memory_mib = 4096

[fabric]
minecraft = "1.21.8"
loader = "0.19.3"
installer = "1.1.2"
```

## TUI controls

- Use the arrow keys or `j` and `k` to select an item.
- Press `n` to create an instance.
- Press `s` or `x` to start or stop the selected instance.
- Press Enter to open its live console.
- Press `m` to manage the selected instance's content.
- Press `v` to inspect cached Fabric runtimes.
- Press `?` for contextual help.
- Press `r` to reload instance files from disk.
- Press `q` to stop running instances and wait before exiting. Press it again
  to terminate a server that does not stop.

In the console, type a Minecraft command and press Enter. Press Escape to
return to the dashboard.

## Instance content

Press `m` on an instance to open its Content workspace. Use Left and Right, or
`h` and `l`, to switch among **Mods**, **Data packs**, and **Resource pack**.
Press Tab to switch between **Installed** and **Discover**. Search input keeps
all letter keys, including `h`, `j`, `k`, `l`, and `m`.

Type a query in **Discover**, then press Enter. Select a compatible result and
press `i` or Enter to install its newest compatible release. Dart verifies
every download against Modrinth's SHA-512 hash.

Mods use the `fabric` loader and go into `mods/`. Data packs use Modrinth's
`datapack` release and go into the active world's `datapacks/` directory. Dart
reads `level-name` from `server.properties`; it uses `world` before Minecraft
creates that file.

A dedicated server sends a resource pack to joining clients through the
`resource-pack` setting. Dart keeps a verified copy under
`.dart/resource-packs/`, then writes the Modrinth CDN URL and SHA-1 to
`server.properties`. Minecraft supports one configured server resource pack,
so installing another replaces the pack previously managed by Dart. Dart
refuses to replace a resource-pack URL that you configured outside Dart.

For Dart-managed content, press `u` to check for an update and `d` to request
removal. Dart asks for confirmation before deletion. JAR and ZIP files that you
add remain visible as `external`; Dart does not overwrite, update, or remove
them.

Dart records the Modrinth project ID, version ID, file name, and checksum in
`instances/<id>/.dart/mods.toml`. Before an install or update, Dart resolves the
complete tree of required Modrinth dependencies for the instance's exact
Minecraft version, Fabric, and a dedicated server. The Mods panel names those
dependencies, then Dart installs them before the selected mod. Exact dependency
versions are preserved, and every downloaded JAR is checksum-verified.

Optional dependencies and modpacks are not installed automatically. If a mod
requires an external file that Modrinth cannot identify, or two mods require
conflicting versions of one project, Dart stops and explains the conflict.
Pack releases with required dependencies also stop with an explanation until
Dart can determine the correct Minecraft install location for each dependency.

## Command line

Create with the latest stable supported Minecraft version:

```sh
cargo run -- --home "$PWD/dart-data" create survival "Survival Server" --accept-eula
```

Choose a Minecraft version while letting Dart resolve Fabric versions:

```sh
cargo run -- --home "$PWD/dart-data" create survival "Survival Server" \
  --minecraft 1.21.8 --accept-eula
```

Pin all three version coordinates or reuse that exact cached runtime:

```sh
cargo run -- --home "$PWD/dart-data" create survival "Survival Server" \
  --runtime 1.21.8/0.19.3/1.1.2 --accept-eula
```

Manage the cache directly:

```sh
cargo run -- --home "$PWD/dart-data" runtimes list
cargo run -- --home "$PWD/dart-data" runtimes download 1.21.8
```

## First server start

The server host must have Java on its `PATH`. Minecraft 26.x requires Java 25.
On Ubuntu 24.04, install the headless runtime:

```sh
sudo apt update
sudo apt install openjdk-25-jre-headless
java -version
```

If Java is installed elsewhere, set `java` in the instance's `dart.toml` to
the executable path. Dart reports the configured command when Java is missing
or is not executable.

The Fabric launcher downloads the matching Minecraft server and libraries when
it first runs. When you select the required TUI checkbox, Dart writes
`eula=true` to the new instance. The CLI writes the same file only when you pass
`--accept-eula`.

Many Fabric mods require Fabric API. When Modrinth marks it as required, Dart
includes Fabric API in the dependency plan and installs the compatible release
automatically.

## Verify Dart

```sh
cargo test
cargo clippy --all-targets --all-features -- -D warnings
./scripts/smoke.sh
```

The tests use local fixtures and do not require Minecraft. The smoke test proves
that a cached runtime is reused and copied into a self-contained instance.

See [the architecture](docs/architecture.md) for module ownership and
[the UI design study](docs/ui-design.md) for the dashboard decision. See
[the Mods workspace decision](docs/mods-design.md) for Modrinth ownership and
dependency details. See [the Content workspace design](docs/content-design.md)
for data-pack and resource-pack storage rules.
