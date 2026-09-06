# Balaur architecture

The decisions, one line each. What is not built yet is `docs/ROADMAP.md`, and
how each of those gets built is `docs/PLAN-*.md`; `docs/NAMING.md` governs the
names.

```
┌────────────────────────────────────────────────────────────┐
│  scripts (game code, and the editor itself)  .rn — Rune    │
├────────────────────────────────────────────────────────────┤
│  balaur_script_rune: host, hot reload, mod files, debugger │
├────────────────────────────────────────────────────────────┤
│  balaur_script: the seam — Bindings, ScriptHost, Value.    │
│  Traits only. No language, no dependencies.                │
├────────────────────────────────────────────────────────────┤
│  modules declared once, reaching every language:           │
│  engine, scene, node, assets, animation, input, physics, ..│
├──────────┬─────────┬────────┬────────┬───────┬─────────────┤
│ physics  │ render  │ audio  │ input  │ anim  │ your plugin │
│ (rapier) │ (kiss3d)│ (rodio)│ (winit)│ (clip)│             │
├──────────┴─────────┴────────┴────────┴───────┴─────────────┤
│  balaur_core: hecs ECS + scene tree + assets + scheduler.  │
│  Names no scripting language.                              │
└────────────────────────────────────────────────────────────┘
```

Backends depend on core; core never depends on a backend. The compiler enforces
the direction, and it is what keeps core language-free.

## Foundations

- **ECS: hecs 0.11** — minimal, archetype-based, the most used maintained ECS
  that is not `bevy_ecs`. Scheduling, hierarchy and resources are Balaur's own.
- **Math: glamx** (glam + `Pose3`/`Rot3`), shared with rapier 0.35 and kiss3d
  0.46, so there are no conversion layers. It re-exports glam whole. The
  workspace pins `libm` and `scalar-math` — the features parry's
  `enhanced-determinism` turns on — so a build without physics matches one with.
- **parry** ships in every build, so depending on it costs a game no bytes. Two
  constraints keep core backend-free: no parry type crosses a public API, and
  every crate names `enhanced-determinism` explicitly. Taken for `Bvh` culling
  and picking, exact ray and point queries, and the 2D shape tools.
- Hand-written instead: `primitive` (parry returns no UVs or normals and lacks
  half the shapes), `csg` (parry has intersection only), `geometry2d`'s booleans
  on `i_overlay`, the 2D convex hull, and triangulation: parry's ear clipper is
  `pub(crate)`, and `i_triangle` either invents vertices a mesh cannot carry or
  aborts on a clockwise loop (`docs/PLAN-rapier.md` item 8).
- **`Engine`** is a clonable `Rc` handle over world, resources, command queue
  and script host, with interior mutability and short borrows: Rust systems and
  script bindings see one state, unmarshalled. Single-threaded by design;
  parallelism goes inside a system.
- **Scene tree over ECS.** A node is an entity with `Name`, `Parent`,
  `Children`, `Transform`, `GlobalTransform`. Paths, transform propagation and
  recursive free are core systems; plugins hang components off the same
  entities, so "node" is an API surface, not a cost.
- A scene's `parent` is an id or a path of names; ids resolve first, and a path
  may not climb out with `..`. The editor normalizes paths to ids on load — a
  replay addresses a node by id.

**Frame schedule.** `First → PreUpdate (reload pump) → Update (scripts,
animation) → FixedUpdate (scripts, physics) → PostUpdate (audio) → SceneSync
(transforms) → Render → Last (deferred destruction)`.

- `queue_free` lands in `Last`, so iteration is never invalidated mid-frame.
- `FixedUpdate` drains one app-owned accumulator in whole `FIXED_DT` steps: 0 in
  a fast frame, up to `MAX_SUBSTEPS` in a slow one. One accumulator, not one per
  plugin — per-plugin ones made the step count depend on wall-clock jitter.
- Order inside a stage is registration order, and core registers the script
  callback first, so `fixed_update` runs before that frame's physics step.

## Scripting

### The seam

`balaur_script` is traits and a neutral `Value`, one dependency (`anyhow`), no
language. Subsystems declare against `Bindings<Engine>`; a backend implements
`ScriptHost<Engine>`. Rune cost one crate and changed nothing else.

- Operations are declared once in core (`node_api.rs` `NODE_OPS`,
  `engine_api.rs` `ENGINE_OPS`) and reach every language. A second language
  costs the call sugar, not the operations.
- `Value` is `Nil/Bool/Int/Num/Str/Bytes/Vec2/Vec3/Color/List/Map`, plus
  `Node(u64)` (opaque entity bits) and `Callback(id)`, valid only for the call
  that received it. `Many` is several return values, not a list.
- Nothing subscribes with a closure — that lifetime is call-scoped. A
  subscription is a node plus a method name: `ScriptHost::call_on(node, method,
  args)`. Persistent callbacks would need an id space with explicit release.
- Most events also ship a polling twin (`animation.just_finished(node)`,
  `input.just_pressed(key)`, `http.responses()`): an event is a frame-scoped
  snapshot.
- `events.subscribe(node, name)` / `events.emit(name, payload)` deliver
  `on_<name>(payload)` at the top of the next `Update`, in emission then
  subscription order. The frame of delay keeps a handler from freeing the node
  being ticked. Not recorded — a replay re-runs the script, which emits again.
- `language` in `project.toml` picks the language; absent means Rune, the one
  this build ships.

### Model

- Lifecycle: `init`, `update(dt)`, `fixed_update(dt)`, `on_free`, `hot_reload`.
  One instance per attached node, with `node` on it.
- Free functions take the instance first (`pub fn update(this, dt)`) and mutate
  it in place. `mod name;` pulls in `name.rn` beside the file — disk in a dev
  run, pack when shipped.

### Exported properties

`pub fn exports() { #{ speed: 2.0 } }` declares what a scene may tune;
`script.props` overrides only what it names.

- At attach: `exports` once per file, defaults written, `props` over them, then
  `init` — so `init` reads tuned values.
- An unexported property is still written, and warns; dropping it would lose an
  edit, and `#[export]` does not exist yet (`docs/PLAN-scripting.md`).
- `props` is sparse: the inspector writes an override only where it differs, so
  changing a default reaches every node that never overrode it. The default's
  type is what the inspector draws, and what keeps `2` from becoming `2.0`.
- It is scene data — packed, digested, replayed. `node:attach_script(path,
  props)` is the same thing at run time.

### Prefabs

`instance = "scenes/crate.toml"` makes the prefab's roots the node's children,
the same rule `scene::instantiate` follows. The node keeps its own name,
transform and components.

- `overrides` is keyed by path from the instance node and holds scene keys,
  including `script.props`.
- **An override patches, it does not replace** (`components::patch`) — through
  `add`, overriding a collider's `half_extents` would reset its `kind`.
- Every `StableId` inside an instance is prefixed by the instance's id
  (`n_crate_b/n_lid`) and nests. That is what a replay prints and what
  replication will address.
- A path naming nothing is reported and kept; a self-containing prefab is an
  error naming the cycle. Scripts attach when the outermost scene finishes.
