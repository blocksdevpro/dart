# Dart

Dart is a terminal interface for local Fabric Minecraft servers. The first
release creates isolated instances, manages Fabric launchers, starts and stops
servers, shows their in-memory status, and provides a live console.

## Run Dart

Build and open the terminal interface:

```sh
cargo run
```

Dart stores its data in `DART_HOME` when that variable is set. Otherwise, it
uses `$XDG_DATA_HOME/dart` or `~/.local/share/dart` on Linux. To keep local
development data in this repository, run:

```sh
DART_HOME="$PWD/dart-data" cargo run
```

Press `n` to create an instance. The wizard lets you reuse a cached launcher,
download the newest stable Fabric runtime, or choose a Minecraft version.
Before Dart creates the instance, you must select the Minecraft EULA checkbox.
Dart never accepts the EULA without your explicit action.

## Use the terminal interface

- Use Up and Down, or `j` and `k`, to select an instance.
- Press `n` to create an instance.
- Press `s` to start the selected instance, or `x` to stop it.
- Press Enter to open its live console. Type a Minecraft command and press
  Enter to send it.
- Press `v` to inspect cached Fabric runtimes.
- Press `r` to reload instance files from disk.
- Press `q` to stop running instances and exit. Press `q` again to terminate a
  server that does not stop.

The status shown by Dart exists only while Dart runs. It does not record a PID
or claim that a server survived a restart of Dart.

## Use the command line

Place `--home` before the command when you want to use a specific data
directory.

Create an instance with the newest stable Fabric runtime:

```sh
cargo run -- --home "$PWD/dart-data" create survival "Survival Server" --accept-eula
```

Choose a Minecraft version and let Dart resolve compatible Fabric versions:

```sh
cargo run -- --home "$PWD/dart-data" create survival "Survival Server" \
  --minecraft 1.21.8 --accept-eula
```

Reuse a cached launcher, or download an exact Fabric runtime when it is
missing:

```sh
cargo run -- --home "$PWD/dart-data" create survival "Survival Server" \
  --runtime 1.21.8/0.17.2/1.1.2 --accept-eula
```

List instances and manage the runtime cache:

```sh
cargo run -- --home "$PWD/dart-data" list
cargo run -- --home "$PWD/dart-data" runtimes list
cargo run -- --home "$PWD/dart-data" runtimes download 1.21.8
```

Without `--accept-eula`, Dart creates the instance but does not write
`eula=true`. Minecraft will require you to accept its EULA before it starts.

## Start a server

The default `java` command must be on `PATH`. To use a different executable,
set `java` in the instance's `dart.toml` file. Minecraft 26.x requires Java
25. Dart mentions that requirement when it cannot find the configured Java
command.

The Fabric launcher downloads Minecraft and its libraries on the first server
start. Dart starts the launcher copied into the instance directory. It never
starts the reusable file in the runtime cache.

## Data layout

```text
$DART_HOME/
├─ runtimes/
│  └─ fabric/
│     └─ <minecraft>/<loader>/<installer>/
│        └─ fabric-server-launch.jar
└─ instances/
   └─ <instance-id>/
      ├─ dart.toml
      ├─ fabric-server-launch.jar
      └─ eula.txt                 # only after explicit acceptance
```

The cache seeds instance creation. Each instance owns its launcher copy, so a
cache change cannot change an existing server. See
[the architecture reference](docs/architecture.md) for the `dart.toml`
contract and module ownership.

## Verify a change

Run the full local check:

```sh
./scripts/check.sh
```

The test suite uses local JAR fixtures and a fake Java process. It does not
need Minecraft or network access.

Read [the contributor guide](docs/contributing.md) before adding a command or
changing a storage or process boundary.
