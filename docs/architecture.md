# Dart architecture

Dart manages local Fabric servers. An instance is a self-contained Minecraft
working directory. A runtime is a reusable Fabric launcher in Dart's cache.

## Release boundary

The first release supports Fabric only. It creates instances, lists them,
downloads and reuses Fabric launchers, starts and stops servers, reports live
state, and provides console access.

It does not manage mods, data packs, resource packs, Minecraft server
configuration, or servers after Dart exits. Those are separate product areas.
Do not add one as a small exception to an existing module.

## Data ownership

`DartPaths` defines every directory Dart owns:

```text
$DART_HOME/
├─ runtimes/fabric/
│  └─ <minecraft>/<loader>/<installer>/fabric-server-launch.jar
└─ instances/
   └─ <instance-id>/
      ├─ dart.toml
      ├─ fabric-server-launch.jar
      └─ eula.txt
```

Creation writes `dart.toml` and an instance-local launcher. It writes
`eula.txt` only when the user explicitly accepts the Minecraft EULA. Minecraft
creates the remaining server files, such as `server.properties` and `world/`,
when it starts.

`InstanceId` accepts one to 64 lowercase letters, digits, and hyphens. Its
first and last characters must be a letter or digit. `FabricVersion` rejects
path separators and traversal characters before Dart uses it in a cache path.

`dart.toml` uses format version `2`:

```toml
format_version = 2
name = "Survival"
java = "java"
min_memory_mib = 1024
max_memory_mib = 4096

[fabric]
minecraft = "1.21.8"
loader = "0.17.2"
installer = "1.1.2"
```

The Java command cannot be empty. Both memory values must be greater than zero,
and the minimum cannot exceed the maximum. Dart validates the file when it
loads an instance.

## Module ownership

```text
main
  -> cli                 argv parsing and text output
  -> ui                  terminal lifecycle and event loop
       -> ui/model        in-memory terminal state
       -> ui/input        keyboard actions and creation workflow
       -> ui/render       read-only Ratatui rendering
       -> service        shared instance workflow
            -> store    instance TOML and transactional filesystem work
            -> runtime  Fabric Meta HTTP and runtime cache
  -> supervisor          Java child processes, stdin, stdout, and stderr

paths                    the authoritative $DART_HOME layout
instance                 validated persistent instance types
```

`cli` and `ui` are thin front ends. Both call `InstanceService` for the
workflow that connects a runtime to an instance. They do not create an
`InstanceConfig`, calculate a launcher path, or copy a launcher.

Within `ui`, state flows in one direction: `input` changes `UiState`, then
`render` displays it. `render` has no filesystem, network, or process calls;
the parent module only owns terminal setup, cleanup, and event scheduling.

`InstanceStore` owns `instances/<id>/`. It parses and writes `dart.toml`, then
creates new instances in a temporary directory before it renames the completed
directory into place. A repeated request with the same configuration repairs a
missing launcher copy. Dart refuses to adopt an unmanaged directory or replace
an instance with different configuration.

`RuntimeStore` owns `runtimes/fabric/`. `FabricClient` owns Fabric Meta HTTP.
It validates remote version metadata and launcher bytes before it writes the
versioned cache file.

`ServerSupervisor` is an actor. It is the only module that owns Java child
handles and standard streams. Front ends send it commands and receive events.
The supervisor starts `fabric-server-launch.jar` from the selected instance
directory with structured process arguments. It never builds a shell command.

## Instance creation

The service and stores follow this sequence:

1. Reuse a cached runtime, resolve a compatible runtime, or request exact
   runtime coordinates.
2. Download a missing launcher to a temporary cache file, verify the JAR
   marker, sync it, and rename it into the runtime cache.
3. Create a temporary instance directory.
4. Write `dart.toml` and copy the cached launcher into that directory.
5. Write `eula=true` only after explicit user acceptance.
6. Rename the completed directory to `instances/<id>/`.

The cache is an input to creation. The instance copy is the executable. That
separation makes the creation operation safe to retry and prevents cache
changes from changing an existing server.

## Process state

`InstanceConfig` and `FabricRuntime` are persistent. `InstanceState`, console
lines, process IDs, and child handles are in memory only.

When Dart exits through `q` or Ctrl-C, it sends Minecraft's `stop` command to
running instances. A second exit request asks the supervisor to terminate
processes that do not stop. A crash or a later restart of Dart does not restore
process ownership.

## Boundaries and tests

Validate raw input at the boundary that receives it:

- `cli` validates arguments.
- `ui` validates text before it creates typed values.
- `store` validates TOML and filesystem names.
- `runtime` validates Fabric Meta metadata and downloaded bytes.

Internal modules receive typed values and do not repeat those checks. Tests use
shared temporary-directory fixtures. The supervisor test uses a local fake Java
script. The smoke test builds the binary and proves the cache-to-instance copy
path without network access.