- In the editor: placed from the palette, opened from its row, drawn one shade
  quieter. Editing a prefab row writes a sparse `overrides` entry, removed again
  when the value returns to the prefab's. Comparison needs
  `scene.component_properties`, since `body3d = "dynamic"` and the full table
  are one component spelled two ways. Structural edits inside an instance are
  refused — the file has nowhere to put them.

### Hot reload

- The host watches the project (`notify`); on save the file recompiles. A
  compile error keeps the previous unit running, reported once.
- Rune compiles to an immutable unit, so the new unit replaces the old,
  instances keep their state, and the next call resolves against new code.
  Save-to-live latency is the watcher's, a few milliseconds. `hot_reload`
  migrates state shapes.
- Content reloads through the same watcher, sorted by extension exactly as
  `Pack::build` does. A `.toml` asset was parsed, so the cache forgets it; a
  texture, model, font, sound or `.wesl` is a source, so only the asset
  generation moves and each consumer re-derives. Textures are keyed by path and
  mtime, so saving one image re-decodes that image alone.

### Debugger

Breakpoints, stepping and a call stack with locals, for scripts in the editor.

- A pause is a parked coroutine plus an engine flag, never a blocked thread —
  play-in-editor shares the process and the host with the editor.
- `Engine::set_debug_scope` names the game's subtree. While paused, inside that
  scope: no `FixedUpdate`, no `update`/`call_all`/`call_on`. Editor scripts keep
  drawing. Instances the tick had not reached run on resume.
- The seam adds four defaulted methods — `set_breakpoints`, `breakpoints`,
  `paused`, `resume(StepMode)` — so a backend without a debugger compiles
  unchanged. The `debugger` module exposes them plus `set_scope`, so the dock is
  plain script.
- Rune: a unit with breakpoints runs through `Vm::execute` + `VmExecution::step`;
  one without keeps the plain call, so the no-debugger cost is unchanged. A
  requested line lands on the next line with code, re-applied after a reload.
  Step over/into/out compare frame depth and line. Locals are the frame's named
  arguments — Rune keeps no other names. A breakpoint inside an async entry
  point (`task::wait`) is let through: a future cannot be parked.

### Components

`App::register_component` takes a TOML schema — a `type` per property from the
closed set (`float`, `bool`, `string`, `enum`, `vec2`, `vec3`, `color`,
`asset`), defaults, enum options, `shorthand`, `readonly` — plus apply, get and
remove hooks.

- `components::Attached` is one bit per component per node, so a free runs only
  the `remove` hooks whose bits are set. Attaching state by another path warns
  in debug and gets no `remove` in release.
- `parse_schema` validates at registration and panics naming the component and
  property: a bad schema fails at boot, not at the first inspector row.
- A tagged union's discriminant is always `kind`; `type` is always the datatype
  (N6). A `color` takes floats or `#rrggbb[aa]`, expanded before `apply`.
- Two verbs: `set_component` merges over schema defaults (whole component),
  `components::patch` merges over the component's own `get` (leaves the rest).
  Animation and the inspector need the second — patching `shape/radius` with the
  first would reset `half_extents`.
- One registration buys the scene key, the node API (`set_component`,
  `get_component`, `has_component`, `remove_component`, `component_names`,
  `scene.component_types`, `scene.component_schema`) and the editor: the
  Add-component palette and every inspector row are generated from the registry,
  so a third-party component needs no editor change.
- In tree: `body3d`/`collider3d`, `body2d`/`collider2d`;
  `shape3d`/`shape2d`/`sprite`, each with its own `color` property, since a tint
  needs something to tint; `widget`.

**Browsing them.** The list is flat — a node is exactly its components.

- **Tags** are facets, several at once (`collider2d` is `2d` *and* `physics`); a
  category path would bury whichever lost. 3D components are tagged `3d` though
  their names carry no marker (D5).
- **Presets** are named recipes over components on one node. Nothing records
  which was used, so components stay free to come and go. Plugins register their
  own; a project adds more in `presets.toml`. Anything spanning nodes is a scene.
- **Expectations** are advisory — a component names others it needs something
  from, and the editor warns while none is present. Nothing blocks. No built-in
  declares one: every candidate was either valid alone (a `collider2d` with no
  `body2d` is static geometry) or a component that should not exist alone.

### 2D

A second set of components over the same tree, on the regular `Transform`.

- `shape2d` (`rect`/`circle`) and `sprite` render through a pan/zoom
  orthographic camera: `render.set_camera_2d(cx, cy, zoom)` in logical px per
  world unit, plus `camera_2d`, `mouse_world_2d`, `draw_line_2d`.
- `body2d`/`collider2d` run in a rapier2d world beside the 3D one — same
  determinism build, accumulator and ordered collections. Both dimensions carry
  the same surface function for function: joints, character controller, queries,
  events.
- A sprite sizes from its image (`pixels_per_unit`, default 100) or one cell of
  a `columns`/`rows` sheet, resolved when set rather than at draw time — so the
  image header is read in every build, headless included.
- Changing the frame moves UVs only and does not bump the renderable's version;
  changing texture or sheet does.
- `physics.set_paused`, `is_paused`, `clear`, `set_sleeping_allowed`,
  `sleeping_allowed`, `set_tuning`, `set_debug_draw`, `set_threads` span both
  worlds, so an editor treats physics as one simulation.
- The editor detects a 2D scene from its components and switches grid, camera,
  gizmo, picking and collider overlays. `examples/angrynerds` is a full 2D game.

### Assets

An asset is content several nodes share. `resource` is the typemap (D1), so
content is `asset`.

- **A string is a reference, a table is a definition** — every asset-typed
  property takes either, implemented once in `components::properties`. An inline
  table is cached and rewritten to the reference naming it, so `apply` only ever
  sees a string.
- Three forms an author writes: `"animations/hero.toml"`,
  `"animations/hero.toml#run"`, `"#hero_idle"` (an `[[assets]]` block in the
  same document). A fourth, `"#!<hex>"`, is written by nobody: an inline
  definition keyed by a digest of its content, so re-applying is idempotent. It
  takes `#entry` too, which makes *Make inline* the exact inverse of *Save as
  file*.
- Core never learns what an asset is: `App::register_asset_type(name, directory,
  doc, parse)` returns an opaque `Rc<dyn Any>` the plugin downcasts.
  `AssetState` is the `DetHashMap` cache keyed by resolved reference;
  `AssetTypeRegistry` is the parser table, read-only after plugin build.
- Sharing is the default; `assets.duplicate` opts out. Cache keys use a
  hand-written FNV-1a over bytes and TOML structure — `std`'s hashers specify
  nothing about their output, and this key must agree across platforms.
- Resolution reuses `ScriptHost::scene_source` (pack or disk), with a
  `ProjectRoot` + `std::fs` fallback for Rust-only apps and tests.
- An unresolved reference **warns** and the scene loads; a definition table that
  does not parse is an error. `apply` errors are fatal to `instantiate_scene`,
  so warning is what keeps one bad path from taking a scene down.
