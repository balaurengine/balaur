# Changelog

One line per feature. Nothing has been released yet, so everything is under
Unreleased; a release is a `v*` tag whose notes become that version's section.

## Unreleased

### Scripting

- Rune is the only scripting language, with a deterministic `libm`-backed `math` module.
- Exported script properties: `pub fn exports()`, `script = { source, props }`.
- Script debugger: breakpoints, stepping, call frames with locals.
- Debug Adapter Protocol on `balaur run --debug <port>`; `--debug-wait`.
- Component handles: `node.body2d.apply_impulse(x, y)`, `get`/`set`/`has`/`remove`/`patch`.
- `script.attempt`, `mod` files from disk and packs, `balaur api`.
- `ScriptHost::call_in(path, function, args)`.
- `hot_reload` on every instance of a reloaded script.
- The script API documents itself: every module, function, component and asset type.

### Scenes and assets

- Prefabs: `instance` builds another scene as a node's children, with `overrides` per path.
- Prefab ids are prefixed by the instance's; overrides patch rather than replace.
- Component tags and presets, project-defined in `presets.toml`.
- Binary assets in packs (format 2, sha256-verified); `assets` mode in `project.toml`.
- `mesh` (OBJ, `.glb`, `.gltf`, inline) and `heightfield` asset types.
- `scene.component_properties`, `scene.with_component`, `scene.node_by_id`.
- `node.descendants`, `node.stable_id`, `node.has_method`, `node.patch_component`.
- Node paths accept `..`; `node:set_parent(other)` keeps the world pose.
- `toml.patch`: write a table into a document, keeping comments and key order.

### Rendering and animation

- Shaders in WESL, linked to WGSL at run time; `material` asset type.
- `sprite` (textured 2D quads with sheets) and `polygon` (GPU-skinned 2D) components.
- 2D lights and shadows: `light2d`, `occluder2d`, `camera.ambient`, a light-map pass.
- GPU skinning for 3D meshes.
- `camera.post`: bloom, SSAO, SSR and depth of field.
- 2D and 3D skeletal animation: `bone2d`, `bone3d`, the `skeleton` module, CPU skinning.
- `modifier2d`: `look_at` and `two_bone_ik`.
- `rotation` clip tracks take quaternion keys.
- New 3D shapes (capsule, cylinder, cone, plane, mesh) and 2D shapes (capsule, polyline).

### Physics

- The rest of Rapier in both dimensions: damping, gravity scale, axis locks, CCD, dominance, mass, per-body sleep, forces and torques.
- Joints: fixed, revolute, prismatic, spherical, rope, spring, pin-slot and generic, with motors, limits, `break_force`, and impulse or reduced-coordinate solvers.
- Inverse kinematics on a reduced-coordinate chain: `physics3d.solve_ik`.
- Character controllers: `character3d` / `character2d`, sliding, stepping, slope limits, ground snapping.
- The query pipeline: raycasts, shape casts, point, shape and box queries, distance and time of impact.
- Collision and contact-force events as `on_collision_start`, `on_collision_stop`, `on_contact_force`; a broken joint as `on_joint_break`.
- One-way platforms.
- The solver threads on rayon, sized to the machine; `physics.set_threads` and `physics.threads`.
- New colliders: 3D capsule, cylinder, cone, triangle, segment, halfspace, trimesh, convex hull, convex decomposition, polyline, heightfield, voxels, voxelized and mesh-fitted; 2D gained all but the solids.
- Collider offsets, 32 collision layers, solver layers, combine rules, contact skin and per-collider mass.
- Editable voxel terrain: `physics3d.set_voxel`, `collider_mesh`.
- A ray-cast vehicle: `vehicle3d` with `wheel3d` children.
- `geometry3d`: convex hulls, convex decomposition, voxelising, plane cuts, boolean intersection, mesh splitting.
- Physics debug draw, `[physics]` tuning, quarantine reporting and step counters.

### Determinism and networking

- One fixed 60 Hz step (`Stage::FixedUpdate`, `--fixed-tick`).
- Per-tick digest with `first_divergence`, `--trace-digest`, and a cross-OS CI check.
- Record and replay: `run --record`, `replay --verify` / `--entries-at`.
- Rollback on one machine: snapshot ring, input journal, re-simulation from a late input.
- Rollback across spawns: run-time nodes get stable ids and the node set is restored.
- `[application]` in `project.toml`: `name`, `main_scene`, `language`, `assets`.
- A settings screen: every setting addressed by path, project settings shipped, editor settings local, `settings.define` for a game's own.
- Search across the scene tree, inspector and assets dock.
- A networked session is recordable and replayable through a `session` replay source.
- `transport::Faulty` adds delay, jitter and loss; `NetSession::stats` reports round-trip time, loss and bytes.
- Every datagram carries the last twelve ticks of a player's input.
- One crate per protocol: `balaur_http`, `balaur_websocket`, `balaur_webtransport`.
- WebTransport over QUIC behind the `Transport` trait, with a self-signed certificate pinned by hash.
- Two engines play in lockstep over a socket: `NetSession` over `Transport`.
- Websockets carry binary frames and negotiate `permessage-deflate`.
- `App::add_replay_setup`: loaded state in the recording's header.

