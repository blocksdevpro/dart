# Working on Dart

Dart is a lightweight Rust TUI for local Fabric Minecraft server instances.
Keep the first release focused on create, list, start, stop, status, Fabric
runtime downloads, and live console access.

## Architecture rules

- Store each instance at `$DART_HOME/instances/<id>/` with its own `dart.toml`
  and `fabric-server-launch.jar`.
- Store reusable launchers at
  `$DART_HOME/runtimes/fabric/<minecraft>/<loader>/<installer>/`.
- Copy a cached launcher into an instance during transactional creation. The
  supervisor must start the instance copy, not the cache file.
- Keep persisted instance configuration separate from in-memory process state.
- Let the supervisor exclusively own Java child handles and standard streams.
- Validate filesystem names, configuration, remote metadata, and downloads at
  their boundaries. Do not build shell command strings.
- Do not accept Minecraft's EULA on a user's behalf.

## Development commands

```sh
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
./scripts/smoke.sh
```

Tests must not depend on a Minecraft installation or live network access. Use a
fake Java process for supervisor behavior and a local JAR fixture for storage.

Update `README.md` and `docs/architecture.md` when the data layout, commands, or
module ownership changes.
