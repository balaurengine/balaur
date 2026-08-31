# Balaur architecture

```
┌────────────────────────────────────────────────────────────┐
│  scripts (game code, and the editor itself)                │
│  .luau or .rn — one language per project                   │
├──────────────────────────────┬─────────────────────────────┤
│  balaur_script_luau (mlua)   │  balaur_script_rune         │
│  host, hot reload, require   │  host, hot reload           │
├──────────────────────────────┴─────────────────────────────┤
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

Backends depend on core; core never depends on a backend. That direction is
what keeps core language-free, and the compiler enforces it.

## Decisions and rationale

### ECS: hecs

The data plane is `hecs` (0.11): the most widely used actively maintained ECS
that is not `bevy_ecs` (`specs` is officially in maintenance mode, `legion`
is unmaintained). It is minimal and archetype-based, and it stays out of the
way: Balaur adds its own scheduling, hierarchy, and resources on top rather
than adopting a framework's opinions.

### Math: glamx

All crates share `glamx` (glam + `Pose3`/`Rot3`), the same math used by
rapier 0.35 and kiss3d 0.46. One math crate across core, physics, and
rendering means zero conversion layers.

### The `Engine` handle

`Engine` is a cheaply clonable handle (`Rc`) over the world, resources,
command queue, and script host, with interior mutability and short borrows.
Rust systems receive it every frame; script binding closures capture it. Both
sides of the FFI see the same state with no marshalling. The engine is
single-threaded by design for now; parallelism can later live *inside*
systems (e.g. rapier's solver) without changing this facade.

### Scene tree over ECS

A Godot-style node is nothing but an entity with `Name`, `Parent`,
`Children`, `Transform`, `GlobalTransform`. Node paths (`"A/B/C"`), transform
propagation, and recursive free are core systems. Plugins hang their own
components off the same entities, so "node" is an API surface, not a data
structure with a cost.

### Frame schedule

Fixed stage order: `First → PreUpdate (reload pump) → Update (scripts) →
PostUpdate (physics, audio) → SceneSync (transform propagation) → Render →
Last (deferred destruction)`. Structural changes requested by scripts
(`queue_free`) are deferred to `Last` so iteration is never invalidated
mid-frame.

## Scripting

### The seam

`balaur_script` is traits and a neutral `Value`, with one dependency
(`anyhow`) and no language in it. A subsystem declares its bindings against
`Bindings<Engine>` and never names a language; a backend implements
`ScriptHost<Engine>` and consumes those declarations. Adding Rune cost one
crate and changed nothing in physics, input, render, audio or ui.

Two things are declared once in `balaur_core` and reach every language:
`node_api.rs` (`NODE_OPS`, the 28 node operations) and `engine_api.rs`
(`ENGINE_OPS`, the `engine`, `scene`, `log` and `assets` modules). Each
backend adds
only its own call sugar, so `node:position()` works in Luau and
`node.position()` in Rune from the same list. A third language costs the
sugar, not the operations.

`Value` carries `Nil/Bool/Int/Num/Str/Vec2/Vec3/Color/List/Map`, plus
`Node(u64)` (entity bits, kept opaque so the crate depends on nothing) and
`Callback(id)` for a script function valid only during the binding call that
received it. Immediate-mode UI callbacks never outlive their call, so the
backend registers on entry and drops on exit — no ownership question and no
interaction with the collector. One more variant is load-bearing at call
sites: `Many` is *several return values*, not a list of one, so
`local text, changed = ui.text_field(...)` reads the way the widget is meant
to. A `Vec3` is one value and arrives as a table, which is why
`node:position()` is indexed or unpacked rather than destructured into three
names.

A project picks its language with `language` in `project.toml`; absent means
Luau. One project, one language: both backends can run in a process, but a
callback minted by one is meaningless to the other, so mixing them inside a
project needs a shared id space first.

### Model

A script declares lifecycle functions — `init`, `update(dt)`, `on_free`,
`hot_reload` — and gets one instance per attached node, with `node` on it.

In Luau a `.luau` file returns a class table and each instance is a table
whose metatable `__index`es it. In Rune a `.rn` file declares free functions
taking the instance first (`pub fn update(this, dt)`); an object handed into a
Rune function is mutated in place, so what a script writes to `this` is what
the host reads next frame.

### Hot reload (core feature)

The host watches the project directory (`notify`). On save, in Luau:

1. recompile the file with the Luau compiler (fails → keep the old class
   running, report the error once);
2. evaluate the new chunk into a fresh class table;
3. swap the *contents* of the existing class table in place (clear + copy).

Because instances reference the class only through their metatable, every
live instance sees the new code immediately while `self` state survives
untouched. The swap is O(class size), microseconds in practice; measured
save-to-live latency is file-watcher latency, a few milliseconds. The
optional `hot_reload` hook lets scripts migrate state shapes.

Rune compiles to an immutable unit, so there is no class table to swap: the
new unit replaces the old one, instances keep their state objects, and the
next call resolves against the new code. A compile error keeps the previous
unit running, the same as Luau.

### Components (plugin feature)

Plugins declare named, schema-described components through
`App::register_component` (`balaur_core::components`). A schema is TOML — a
`type` per property from the closed set `PROPERTY_TYPES` (`float`, `bool`,
`string`, `enum`, `vec2`, `vec3`, `color`, `asset`), defaults, enum options, a
`shorthand` marker for scalar scene syntax and a `readonly` marker the
inspector honours — plus apply/get/remove hooks.
`parse_schema` validates all of that at registration and panics naming the
component and the property, so a malformed schema fails at boot rather than
at the first inspector row. A tagged-union property is always called `kind`,
which is why `type` is the datatype and never the discriminant
(`docs/NAMING.md` N6); a `color` property takes either channel floats or
`#rrggbb` / `#rrggbbaa`, expanded once for every component before `apply`
sees it. Writing a component has two verbs, and the difference matters:
`set_component` merges the params over the *schema defaults*, so it rewrites
the whole component, while `components::patch` merges over what the
component's own `get` reports, so writing one property leaves the others
where they were. Animation and the editor's inspector both want the second
one — animating `shape/radius` with the first would silently reset
`half_extents`. One registration buys: the scene-file key, the
runtime node API (`node:set_component / get_component / has_component /
remove_component / component_names`, `scene.component_types()`,
`scene.component_schema(name)`), and full editor support — the editor's
"Add component" palette and its property inspectors are generated from the
registry, so third-party plugin components are addable and editable with
zero editor changes. Physics registers `body`/`collider` (3D) and
`body2d`/`collider2d` (2D), rendering registers `shape`/`shape2d` and
`color`, and the UI plugin registers `widget`: game UI elements (label /
button / panel, anchored to the screen) that live as scene-tree nodes and
draw through the widget layer (`ui.set_widget_layer` scopes/toggles it —
the editor previews widgets in its viewport).

