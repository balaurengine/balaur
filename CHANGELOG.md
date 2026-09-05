# Changelog

High-level features, one line each. Nothing has been released yet, so everything
is under Unreleased; a release is a `v*` tag whose notes become that version's section.

## Unreleased

### Scripting

- Rune scripting with a deterministic `math` module.
- Exported script properties.
- Debugger with breakpoints, stepping and frames; Debug Adapter Protocol.
- Component handles on nodes.
- Modules from disk and packs; `balaur api`.
- Hot reload.
- Self-documenting script API.

### Scenes and assets

- A frame's queued frees run as one pass per parent, so freeing a whole container is linear.
- Children are indexed by name, so a path lookup is one hash per segment; freeing a node with no components asks no plugin anything.
- Prefabs with per-path overrides.
- Hot reload for textures, models, fonts and sounds: a saved file moves the asset generation and every node built from one is built again.
- The browser editor keeps the project it is editing, so a refresh comes back to the scene as it was left.
- Component tags and presets.
- Binary asset packs, sha256-verified.
- Mesh (OBJ, glTF) and heightfield assets.
- Scene and node query APIs; reparenting keeps the world pose.
- Node visibility, z-index and tags.
- Comment-preserving TOML patching.
- Stable asset ids: `id://` references through `assets/index.toml`, and `assets.rename` rewriting every reference to a moved file.
- `sprite_sheet` assets with frames of any size, tags and slices; `sprite.sheet` draws them.
- `balaur import` reads `.aseprite` files into an atlas, a sheet and a clip per tag.

### Rendering

- The 2D camera zooms out to a hundredth of a pixel per unit, enough to frame a pixel-scale level whole.
- WESL shaders and material assets; screen-reading materials.
- Sprites, atlas regions, tilemaps and GPU-skinned 2D polygons.
- 2D lights and shadows.
- GPU skinning for 3D meshes.
- Post-processing: bloom, SSAO, SSR, depth of field.
- 2D and 3D skeletal animation; IK and look-at modifiers.
- Quaternion rotation tracks.
- More 2D and 3D shapes; polyline strips with gradients and textures.
- Textured particles with end colour, end size and one-shot bursts.
- Immediate-mode 2D draw calls.

### Physics

- Full Rapier surface in 2D and 3D: body parameters, CCD, forces, sleep.
- Joints with motors, limits and breaking; impulse and reduced-coordinate solvers.
- Inverse kinematics.
- Character controllers.
- Query pipeline: raycasts, shape casts, point and shape queries.
- Collision, contact-force and joint-break events.
- One-way platforms.
- Multithreaded solver.
- All Rapier collider shapes, including voxels and mesh-fitted.
- Collision layers, solver layers, offsets, per-collider mass.
- Editable voxel terrain.
- Ray-cast vehicles.
- 3D geometry: hulls, decomposition, voxelising, booleans.
- Physics debug draw and tuning.
- A joint on a node with no body ties the nearest body above it.
- `physics3d/step` and `physics2d/step` profiler spans.
- `raycast` stops at the nearest hit rather than visiting every collider along the ray.
- Creating a body composes the node's pose from its ancestors rather than propagating the whole tree.

### Determinism and networking

- Fixed 60 Hz step.
- Per-tick digest with a cross-OS CI check.
- Record and replay.
- Rollback with stable node ids across spawns.
- Networked sessions are recordable.
- Faulty transport for testing; session stats.
- Redundant input per datagram.
- HTTP, WebSocket and WebTransport crates behind one `Transport` trait.
- Lockstep sessions over a socket.
- Recorded platform facts: platform, device id, time, focus, dark mode, locale.

### UI and text

- Containers, layout, focus, themes, presets and surfaces.
- Widget kinds: draw, scroll, tab, image, check, dropdown, slider, progress, grid, flow, fold, dialog, separator.
- Text field widget with IME composition.
- Nine-patch images; draggable seams.
- `ui.image` draws one region of a file, and `ui.image_button` answers a click, so an atlas can be shown and picked from a tile at a time.
- Fill anchors, insets, UI scale, scroll deadzone.
- Shaped text through cosmic-text: bidi, complex scripts, CJK breaks, font fallback.
- Markup tags, font weight and style.
- On-screen keyboard height.

### Batteries

- Input actions with rebinding.
- Save games, atomic and versioned with migrations.
- Localization with plurals.
- Event bus.
- Audio buses, audio events, positional audio.
- Gamepad rumble, gyro, acceleration and touchpad.
- Tween delays, chaining and value tweens.
- Fixed-step task waits.
- 2D geometry: triangulation, booleans, hulls.
- Hashing, base64, UUIDs.
- HTTP downloads to file with progress.
- Safe area, refresh rate, keep-awake, vibrate, back button.
- Splash screen.
- `[application]` manifest section; settings screen.
- Plugin requirements and toggles.
- `balaur test`.
- Safe export directory check.
- `scripts/lint.sh` mirrors CI; pre-push hook.
- `examples/benchmark`: the Godot suites' physics and scene-tree cases, run headless, in the editor or on the web.
- `scripts/bench_compare.py` writes `docs/BENCHMARKS.md` from a run beside Godot's own results.
- `balaur run <project> -- <args>` passes arguments to `engine::args()`.

### Web

- Browser audio.
- Phoenix (`gamend`) over Fetch and WebSocket.
- `web` module: messages, visibility, user agent, location.
- User directory persists in `localStorage`.
- Hidden tabs keep ticking.
- `balaur export --target web`.

### Platform services

- `platform` module: sign-in, achievements, leaderboards, cloud saves, presence.
- Apple: Game Center, iCloud saves, identity, dashboard and access point.
- In-app purchases.
- Notifications, push and URL handling.
- `[apple]` manifest; export writes `Info.plist`, entitlements and signs macOS apps.
- Rollback-safe store writes.

### Editor

- Undo/redo, copy/paste, collapsible inspector, search.
- Prefab instances and overrides.
- Rig, Polygon and Tiles tools.
- The Tiles tool paints a tilemap from a palette cut out of its tile set: paint, erase, a rectangle fill, and layers as sibling nodes.
- Ray picking, asset filesystem verbs, language server linting.
- Profiler dock; `--timings`.
- Showcase and UI audit scripts.
- Stage shell with tabbed, resizable, animated docks.
- Shell built from engine widgets, themed.
- Bundled fonts and icons; project branding.
- Narrow-window layout; `--state scale`.
- One top bar with closable document tabs; docks minimise to a handle; script rename, pick and detach.
- Renaming in the Assets dock rewrites every reference to the file.

### Breaking

- `shape`/`body`/`collider` are `shape3d`/`body3d`/`collider3d`; scripts use `physics3d`.
- `color` is a shape property, not a component.
- Luau removed.
- Contact filter hooks removed; use layers and masks.
- `f64` physics removed.
- `animation.is_running` is `animation.is_tween_running`.
- `ui.select` is `ui.dropdown`.
- One plugin trait, `balaur_plugin::Plugin`; registration through `Registry` only.
- `name`/`main_scene` live under `[application]`.
