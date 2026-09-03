# Changelog

## Unreleased

- The script API documents itself: every module says what it is for, every function says what it does and which components it acts on, and every component and asset type carries a summary. `balaur api` prints all of it, `scripts/api_lints.py` fails the build when a registered function has no entry, and the website's reference is generated from it.
- Save games: `save.write(slot, data)` / `save.read(slot)` / `save.slots()` / `save.remove(slot)`, stored per user and written atomically. `[save] version` stamps every file and `[save] migrate` names the script whose `migrate_save(version, data)` brings an older one forward, one version step per call; a save from a newer build is refused rather than misread.
- `ScriptHost::call_in(path, function, args)`: call a public function in a script file with no instance, for a project-level hook.
- Profiler: `App::tick` times every stage, `engine.timings()` returns the last frame (`{ frame, fixed_steps, stages, spans }`), `balaur run --timings` prints mean/worst/share-of-frame when a run ends, and the editor gains a Profiler dock. `timings::measure(eng, name, ..)` names anything finer than a stage; core names its own script and transform work.
- `scene.component_properties(name, params)`: what a component's `apply` would receive, so a tool can compare a shorthand against a full table.
- Prefabs are placed from the editor: a palette command per scene file adds a child instancing it, and an instance row has an *open* button into the prefab itself.
- Prefabs in the editor: an instance's nodes appear in the tree in place, an edit inside one is written as an override on the node that named the prefab, and an edit back to the prefab's own value removes it. Structural edits inside an instance are refused.
- `App::add_replay_setup`: state a plugin loads rather than simulates goes in the recording's header and is restored before the first tick. Input bindings are the first user, so rebinding a key cannot change what a recording replays.
- Prefabs: a node names another scene file with `instance`, whose roots become its children. `[nodes.overrides."Path/Inside"]` edits one node of the instance — components, the transform, or a `script`'s exported properties — ids inside are prefixed by the instance's own (`n_crate_b/n_lid`), and a prefab that contains itself is an error naming the cycle.
- Input actions: `[input.actions]` in `project.toml` binds a name to keys, mouse buttons, gamepad buttons, axes and key pairs; scripts read `input.action_value` / `action_pressed` / `action_just_pressed` / `action_just_released`. `input.bind` rebinds and saves to the user data directory, `input.reset_bindings` undoes it. Actions are derived from the recorded raw input each frame, so a replay reproduces them.
- Editor: inspector sections (Transform, each component, Script, Events) fold on a click of their heading.
- Component handles: `node.body2d.apply_impulse(x, y)` is `physics2d::apply_impulse(node, x, y)` with the node bound, and every handle has `get()`, `set(table)`, `has()` and `remove()`. A module declares the components it drives with `Bindings::drives`, typed bindings record their signatures, and `balaur api` prints both, so the reference lists a component's functions on its own page.
- Shaders are written in WESL (WGSL plus imports, `@if` variants and dead-code elimination) and linked to WGSL at run time; the 2D skinning shader moved out of Rust into `shaders/skinned_2d.wesl`.
- `material` asset type: a `shader`, its `[features]` and the `[params]` its `Params` struct takes, read off the linked shader rather than declared twice. `sprite` takes a `material`; a shader that will not link logs once and the sprite draws with the built-in material. `.wesl` files ship in a pack. Saving a shader or a material relinks it and rebuilds the nodes drawing with it, and `render::check_material(path)` answers with the file, line and column a broken one failed at. `mesh` and `shape3d` take one too, lit by the scene's lights and fog. The inspector lists a material's params off its linked shader and writes them back; a `.wesl` opens in the code editor, highlighted as WESL. Shader functions marked `@const` are asserted on headless, and a call to a name nothing declares fails to link instead of reaching a GPU. A plugin can register shader modules a project's shader imports. `render::set_channel` draws normals, UVs, depth or the bare texture instead of the picture, and clicking a shader's gutter draws what that line computes.
- Exported script properties: `pub fn exports()` declares tunable defaults, the `script` scene key takes `{ source, props }` beside the plain path, and `props` is written onto the instance before `init`. The inspector lists them per node with undo, writing an override only where it differs from the default. `node:attach_script(path, props)` and `script::exports(path)` are the run-time and tool-facing halves.
- A `Transport` trait with one reliable ordered channel and unreliable datagrams, polled per tick rather than awaited, and `WebsocketTransport` under it: both deliveries share one socket behind a one-byte tag, and a datagram is sent reliably where the transport has no unreliable mode.
- Rollback on one machine: `rollback::Session` keeps a snapshot ring and an input journal, predicts a missing input by repeating the last one, and re-simulates from the tick a late input contradicts. Scripts read `rollback::input(player)` and `rollback::is_resimulating()`; outbound network work is suppressed while a tick re-runs.
- Rollback across spawns: a node spawned at run time gets a stable `<authority>:<counter>` id, and a snapshot restores the node set — freeing what was spawned since and respawning what was freed, with its components and script.
- Websockets carry binary frames: the script value model gains bytes, `websocket.send` takes a string or bytes, and a binary frame arrives as `{ socket, kind = "binary", bytes }`.
- Websockets negotiate `permessage-deflate` (RFC 7692) and take per-connection options: `websocket.connect(node, url, { compression = false, headers = { Authorization = "..." } })`. A `[net]` table in `project.toml` sets the defaults (`websocket_compression`, `http_timeout`).
- Determinism: one fixed 60 Hz step (`Stage::FixedUpdate`, `fixed_update(dt)`, `balaur run --fixed-tick`).
- Per-tick simulation digest with `first_divergence`, `--trace-digest`, and a cross-OS CI check.
- Record and replay: `run --record`, `replay --verify` / `--entries-at`, sources for input, gamepad, net, gamend.
- Rollback snapshots (`SnapshotRing`) over transforms, RNG, both physics worlds and script state.
- `sprite` component: textured 2D quads with sheets.
- Component tags and presets (`sprite2d`, `rigid_body2d`, ...), project-defined in `presets.toml`.
- New 3D shapes (capsule, cylinder, cone, plane, mesh) and 2D shapes (capsule, polyline).
- New colliders: 3D capsule, cylinder, cone, triangle, trimesh, convex hull, polyline, heightfield; 2D capsule.
- `mesh` (OBJ or inline) and `heightfield` asset types.
- Binary assets in packs (format 2, sha256-verified); `assets` mode in `project.toml`.
- `engine.tick()` for scripts.
- Editor undo/redo (`Ctrl+Z` / `Ctrl+Shift+Z`).
- `docs/DETERMINISM.md` and a generated `docs/generated/assets.md`.
- 2D skeletal animation: `bone2d` (rest poses) and the `skeleton` script module (`apply_rest`, `overwrite_rest`, `bones`); a clip keys bones by path.
- `polygon` component: a textured 2D polygon, GPU-skinned to a rig; `mesh` assets take `[x, y]` positions, `internal`, `polygons` (ear-clipped), `uvs` and `skin.bones` weights.
- Node paths accept `..`; `render.texture_size(path)`.
- Editor: Rig bone tool, bone gizmos, rest-pose buttons; Polygon tool with Points / Polygons / UV / Weights modes; keying writes to the nearest animated ancestor; game-relative texture paths resolve in the editor; `--state` takes a comma list and `shot=<png>`.
- `examples/rig`: a three-bone limb under a looping clip.
- Script debugger: breakpoints, continue and step over / into / out, call frames with named locals; the `debugger` script module; the editor's gutter markers, Debugger dock and `F5` / `F10` / `F11` / `Shift+F11`.
- Debug Adapter Protocol: `balaur run --debug <port>` serves DAP, so VS Code or any DAP client sets breakpoints, reads frames and locals, and steps a running game. `--debug-wait` holds the boot until a client attaches, which is the only way to catch a breakpoint in `init`. A client's Pause button stops the game at the next line a script runs, over the new `debugger::request_break()`; the editor has the same on `F6`.
- 3D: `bone3d`; `mesh` reads `.glb` and `.gltf` files (positions, normals, UVs, skin; side `.bin` files and `data:` URIs) and gains `skeleton` and `texture`; CPU skinning in the backend; `balaur import model.glb` writes the rig, mesh node, clips, side files and base colour texture; `.bin` ships in packs; `examples/rig3d`.
- `modifier2d`: `look_at` and `two_bone_ik` rig modifiers, run after the clip each frame.
- Editor: bones pick and grow in the 3D viewport too.
- `rotation` clip tracks take quaternion keys; `balaur import` keeps a file's rotations as they are.
- `node:set_parent(other)`: move a node under another, keeping its world pose.
- Rune: a deterministic `math` module (`libm`-backed, with `PI` / `TAU` / `INF`), `script.attempt`, `mod` files that load from disk and from packs, and `balaur api` on the Rune host.
- Editor: skinned polygons deform live again (a `..` skeleton path was taken for a file); `--state dock:<name>`.

### Breaking

- `animation.is_running` is `animation.is_tween_running`: it takes a tween handle, where `animation.is_playing` takes a node and asks about its clip, and the old pair of names said neither.
- Luau is removed: Rune is the one scripting language. `.luau` scripts, `language = "luau"` / `"mixed"`, `balaur_script_luau` and the `balaur::luau` alias are gone; the editor and every example are Rune.
- `ui.select` is `ui.dropdown`.
- `shape`/`body`/`collider` are now `shape3d`/`body3d`/`collider3d`; body/collider script APIs moved to `physics3d`.
- `color` is a property of `shape3d`, `shape2d`, `sprite` and `particles`, not a component.