### 2D (core feature)

2D is not a separate engine; it is a second set of components over the same
scene tree. A 2D node uses the regular `Transform`: x/y translate, the
z-axis rotation spins it, x/y scale. `shape2d` (`rect`/`circle`) mirrors
into kiss3d's 2D scene graph and renders through a pan/zoom orthographic
camera (`render.set_camera_2d(cx, cy, zoom)` — zoom in logical px per world
unit; `render.camera_2d()`, `render.mouse_world_2d()` and
`render.draw_line_2d(...)` cover picking and overlays). `body2d`/`collider2d`
run in a rapier2d world that lives alongside the 3D one, with the same
`enhanced-determinism` build, fixed 60 Hz accumulator and ordered
collections; the `physics2d` module adds bodies and colliders
(`physics2d.add_body`, `physics2d.add_collider`), impulses, velocities and
`physics2d.max_contact_impulse(node)` for impact-driven gameplay.
`physics.set_paused`, `is_paused`, `clear`, `set_sleeping_allowed` and
`sleeping_allowed` span both worlds, so editors treat "physics" as one
simulation. The editor detects a 2D scene from its
components and switches automatically: 2D grid, pan/zoom camera, a 2D
selection gizmo (translate box, corner scale brackets, per-axis edge
handles, rotation ring), click-picking in world space, and 2D collider
overlays — see `examples/angrynerds` for a complete 2D game.

### Assets (core feature)

