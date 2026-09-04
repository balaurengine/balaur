# Changelog

One line per feature. Nothing has been released yet, so everything is under
Unreleased; a release is a `v*` tag whose notes become that version's section.

## Unreleased

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
- 2D lights and shadows: `light2d`, `occluder2d`, `camera.ambient`, and a light-map pass every sprite, polygon and tile is multiplied by.
- GPU skinning for 3D meshes; the CPU path stays for a node with its own `material` and as the test reference.
- `camera.post`: bloom, SSAO, SSR and depth of field, with `bloom_threshold` and `bloom_intensity`.
- Editor: `light2d` and `occluder2d` gizmos behind a Lights chip, and the Polygon tool traces an occluder's outline.
- 2D and 3D skeletal animation: `bone2d`, `bone3d`, the `skeleton` module, CPU skinning.
- `modifier2d`: `look_at` and `two_bone_ik`, run after the clip.
- `rotation` clip tracks take quaternion keys.
- `examples/rig` is one figure: the arm's bone chain branches off the body's top bone rather than hanging beside it from the character root, so one clip drives one skeleton.
- New 3D shapes (capsule, cylinder, cone, plane, mesh) and 2D shapes (capsule, polyline).

### Physics

- The rest of Rapier, in both dimensions: every rigid-body property rapier has — damping, gravity scale, axis locks, CCD, dominance, mass, per-body sleep, forces and torques.
- Joints: fixed, revolute, prismatic, spherical, rope, spring, pin-slot and generic, with motors, limits, `break_force`, and impulse or reduced-coordinate solvers.
- Inverse kinematics on a reduced-coordinate chain: `physics3d.solve_ik`.
- Character controllers: `character3d` / `character2d` and `move_character`, sliding, stepping, slope limits, ground snapping, and pushing what they walk into.
- The whole query pipeline: raycasts, shape casts, point, shape and box queries, pairwise distance and time of impact, filtered by body kind, layers, an excluded node or a script predicate.
- Collision and contact-force events reach the node's script as `on_collision_start`, `on_collision_stop` and `on_contact_force`; a broken joint as `on_joint_break`.
- The three physics hooks — `filter_contact`, `filter_overlap`, `modify_contacts` — and one-way platforms.
- New colliders: 3D capsule, cylinder, cone, triangle, segment, halfspace, trimesh, convex hull, convex decomposition, polyline, heightfield, voxels, voxelized and mesh-fitted; 2D gained all of them but the solids.
- Colliders carry offsets, 32 collision layers, solver layers, combine rules, contact skin and per-collider mass; one on a child node belongs to the body above it, which is how a body carries several shapes.
- Voxel terrain is editable while the game runs: `physics3d.set_voxel`, and `collider_mesh` tessellates any grid for drawing.
- A ray-cast vehicle: `vehicle3d` with `wheel3d` children, suspension tuning, engine force, brake and steering.
- `geometry3d`: convex hulls, convex decomposition, voxelising, cutting a mesh with a plane, boolean intersection, and separating a mesh into its pieces.
- Physics debug draw (`physics.set_debug_draw`), the solver's own tuning through `[physics]` in `project.toml` or `physics.set_tuning`, quarantine reporting and step counters.
- Two build-time choices: `parallel` runs rapier's coloured solver on rayon, and `f64` swaps in double precision for worlds tens of kilometres across.

### Determinism and networking