- `assets` is `load`, `duplicate`, `exists`, `reload`. `load` hands back the
  definition *table*: `Value` has no variant for `Rc<dyn Any>`, and per-backend
  userdata is what the seam exists to avoid.
- `assets.rename` moves a file and rewrites every `.toml` through `toml_edit`,
  comments intact. `assets/index.toml` maps `id = "path"`, and `id://<id>`
  stands in for a path anywhere, resolved before the cache key.
  `assets.assign_id` writes a digest of path and content, so a rebuilt index
  gives a file the id it had. Script sources are not rewritten. Binary assets
  landed with pack format 2, verified by sha256.

### Animation

`balaur_anim` is a plugin: one asset type, one component, one script module, one
system in `Update`. It depends on core and no other plugin crate — a test reads
its `Cargo.toml` and fails if one appears.

- A clip is a length, a loop mode (`none | loop | pingpong`) and tracks; a track
  is a node path, a property, an interpolation (`step | linear | cubic`) and
  keys. A library is the same file with `[clips.<name>]` entries, addressed
  `hero.toml#idle`; the entry inherits the document's `type`, so nothing in core
  knows the word `clips`.
- **Property addressing reuses the component registry**: `position`,
  `rotation_euler`, `scale` are the transform, anything else is
  `component/property` through `patch`. So `color/rgba`, `shape/radius` and
  `widget/x` animate, and a third-party component animates the day it registers.
  A track with no `property` is a method track, calling through `call_on`.
- Rotation keys are authored as euler radians (the spelling
  `set_rotation_euler` uses, readable in a diff) and interpolated as
  quaternions, the only way past ±180° that takes the short way. A `rotation`
  track takes the quaternion — what an imported `.glb` holds.
- The sampler is `(clip, time) -> pose`, pure and reachable with no `Engine`, so
  blend trees can compose samples later.
- **A tween is a generated clip**: one sampler, two authoring front-ends, no
  second interpolation path. Steps are sequential; `parallel = true` joins the
  previous, and the next non-parallel step waits for the group. `to`, `by`,
  `from`, `target`, `interval`, `call`. A tween dies with its node.
- The spec is data, not a chainable builder: a handle object means new userdata
  in every backend and every future language. A handle travels as an integer,
  and `animation.stop` takes a node or a handle rather than a second destruction
  verb (N1). Data also makes a tween serialisable, so the editor authors one and
  it hot reloads.
- Easing is Godot's 12 transitions in 4 modes, with its names and shapes. Every
  curve maps 0→0 and 1→1 exactly, asserted with `assert_eq!` — without endpoint
  guards `expo` and `elastic` land a float short.
- Determinism: playback has its own 1/60 accumulator capped at four catch-up
  steps, so the sampler only sees `FIXED_DT`. Folding is floor-and-subtract;
  every transcendental is `libm`'s, glam's included. Players and tweens live in
  `DetHashMap`s and apply in insertion order.
- Scheduling: after the script tick, so `animation.play()` lands the same frame;
  before `PostUpdate`, where physics reads `Transform` for kinematic bodies, so
  an animated platform pushes what stands on it with no wiring. A step records
  deferred effects rather than applying in place — an `apply` may want the world
  mutably, and a handler may free its own node.
- Not now, not precluded: blend trees, state machines, retargeting.

### Objects: every shape is a mesh

A shape is a function from parameters to `MeshData` in `balaur_core::primitive`
on `libm`. So a collider fitted to a torus, a ray picking one, a headless test
and the triangles on screen are the same triangles.

- Ten 3D primitives, six 2D; `path` adds beziers and what they extrude, revolve
  and sweep into; `csg` combines two with a BSP over their faces — written out
  because the candidate crates reach for parry and a BSP has no transcendental
  to pin. `balaur_ui::glyph` is the one mesher outside core: shaping a word
  needs that crate's font set.
- A `mesh` asset names a model file, a primitive, a word or a path to thicken;
  the two that reach another asset resolve through `mesh::load_from`.
- A node draws `Shape::Solid` (parameters), `Shape::Mesh` (an asset) or
  `Shape::Built` (what a `boolean3d` settled on) — the split `Shape2d::Polygon`
  already used. A `cloner` multiplies what is under it (`core::cloner` places
  the copies, `render::instancing` splits each matrix for the shader); automatic
  instancing will draw through that seam.

### Skeletons and skins

A bone is a node with a rest pose (`bone2d`, registered by `core::skeleton`).
There is no skeleton component: a skin names its rig by node path
(`polygon.skeleton = ".."`), and the rig's bones are that node's descendants
carrying a bone, in tree order — the order a skin numbers them.

- Nothing in `balaur_anim` knows the word bone: a clip keys `target =
  "Hip/Thigh"`, and the digest and snapshot ring already cover it. Bones live in
  core so rendering, the editor's registry and future physics reach them. The
  `skeleton` module is `apply_rest`, `overwrite_rest`, `bones`.
- **A skin is a `polygon`**: `[x, y]` positions, an `internal` count of trailing
  interior vertices, optional `polygons` index loops (with interior vertices the
  author draws them — automatic triangulation bends badly), `uvs`, and
  `skin.bones` folded at parse to the four heaviest influences, renormalised.
  Ear clipping is `core::triangulate` (`f32`, no dependency), so a headless test
  asserts the triangle list. Absent UVs centre the texture at `pixels_per_unit`,
  v downward, from the image header.
