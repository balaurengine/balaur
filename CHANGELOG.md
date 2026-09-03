# Changelog

One line per feature. Versions are the workspace's; a release is a `v*` tag
whose notes are that version's section.

## Unreleased

## 0.2.0 — 2026-09-03

### Scripting

- Rune is the only scripting language; a deterministic `libm`-backed `math` module.
- Exported script properties: `pub fn exports()`, `script = { source, props }`, edited in the inspector.
- Script debugger: breakpoints, stepping, call frames with locals.
- Debug Adapter Protocol on `balaur run --debug <port>`; `--debug-wait` holds the boot.
- Component handles: `node.body2d.apply_impulse(x, y)`, plus `get`/`set`/`has`/`remove`.
- `script.attempt`, `mod` files from disk and packs, `balaur api`.
- `ScriptHost::call_in(path, function, args)` for project-level hooks.
- The script API documents itself: every module, function, component and asset type.

### Scenes and assets

- Prefabs: `instance` builds another scene as a node's children, with `overrides` per path.
- Prefab ids are prefixed by the instance's, so two instances never collide.
- Overrides patch rather than replace, and reach components, the transform and `script` props.
- Component tags and presets, project-defined in `presets.toml`.
- Binary assets in packs (format 2, sha256-verified); `assets` mode in `project.toml`.
- `mesh` (OBJ, `.glb`, `.gltf`, inline) and `heightfield` asset types.
- `scene.component_properties(name, params)`: what a component's `apply` would receive.
- Node paths accept `..`; `node:set_parent(other)` keeps the world pose.

### Rendering and animation

- Shaders in WESL, linked to WGSL at run time; `material` asset type; `sprite` takes a material.
- `sprite` component: textured 2D quads with sheets.
- `polygon` component: a textured 2D polygon, GPU-skinned to a rig.
- 2D and 3D skeletal animation: `bone2d`, `bone3d`, the `skeleton` module, CPU skinning.
- `modifier2d`: `look_at` and `two_bone_ik`, run after the clip.
- `rotation` clip tracks take quaternion keys.
- New 3D shapes (capsule, cylinder, cone, plane, mesh) and 2D shapes (capsule, polyline).

### Physics

- The rest of Rapier: joints of every kind, motors, limits, breakable, character controllers.
- New colliders: 3D capsule, cylinder, cone, triangle, trimesh, convex hull, polyline, heightfield; 2D capsule.

### Determinism and networking

- One fixed 60 Hz step (`Stage::FixedUpdate`, `--fixed-tick`).
- Per-tick digest with `first_divergence`, `--trace-digest`, and a cross-OS CI check.
- Record and replay: `run --record`, `replay --verify` / `--entries-at`.
- Rollback on one machine: snapshot ring, input journal, re-simulation from a late input.
- Rollback across spawns: run-time nodes get stable ids and the node set is restored.
- Two engines play in lockstep over a socket: `NetSession` over a `Transport` trait.
- Websockets carry binary frames and negotiate `permessage-deflate`.
- `App::add_replay_setup`: loaded state goes in the recording's header, restored before tick one.

### Batteries

- Input actions: `[input.actions]` binds a name to keys, mouse, pad buttons, axes and key pairs.
- Rebinding through `input.bind`, saved per user; a recording carries the bindings it used.
- Save games: `save.write` / `read` / `slots` / `remove`, atomic, versioned, with migrations.
- Localization: `strings/<locale>.toml`, `strings.tr` with interpolation and plural forms.
- Game UI containers: `row`, `column` and a `panel` that lays out, with `padding`, `gap`, `align`.
- Widget focus, keyboard and pad, with `focusable`, `on_focus` and the `ui.focus_*` verbs.
- `widget_theme` asset: fill, stroke, radius and padding per widget kind, inherited.
- Audio buses: `[audio.buses]`, routing per sound, `audio.set_bus_volume`.
- Audio events: `audio/events.toml`, variations taken in turn.

### Editor

- Undo/redo, collapsible inspector sections, node copy and paste, `--state dock:<name>`.
- Prefabs: instance rows in the tree, edits written as overrides, placed from the palette.
- Rig and Polygon tools; bones pick and grow in the 3D viewport.
- Ray picking, the Assets dock's filesystem verbs, in-editor linting with a language server.
- Profiler dock; `balaur run --timings` prints per-stage mean, worst and share of a frame.
- Showcase pipeline: `scripts/showcase.sh` retakes the website's images and clips.

### Breaking

- `shape`/`body`/`collider` are now `shape3d`/`body3d`/`collider3d`; script APIs moved to `physics3d`.
- `color` is a property of `shape3d`, `shape2d`, `sprite` and `particles`, not a component.
- Luau is removed: `.luau`, `language = "luau"`/`"mixed"` and `balaur_script_luau` are gone.
- `animation.is_running` is `animation.is_tween_running`.
- `ui.select` is `ui.dropdown`.