- One fixed 60 Hz step (`Stage::FixedUpdate`, `--fixed-tick`).
- Per-tick digest with `first_divergence`, `--trace-digest`, and a cross-OS CI check.
- Record and replay: `run --record`, `replay --verify` / `--entries-at`.
- Rollback on one machine: snapshot ring, input journal, re-simulation from a late input.
- Rollback across spawns: run-time nodes get stable ids and the node set is restored.
- `project.toml` puts what the game is under `[application]`: `name`, `main_scene`, `language` and `assets`. `[plugins]` stays its own table. The file now reads the way the settings screen addresses it, `application/name` being `[application] name`; a manifest with those keys at the top level no longer loads.
- A settings screen. Every setting the engine, its plugins and a game declare is addressed by path the way Godot does it — `physics/solver_iterations`, `editor/appearance/theme` — and the path is also where it is stored in the file. Project settings ship with the game; editor settings never leave the machine. Searchable, grouped by path, and a game defines its own with `settings.define`. The Netcode page turns fault injection on for every session link.
- Search in the scene tree, the inspector and the assets dock, through one shared field and one matching rule. `ui.modal` gained a `height`.
- A networked session is recordable and replayable: peer payloads travel through a `session` replay source, so a desync reproduces from a file with no peer attached. `transport::Faulty` adds delay, jitter and datagram loss to any transport, and `NetSession::stats` reports round-trip time, loss and bytes each way.
- Inputs now repeat: every datagram carries the last twelve ticks of a player's input, so a dropped packet no longer diverges the tick it carried. Found by testing against 5% loss.
- The networking crate is now one crate per protocol: `balaur_http`, `balaur_websocket` and `balaur_webtransport`, each its own plugin and its own cargo feature. `[net]` in project.toml becomes `[http]` and `[websocket]`, with the keys dropping the prefix their table now carries.
- WebTransport over QUIC behind the same `Transport` trait, so a datagram is finally a real unreliable datagram. A server generates a short-lived self-signed certificate by default and the client pins it by hash. Off by default: it is the expensive dependency, and its own switch.
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
- Containers assign rects: `grow` divides a container's leftover along its own direction, `min_width`/`min_height` are the author's floor, and a child that overflows no longer moves its siblings.
- `kind = "draw"`: a rect the scene places and a script fills, named `method` on the node's own script or `scripts/file.rn:function`.
- `kind = "scroll"`: a box that holds its size and clips what runs past it; `kind = "tab"`: one child showing, `active` naming it, written back when a tab is clicked.
- `handle` on a row or column: draggable seams that write the new size onto whichever neighbour states one.
- Widgets are measured before they are placed, from the font atlas rather than from last frame, so a container divides its room by what its children need now.
- Widget presets: `label`, `button`, `panel`, `row`, `column`, `scroll`, `tab` and `draw`, so the picker offers "Column" beside "Sprite2D".
- `node.patch_component`: change one property and leave the rest, the verb `set_component` is not; `patch` on a component handle too.
- `exports()` takes a spec table beside a bare default — `type`, `min`, `max`, `options`, `help`, `order` — so an exported property gets the inspector row its type deserves.
- `hot_reload` is called on every instance of a reloaded script; it was documented and never dispatched.
- `events`: `subscribe`, `emit`, `unsubscribe` and `emitted`, delivered at the top of the next frame's update.
- `node.descendants`, `node.stable_id`, `node.has_method`, `scene.with_component` and `scene.node_by_id`.
- `toml.patch`: write a table into a document, keeping its comments, key order and unmodelled tables.
- Widget `wrap`, `text_align` and an `image` kind.
- Keyboard focus is opt in: `ui.set_keyboard_focus`, and on for a project declaring the `ui_*` actions.
- `engine.tick()` answers a whole number, so `tick() % 60` is legal Rune.
- Widget surfaces: a root's `layer` names where it draws, `ui.set_widget_surface` places one, and an unconfigured name is the whole screen — so a tool's own chrome and a game's HUD no longer share one rect.
- `ui.widget_rect(node)`: where a widget node was last drawn, for a script placing something against it.
- A `draw` node with no script of its own asks the nearest scripted ancestor.
- Audio buses: `[audio.buses]`, routing per sound, `audio.set_bus_volume`.
- Audio events: `audio/events.toml`, variations taken in turn.
- Positional audio: a `listener` node, `positional` sounds with distance attenuation, stereo pan and doppler, and a `position` in `audio.play` and `audio.play_event`.
- `audio.set_volume` holds through a later bus change: the volume a script asked for is the one a slider recomputes from.
- Gamepad rumble: `input.gamepad_rumble` / `stop_rumble` / `can_rumble`, both motors.
- Pad motion and touchpad: `input.gamepad_gyro`, `gamepad_acceleration`, `gamepad_touches`, read from a PlayStation pad's HID reports over USB or Bluetooth, recorded and replayed like every other input.
- Loaded plugins are recorded: `balaur_core::plugins`, and a plugin's `requires` is checked at load.
- Optional modules are one line in `balaur`'s `modules!` table, loaded in requirement-then-name order.
- Every plugin loads from one ordered set, sorted by requirement then name.
- `[plugins]` in `project.toml` turns a module off; asking for one the build has not got is an error.
- `engine.plugins` and `engine.has_plugin` for scripts.

### Platform services