### Batteries

- Input actions: `[input.actions]` binds a name to keys, mouse, pad buttons, axes and key pairs.
- Rebinding through `input.bind`, saved per user and carried in a recording.
- Save games: `save.write` / `read` / `slots` / `remove`, atomic, versioned, with migrations.
- Localization: `strings/<locale>.toml`, `strings.tr` with interpolation and plurals.
- UI containers: `row`, `column`, `panel`, with `padding`, `gap`, `align`, `grow`, `min_width`/`min_height`.
- Widget kinds: `draw`, `scroll`, `tab`, `image`, plus `wrap` and `text_align`.
- Widget focus for keyboard and pad: `focusable`, `on_focus`, `ui.focus_*`, `ui.set_keyboard_focus`.
- `widget_theme` asset: fill, stroke, radius and padding per widget kind, inherited.
- `handle` on a row or column: draggable seams.
- Widget presets: `label`, `button`, `panel`, `row`, `column`, `scroll`, `tab`, `draw`.
- Widget surfaces: a root's `layer`, `ui.set_widget_surface`, `ui.widget_rect`.
- `events`: `subscribe`, `emit`, `unsubscribe`, `emitted`.
- Audio buses (`[audio.buses]`, `audio.set_bus_volume`) and audio events (`audio/events.toml`).
- Positional audio: a `listener` node, distance attenuation, stereo pan and doppler.
- Gamepad rumble: `input.gamepad_rumble` / `stop_rumble` / `can_rumble`.
- Pad motion and touchpad: `input.gamepad_gyro`, `gamepad_acceleration`, `gamepad_touches`.
- Plugins: `requires` checked at load, `[plugins]` turns a module off, `engine.plugins` and `engine.has_plugin`.
- `export -o <dir>` refuses a directory that is not an earlier export of this game.
- `scripts/lint.sh` runs what CI runs; `.githooks/pre-push` runs it before a push.

### Platform services

- `platform.*`: sign-in, achievements, leaderboards, cloud saves and presence over one module.
- Apple: Game Center, iCloud key-value cloud saves, `apple.identity` for server verification.
- A store write on a tick a rollback could take back waits until that tick settles.
- `[apple]` in `project.toml`: bundle id, team, version, deployment target, `Info.plist` keys, capabilities.
- `balaur export` writes `Info.plist` and `<game>.entitlements`, and signs a macOS `.app`.
- Game Center screens: the sign-in sheet, `apple.show_dashboard`, `apple.access_point`.
- In-app purchases: `apple.products`, `purchase`, `entitlements`, `restore_purchases`, `finish_purchase`.
- Notifications and URLs: `apple.request_notifications`, `notify`, `cancel_notification`, `register_for_push`, `watch_urls`, `apple.watch`.
- `apple.sign_in` carries the player's name; `apple.credential_state`.
- `platform.scores` takes `scope`, `period` and `start`.

### Editor

- Undo/redo, collapsible inspector sections, node copy and paste, `--state dock:<name>`.
- Prefabs: instance rows in the tree, edits written as overrides, placed from the palette.
- Rig and Polygon tools; bones pick and grow in the 3D viewport.
- Ray picking, the Assets dock's filesystem verbs, in-editor linting with a language server.
- Profiler dock; `balaur run --timings` prints per-stage mean, worst and share of a frame.
- Showcase pipeline: `scripts/showcase.sh`; `scripts/uiaudit.sh` captures one screenshot per screen.
- Stage shell: the scene fills the window, every panel is a sheet over it, sized by `layout.rn`.
- Left, bottom and right are tabbed docks, each minimising to its edge; the bottom collapses to its tab row.
- Docks resize by dragging the seam beside them, and animate open and closed.
- The editor's shell is `widget` nodes the engine lays out, themed by `editor/themes/*.toml`.
- Bundled type: Source Sans 3, JetBrains Mono and Phosphor icons.
- The brand is the edited project's `icon.png` and name when it ships one.
- A narrow window sheds status columns and shrinks the inspector's label column; `--state scale:<f>`.

### Breaking

- `shape`/`body`/`collider` are now `shape3d`/`body3d`/`collider3d`; script APIs moved to `physics3d`.
- `color` is a property of `shape3d`, `shape2d`, `sprite` and `particles`, not a component.
- Luau is removed: `.luau`, `language = "luau"`/`"mixed"` and `balaur_script_luau` are gone.
- The `filter_contact`, `filter_overlap` and `modify_contacts` script hooks and the `hooks` collider key are gone; use `layers`/`mask` and `solver_layers`/`solver_mask`.
- The `f64` physics feature is gone.
- `animation.is_running` is `animation.is_tween_running`.
- `ui.select` is `ui.dropdown`.
- `load_order` and `load_extensions_in` take the names already loaded; `App::plugins` returns `PluginInfo`.
- One plugin trait: `balaur_core::Plugin` and `App::add_plugin` are gone, `balaur_plugin::Plugin` is it.
- Plugins register through `Registry` alone; `Registry::app()` is gone, `Registry::config()` hands a plugin its `[plugins]` table.
- A manifest with `name`/`main_scene` at the top level no longer loads; they belong under `[application]`.
