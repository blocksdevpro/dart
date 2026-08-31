# Contribute to Dart

Read the module that owns the behavior before changing it. Dart is small on
purpose, so a feature that needs many modules usually has the wrong boundary.

## Find the owner

- Start in `src/cli.rs` for command-line parsing and command output.
- Start in `src/ui/mod.rs` for terminal setup and event-loop behavior.
- Start in `src/ui/input.rs` for keyboard behavior or the create-instance
  wizard workflow.
- Start in `src/ui/model.rs` for in-memory terminal state and server-event
  projection.
- Start in `src/ui/render.rs` for visual layout, copy, colors, or widgets.
- Start in `src/service.rs` for a workflow used by both the CLI and TUI.
- Start in `src/instance.rs` for validated persistent types.
- Start in `src/store.rs` for instance files and `dart.toml`.
- Start in `src/runtime.rs` for Fabric Meta requests or the runtime cache.
- Start in `src/supervisor.rs` for Java process lifecycle or console I/O.
- Start in `src/paths.rs` when a data-layout path changes.

Do not put filesystem, HTTP, or process calls in `ui/render.rs`. Do not let
`cli.rs` or the UI duplicate instance-creation policy. Add shared lifecycle
work to `InstanceService`.

## Preserve the rules

- Store instances at `$DART_HOME/instances/<id>/` with `dart.toml` and an
  instance-local `fabric-server-launch.jar`.
- Store cached launchers under
  `$DART_HOME/runtimes/fabric/<minecraft>/<loader>/<installer>/`.
- Start the instance launcher, never the cached launcher.
- Keep persistent configuration separate from process state.
- Let `ServerSupervisor` own Java child handles and streams.
- Validate command input, TOML, filesystem names, remote metadata, and
  downloaded files where they enter Dart.
- Never accept the Minecraft EULA without explicit user action.

## Add a feature

Start with the caller. Write the CLI or TUI action that a user needs, then
decide which module owns its rule. Keep each front end focused on input and
presentation. Add an application workflow only when both front ends need the
same policy.

Add a focused test beside the module that owns the rule. Use
`src/test_support.rs` for temporary directories. Tests must not require a
Minecraft installation or live network access. For process behavior, use a
fake Java executable as `supervisor.rs` does.

When you change a command, data layout, or ownership boundary, update
`README.md` and `docs/architecture.md` in the same change.

## Check your work

Run the full check before sending a change:

```sh
./scripts/check.sh
```

The script runs formatting, tests, Clippy with warnings denied, and the binary
smoke test. The smoke test creates a temporary Dart home, inserts a local JAR
fixture into the cache, then verifies that instance creation copies it.