- **The palette is computed by the engine, uploaded by the backend.**
  `joint_matrices_2d` is a pure function of the tree: global pose × inverse rest
  pose down the rig, carried into the skin's space — so a skin need not sit
  under its rig, and a flipped rig flips its skin. The backend draws through
  `render::skinned_2d` (kiss3d's `Material2d`, self-contained WGSL) rather than
  kiss3d's `SkinnedMesh2d`, which walks its own bone chain and has no scale in
  its model transform. Zero-weight vertices stay where they were authored.
  `skin_positions` is the CPU twin the headless tests assert against.
- **3D is the same model with import instead of tracing.** `bone3d` is the same
  `Bone`; `mesh` gains `skeleton` and `texture` and reads a self-contained
  `.glb` (`core::glb`) with inverse bind matrices re-based to the rig root.
  `balaur import model.glb` writes the joint hierarchy, a mesh node and every
  animation as plain TOML the editor edits. The `gltf` crate builds without its
  image feature — nothing here decodes a texture. GPU skinning is
  `skinned_3d::attach`; the CPU twin stays for a node with its own `material`
  and for the tests. A `.gltf` reads its buffers through a caller-supplied
  `SideReader`, or a `data:` URI decoded in core.
- **Modifiers have the last word.** `modifier2d` is Godot's
  `SkeletonModification2D` as one component: `look_at`, and `two_bone_ik` (the
  analytic solve, `flip` choosing the elbow, an out-of-reach target
  straightening the chain). It runs after animation, from local transforms as
  they are now, in entity order, on `libm`.

### Binding API

One call per entry point; conversions are inferred.

```rust
let m = app.script_module("physics")?;
m.function("apply_impulse", |eng, (node, x, y, z): (UserDataRef<NodeRef>, f32, f32, f32)| {
    ...
})?;
```

Plugins register scene keys too (`app.scene_key_handler("collider3d", ...)`),
applied in plugin registration order. `balaur_physics` is the reference
implementation; its `scalar.rs` is the one place a number changes width between
the engine's `f32` and rapier's.

### Modules and extensions

A **module** is linked in behind a cargo feature; an **extension** is loaded
from a shared library at run time. Both implement `balaur_plugin::Plugin` —
`manifest()` and `declare(&mut Registry)` — so one source ships either way.

- `Registry` names resources, systems, components, presets, asset types, replay
  sources and setup, and script modules. No trait objects; generics only where a
  Rust type must be named, since a `fn` pointer crosses an ABI boundary where an
  `impl Trait` does not.
- There is no `app()` escape hatch: every registration is a verb a C extension
  could be handed. `engine()` covers what a plugin reads while declaring;
  `config()` hands it `[plugins]`' table for its own name.
- `PluginRegistry` records every plugin that registered, and `requires` is
  checked against it, so a name crosses the module/extension boundary either
  way. `load_all` orders the whole set before any of it registers, so a missing
  requirement is refused rather than found halfway. Load order is by name then
  requirements, never directory iteration — it decides registration order, which
  decides the simulation.
- `[plugins]` in `project.toml` switches modules off or configures them. Asking
  for one nothing registered is an error naming the feature to rebuild with;
  turning off one every build has is refused — a setting that cannot be honoured
  must not look like it was.

**Two boundaries, because Rust has no stable ABI.** `load_extension` picks by
exported symbols.

| | Rust extension | C extension |
| --- | --- | --- |
| Exports | `balaur_plugin_abi`, `balaur_plugin_create` | `balaur_extension_abi` + three more |
| Crosses | `Box<dyn Plugin>`, `Registry`, `anyhow::Result` | `#[repr(C)]` only |
| Checked by | rustc + engine version + registry abi | one ABI version number |
| Written in | Rust, that exact rustc | anything: C, Odin, Zig, C++ |

- The Rust tag is `#[repr(C)]` and fixed-size, read *before* anything
  Rust-shaped crosses: reading a `String` from another compiler's library is the
  undefined behaviour the check prevents. `balaur_plugin`'s build script stamps
  host and plugin separately.
- **The Rust path has a ceiling**: `TypeId` hashes the crate as cargo compiled
  it, so host and out-of-tree extension hold two keys for one `ProjectRoot`
  (measured in `crates/balaur_plugin/tests/extension.rs`). An extension may own
  state; it may not reach the engine's.
- **The C path has no ceiling**: no `TypeId`, state behind its own `void*`, four
  symbols, and a table of host function pointers rather than resolving symbols
  back into the executable. The header is committed, with `_Static_assert`s on
  every size and offset and a Rust test asserting the same numbers.
- **No allocation crosses it**: every `BalaurValue` is a borrowed view valid for
  its call, and the host copies before returning. Tier 1 today is script modules
  of functions and constants (`docs/PLAN-c-api.md`).

### Precompiled packs

`balaur export` compiles every script at optimization level 2 — dev mode's own
configuration, so shipped bytecode is what was tested — and bundles scripts,
scenes and manifest into a `.bpak`. Packed runs build no compiler and no watcher.

- `balaur::boot_pack(include_bytes!(...))` makes a self-contained binary. It is
  pure interpretation, so it ships where JIT is banned, iOS included. CI
  cross-compiles to iOS, Android and wasm on every push to main.
- The web target is wasm-bindgen's, not emscripten's: kiss3d declares its web
  dependencies there and wgpu reaches WebGPU through `web-sys`. Audio is a stub
  on wasm (no cpal host compiles there), and `balaur_webtransport` is left out
  until it grows the same stub.
- A pack is written in sorted key order, so two exports of one source tree give
  the same bytes anywhere. CI exports every example twice per platform and diffs
  the digests across the matrix — hashed maps once gave five files from ten
  exports.

### Fused executables

`--target <platform>` appends the pack to the runtime template CI publishes:

```text
[ template executable ][ pack bytes ][ pack length: u64 LE ][ "BPAKSELF" ]
```

- ELF, Mach-O and PE ignore trailing bytes, so the fused file runs. At startup
  the CLI reads its own executable (`core::fused`): a pack means it is a game
  and argv is never read. One binary is the editor, the CLI and every game's
  runtime.
- Templates resolve from `BALAUR_TEMPLATES`, then `templates/` beside the
  executable, then `<data dir>/balaur/templates/<build id>`. A missing desktop
  template is offered for download from the release this build came from and
  verified against its `SHA256SUMS` — pinned exactly, because a pack must only
  meet the runtime its compiler shipped with. The prompt needs a terminal or
  `--download`; `--no-download` forbids it.
- `scripts/package.sh` bakes a build id and every release carries a VERSION
  asset, which catches a Monday nightly meeting Wednesday's template and drives
  `balaur update` — one command replacing binary, editor, template and header,
  which only work as a set. A source build refuses and points at git.
- A signed macOS game is `export --app`: a `.app` with the pack in
  `Contents/Resources`, because codesign seals resources but never bytes
  appended to a flat binary. Signing happens after export.
- The exporter sets the execute bits on its output: a template from a zip or an
  artifact store has lost them.
- Rune resolves `input::just_pressed` at compile time, so `balaur::build_pack`
  boots the app the game would boot and compiles through its host. A bare
  `rune::Context` rejects every script that touches the engine.

## Game UI: widgets are nodes

A `widget` is a component on a node, so a menu is a subtree the editor edits
like anything else.

- `row`, `column` and `panel` lay out their widget children — along the
  direction, `gap` apart, `padding` inside, `align`ed across. A child's own
  `anchor`/`x`/`y` is ignored: a menu that moved when you nudged one entry would
  not be a menu.
- A widget's parent is its nearest *widget* ancestor, so a grouping node changes
  nothing. Only containers adopt what is under them.
- The layout is egui's own, the same arithmetic the editor's panels use. Layout
  is presentation and never touches the digest; wrapping or percentages are what
  would justify `taffy`.

**Focus.** One focused widget per screen, held as a resource so moving it is one
write. It walks widgets in scene order and wraps.

- Whether a widget is a stop is **derived, not declared** — focus exists to
  activate something. `focusable = false` takes a candidate out; it cannot put
  one in. Hidden, freed or unfocusable releases focus.
- An accept is a click by another name (same `clicked`, same `on_click`), so a
  mouse menu works on a pad unchanged. `on_focus` fires only on arrival.
- egui drives keyboard focus, so a menu needs no input plugin. A pad goes
  through `ui.focus_next/previous/activate_focused`, which `standard_app` maps
  to the actions `ui_next`, `ui_previous`, `ui_accept` — wiring in the
  assembling crate, since `balaur_ui` has no pads and `balaur_input` no widgets.

**Themes.** A `widget_theme` asset owns how a *kind* is drawn — `fill`,
`stroke`, `stroke_width`, `radius`, `padding` under a table named for the kind.

- A widget takes the theme of the nearest ancestor naming one, so a screen is
  themed by its root and a dialog may differ inside it. An omitted kind keeps
  the built-in look.
- Parsing never fails: a bad colour is reported and dropped, because a game that
  would not start over one is worse than a game that starts plain.
- Neither side can contradict the other, so no widget property needed an
  "unset" sentinel.

## Audio

**Buses** form a tree (`[audio.buses] ui = { volume = 1.0, parent = "sfx" }`).

- A sound's gain is its own volume times every bus to the root. `master` exists
  whether declared or not.
- `set_bus_volume` re-applies to what is already sounding — the difference
  between a mixer and a default; a handle remembers its bus and starting volume.
- An undeclared bus is unity, not silence, so a typo stays audible and findable.
  A parent cycle is cut and reported.

**Events** are named sounds in `audio/events.toml`. A script says
`audio.play_event("hit")`; which file, level and bus is the sound designer's.
**Variations are taken in turn, not at random** — a rotation must not repeat,
and the engine RNG would put what a player hears into the simulation's stream.
Where the rotation got to is presentation: not snapshotted, not digested.

**Positional.** A `listener` node is the ears: distance sets volume, offset
across its right sets pan. A sound is placed by its `sound` component or per
call; `audio.set_listener` covers a game whose ears are not a node.

- Attenuation is inverse-distance, full inside `min_distance`, halving per
  doubling, cut at `max_distance`. Pan is equal-power amplitude computed with
  square roots rather than the usual sine pair — platform trigonometry differs.
- Doppler is OpenAL's model over frame-to-frame velocities, off unless a sound
  asks, clamped to an octave: on a jittering position it is a warble, on a
  teleport a screech.
- Placement multiplies the bus chain rather than replacing it, so a positional
  sound still answers the `sfx` slider. It runs in `SceneSync`, so ears and
  emitters use the poses the frame will draw.
- rodio's `Spatial` is not used — it folds distance into pan through a fixed
  inverse square with no notion of a game's unit. `ChannelVolume` under the
  engine's arithmetic gives the scene's numbers, and makes a positional sound
  mono.
- Nothing is read back off a sink: `placement_of` answers the same on a runner
  with no sound card, and `audio.distance_gain` / `audio.pan` give an overlay
  the numbers applied.

## Localization

`strings/<locale>.toml` per language, read with `strings.tr("menu.play")`.

- A missing key falls back one hop to the project's fallback locale. A key
  neither has comes back **as itself** — visible in the game, where an empty
  label would hide.
- `{name}` takes the argument called `name`; an unfilled placeholder is left
  alone so the hole shows. An `n` argument picks the plural form.
- Plural rules are a named handful (English, Romanian, the Slavic shape, French,
  the one-form languages), not the CLDR table; an unlisted language gets the
  English rule, and a missing form falls to `other`.
- A widget takes part through `text_key`, resolved every frame. Saving a strings
  file forgets the catalogues, so a translation hot reloads like a script.

## Save games

`save.write(slot, data)` and `save.read(slot)`, per user, under
`user_data_dir()`. A save is whatever the game puts in the table; the engine
decides three things.

- **Where it lives.** A slot name is letters, digits, `-` and `_`, so
  `../../id_rsa` is refused.
- **That a half-written file cannot replace a good one.** Written beside the
  target and renamed over it.
- **What version wrote it.** `[save] version` is what this build writes;
  `migrate` names a script whose `migrate_save(version, data)` runs once per
  version step, so a migration only knows adjacent shapes. A file from a newer
  build is refused. `ScriptHost::call_in(path, function, args)` is the seam: a
  function in a script *file*, with no instance.

## Store and platform services

One plan per store (`PLAN-apple.md`, `PLAN-google.md`, `PLAN-steam.md`); two
things are shared.

- **Two modules, not one.** `platform.*` is the verbs every store has; `apple.*`
  is what only Apple has. Not a lowest common denominator, because the native
  module sits beside it, and a missing verb answers `unsupported` rather than
  pretending. `balaur_platform` owns the seam, one backend fills it, and with
  none every call still answers.
- **Delivery is the engine's usual one**: an id out, the answer across a
  channel, `ExternalIo` landing it at `Stage::First` of a later tick — recorded,
  replayable with no store present, dispatched to `on_platform` and to whoever
  awaits the id.
- **A write waits for its tick to settle.** A rollback cannot take an
  achievement back, so an outward call is held until its tick leaves the
  snapshot ring (`rollback::Clock`; `u64::MAX` with no session). Reads are never
  held.
- **One place needs Swift, fenced off.** StoreKit 2 has no Objective-C
  interface, so `crates/balaur_apple/swift` is ~100 lines linked by `swift-rs`,
  with a request id out and a JSON object back — a field the App Store adds
  reaches a script with no Rust changing.
- **Arrivals nobody asked for carry request 0** (a sign-out, a renewal, a
  notification, an opened URL). They queue until a pump allowed to touch the
  outside moves them, since anything reaching the channel during a replay would
  be taken for recorded input. `Engine::next_token` starts at 1. The window
  layer owns the application delegate, so the engine proxies it, forwarding
  unanswered selectors, from the first call that needs it.
- **Capabilities are export-side.** `[apple]` in `project.toml` names identifier,
  team, deployment target and capabilities; `balaur export` writes the
  `Info.plist` and `.entitlements` and refuses what the minimum OS cannot
  satisfy. Frameworks link into Apple templates unconditionally — a prebuilt
  template cannot add one later.

## Timings

`App::tick` measures each stage and publishes the frame whole, so a reader never
sees half of one.

- `engine.timings()` gives `{ frame, fixed_steps, stages, spans }` in seconds;
  the Profiler dock draws a bar per stage against 16.7 ms, and `balaur run
  --timings` prints mean, worst and share of a frame.
- Stages are coarse on purpose — nine `Instant::now()` calls a frame, beneath
  the noise. Finer is a named span (`timings::measure(eng, "physics/step",
  ...)`), paid for by the plugin that asks. Core names `scripts/update`,
  `scripts/fixed_update`, `scripts/reload`, `scene/transforms`.
- `fixed_steps` sits beside the stages because a free-looking `fixed_update`
  usually means the accumulator had nothing to drain.
- **Timings are an observer**: never recorded, replayed or hashed, so a
  `fixed_update` branching on one would desync and the digest would say so.

## Determinism

Identical inputs produce bit-for-bit identical simulation on every platform.

Rune fits: IEEE-754 doubles and 64-bit integers that never mix, `+ - * /` and
`sqrt` exactly specified, a single-threaded interpreter with no codegen.

| Hazard | Status |
| --- | --- |
| `f64::sin/cos/exp/pow/...` call the platform libm | **Done** — the `math` module is pure-Rust `libm`; Rune has no transcendentals, and our fork puts its `powf`/`powi` on libm (`crates/balaur_script_rune/tests/pow.rs`) |
| Object iteration order is the hash map's | **Done** — the fork hashes with `XxHash64` at a fixed seed, so order is the same everywhere. Still not *insertion* order: sort the keys. Upstream's `ahash` seeds from `getrandom` and its AES and software paths disagree |
| A random source seeded from entropy | **Done** — `rng` is an engine-owned PCG32 with a fixed default seed |
| Wall-clock or variable `dt` in simulation | **Done** — `FixedUpdate` runs on one accumulator at `FIXED_DT`; `--fixed-tick` pins the frame too, so an interactive run reproduces a headless one. Input is one snapshot per frame |

Engine-side:

- rapier's `enhanced-determinism` workspace-wide; physics at `f32` (the `f64`
  feature was dropped 2026-09-04 — nothing needed it, and a width CI never built
  had rotted twice).
