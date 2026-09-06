# Changelog

One line per feature. Nothing is released yet; a release is a `v*` tag whose
notes are that version's section.

## Unreleased

### Scripting

- Rune scripting with a deterministic `math` module.
- Exported script properties.
- Debugger with breakpoints, stepping and frames; Debug Adapter Protocol.
- Component handles on nodes.
- Modules from disk and packs; `balaur api`.
- Hot reload.
- Self-documenting script API.
- Named events between scripts, scoped to one emitter or heard from anyone.

### Scenes and assets

- `balaur import level.tmx` and `level.ldtk` bring a Tiled map or an LDtk project in as tilesets, their atlases and a scene per level.
- A frame's queued frees run as one pass per parent.
- Children indexed by name: a path lookup is one hash per segment.
- Freeing a node with no components asks no plugin anything.
- Prefabs with per-path overrides.
- Hot reload for textures, models, fonts and sounds.
- The browser editor keeps the project it is editing across a refresh.
- Component tags and presets.
- Binary asset packs, sha256-verified.
- Mesh (OBJ, glTF) and heightfield assets.
- Import settings beside a file: `art/hero.png.toml`, with `[import.<kind>]` defaults in `project.toml`.
- Nearest-neighbour filtering and linear-data textures, per image or per project.
- An Import tab beside the Inspector: the selected file's settings, where each value comes from, and a clear that drops the key.
- Bitmap font descriptors ride in a pack, so a `text2d` naming one draws in an exported game.
- Scene and node query APIs; reparenting keeps the world pose.
- Node visibility, z-index and tags.
- Comment-preserving TOML patching.
- Stable asset ids: `id://` through `assets/index.toml`; `assets.rename` rewrites every reference.
- `sprite_sheet` assets with frames of any size, tags and slices; `sprite.sheet` draws them.
- `balaur import` reads `.aseprite` files into an atlas, a sheet and a clip per tag.

### Rendering

- Tile maps: a tileset that says what each tile is, collision from its solid cells as one voxel shape, autotiling from an ordered rule table, animated and light-blocking tiles, isometric and hexagonal layouts.
- The 2D camera zooms out to a hundredth of a pixel per unit.
- WESL shaders and material assets; screen-reading materials.
- Sprites, atlas regions, tilemaps and GPU-skinned 2D polygons.
- 2D lights and shadows.
- GPU skinning for 3D meshes.
- Post-processing: bloom, SSAO, SSR, depth of field.
- 2D and 3D skeletal animation; five modifier kinds in each: `look_at`, `two_bone_ik`, `fabrik`, `ccdik` and `jiggle`.
- `polygon/deform` tracks: an `[dx, dy]` offset per vertex, added before skinning.
- A clip played on another rig, through a `bone_map` against a `skeleton_profile`.
- Quaternion rotation tracks.
- More 2D and 3D shapes; polyline strips with gradients and textures.
- Ten 3D primitives and six 2D ones, every one a mesh built headless.
- A word as geometry: glyph outlines filled, counters left as holes.
- `path2d` and `path3d` assets, stroked, extruded with a bevel, revolved, and swept along a rail.
- `boolean3d` and `boolean2d`: a node drawn as its children joined, cut out of one another, or overlapped.
- A `cloner` that draws its subtree along a line, around a ring or through a grid, in one call per mesh.
- Vertex colours and named morph targets from glTF; clips drive `mesh/morph.<name>`.
- Textured particles with end colour, end size and one-shot bursts.
- Immediate-mode 2D draw calls.

### Physics

- Full Rapier surface in 2D and 3D: body parameters, CCD, forces, sleep.
- Joints with motors, limits and breaking; impulse and reduced-coordinate solvers.
- Inverse kinematics.
- Ragdolls built from a rig, blended back onto the bones by a weight.
- Character controllers.
- Query pipeline: raycasts, shape casts, point and shape queries.
- Collision, contact-force and joint-break events.
- One-way platforms, whichever side of a contact pair the platform is on.
- Internal edges fixed on heightfield and 2D trimesh colliders; one-sided 2D polyline.
- Multithreaded solver.
- All Rapier collider shapes, including voxels and mesh-fitted; voxels in 2D as well, editable from a script.
- Collision layers, solver layers, offsets, per-collider mass.
- Editable voxel terrain.
- Ray-cast vehicles.
- 3D geometry: hulls, decomposition, voxelising, booleans.
- Physics debug draw and tuning.
- A joint on a node with no body ties the nearest body above it.
- `physics3d/step` and `physics2d/step` profiler spans.
- `raycast` stops at the nearest hit.
- Creating a body composes the node's pose from its ancestors.

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

- Text in the world: `text2d` and `text3d` shaped by the engine the widgets use, with outline, shadow, markup, a font chain, wrapping, billboarding and `text_key` localisation.
- `render.draw_text` and `render.draw_text_2d` for a label a tool draws in one frame, and `render.text_size` to measure one.
- AngelCode `.fnt` bitmap fonts, so a pixel face ships as the artist drew it.
- A measurement and a text mesh see the project's fonts and the bundled ones only, never the machine's, so every platform answers the same.

- `ui.set_lazy`: the UI pass runs only when something asks for it; the editor turns it on.
- Containers, layout, focus, themes, presets and surfaces.
- Widget kinds: draw, scroll, tab, image, check, dropdown, slider, progress, grid, flow, fold, dialog, separator.
- Text field widget with IME composition.
- Nine-patch images; draggable seams.
- `ui.image` draws one region of a file; `ui.image_button` answers a click.
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
- `balaur export` reports what the pack weighs, by section, extension and largest entry, and names the assets nothing references.
- `balaur export --report` measures without writing.
- `[export] strip` drops assets no scene, script or `keep` glob names.
- `[export] images`, `fonts` and `audio` re-encode losslessly at export: PNG recompressed or written as WebP, a face subset to the characters the project shows, WAV written as FLAC.
- Lossy export modes, each its own key: `images = "quantised"` with `images_quality`, and `audio = "vorbis"` with `audio_quality`.
- `recode` in a picture's import settings overrides the export's mode for that file alone.
- `scripts/lint.sh` mirrors CI; pre-push hook.
- `examples/benchmark`: the Godot suites' physics and scene-tree cases, headless, in the editor or on the web.
- `scripts/bench_compare.py` writes `docs/BENCHMARKS.md` from a run beside Godot's own results.
- `balaur run <project> -- <args>` passes arguments to `engine::args()`.

### Web

- Projects kept in IndexedDB, textures and sounds included.
- A folder opened from your machine in the browser editor; the project taken back out as a zip.
- `export` in a tab: a `.bpak` and a zipped web bundle. Native targets still need a linker.
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

- The Tiles tool paints in every direction, fills, draws lines, stamps blocks, picks a tile off the map and paints terrain; a Set panel writes collision, one-way, light and terrain back to the tile set.
- Undo/redo, copy/paste, collapsible inspector, search.
- Prefab instances and overrides.
- Rig, Polygon and Tiles tools.
- Rigging panels: a Weights dock with auto and smooth weights, modifier gizmos, bone names in the viewport, and Mirror in both tools. A mesh traced from a texture's alpha, deform keys, a Bone map dock, and Create Physical Skeleton.
- The Tiles tool: a palette cut from the tile set, paint, erase, rectangle fill, layers as sibling nodes.
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
