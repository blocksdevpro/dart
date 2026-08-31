# Instance content workspace

Dart manages three kinds of Modrinth content for one Fabric instance. Mods,
data packs, and resource packs share discovery controls, but Minecraft gives
each kind a different install contract.

## Usage

```text
content = content_manager.list(instance, kind)
results = content_manager.search(kind, query, minecraft)
plan = content_manager.prepare_install(kind, project, minecraft)
report = content_manager.apply_plan(instance, plan)
```

The TUI selects a `ContentKind` and does not decide paths, Modrinth loaders, or
server properties.

## Types

```rust
enum ContentKind {
    Mod,
    DataPack,
    ResourcePack,
}

enum ContentInstallPlan {
    Mod(ModInstallPlan),
    DataPack(PackInstallPlan),
    ResourcePack(PackInstallPlan),
}

enum InstalledContent {
    Mod(InstalledMod),
    DataPack(InstalledPack),
    ResourcePack(InstalledPack),
}
```

`ContentInstallPlan` prevents the TUI from applying a plan through the wrong
store. A data pack and a resource pack cannot share an install path.

## Interfaces and ownership

`ContentManager` is the TUI-facing coordinator. It dispatches operations to
`ModManager` or `PackManager` by `ContentKind`.

`ModManager` continues to own Fabric mod dependency resolution. `PackManager`
owns Modrinth pack compatibility, pack downloads, and pack persistence.
`PackStore` owns world-path discovery, checksums, manifests, and
`server.properties` updates.

Minecraft owns `mods/`, `<world>/datapacks/`, and `server.properties`. Dart
owns the matching manifests under `.dart/` and the verified resource-pack copy
under `.dart/resource-packs/`.

## Module map

```text
ui -> ContentManager -> ModManager  -> ModStore
                    -> PackManager -> PackStore
                    -> Modrinth HTTPS
```

The app holds typed display state. It does not hold HTTP responses or file
handles.

## Flow

### Data pack

1. Search Modrinth for the exact Minecraft version and the `datapack` type.
2. Select the newest listed version whose loader is `datapack`.
3. Download and verify the ZIP with SHA-512.
4. Read `level-name` from `server.properties`, or use `world` when the file
   does not exist.
5. Write the ZIP to `<world>/datapacks/` and update `.dart/datapacks.toml`.

### Resource pack

1. Search Modrinth for the exact Minecraft version and the `resourcepack`
   type.
2. Select the newest listed version whose loader is `minecraft`.
3. Download and verify the ZIP with SHA-512.
4. Save the verified copy under `.dart/resource-packs/`.
5. Set `resource-pack` and `resource-pack-sha1` in `server.properties` to the
   Modrinth CDN URL and hash. Minecraft clients download the pack when they
   join.

The dedicated-server property supports one configured resource pack. Installing
a different resource pack replaces Dart's previous managed selection.

## Alternatives

One option was to add separate Mods, Data packs, and Resource packs screens.
That duplicates search, progress, update, and removal behavior and makes the
dashboard carry three entry keys.

Another option was to download resource packs into a top-level
`resourcepacks/` directory. A dedicated server does not load that directory,
so the UI would report an install that has no effect for players.

The chosen design uses one Content workspace and a typed manager. Shared UI
behavior stays shared, while each content kind keeps its Minecraft-specific
storage rules.

## Risks

A custom `level-name` can point outside the instance. Dart accepts only a safe
single directory name before it writes a data pack.

Modrinth projects can publish both a Fabric mod and a data-pack ZIP. Dart
selects by the release loader, not only by the project's primary type.

Data-pack or resource-pack releases with required dependencies are rejected
until Dart can assign every dependency to a safe Minecraft install contract.
