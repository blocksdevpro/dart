# Dart TUI design study

Dart needs to remain understandable before a user learns its shortcuts. These
three sketches test different interaction models for the same v1 features.

## A. Table and modal

```text
┌ Dart ─────────────────────────────────────────────────────────────┐
│ Instances                         │ Selected instance             │
│ > survival  ● running             │ Fabric 1.21.8 · 0.17.2        │
│   creative  ○ stopped             │ Memory 1–4 GiB                │
│                                   │ [Start] [Console] [Stop]       │
├───────────────────────────────────┴───────────────────────────────┤
│ n New   s Start   x Stop   Enter Console   ? Help                 │
└───────────────────────────────────────────────────────────────────┘
```

This is easy to build, but the details pane becomes a dumping ground as Dart
gains mods, packs, and settings.

## B. Command palette

```text
┌ Dart / survival ──────────────────────────────────────────────────┐
│ Status: running · 4 players · Fabric 1.21.8                       │
│ Recent console output                                             │
│ ...                                                               │
│                  ┌ Run an action ─────────────────┐               │
│                  │ > stop                         │               │
│                  │   open console                 │               │
│                  │   manage mods                  │               │
│                  └────────────────────────────────┘               │
└───────────────────────────────────────────────────────────────────┘
```

This scales to many actions and is fast for experts. It hides discovery behind
one interaction and makes instance-to-instance comparison awkward.

## C. Navigator and inspector — selected

```text
┌ DART ─ Instances 2 ─ Running 1 ───────────────────────────────────┐
│ INSTANCES                         │ OVERVIEW                       │
│ > ● survival                      │ Survival                       │
│   ○ creative                      │ ● RUNNING · pid 4212           │
│                                   │ Fabric 1.21.8                   │
│                                   │ Loader 0.17.2                   │
│                                   │ Memory 1–4 GiB                  │
│                                   │                                │
│                                   │ Recent activity                 │
│                                   │ Server ready                    │
├───────────────────────────────────┴────────────────────────────────┤
│ ✓ Ready                                                           │
│ ↑↓ Select  n New  s Start  x Stop  Enter Console  ? Help  q Quit  │
└────────────────────────────────────────────────────────────────────┘
```

The navigator keeps selection stable while the inspector changes with it. The
focused row, status marker, transient notice, and contextual bottom bar give
immediate feedback. Creation is a short wizard rather than a large form:

```text
Identity → Runtime → Review and required EULA acceptance
             ├─ reuse an installed launcher
             ├─ download latest stable
             └─ download a chosen Minecraft version
```

This layout borrows the useful structure—not the visual branding—of established
TUIs: persistent panels and a contextual bottom line, visible focus, a help
overlay, and background work represented by a spinner. It also leaves a clear
place for later Mods and Packs tabs without putting those unfinished features
in v1.
