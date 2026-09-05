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

### Web

- Audio in the browser through rodio's WebAudio host, opened on the first gesture; `audio.ready`.
- `gamend.*` in the browser: Fetch and the WebSocket API drive the same Phoenix protocol code as the native workers.
- `web.*`: `listen`, `messages`, `post_message`, `visible`, `user_agent`, `location`, `hardware_concurrency` — recorded, nil off the web.
- The user directory survives a reload: `fs`, `save` and `settings` mirror into `localStorage`.
- A hidden tab keeps ticking on a timer, so heartbeats and the fixed step continue.
- `balaur export --target web`: the template, the pack and a shell page a static host serves; a project's `web/index.html` overrides it.
- `save.slots`, `save.remove`, input rebindings and the sound cache go through the file backend, so a virtual filesystem serves them.

### Text

- Labels and captions are shaped through cosmic-text: bidi, Arabic joining, Thai and Devanagari reordering, CJK line breaks, fallback across the project's `fonts/` chain, drawn from an atlas the widget layer owns.
- `markup` on a widget: `[b]`, `[i]`, `[color=#hex]`, `[center]`, `[right]`, `[wave amp=N freq=N]`, `[img=path width=N]`.
- `font_weight` and `font_style` on a widget.
- The `field` widget kind: a line the player types into, with `placeholder`, `max_length`, `secret`, `numeric`, `on_change(text)` and `on_submit(text)`.
- `input.keyboard_height`: what the on-screen keyboard covers, from the page's visual viewport.

### Canvas

- `visible` and `z_index` on every node: a hidden node hides its subtree and physics is untouched; layers add to the parent's unless `z_relative = false`. Both in the digest and the rollback snapshot, and as `node.visible`, `set_visible`, `global_visible`, `z_index`, `set_z_index`, `global_z_index`.
- `region_origin` and `region_size` on `sprite`: an atlas cell in texture pixels, sizing the quad from the cell.
- `tilemap`: `material`, `cells` as rows of tile ids past the 36-character alphabet, and `set_cell` / `cell` on the handle.
- `shape2d` polylines are triangle strips with round joins and caps; `gradient` fades the colour along the chain and `texture` rides it.
- `particles`: `texture`, `color_end`, `size_end`, `one_shot` and `explosiveness`; particles are quads rather than points.
- `render.draw_circle_2d`, `draw_rect_2d`, `draw_arc_2d`, `draw_polyline_2d`, `draw_texture_2d`: world-space shapes for one frame, over the scene.

### Widgets and batteries

- Widget kinds `check`, `dropdown` (with `options`), `slider` (`value`, `min`, `max`, `step`), `progress`, `grid` (`columns`), `flow`, `fold` (`open`), `dialog` and `separator`; `on_change` carries the new state.
- Nine-patch pictures: `slice` on the `image` kind and `image` + `slice` on a `widget_theme` entry.
- `anchor = "fill"` with `inset` on a root widget; `ui.set_scale` goes down to 0.25; `deadzone` on `scroll` so a tap on a child lands under a finger.
- `ui.widget_rect` answers for roots as well as children; `balaur_ui::widget_rect` for hosts.
- `task::frames(n)` and `task::seconds(t)` wait on the fixed step, so they replay.
- Node `tags`: `node.tags`, `has_tag`, `add_tag`, `remove_tag`, `scene.tagged(name)`; a `tags` key in scene files; in the digest and the snapshot.
- Tweens: `delay`, `then = <handle>` (waits for another tween, then reads its start values), `on_tween_finished(handle)`, `animation.tween_value` / `tween_value_of` for a number a script owns.
- `geometry2d`: `triangulate`, `contains`, `segments_intersect`, `area`, `is_clockwise`, `convex_hull`, `union`, `intersection`, `difference` (fixed-point booleans, so every platform lands on the same vertices).
- `hash.sha256` / `sha256_text`, `encoding.base64` / `from_base64`, `rng.uuid` from the engine's own stream.
- `http.request` takes `save_to`: a 2xx body streams to a file under the user directory, `on_progress` hears each chunk, the reply carries `path`.
- Facts, recorded so a replay answers as the original did: `engine.platform`, `device_id`, `unix_time`, `focused`, `dark_mode`, `strings.system_locale`; `on_focus_changed(bool)` and `on_dark_mode(bool)` on every script when they change; `on_quit_requested` before the window closes.
- `render.safe_area`, `render.refresh_rate`, `render.set_keep_awake`; `input.vibrate`; the Android back button and the browser's back key arrive as `KEY_NAVIGATE_BACKWARD`.
- `[application] splash` and `splash_seconds`: a picture over the first seconds on every target.
- `balaur test`: every `tests/**/*.rn` on its own node in a headless copy of the project; a script error, `assert!` included, fails it.
- Schema types `vec4` and `strings`.

### Shaders

- `features = { screen = true }` on a `material` binds the last frame drawn as `screen_texture`; `sample_screen(screen_uv(in.clip_position))` reads it. One copy pass at the end of the frame, only while a material asks; a frame behind, since kiss3d draws all 2D objects in one pass.

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