- `DetHashMap` (`IndexMap` + unseeded FxHasher) wherever order matters, never
  bare `HashMap`. Scene instantiation and scene-key order are deterministic.
- Rendering and audio are pure observers; a headless run computes what a
  windowed run computes.
- Transcendentals use `libm`, glam's included (`glamx/libm`,
  `glamx/scalar-math`). `house_lints.py` fails a bare `.sin()` in Rust; the rune
  fork closes the script side.
- Dev-mode and shipped bytecode come from one compiler configuration.
- The solver threads on rayon, always: `tests/threads.rs` asserts one thread and
  eight give the same digest, so the default follows the machine (one less than
  `available_parallelism`, capped at eight). The price was the three script
  physics hooks — rapier compiles its threaded pipeline only under
  `not(unsync-callbacks)`, so `filter_contact` and `filter_overlap` gave way to
  the `layers`/`mask` groups and `modify_contacts` is gone. One-way platforms
  survive: their axis was always collider data.
- `crates/balaur_physics/tests/determinism.rs` asserts two runs match per tick,
  and CI diffs a per-tick digest across Linux, macOS and Windows
  (`scripts/determinism_trace.sh`). `macos-latest` is arm64, so the architecture
  that could contract `a*b+c` into an FMA is in the matrix.

### The digest