An asset is game content several nodes want to *share* and that is too big to
inline in a scene node: an animation clip today, a prefab or a material later.
`Engine::resource::<T>()` already means the typemap of engine singletons, so
content takes the other word rather than putting two concepts under one
(`docs/NAMING.md` D1). This is what Godot calls a Resource.

**One rule: a string is a reference, a table is a definition.** Every
asset-typed component property accepts either, which is the whole
external-versus-inline duality in one sentence, and it is implemented once in
`components::properties` — so every asset type anyone ever registers inherits
it with no code. An inline table is recorded in the cache and rewritten to the
reference that now names it, which means an `apply` hook only ever sees a
string.

Three reference forms an author writes, and no more: `"animations/hero.toml"`
is the whole file, `"animations/hero.toml#run"` is the entry named `run`
inside it, and `"#hero_idle"` is an `[[assets]]` block in the same scene
document (Godot's `[sub_resource]`, scoped to the scene that declares it). A
fourth, `"#!<hex>"`, is written by nobody: it is what an inline definition is
rewritten to, keyed by a digest of its content so that re-applying a component
is idempotent rather than leaking a cache entry per frame. It takes a
`#entry` suffix for the same reason a file does, because an inline table is as
free to be a library of named assets as a file is — which is what lets the
editor's Make inline be the exact inverse of its Save as file.

Core never learns what an asset *is*. A plugin registers a parser with
`App::register_asset_type(name, parse)`, the parser returns an opaque
`Rc<dyn Any>`, and the plugin downcasts it — the same trick the typemap plays.
`AssetState` is the `DetHashMap` cache keyed by the *resolved* reference, so
two spellings of one asset cannot produce two entries and iteration order is
fixed; `AssetTypeRegistry` is the parser table, appended at plugin build and
read-only afterwards. Sharing is the default, and `assets.duplicate` opts out
with a private copy. Cache keys are hashed with a hand-written FNV-1a over
bytes and over TOML structure, because `std`'s hashers specify nothing about
their output and this key has to agree across platforms.

Source resolution costs no new plumbing: `ScriptHost::scene_source` already
resolves project-relative TOML from the pack in a packed run and from disk
otherwise, and `Pack::build` already bundles every `.toml`, so a shipped game
resolves an asset exactly as a dev run does. A `ProjectRoot` + `std::fs`
fallback covers Rust-only apps and tests, where no script host is running.

A reference that does not resolve **warns**; the node and the rest of the
scene load. Component `apply` errors are fatal to `instantiate_scene`, so the
alternative is one bad clip path taking a whole scene down — which is what an
editor opening someone else's project hits, and what Godot logs rather than
refuses. A definition *table* that does not parse is still an error, reported
where it was written.

`asset` is one more entry in the component schema's closed `PROPERTY_TYPES`
set (`library = { type = "asset", asset = "animation_clip" }` — `type` is the
datatype key, so the asset's own type name needs a second key, N6), and it
buys a reference row in the generated inspector for every asset type anyone
registers. The `assets` script module is `load`, `duplicate`, `exists` and
`reload`. `assets.load` hands a script the resolved *definition table*, not
the parsed object: `Value` has no variant for an opaque `Rc<dyn Any>` and
adding one would mean per-backend userdata in Luau and in Rune, which is
exactly what the seam exists to avoid. The parsed side belongs to the plugin
that registered the type.

Not done, deliberately: stable ids (`uid://`, GUIDs) — paths until renames
actually hurt — and binary assets, since a pack holds text today.

### Animation (plugin feature)

`balaur_anim` is a plugin like any other: one asset type (`animation_clip`),
one component (`animation`), one script module (`animation`), one system in
`Stage::Update`. It depends on `balaur_core` and on no other plugin crate, and
a test reads its own `Cargo.toml` and fails if one ever appears.

A clip is a length, what to do with time past that length
(`none | loop | pingpong`), and a list of tracks. A track names a node
relative to the player, a property, an interpolation (`step | linear |
cubic`), and keys. A library document is the same file with entries under
`[clips.<name>]`, addressed `hero.toml#idle` — the asset layer cuts the entry
out and the entry inherits the document's `type`, so there is no second asset
type and nothing in core knows the word `clips`.

**Property addressing reuses the component registry.** `position`,
`rotation_euler` and `scale` are the transform; anything else is
`component/property`, resolved through `ComponentRegistry` and
`components::patch` when the pose is written. That is why `color/rgba`,
`shape/radius` and `widget/x` animate — and why a third-party plugin's
components animate the day they are registered — while `balaur_anim` depends
on neither the renderer nor the UI. A track with no `property` at all is a
method track: its keys are moments at which to call a script method through
`ScriptHost::call_on`. Absence is the honest spelling for that, since every
alternative puts a fake property into the closed set the inspector reads.

Rotation keys are authored as euler radians — the same spelling as
`set_rotation_euler`, and readable in a diff — and interpolated as
quaternions, which is the only way past ±180° that takes the short way.

The sampler is `(clip, time) -> pose`, pure and reachable with no `Engine` at
all, so blend trees and state machines can compose samples later without the
data model moving under them. That is the one thing §3.7 of the plan asked the
implementation to preserve, and it is also what makes tweens possible:

**A tween is a generated clip.** `animation.tween(node, spec)` builds a `Clip`
from a list of steps, capturing start values off the node as it goes, and runs
it through the same sampler, the same pose write and the same fixed step. One
sampler, two authoring front-ends, and no second interpolation path anywhere
in the crate. Steps are sequential by default; `parallel = true` joins a step
to the one before it (DOTween's `Join`, Godot's `parallel()`), and the next
non-parallel step waits for the whole group (Godot's `chain()`). `to` is
absolute, `by` relative, `from` explicit, `target` tweens another node, and
`interval` and `call` sequence among the property steps. A tween dies with its
node.

The spec is data rather than Godot's chainable builder
(`create_tween().tween_property(...).set_ease(...)`), because a handle object
with methods means new userdata in *every* backend and again in every future
language — precisely what declaring once against the seam exists to avoid. A
handle travels as an integer, and `animation.stop` takes a node or a handle
rather than growing a second destruction verb (N1). The data form also makes a
tween serialisable, so the editor can author one and it hot reloads, which
none of the engines surveyed offers.

Easing is 12 transitions in 4 modes with Godot's names and Godot's shapes, so
ported curves behave. Every curve maps 0→0 and 1→1 exactly, asserted with
`assert_eq!` rather than a tolerance: without explicit endpoint guards `expo`
and `elastic` land a float short of where the tween was sent.

**Determinism.** The determinism table promises that a variable dt never
reaches simulation, and animation keeps that the same way physics does:
playback advances on its own 1/60 accumulator inside `AnimationState`, capped
at four catch-up steps, and the sampler only ever sees `FIXED_DT` — never
`engine.time()`, which is what the editor's Luau prototype did. Time folding
is floor-and-subtract rather than `%`, and span decomposition uses only
`floor`, `f32` arithmetic and `i32` parity, all IEEE-exact. Every
transcendental — the easing curves, euler↔quaternion, slerp — is `libm`'s
rather than `f32::sin`/`powf`, which route to the platform libm and differ
across OSes; `glam`'s `Quat::from_euler` and `Quat::slerp` are avoided for the
same reason. Players and tweens live in `DetHashMap`s, and the effects a step
produces are applied in insertion order.

Two scheduling consequences are load-bearing. The system runs in
`Stage::Update` *after* the script tick, so `animation.play()` takes effect the
same frame; and *before* `Stage::PostUpdate`, where physics reads `Transform`
for kinematic bodies — so an animated moving platform pushes what stands on it
with no extra wiring (`examples/hello`). And a step records its work as
deferred effects rather than applying it in place: a component's `apply` may
want the world mutably, and a script handler may play, stop, spawn or free
anything including its own node.

Deliberately not now, and not precluded: blend trees and state machines
(AnimationTree, AnimatorController), skinning and retargeting — which need
skinned meshes in the render backend first, though the track model already
fits `bone/3/rotation`.

### Binding API (plugin feature)

Wrapping a Rust crate for scripting is one call per entry point:

```rust
let m = app.script_module("physics")?;
m.function("apply_impulse", |eng, (node, x, y, z): (UserDataRef<NodeRef>, f32, f32, f32)| {
    ...
})?;
```

Argument/return conversions are inferred. Plugins can also register scene
file keys (`app.scene_key_handler("collider", ...)`), which are applied in
plugin registration order (deterministic, and plugins know the right order
for their own keys). `balaur_physics` is the reference implementation.

### Precompiled packs (core feature)

`balaur export` compiles every script (optimization level 2, the same
compiler configuration dev mode uses, so shipped bytecode is what was
tested), bundles scripts + scenes + manifest into a `.bpak`. Packed runs
construct no compiler and no watcher. `balaur::boot_pack(include_bytes!(...))`
turns a pack into a self-contained release binary; on platforms without
JIT/codegen restrictions this is still pure interpretation of precompiled
bytecode, so it ships everywhere (iOS included). CI cross-compiles the
headless engine to `aarch64-apple-ios`, `aarch64-linux-android` and
`wasm32-unknown-emscripten` on every push to main, so that last sentence is
checked rather than asserted. Emscripten and not `wasm32-unknown-unknown`,
because Luau is C++ and needs a libc to compile against; audio is a stub on
wasm because no cpal host compiles there (see `balaur_audio`).

A pack is written in sorted key order, which makes exporting the same sources
twice give the same bytes — on the same machine and on any other. CI exports
every example twice per platform and diffs the digests across the matrix.
Before that, the maps were hashed and ten exports of a three-script project
produced five different files, which quietly rules out reproducible builds and
any "did the content change?" check downstream.

### Fused executables (how a game ships)

A pack is data, so `balaur export` on its own produces something that still
needs an engine beside it. `--target <platform>` instead appends the pack to a
*runtime template* — the engine binary CI publishes for that platform — and
writes a trailer:

```text
[ template executable ][ pack bytes ][ pack length: u64 LE ][ "BPAKFUSE" ]
```

ELF, Mach-O and PE all ignore trailing bytes, so the fused file still runs. On
startup the CLI reads its own executable (`balaur_core::fused`); finding a pack
means it is a shipped game, so it boots that and never looks at argv, and
finding nothing means it is the CLI. One binary is therefore the editor, the
CLI, and every game's runtime, which is why the release builds one artifact and
ship it twice.

The alternative — `include_bytes!` and a rebuild per game — needs Rust on the
machine doing the shipping, which rules out anyone who downloaded the editor.
That route still exists for people building their own binary.

Two consequences worth stating. Appending invalidates a macOS code signature,
so signing happens after export, not before. And a template that came through a
zip or a CI artifact store has usually lost its executable bit, so the exporter
sets the execute bits on its output rather than inheriting them — inheriting
produces a game that cannot be launched.

Export is also where a statically-resolved language differs from a dynamic
one. Luau resolves `input.just_pressed` at run time, so a file compiles on its
own. Rune resolves `input::just_pressed` while compiling, so validating an
export needs the modules the plugins register: `balaur::build_pack` boots the
app the game would boot (`AppConfig::export`) and compiles through its host.
Compiling against a bare `rune::Context` instead — which is what it used to do
— rejects every script that touches the engine.

## Determinism

Cross-platform determinism is a core feature: identical inputs must produce
bit-for-bit identical simulation on every platform.

**Is Luau compatible with that? Yes, with specific measures.** Luau numbers
are IEEE-754 doubles; `+ - * / sqrt floor` are exactly specified by IEEE-754
and reproducible everywhere. The VM is single-threaded and, as configured
here (no `luau-jit` feature), interpreted, so evaluation order is fixed.
The hazards, and how Balaur addresses or will address them:

| Hazard | Status |
| --- | --- |
| `math.sin/cos/exp/pow/...` call the platform libm; results differ across OS/libc | **Done:** `balaur_script_luau::det` rebinds them to pure-Rust `libm` implementations (MUSL algorithms, bit-identical everywhere). Luau's compiler turns `math.*` into fastcalls that bypass the global table, so `compiler()` also disables those builtins via `Compiler::disabled_builtins`; a test proves O2-compiled code routes through the rebound table. Remaining hole: the `^` exponent operator calls the C `pow` inside the VM and cannot be intercepted from the embedding API; simulation code must call `math.pow` instead (lintable later). |
| `pairs()` order on tables with table/userdata keys depends on pointer values | Rule: simulation-affecting iteration uses arrays or string/number keys. The editor can lint for this; an engine-provided ordered map is planned. |
| `math.random` seeded from entropy | **Done:** `math.random`/`math.randomseed` are rebound to an engine-owned PCG32 stream (fixed default seed, so a fresh run is reproducible by construction), also exposed as the `rng` module (`rng.seed/random/range/int`). |
| Wall-clock (`os.clock`, variable `dt`) leaking into simulation | Physics and animation each step on their own fixed 60 Hz accumulator; input is captured once per frame into a snapshot (replayable). **Planned:** `fixed_update(dt)` script callback and a fixed-tick mode where `update` sees a constant dt. |
| Luau native codegen (JIT) may change float behavior | Not enabled; deterministic builds keep it off. |

Engine-side measures already in place:

- rapier's `enhanced-determinism` feature is enabled workspace-wide.
- Order-sensitive Rust iteration uses `balaur_core::collections::DetHashMap`
  (`IndexMap` + unseeded FxHasher: insertion-order iteration, O(1) lookups,
  the same recipe as parry under `enhanced-determinism`), never bare
  `HashMap` iteration.
- Scene instantiation order and scene-key application order are deterministic.
- Rendering and audio are pure observers of simulation state; a headless run
  computes exactly what a windowed run computes (`t = 1.12` touchdown in the
  demo, in both modes and from the exported pack).
- Transcendentals on a simulation path use the `libm` crate rather than
  `f32::sin`/`powf`, which route to the platform libm: `balaur_anim`'s easing
  curves, euler↔quaternion conversion and slerp are all written out on it
  rather than taken from `glam`. Remaining hole: `balaur_core` still builds a
  rotation with `Quat::from_euler` at scene load (`project.rs`,
  `node_api.rs`), so an authored `rotation_euler` is one platform-libm call
  away from the animated one.
- Dev-mode and shipped bytecode come from one compiler configuration.
- `crates/balaur_physics/tests/determinism.rs` asserts two runs match bit for
  bit; CI on multiple platforms should compare the same digest across OSes.

## UI: egui for scripts

`balaur_ui` exposes an immediate-mode `ui` module rendered with egui
inside the kiss3d window. Scripts implement a `draw_ui` lifecycle method;
the backend runs it once per frame inside the egui pass. The bridge keeps a
stack of the `Ui` being built: panel/container calls push their child `Ui`
around the script callback, so a script composes layouts exactly like Rust egui
code. Widgets take colors per call, so complete themes (the editor ships
the handoff's dark and light token sets) live in scripts and hot reload
with them. Fonts load from `<project>/fonts/*.ttf` when present, with the
named families `heading` / `ui` / `mono` always available.

All `ui` dimensions are design pixels: `ui.set_scale(f)` multiplies every
metric (fonts, heights, panels) and the query functions divide back, so
scripts author against the design's pixel values at any zoom. HiDPI is
separate and automatic (egui pixels-per-point from the window scale
factor); `set_scale` is comfort zoom on top, ⌘+/⌘− in the editor. The
`ui.code_editor` widget is an editable, Luau-syntax-highlighted buffer
(persistent per id) used by the editor's Script persona.

## The editor is a game

`editor/` is a regular Balaur project: one node, whose scripts draw the
whole shell through the `ui` module, following the persona-based design
handoff (five personas, fixed five-region layout, command palette as the
single overlay, dark/light token sets). `balaur edit <project>` runs the
editor with the game's path in `engine.args()`; the editor parses the
game's scene TOML into a document model, mirrors it as a real node subtree
(`scene.instantiate` with `scripts = false`) so the viewport shows the
actual scene and physics builds the actual bodies (paused until play), and
writes edits back with `toml.encode`/`fs.write`. Play attaches the game's
scripts to the mirrored nodes (by absolute path) and unpauses — the actual
game code runs against the actual scene inside the editor; stop rebuilds
the mirror from the document. The Script persona is a real editor: buffers
persist per file, ⌘S writes to the game project and hot reloads the code
(live, when playing). Editing
any editor script hot reloads the editor itself.

Tooling bindings the editor is built on (all reusable): `fs` (read / write
/ list, project-relative), `toml` (parse / encode), `scene.instantiate`,
`engine.args`, `engine.reload_script`, `require` with in-place module hot
reload, `log.recent` (the ring buffer behind the Output dock),
`physics.set_paused/clear/set_sleeping_allowed`,
`render.set_background/set_grid/draw_line/shape/color`,
`render.set_camera` (zoom pill on the orbit camera), and
`render.camera_pose` + `render.set_camera_input`, which power the viewport
tools. The selection gizmo follows steadyum's design: an orange bounding-box
frame; dragging anywhere on a face translates in that face's plane (with
Snap); the small floating rectangle at each face's center scales along the
face normal; corner brackets scale uniformly; and the rotation ball — three
axis rings, radius capped relative to camera distance, rendered always on
top via depth-biased lines — rotates and wins the pick over the face behind
it. The hot element tints deep blue. Hit-testing and drag math are
screen-space Luau in `editor/scripts/gizmo.luau`, and the backend inhibits
the orbit camera while the gizmo is hot. The rail tools remain as
drag-anywhere fallbacks.
The scene tree draws connector rails with collapse arrows and colored
type icons, and every row has a right-click context menu (add child,
attach script, duplicate, delete) via `ui.pill`'s `menu` callback and
`ui.menu_item`.

The Animate persona edits an engine clip. `editor/scripts/anim.luau` is an
adapter, not a second animation system: it maps the scene document's
`[nodes.animation]` onto the `assets` and `animation` script modules, and the
Timeline dock's ruler scrubs by seeking the node's actual player, so what the
editor previews is what ships. Lanes are the clip's own tracks — a component
track or a method track draws like a transform track, because the clip is the
engine's. The clip is held inline in the scene (`[nodes.animation.library]`, a
table is a definition) or in `animations/<node>.toml` (a string is a
reference), and **Save as file** / **Make inline** move it between the two;
both forms hold byte-identical documents, which is what makes them exact
inverses. Preview goes through `animation.define`, so the table being edited
is parsed by the engine's parser even before it is written to disk.

Note: the workspace patches `kiss3d` to the local checkout at
`~/work/kiss3d` for the macOS ⌘-modifier fix in its egui integration
(committed there as `fix: report the macOS Command (Super) key in egui
modifiers`); drop the `[patch.crates-io]` entry once that ships upstream.

## Input

`balaur_input` owns a backend-agnostic `InputSnapshot` (keys by name, mouse,
scroll, per-frame edges) and the `input` module (`is_down`,
`just_pressed`, `just_released`, `mouse_position`, `mouse_delta`,
`is_mouse_down`, ...). The kiss3d backend pumps OS events into it once per
frame; headless runs read neutral state. Because the whole frame's input is
one snapshot, recording it per tick is all a replay system needs.

## Camera and screenshots

The `render` module exposes `render.set_camera(ex, ey, ez, tx, ty, tz)`
backed by a `CameraConfig` resource; the kiss3d backend applies changes and
keeps its interactive orbit controls in between. `ScreenshotRequest` (CLI:
`balaur run --screenshot out.png`) saves a frame's framebuffer to PNG, for
debugging, CI golden images, and editor thumbnails later.

## Roadmap

1. `--!strict` type checking surfaced in editor diagnostics; palette-driven
   node search.
2. Editor: drag gizmos in the viewport, scene saving UX (autosave/undo),
   multi-scene tabs, text editing in the script view (today it is a viewer;
   editing happens in your $EDITOR and hot reloads).
3. Prefabs/sub-scenes. The asset layer landed, so a prefab is another
   registered asset type rather than new machinery; what remains is the scene
   key that instantiates one and the rule for overriding properties on the
   instance.
4. Binary assets (audio, textures, fonts) in packs, with content hashing. TOML
   assets already pack and resolve by reference — a packed run loads a clip
   exactly as a dev run does — but a pack holds text only, so binary content
   needs a second section and a hash per entry.
5. Editor: selection, inspector (reflect components to Lua), gizmos, scene
   saving (world → TOML serialization).
6. `fixed_update` script callback + fixed-tick mode (determinism table).
7. Parallel system execution inside the data plane once profiling demands it.