- `platform.*`: sign-in, achievements, leaderboards, cloud saves and presence over one module, whichever store is loaded. With none, every call answers `unsupported`.
- Apple: Game Center behind `platform.*`, iCloud key-value cloud saves, and `apple.identity` for the server that verifies a player.
- A store write made on a tick a rollback could still take back waits until that tick settles; reads go out at once.
- `[apple]` in `project.toml`: bundle id, team, version, deployment target, extra `Info.plist` keys, and the capabilities an export turns into entitlements.
- `balaur export` writes `Info.plist` and `<game>.entitlements` from that table, and hands the entitlements to `codesign` for a macOS `.app`.
- Game Center's own screens: the sign-in sheet is presented, `apple.show_dashboard` opens the dashboard, `apple.access_point` shows the badge.
- In-app purchases: `apple.products`, `purchase`, `entitlements`, `restore_purchases` and `finish_purchase`, over a StoreKit 2 shim in Swift. iOS 15 and macOS 12 are the floor.
- Local notifications, push tokens and opened URLs: `apple.request_notifications`, `notify`, `cancel_notification`, `register_for_push`, `watch_urls`, and `apple.watch` for the arrivals nobody asked for.
- `apple.sign_in` carries the player's name, and `apple.credential_state` says whether a saved account is still good.
- A sign-in handler stays subscribed: signing out in the OS reaches the same method.
- `platform.scores` takes `scope`, `period` and `start`.
- A Mac with no Swift toolchain still builds the engine; purchases say so instead of failing to link.

### Editor

- Undo/redo, collapsible inspector sections, node copy and paste, `--state dock:<name>`.
- Prefabs: instance rows in the tree, edits written as overrides, placed from the palette.
- Rig and Polygon tools; bones pick and grow in the 3D viewport.
- Ray picking, the Assets dock's filesystem verbs, in-editor linting with a language server.
- Profiler dock; `balaur run --timings` prints per-stage mean, worst and share of a frame.
- Showcase pipeline: `scripts/showcase.sh` retakes the website's images and clips.
- `scripts/uiaudit.sh` captures one screenshot per editor screen; `--state tab:<id>` picks a document.
- Fixed: the centre ran the split branch every frame, so a collapsed code pane sat over the viewport.
- Stage shell: the scene fills the window and every panel is a sheet over it, sized by `layout.rn`.
- The bottom dock collapses to its tab row; plugin windows use the editor's own chrome.
- Left, bottom and right are tabbed docks: a tab's menu moves it to another, and each minimises to its edge.
- The brand is the edited project's `icon.png` and name when it ships one.
- `ui::pill`, `ui::text_field` and `ui::dropdown` are tiles unless asked for `round`.
- The editor's shell is `widget` nodes the engine lays out: `Editor/Shell` in `scenes/main.toml` holds the boxes, `layout.rn` states only the sizes that move and reads the rects back.
- The editor's look is a theme asset: `editor/themes/*.toml` holds colour tokens and named roles a widget takes with `role:`.
- Bundled type: Source Sans 3 at two weights, JetBrains Mono and Phosphor icons, with the OS chained for scripts balaur does not ship.
- Fixed: a sheet's content spilled over its neighbours; `ui::overlay` clips to the rect it was given.
- The status strip moved into the bottom dock's foot; sheets separate by tone rather than an outline.
- Viewport chips recede until the pointer comes for them.
- Asset cards carry their own name and a typed mark; the timeline has a ruler, a playhead and keys that can be clicked.
- `ui::cursor_y` reports where the next widget lands, so a rule can be drawn through rows not yet laid out.
- The persona bar centres on the document column; the inspector reaches the bottom gutter; `ui::toggle` takes its size from the theme.
- Vector fields are inputs, not pills; node marks come from the icon set and lost their disc.
- Node marks are coloured by kind — 2D, 3D, physics, bone, interface — from tokens in the theme.
- The tree is set in the mono face; every dock minimises with the same glyph; the tree and inspector reach the dock's bottom edge.
- Fixed: sheets stacked in the top-left corner on a small window — egui was dragging an overflowing area back on screen.
- A narrow window sheds status columns and shrinks the inspector's label column instead of overflowing; `--state scale:<f>` captures it.
- The tree draws real depth guides with an elbow at the last child; the tool rail collapses like the docks; the transport lost its nested frame.
- One gap between every sheet and one corner radius across the shell; the bars are no longer capsules.
- The persona bar and the document tabs are one object: same height, same tab, same corner, centred on the same axis.

### Breaking

- `shape`/`body`/`collider` are now `shape3d`/`body3d`/`collider3d`; script APIs moved to `physics3d`.
- `color` is a property of `shape3d`, `shape2d`, `sprite` and `particles`, not a component.
- Luau is removed: `.luau`, `language = "luau"`/`"mixed"` and `balaur_script_luau` are gone.
- `animation.is_running` is `animation.is_tween_running`.
- `ui.select` is `ui.dropdown`.
- `load_order` and `load_extensions_in` take the names already loaded; `App::plugins` returns `PluginInfo`.
- One plugin trait: `balaur_core::Plugin` and `App::add_plugin` are gone, `balaur_plugin::Plugin` is it.
- Plugins register through `Registry` alone: presets, asset types, digests, snapshots, replay setup, `engine()`.
- `Registry::app()` is gone; `Registry::config()` hands a plugin its `[plugins]` table.
- `engine.plugin_version` for scripts.