`core::digest` folds the simulation into one 64-bit number per tick: what a test
asserts, what CI diffs, what a peer would exchange.

- `entries` hashes labelled slices, so a mismatch is a location
  (`n_ball_7/body2d`); `first_divergence` reports the first slice two runs
  disagree on, including a node present on one side only.
- Floats enter as `to_bits`: comparing numerically would call two runs equal
  after they had drifted an ulp, which is the drift that desyncs later.
- **Labels are stable ids, not paths**, so two peers agree on the name of what
  diverged. Godot's `MultiplayerSynchronizer` resolves by `NodePath`, needs a
  negotiated path cache, breaks on reparent and cannot resolve run-time nodes.
- **Components are not the whole simulation** — `body3d` reports its kind, not
  its velocity. Plugins add what a step computes through
  `App::add_digest_source`; physics adds velocity and sleep state in both
  dimensions.
- The walk is scene-tree order: peers that agree have the same tree, and a
  reparent is state worth catching.

### Record and replay

`balaur run <project> --record session.blr` writes JSON Lines: a header, then
one line per tick with that tick's external input, its step and its digest.
Line-oriented so it greps and streams, flushed per frame so a crash leaves every
tick.

- `balaur replay session.blr --verify` re-feeds and re-checks, stopping at the
  first disagreement with a non-zero exit. `--entries-at <tick>` prints that
  tick's labelled components: run it on both machines and diff.
- Eight sources register (`input`, `gamepad`, `http`, `websocket`, `gamend`,
  `platform`, `apple`, `session`) plus one setup, `input_bindings`. Most
  serialize by derive; `Pad` needs a hand-written conversion because an axis
  name is a `&'static str`.
- **Restore re-enters the real path**: a recorded `NetEvent` goes down the same
  channel the workers use, so dispatch, handler lookup, await-wake and arrival
  order are the originals.
- **Capture and restore are symmetric only for passive sources.** Input and the
  gamepad only receive, so `add_replay_resource::<T>` is one line. A socket is
  not: those subsystems hold a `replay::ExternalIo<E>`, which hands out the
  worker `Sender` only inside `start`, which does nothing while replaying. The
  check cannot be forgotten because there is no other way out; the handler stays
  registered so the recorded reply lands. `balaur_http`'s tests bind a listener
  and assert it never accepts.
- **The step is recorded per tick**, as bits, so a variable-rate recording
  replays exactly.
- **Restore is core's own first system**, ahead of every plugin, because the net
  pump also runs in `First`. A driver sets the `ReplayFeed` resource before the
  tick rather than adding a system.
- **Sources are registered, not hardcoded** (`add_replay_source`): core knows
  nothing about input, and a source missing from an older file is left alone, so
  recordings outlive the subsystems they predate.

### What determinism is still missing

1. **Nothing forces gameplay into `fixed_update`** — a game may still simulate
   from `update`. A lint is the likely answer.
2. **Hot reload is a hazard by design.** A verified or networked run must
   disable it or record the reload as an event.
3. **A run-time `scene.instantiate` reuses the file's ids**, so two copies
   collide. The fix is a minted prefix per instance, held back because the
   editor mirrors the game scene through that call and addresses it by id.
4. **Nothing forces a new subsystem to use `ExternalIo`.** A lint on
   `std::sync::mpsc::channel` outside core is the next guard.

### Rollback

`core::snapshot` keeps a registry of sources (`save` to JSON, `load` back) and a
`SnapshotRing` of the last N ticks.

- Core snapshots only `Transform`s and the RNG; every subsystem registers its
  own (physics through serde, scripts through `save_state`/`load_state`). Core
  does not snapshot components: re-adding a `body3d` would rebuild the body and
  discard its velocity.
- Scripts are hybrid — define the two methods, or get plain fields captured
  (functions and userdata skipped, nodes as entity bits). `self.node` is never
  captured; the host owns that binding.
- **Restore puts the node set back**: the `nodes` source records id, name,
  parent, components and script, and registers first, so it frees what was
  spawned and respawns what was freed before any other source writes.
  Components go back through the same `apply` a scene file uses. Keyed by
  `StableId`, with the entity index as fallback for a hand-built tree. The id
  counter is snapshotted, so a re-run mints the id the first run did.
- `rollback::Session` owns the tick, the ring and a journal of who pressed what
  when. A missing input is predicted by repeating the last one — right for a
  held button, wrong exactly when it changed; a disagreeing real input restores
  and re-runs, an agreeing one costs nothing.
- The re-run is invisible because the clock is a snapshot source, and
  `is_resimulating` is checked by `ExternalIo::start` alongside `is_playing`.
- Scripts see `rollback.input(player)` and `rollback.is_resimulating()`.
- The session steps at `App::fixed_step` and takes no `dt`: the substep
  accumulator lives outside the snapshot, so a variable step could re-run a tick
  with a different number of steps.
- `core::netsession` puts the session behind a `Transport`. Every datagram
  carries the last twelve ticks of that player's input: inputs are never
  retransmitted, so one dropped would be a permanent divergence — repeating
  costs a few bytes and lets one packet repair every gap behind it. Invisible on
  loopback, obvious under `transport::Faulty`.
- Payloads arrive through `PeerTraffic`, an `ExternalIo` behind a `session`
  replay source, so a recorded session replays with nothing on the other end.
  `NetSession::stats` (round trip, loss from sequence gaps, bytes) is an
  observer. `Session::stale_inputs` counts inputs older than the ring — a
  divergence to resync out of, not a log line.
- Not done: replicating state. Inputs are all that cross.

## Settings

One registry (`core::settings`) addressed by path:
`physics/solver_iterations`, `editor/appearance/theme`.

- The path is the storage — `editor/appearance/theme` is `[editor.appearance]
  theme` — so there is no second registry of tables. The manifest follows:
  `application/name` is `[application] name`.
- A setting uses the same property spec a component does, so settings and
  inspector rows render from one code path. Plugins declare from `build`, a game
  from a script with `settings.define`, and nothing distinguishes them after.
- Two scopes: `Project` (in `project.toml`, shipped, version-controlled) and
  `Editor` (the person's data directory, never the project). Fault injection is
  editor-scoped so packet loss cannot be committed into a manifest, and a test
  asserts it.
- Writing back touches only the paths a scope declares, so comments and
  unrelated tables survive.
- Editor settings apply as they change; project settings do not. One read only
  at startup says `applies = "restart"`.

## Networking and state sync

Transport, session and rollback are built; replication is not.
`PLAN-networking.md` has the ordered steps, `PLAN-sessions.md` the script API,
`PLAN-gamend.md` the server, `PLAN-voice.md` voice.

| Primitive a replication layer needs | Where it already is |
| --- | --- |
| Cross-machine identity | `StableId` — survives rename, reparent, reload |
| Generic property read/write | `ComponentRegistry`'s `get` / `patch` hooks |
| A wire schema | Component schemas: every property declares a `type`, with defaults |
| A tick clock | `Engine::tick`, `FIXED_DT`, `App::set_fixed_dt` |

The schema is the notable one: Godot ships a separate `SceneReplicationConfig`
because its nodes have no property schema. An encoder generated off the registry
replicates third-party components with no code in the plugin.

- **Which architecture.** Lockstep costs O(players) and nothing for world size,
  but needs full save/restore and cannot hide information; server-authoritative
  delta replication tolerates nondeterminism, allows join-in-progress and
  resists cheating, at bandwidth that scales with the world. Build the second,
  factored so the first falls out — both need the same identity, property access
  and clock.
- **Delta encoding** waits on change detection: a generation counter per
  `(entity, component)`, as `sprite` already does for UVs. Per tick and observer,
  send the changed properties since that observer's last ack against a ring of
  baselines; quantisation comes off the schema's range.
- **Predicting and reconciling.** The client applies its own input on the tick
  it is pressed and holds it pending until a delta acks that tick; the
  correction restores and replays what is pending, over one node. Unowned nodes
  are not predicted — drawn a send interval behind and interpolated between the
  two states bracketing render time. A visible correction decays out of the
  render transform and never feeds back into simulation. A server testing a hit
  rewinds to the tick the shooter saw.
- **Transport**: HTTP, WebSocket and WebTransport over QUIC (off by default),
  plus Gamend. One crate per protocol — a game wanting `http.request` should not
  compile a QUIC stack — and nothing shared beneath them, since `ExternalIo`,
  `Transport`, `Handler` and the token space are in core.
- WebSocket is TCP, so one lost packet stalls everything behind it: right for
  turn-based, wrong for twitchy. Its worker speaks frames itself so
  `permessage-deflate` can set the reserved bit tungstenite's message API
  refuses, and so a listener can unmask client-to-server frames.
- `core::transport::Transport` is the seam: one reliable ordered channel,
  unreliable datagrams, and a `receive` polled once per tick. It is shaped by
  QUIC, so a transport missing a guarantee fakes it — the websocket sends a
  datagram reliably, which costs latency and is never wrong. Opening a link goes
  through `ExternalIo::start`.
- `balaur_webtransport` is where a datagram is genuinely one: `quinn` behind the
  trait via `web-transport-quinn`, on a worker thread with a current-thread
  runtime, so no runtime goes near the tick. Its reliable channel is one
  bidirectional stream, so a payload travels behind a four-byte length. QUIC is
  always TLS: a self-signed certificate pinned by hash, which is what a browser
  accepts; a shipped server passes real files.
- The target is QUIC/WebTransport as the *single* transport, native and
  in-browser — not for raw speed, but to avoid maintaining a native UDP path and
  a browser WebRTC path. WebRTC data channels stay a fallback for browser P2P
  without a relay; raw UDP is never exposed to scripts.
- **The script-facing shape** is declarative in the scene (`replicate = {
  authority = "owner", components = [...], mode = "on_change" }`) and
  signal-shaped in scripts. RPCs address a node by `StableId`, never by path.

## UI: egui for scripts

`balaur_ui` exposes an immediate-mode `ui` module drawn with egui inside the
kiss3d window; scripts implement `draw_ui`, run once per frame in the egui pass.

- The bridge keeps a stack of the `Ui` being built, so a script composes layouts
  like Rust egui code.
- Widgets take colors per call, so themes live in scripts and hot reload. Fonts
  load from `<project>/fonts/*.ttf`; `heading` / `ui` / `mono` always exist.
- Dimensions are design pixels: `ui.set_scale(f)` multiplies every metric and
  the queries divide back. HiDPI is separate and automatic; `set_scale` is
  comfort zoom (⌘+/⌘−).
- `ui.code_editor` is an editable syntax-highlighted buffer, persistent per id.

## The editor is a game

`editor/` is a regular Balaur project: one node whose scripts draw the shell —
five personas, a fixed five-region layout, the command palette as the single
overlay, dark and light token sets.

- `balaur edit <project>` puts the game's path in `engine.args()`. The editor
  parses the game's scene TOML into a document, mirrors it as a real subtree
  (`scene.instantiate` with `scripts = false`) so the viewport shows the actual
  scene and physics builds the actual bodies (paused until play), and writes
  edits back with `toml.encode`/`fs.write`.
- Play attaches the game's scripts to the mirrored nodes and unpauses; stop
  rebuilds the mirror from the document. Editing an editor script hot reloads
  the editor.
- Reusable bindings it is built on: `fs`, `toml`, `scene.instantiate`,
  `engine.args`, `engine.reload_script`, `require` with in-place module reload,
  `log.recent`, `physics.set_paused/clear/set_sleeping_allowed`,
  `render.set_background/set_grid/draw_line/shape/color`, `render.set_camera`,
  `render.camera_pose`, `render.set_camera_input`.
- The gizmo follows steadyum's: an orange bounding-box frame, a face drag
  translating in that face's plane, a rectangle at each face centre scaling
  along its normal, corner brackets scaling uniformly, and a rotation ball whose
  depth-biased rings win the pick over the face behind. Hit-testing and drag
  math are screen-space script in `gizmo.rn`; the backend inhibits the orbit
  camera while the gizmo is hot.
- The Animate persona edits an engine clip: `anim.rn` is an adapter over the
  `assets` and `animation` modules, and the Timeline scrubs the node's actual
  player, so what is previewed is what ships. The clip lives inline
  (`[nodes.animation.library]`) or in `animations/<node>.toml`; **Save as file**
  and **Make inline** are exact inverses because both hold byte-identical
  documents. Preview goes through `animation.define`, so the engine's parser
  reads the table before it is written.
- The clip belongs to the *player* — the selection or its nearest ancestor
  carrying `animation` — so keying a bone writes a track on the character's
  clip, addressed by path.
- `rig.rn` draws bones as Godot's diamonds and grows a chain by clicking;
  `polygon.rn` turns Godot's UV window into four viewport modes (Points,
  Polygons, UV, Weights). Every edit records history and then pushes the
  document's polygon onto the live node, so undo and the saved scene agree.
- The editor's engine resolves files against the editor's own root, so
  `model.build_mirror` makes a game-relative path absolute and `doc_value` makes
  it relative again.
- `balaur edit <game> --state "anim,select:Limb,tool:polygon,mode:weights,shot=out.png"`
  screenshots any state offscreen; `scripts/e2e.sh` runs the self-tests headless.
- The workspace patches `kiss3d` to `github.com/Ughuuu/kiss3d` (`mobile-fixes`)
  for the macOS ⌘ fix and iOS/Android — drop it once both ship upstream. The
  `rune` patch is not temporary: it is what puts `powf` and `powi` on `libm`.

## Input

`balaur_input` owns a backend-agnostic `InputSnapshot` (keys by name, mouse,
scroll, per-frame edges) and the `input` module. The kiss3d backend pumps OS
events into it once per frame; headless reads neutral state. One snapshot per
frame is all a replay needs.

**Actions.** A game asks for `"jump"`, not `Space`:

```toml
[input.actions]
jump = ["Space", "gamepad:South"]
move_x = ["keys:A,D", "axis:LeftStickX"]
fire = ["mouse:left"]
```

- Five binding forms: a key name, `mouse:left`, `gamepad:South`,
  `axis:LeftStickX` (`+`/`-` for one direction), `keys:A,D` (two keys as one
  axis, first negative).
- Scripts read `action_value` (-1..1), `action_pressed`, `action_just_pressed`,
  `action_just_released`. Every binding contributes and the action takes the
  value furthest from rest, so one action serves a key, a stick and a d-pad. An
  axis has a deadzone, counts as pressed past half throw, and takes its edges
  from comparing frames.
- **The raw snapshot stays underneath**: actions are recomputed in `First` after
  the pad poll and after a replay restored the recording's snapshot, so nothing
  about an action is recorded or rolled back.
- The binding table *is* recorded, in the header beside the RNG seed — a player
  who rebinds after recording would otherwise replay with a different action
  firing. `App::add_replay_setup` is that seam, and a recording made before a
  plugin declared its setup still plays.
- `input.bind` saves every rebinding to `input.toml` in the user data directory;
  `reset_bindings` goes back to the project's. An undeclared action reads 0 and
  warns once.

**Gamepads, motion and rumble.**

- Pads are polled inside the tick (`First`), not by the windowed backend: a
  controller is not a window event, and a headless run sees one. Not while
  replaying — the recorded pads were just restored.
- **Two readers, one pad, no overlap.** gilrs reads buttons and axes; gyro,
  accelerometer and touchpad come from `sensors.rs` over raw HID (DualSense and
  DualShock 4, USB and Bluetooth, offsets from Linux's `hid-playstation.c`),
  matched by vendor and product and told apart by order. A backend covering both
  readings replaces both rather than joining them.
- Rumble is output through `gilrs::ff`, so a recording never carries it — the
  script asks again on replay. `can_rumble` *is* recorded, because a script may
  branch on it. No gyroscope, no HID and no pad all read zero.

## Showcase: the manual's pictures are a test

`scripts/showcase.sh` drives the editor offscreen with `--state`. A still is
`shot=<png>` at frame 60; a clip is `show:<name>`, a sequence in `showcase.rn`
of the calls the UI would make plus the input a person would feed, captured by
`frames=<dir>` and encoded at 30 fps. Offscreen frames advance on the fixed
step, so a clip is the same clip every time.

- Two phases: input is fed from `update`, before the game reads the snapshot;
  anything a click on the chrome would do runs from `draw_ui`, after it. Driving
  the editor from `update` rebuilt the scene under the game's own `update`.
- Input goes through `input.feed_key`/`feed_mouse`/`feed_mouse_button`, which
  call the snapshot's own feeders, so a fed frame is indistinguishable from a
  window's and the recorder records it. The pointer is drawn by `inputview.rn`
  from the snapshot rather than the OS cursor, for the same reason. egui never
  sees fed input, so a sequence puts the cursor on a control and calls the verb
  under it. The gizmo stands down while a sequence runs.
- A sequence restores the files it edited, and the script restores its own
  backup after each take.
- Taking the clips found three engine bugs: a material given by absolute path
  resolved its shader against the editor's root; stopping play cleared the
  physics world while script instances were attached (fixed by
  `node.detach_script()`); and actions were read from the editor's own
  `project.toml`, so a played game's actions read zero (fixed by
  `input.declare_actions`).

## Post-processing

`camera.post` names which screen-space passes run — `bloom`, `ssao`, `ssr`,
`dof` — with `bloom_threshold` and `bloom_intensity` beside them, since bloom is
unusable without the two numbers and the rest need none. It is a `flags`
property, so a later pass adds no boolean.

The chain is the backend's: the camera says which passes run, the renderer
decides the order, because order is a property of how passes compose. A user
pass is `docs/PLAN-shaders.md`'s remaining phase. Post-processing is an observer
like the rest of rendering.

## Camera and screenshots

`render.set_camera(ex, ey, ez, tx, ty, tz)` writes a `CameraConfig`; the backend
applies it and keeps its orbit controls in between. `render.screenshot(path)`
saves a frame to PNG.

Capture is a binding rather than a flag because *when* to capture is the
caller's business — once the level loaded, or after the hit lands. The
`ScreenshotRequest` resource stays public for a Rust embedding.

| Mode | GPU | Window | For |
| --- | --- | --- | --- |
| headless | no | no | tests, CI, servers, determinism runs |
| offscreen | yes | no | screenshots, automation, visual CI |
| windowed | yes | yes | playing and editing |

- Headless builds no renderer at all, so tests and `e2e.sh` stay fast and the
  engine runs where there is no GPU. It is also what makes the determinism
  claim checkable.
- Offscreen (`--offscreen`, `balaur::run_offscreen`) is the same renderer
  against a surface-less wgpu context: no OS window, real GPU output,
  `snap_image` reads it back. It shares one frame body with the windowed loop
  and advances at a fixed 1/60 step, so frame 90 is the same frame every time.
- A screenshot needs a GPU, not a window, so both rendering modes serve one.
  Headless says so rather than leaving no file and a zero exit code.
- The mode is a launch decision (`--headless`, `--offscreen`); nothing at
  runtime can promote a headless run.
- Rendering stays a pure observer in every mode — that is what lets the three
  agree bit for bit.
