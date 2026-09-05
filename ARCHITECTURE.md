# Balaur architecture

```
┌────────────────────────────────────────────────────────────┐
│  scripts (game code, and the editor itself)                │
│  .rn — Rune                                                │
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
rendering means zero conversion layers. There is no separate `glam`
dependency: glamx re-exports the whole of it, so `use glamx::Vec3` is the
one spelling.

The workspace pins glamx to `libm` and `scalar-math`, the two features
parry's `enhanced-determinism` turns on. They would otherwise arrive only by
feature unification from rapier, which means a crate built without physics in
its graph — `cargo test -p balaur_anim` — would silently get the platform
libm and the SIMD paths instead. Pinning them makes every build the same one.

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

Fixed stage order: `First → PreUpdate (reload pump) → Update (scripts,
animation) → FixedUpdate (scripts, physics) → PostUpdate (audio) → SceneSync
(transform propagation) → Render → Last (deferred destruction)`. Structural
changes requested by scripts (`queue_free`) are deferred to `Last` so
iteration is never invalidated mid-frame.

`FixedUpdate` is the odd one: the app drains one accumulator into whole
`FIXED_DT` steps and runs the whole stage once per step, so it may run zero
times in a fast frame and up to `MAX_SUBSTEPS` in a slow one. Everything that
decides the simulation lives there and is handed `FIXED_DT`, never the frame's
measured time. One accumulator rather than one per plugin is the point: when
physics kept its own, the number of steps it took in a given frame depended on
wall-clock jitter, so a force a script applied in `update` landed before a
different number of steps on every machine.

Order inside the stage is registration order, and core registers the script
callback first, so `fixed_update` always runs before the physics step that
frame — a force applied there lands on the step it was meant for.

## Scripting

### The seam

`balaur_script` is traits and a neutral `Value`, with one dependency
(`anyhow`) and no language in it. A subsystem declares its bindings against
`Bindings<Engine>` and never names a language; a backend implements
`ScriptHost<Engine>` and consumes those declarations. Adding Rune cost one
crate and changed nothing in physics, input, render, audio or ui.

Two things are declared once in `balaur_core` and reach every language:
`node_api.rs` (`NODE_OPS`, every node operation) and `engine_api.rs`
(`ENGINE_OPS`, the `engine`, `scene`, `log` and `assets` modules —
`docs/generated/script-api.md` counts both, so no number is written twice).
Each
backend adds
only its own call sugar, so `node.position()` in Rune comes from the same
list. A second language costs the sugar, not the operations.

`Value` carries `Nil/Bool/Int/Num/Str/Bytes/Vec2/Vec3/Color/List/Map`, plus
`Node(u64)` (entity bits, kept opaque so the crate depends on nothing) and
`Callback(id)` for a script function valid only during the binding call that
received it. Immediate-mode UI callbacks never outlive their call, so the
backend registers on entry and drops on exit — no ownership question and no
interaction with the collector.

That call-scoped lifetime is why nothing subscribes with a *closure*: a script
cannot register a handler and be handed a payload three frames later. What a
subscription addresses instead is a node and a method name, which survive a
frame boundary because the engine already knows how to resolve them.
`ScriptHost::call_on(node, method, args)`
calls a method the engine knows by name, with arguments, on one node's
instance — `on_animation_finished(name)`, a widget's `on_click`, an animation
method track. A backend puts the instance first, so a handler reads exactly
like `update(dt)` does in that language. Most such events also ship a polling
twin for scripts that would rather ask than declare a method
(`animation.just_finished(node)`, `input.just_pressed(key)`,
`http.responses()`), which is the same shape the whole engine uses: an event
is a frame-scoped snapshot. Persistent *callbacks* would need an id space with
explicit release, and nothing has needed one.

The `events` module is that shape generalised, for the case the engine is not
the one announcing: `events.subscribe(node, name)` records a node against a
name, anything calls `events.emit(name, payload)`, and a core system delivers
it as `on_<name>(payload)` at the top of the next frame's `Stage::Update`, in
emission order and then subscription order. A frame's delay is the price of
never running a handler inside the call that emitted, where it could free the
node being ticked. `events.emitted(name)` is the polling twin, and nothing is
recorded for replay: an emit comes from a script, and a replay re-runs the
script, which emits again. One more variant is load-bearing at call
sites: `Many` is *several return values*, not a list of one, so
`let (text, changed) = ui::text_field(...)` reads the way the widget is meant
to. A `Vec3` is one value with `x`, `y`, `z`, which is why `node.position()`
is read by field or unpacked rather than destructured into three names.

A project states its language with `language` in `project.toml`; absent
means Rune, and Rune is the one language this build ships. The seam is what
keeps a second one to one crate.

### Model

A script declares lifecycle functions — `init`, `update(dt)`, `on_free`,
`hot_reload` — and gets one instance per attached node, with `node` on it.

A `.rn` file declares free functions taking the instance first
(`pub fn update(this, dt)`); an object handed into a Rune function is mutated
in place, so what a script writes to `this` is what the host reads next
frame. `mod name;` pulls in `name.rn` beside the file: from disk in a dev
run, from the pack when shipped.

### Exported properties (core feature)

A script says which of its values a scene may tune, by returning a table of
defaults from `exports`:

```rune
pub fn exports() {
    #{ speed: 2.0, clockwise: true }
}
```

A node's `script` key then takes a table beside the string form, and sets
only what it overrides:

```toml
[nodes.script]
source = "scripts/spinner.rn"

[nodes.script.props]
speed = 3.5
```

At attach the host evaluates `exports` once per file, writes the defaults
onto the instance, writes the node's `props` over them, and only then calls
`init` — so `init` reads tuned values rather than asking for them. A
property the script does not export is still written, and warns: dropping it
would lose an edit, and until `#[export]` exists (`docs/PLAN-scripting.md`
phase 3) a typo has no other way to announce itself.

`props` is sparse by construction. The inspector writes an override only
where the value differs from the default and removes it when it returns, so
changing a default in the script reaches every node that never overrode it.
The type is the default's own type, which is what tells the inspector
whether to draw a drag value, a toggle or a field, and what keeps a count
declared as `2` from becoming `2.0` when it is dragged.

Everything here is scene data — the pack carries it, a digest covers what it
changed, and a replay reproduces it. `node:attach_script(path, props)` is the
same thing at run time, so a spawned node is tuned the way an authored one is.

### Prefabs (core feature)

A prefab is a scene file; a node instantiates one with the `instance` key.

```toml
[[nodes]]
id = "n_crate_b"
name = "CrateB"
instance = "scenes/crate.toml"
position = [-0.5, 5, 1.5]

[nodes.overrides."Crate/Lid"]
color = [0.9, 0.3, 0.25]
```

The prefab's roots become the node's children — the same thing
`scene::instantiate` does from a script, so there is one rule for building a
scene under a node rather than two. The node keeps its own name, transform
and components: those are the instance's, and its position moves the whole
prefab.

`overrides` is keyed by path from the instance node and holds scene keys —
components, the transform, and `script`, whose `props` merge over the ones the
prefab set, so one enemy file becomes a fast enemy and a slow one. A path that
names nothing is reported and kept rather than dropped: the prefab may have
moved the node, and the edit is still in the file.

**An override patches; it does not replace.** The prefab already described the
component whole, so an override naming one property leaves the rest alone —
`components::patch`, not the `add` a scene key normally goes through. Through
`add` an override of a collider's `half_extents` would quietly put its `kind`
back to the schema default, which is the kind of bug a scene file cannot show
you.

Identity is what makes two instances of one file distinguishable: every
`StableId` inside an instance is prefixed by the instance's own id, so the
crate lid above is `n_crate_b/n_lid` and its twin is `n_crate_a/n_lid`. The
prefix nests, so a prefab inside a prefab prefixes twice. That is the name a
replay's `--entries-at` prints and the one a replication layer will address.

A prefab that contains itself is an error naming the cycle, not a hang.
Scripts inside an instance attach when the *outermost* scene is finished, so
`init` can still look up anything the whole tree declares.

**In the editor** a prefab is placed from the palette — one command per scene
file under `scenes/`, which adds a child instancing it — and an instance's own
row carries an *open* button into the prefab, since a change meant for every
instance belongs in the file rather than in an override. Its nodes are read
from that file and shown in the tree in place, one shade quieter, so what you
see is the tree the game will build. Those rows are not the scene file's: editing one writes an `overrides`
entry on the node that named the prefab — only the properties that differ, to
match the patch — and an edit that lands back on the prefab's own value removes
it again, the same sparseness a script's exported properties get one level up.
Comparing the two needs `scene.component_properties(name, params)`, because a
prefab may write `body3d = "dynamic"` where the editor writes the whole table
and those are one component spelled two ways: the engine says what a spelling
means, and the editor compares that. Structure is refused: adding, duplicating or
deleting a node inside an instance would have nowhere in the file to live, and
the editor says which prefab to open instead.

### Hot reload (core feature)

The host watches the project directory (`notify`). On save, the file is
recompiled; a compile error keeps the previous unit running and is reported
once. Rune compiles to an immutable unit, so there is nothing to patch: the
new unit replaces the old one, instances keep their state objects, and the
next call resolves against the new code. Measured save-to-live latency is
file-watcher latency, a few milliseconds. The optional `hot_reload` hook
lets scripts migrate state shapes.

### Debugger (core feature)

Breakpoints, stepping and a call stack with locals, for scripts running in
the editor.

A pause is a parked coroutine plus an engine flag, never a blocked thread:
play-in-editor runs the game in the same process and the same script host
as the editor, so the frame loop has to keep going. `Engine::set_debug_scope`
names the subtree that is the game — the editor's mirror during play, the
whole tree under `balaur run`. While a script is paused the engine is
*frozen*: `Stage::FixedUpdate` does not run, so physics holds, and every
script host skips `update`, `call_all` and `call_on` for instances inside the
scope. Editor scripts live outside it and keep drawing. A breakpoint hits
mid-batch; the host keeps the instances that tick had not reached and runs
them on resume, before the next tick.

The seam carries four methods — `set_breakpoints`, `breakpoints`, `paused`,
`resume(StepMode)` — defaulted, so a backend without a debugger compiles
unchanged. The `debugger` script module exposes them plus `set_scope`, so
the editor's dock is plain script.

Rune: a unit with breakpoints is entered through `Vm::execute` and driven
one instruction at a time with `VmExecution::step`; a unit with none keeps
the plain call, so the no-debugger cost is unchanged. A requested line lands
on the first instruction of the next line with code, found through the
unit's debug info, and the host re-applies the requested lines after a hot
reload. A hit parks the owned execution in host state together with its
instruction pointer, which `into_owned` drops and the host restores by hand.
Step over, into and out compare the call-frame depth and the line against
those at the pause. Frames come from the execution's call stack and the
unit's function table; locals are the frame's named arguments, since Rune
keeps no names for its other slots. One limit worth knowing: an async entry
point (`task::wait`) runs as a future and cannot be parked, so a breakpoint
inside one is let through.

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
zero editor changes. Physics registers `body3d`/`collider3d` and
`body2d`/`collider2d`, rendering registers `shape3d`/`shape2d`/`sprite`
(each carrying its own `color` property: a tint means nothing without
something to tint, so it is not a component of its own), and the UI plugin
registers `widget`: game UI elements (label /
button / panel, anchored to the screen) that live as scene-tree nodes and
draw through the widget layer (`ui.set_widget_layer` scopes/toggles it —
the editor previews widgets in its viewport).

### Browsing components: tags, presets, expectations

The component list is flat by design -- a node is exactly the components on
it, and there is no type to inherit from. Three pieces of metadata make that
list navigable without adding one back:

**Tags** are facets, and several apply at once: `collider2d` is `2d` *and*
`physics`. A single category path would force a choice and bury whichever
lost, so the palette filters on tags and one component appears under both.
3D components are tagged `3d` even though their names carry no marker (D5).

**Presets** are named recipes over components, applied to one node:
`rigid_body2d` adds `body2d { kind = "dynamic" }` and `collider2d`. A preset
is not a type -- nothing records which one was used, so every component stays
free to add or remove afterwards, and an engine that recorded "this is a
RigidBody2D" would have to defend that claim forever. Plugins register the
presets for the components they own, and a project adds its own in
`presets.toml`. Anything spanning more than one node is a scene, which
`scene.instantiate` already covers.

**Expectations** are advisory: a component names others it needs *something*
from, any one of which will do, and the editor warns while none is present.
Nothing blocks, because a script may add the missing piece on a later tick.
No built-in component declares one, which is a finding rather than an
oversight: every candidate turned out to be either perfectly valid (a
`collider2d` with no `body2d` is standalone static geometry) or a sign the
component should not exist on its own -- the old `color` component could only
tint something else, which is why colour became a property of each
renderable instead. The mechanism is there for plugins, and for cases where
erroring would be too strict.

### 2D (core feature)

2D is not a separate engine; it is a second set of components over the same
scene tree. A 2D node uses the regular `Transform`: x/y translate, the
z-axis rotation spins it, x/y scale. `shape2d` (`rect`/`circle`) and
`sprite` (a textured quad) mirror
into kiss3d's 2D scene graph and render through a pan/zoom orthographic
camera (`render.set_camera_2d(cx, cy, zoom)` — zoom in logical px per world
unit; `render.camera_2d()`, `render.mouse_world_2d()` and
`render.draw_line_2d(...)` cover picking and overlays). `body2d`/`collider2d`
run in a rapier2d world that lives alongside the 3D one, with the same
`enhanced-determinism` build, fixed 60 Hz accumulator and ordered
collections; the `physics2d` module adds bodies and colliders
(`physics2d.add_body`, `physics2d.add_collider`), impulses, velocities and
`physics2d.max_contact_impulse(node)` for impact-driven gameplay. Both
dimensions carry the same surface, function for function — joints, the
character controller, the query pipeline, collision events — because a 2D game
should not be the poor relation of a 3D one.
A sprite is sized from its own image unless the author says otherwise:
`pixels_per_unit` (default 100) converts image pixels to world units, and a
`columns`/`rows` sheet sizes the quad to one cell rather than the whole
texture. That size is resolved when the sprite is set, not at draw time, so it
lands in the component a script reads back — which is why the image header is
read in every build, windowed or not. A headless run that computed a different
size from a windowed one would be a determinism bug, not a rendering detail.

Changing the frame moves UVs only and deliberately does not bump the
renderable's version, so an animation flipping frames never makes the backend
tear down and rebuild its node; changing the texture or the sheet does.

`physics.set_paused`, `is_paused`, `clear`, `set_sleeping_allowed`,
`sleeping_allowed`, `set_tuning`, `set_debug_draw` and `set_threads` span both
worlds, so editors treat "physics" as one simulation. The editor detects a 2D scene from its
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
`App::register_asset_type(name, directory, doc, parse)`, the parser returns an opaque
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
adding one would mean per-backend userdata in every backend, which is
exactly what the seam exists to avoid. The parsed side belongs to the plugin
that registered the type.

Not done, deliberately: stable ids (`uid://`, GUIDs) — paths until renames
actually hurt. Binary assets landed with pack format 2, which carries them
beside the text and verifies each by sha256.

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
quaternions, which is the only way past ±180° that takes the short way. A
`rotation` track takes the quaternion itself (`[x, y, z, w]`): that is what
an imported `.glb` clip holds, and keeping it means a re-export gives the
file's numbers back.

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
`engine.time()`, which is what the editor's first prototype did. Time folding
is floor-and-subtract rather than `%`, and span decomposition uses only
`floor`, `f32` arithmetic and `i32` parity, all IEEE-exact. Every
transcendental — the easing curves, euler↔quaternion, slerp — is `libm`'s
rather than `f32::sin`/`powf`, which route to the platform libm and differ
across OSes. glam's own `Quat::from_euler` and `Quat::slerp` land on the same
libm, because the workspace pins `glamx/libm`. Players and tweens live in
`DetHashMap`s, and the effects a step produces are applied in insertion
order.

Two scheduling consequences are load-bearing. The system runs in
`Stage::Update` *after* the script tick, so `animation.play()` takes effect the
same frame; and *before* `Stage::PostUpdate`, where physics reads `Transform`
for kinematic bodies — so an animated moving platform pushes what stands on it
with no extra wiring (`examples/hello`). And a step records its work as
deferred effects rather than applying it in place: a component's `apply` may
want the world mutably, and a script handler may play, stop, spawn or free
anything including its own node.

Deliberately not now, and not precluded: blend trees and state machines
(AnimationTree, AnimatorController) and retargeting. Skinning landed as the
section below, and it cost this crate nothing: a bone is a node, so a clip
already animates it by path.

### Skeletons and skins (core + render)

A bone is a node with a rest pose — the `bone2d` component, registered by
`balaur_core::skeleton`. There is no skeleton component: a skin names its
rig by node path (`polygon.skeleton = ".."`; `scene::find_node` learned `..`
for it) and the rig's bones are that node's descendants that carry a bone,
in tree order, which is the order a skin numbers them. That is the whole
data model, and it is why nothing in `balaur_anim` knows the word bone: a
clip on the character keys `target = "Hip/Thigh"`, the digest already hashes
every bone's transform, and the snapshot ring already restores it.

Bones live in core for the reason `mesh` does: rendering skins with them,
the editor edits them through the registry, and physics will attach to them.
The two whole-rig verbs are the `skeleton` script module, declared once
beside `engine` and `scene`: `apply_rest` (Godot's *Reset to Rest Pose*),
`overwrite_rest` (*Overwrite Rest Pose*) and `bones`.

**A skin is a `polygon`**: a textured 2D polygon whose `mesh` asset carries
positions as `[x, y]` pairs, an `internal` count of trailing interior
vertices, optional `polygons` (index loops, each ear-clipped — the Godot
rule: with interior vertices the author draws the polygons, because
automatic triangulation bends badly), `uvs`, and `skin.bones` — one weight
per vertex per bone, folded at parse to the four heaviest influences per
vertex, renormalised. Ear clipping is written out in
`balaur_core::triangulate` (`f32` arithmetic and comparisons, no
transcendentals, no dependency) so a headless test asserts the triangle
list. Absent UVs default to the texture centred on the node's origin at
`pixels_per_unit`, v downward, read off the image header in every build —
the same reason `sprite` sizes itself headless — so a polygon traced over a
sprite shows that sprite undistorted.

**The palette is computed by the engine, uploaded by the backend.**
`skeleton::joint_matrices_2d` is a pure function of the scene tree: for each
bone, current global pose times the inverse of its rest pose composed down
from the rig, carried into the skin node's own space — so a skin need not be
under its rig, and a flipped or scaled rig flips or scales its skin.
`Mat3::from_quat`, affine inverse and multiplication are arithmetic only;
the only sin/cos on the path is `libm`'s, at the rest pose. The render
backend's `Renderable2d` gains `Shape2d::Polygon` with a `PolygonMesh`
beside it, and its 2D sync hands the palette each frame to a skinning
material written in `balaur_render::skinned_2d` against kiss3d's `Material2d`
trait, with a self-contained WGSL shader — rather than kiss3d's own
`SkinnedMesh2d`, which walks a bone chain it owns and has no scale in its
model transform. A vertex whose weights sum to zero is left where it was
authored, so a rigid polygon draws through the same pipeline with an empty
palette. Rendering stays a pure observer: `skeleton::skin_positions` is the
CPU twin of the shader, and the headless test deforms a strip with it while
`examples/rig`'s headless and offscreen digests agree tick for tick.

**3D is the same model with import instead of tracing.** `bone3d` is the
same `Bone` on every axis (one struct backs both keys, with a flag so each
key reports only its own), `mesh` gains `skeleton` and `texture`, and the
`mesh` asset reads a self-contained `.glb` (`balaur_core::glb`): rigid
primitives baked into the file's frame, skinned ones left in bind space, the
first skin's joints and weights, and its inverse bind matrices re-based from
the file's scene root to the rig root so the palette formula is the 2D one
with the rest inverse swapped for the file's. `balaur import model.glb`
writes what the file cannot say as a mesh — the joint hierarchy as nodes
with `bone3d` rest poses, one mesh node at the model's root pointing at
`models/model.glb` through an inline definition, and every animation as a
clip keying those nodes by path (quaternions to euler on the same `libm`;
cubic-spline tangents dropped to linear) — as plain TOML the editor edits
like any other scene. The `gltf` crate builds without its image feature:
nothing here decodes a texture, and a pack hands the reader bytes. 3D
skinning runs on the GPU — `skinned_3d::attach` uploads the palette to a
material written against kiss3d's own trait — and the CPU twin
(`skin_positions_3d` and `skin_normals_3d` through `modify_vertices` /
`modify_normals`) stays for the two cases the GPU path cannot serve: a node
carrying its own `material`, and the headless test that asserts the
arithmetic. `examples/rig3d` is a column imported this way, swaying on the clip
its file carried. A `.gltf` reads the same way: its buffers come from the
files beside it through a `SideReader` the caller supplies (the project
reader at load, the file system at import) or from a `data:` URI decoded in
core, and `balaur import` carries the `.bin` and the base colour texture
along under `models/`.

**Modifiers have the last word.** `modifier2d` (`balaur_anim::modifier`) is
Godot's `SkeletonModification2D` as one component: `look_at` turns a bone so
its child points at a node, `two_bone_ik` bends a root, middle, tip chain so
the tip reaches one — the analytic solve, `flip` choosing the elbow's side,
a target out of reach straightening the chain toward it. It runs in
`Stage::Update` after the animation system, so a clip poses the rig and the
modifier corrects it every frame, from poses composed out of the local
transforms as they are now rather than last frame's globals. Modifiers run in
entity order, so two on one bone land the same way twice; every
transcendental is `libm`'s. The rig example's arm reaches for a target the
clip moves, which is the whole pattern: animate the target, let the solver
pose the limb.

### Binding API (plugin feature)

Wrapping a Rust crate for scripting is one call per entry point:

```rust
let m = app.script_module("physics")?;
m.function("apply_impulse", |eng, (node, x, y, z): (UserDataRef<NodeRef>, f32, f32, f32)| {
    ...
})?;
```

Argument/return conversions are inferred. Plugins can also register scene
file keys (`app.scene_key_handler("collider3d", ...)`), which are applied in
plugin registration order (deterministic, and plugins know the right order
for their own keys). `balaur_physics` is the reference implementation, and by
now a large one: bodies, colliders, joints, characters, vehicles, queries,
events and hooks, in two dimensions, at either scalar width.
`docs/generated/script-api.md` is the map of it, and `docs/PLAN-rapier.md`
carries the few things left open. Its `scalar.rs` is worth reading before any
change to the crate: it is the one place a number changes width between the
engine's `f32` and whatever rapier was built at.

### Modules and extensions (plugin feature)

A **module** is a plugin linked into the binary and switched on by a cargo
feature; an **extension** is a plugin loaded from a shared library at run time.
Every plugin in tree implements one trait, `balaur_plugin::Plugin` —
`manifest()` and `declare(&mut Registry)` — so one source ships either way.
`Registry` names resources, systems, components, presets, asset types, replay
sources and setup, and script modules, and is free of trait objects and of
generics but for the two that name a Rust type: a `fn` pointer crosses an ABI
boundary where an `impl Trait` does not.

The `app()` escape hatch is gone. Every registration a plugin performs, down
to `register_body_component` and `dim2::build`, is spelled in verbs that a C
extension could be handed, and anything missing has to be added as one rather
than reached around. `engine()` covers what a plugin reads while declaring — a
settings group, a store backend, the project's files — as a borrow rather than
the `App`, because an engine handle is what a C extension would be given
anyway, and `config()` hands it whatever `[plugins]` wrote under its name.

Every plugin that finishes registering is appended to `PluginRegistry`
(`balaur_core::plugins`), and a plugin's `requires` is checked against it at
load, so a name may reach across the module/extension boundary in either
direction. `standard_app` builds one set and hands it to `load_all`, which
orders the whole set before any of it registers: a missing requirement is
refused rather than found halfway through. Load order is sorted by name and
then by declared requirements, never by directory iteration: load order
decides registration order, which decides the simulation.

`standard_plugins` is one flat list: the engine's own five, `platform`, and
whatever modules the build linked in. There is no second trait and no second
way to load one.

A project picks from what the build linked in, through `[plugins]` in
`project.toml`: every module loads unless named `false` there, and a table
(`http = { timeout = 5 }`) means "on, with these" — the plugin reads it back
through `Registry::config`, keyed by its own name so it cannot see another's. Asking for one
nothing registered is an error naming the feature to rebuild with, and turning
off one every build has is refused rather than ignored — a setting that cannot
be honoured must not look like it was. The check runs after extensions load,
against `PluginRegistry`, so one rule covers a module and an extension without
knowing which a name is. `docs/PLAN-plugins.md` carries the reasoning and what
is still open.

**Two boundaries, because Rust has no stable ABI.** A dylib passing Rust types
across `dlopen` needs the identical compiler on both sides, and a mismatch is
undefined behaviour rather than a version error. So an extension comes in one
of two shapes, and `load_extension` picks by which symbols it exports:

| | Rust extension | C extension |
| --- | --- | --- |
| Exports | `balaur_plugin_abi`, `balaur_plugin_create` | `balaur_extension_abi` + three more |
| Crosses | `Box<dyn Plugin>`, `Registry`, `anyhow::Result` | `#[repr(C)]` only |
| Checked by | rustc + engine version + registry abi | one ABI version number |
| Written in | Rust, that exact rustc | anything: C, Odin, Zig, C++ |

The Rust path is checked by a `#[repr(C)]` fixed-size tag read *before*
anything Rust-shaped crosses, because reading a `String` out of a library built
by another compiler is the undefined behaviour the check exists to prevent. The
fingerprint is stamped by `balaur_plugin`'s build script, separately for the
host and for every plugin — the only moment either can observe which compiler
is building it.

**The Rust path has a ceiling.** Resources are keyed by `TypeId`, and a
`TypeId` hashes the crate's identity as cargo compiled it — package, features,
profile, and its dependencies' metadata. The host and an out-of-tree extension
each compile their own `balaur_core`, so `ProjectRoot` is one type to a reader
and two different keys to the resource map (measured, in
`crates/balaur_plugin/tests/extension.rs`). An extension may own state; it may
not reach state the engine owns.

**The C path does not have that ceiling**, because it computes no `TypeId` and
keeps its state behind its own `void*`. An extension exports four symbols,
receives a table of host function pointers (rather than resolving symbols back
into the executable, which is not portable), and speaks only `#[repr(C)]`
types. The header is hand-written and committed, with `_Static_assert`s on
every size and offset and a Rust test asserting the same numbers, so a layout
change fails both compiles.

The rule that makes the C boundary small: **no allocation crosses it.** Every
`BalaurValue` is a borrowed view valid only for the call it appears in, and the
host copies before returning — the same discipline `Value::Callback` already
used for script functions, generalised. There is no free function and no
allocator question. Today it reaches Tier 1 of `docs/PLAN-c-api.md`: script
modules of functions and constants. Components, systems and calling back into
script are not in it yet.

### Precompiled packs (core feature)

`balaur export` compiles every script (optimization level 2, the same
compiler configuration dev mode uses, so shipped bytecode is what was
tested), bundles scripts + scenes + manifest into a `.bpak`. Packed runs
construct no compiler and no watcher. `balaur::boot_pack(include_bytes!(...))`
turns a pack into a self-contained release binary; on platforms without
JIT/codegen restrictions this is still pure interpretation of precompiled
bytecode, so it ships everywhere (iOS included). CI cross-compiles the engine
to `aarch64-apple-ios`, `aarch64-linux-android` and `wasm32-unknown-unknown`
on every push to main, so that last sentence is checked rather than asserted.
The web target is wasm-bindgen's rather than emscripten's, because kiss3d
declares its web dependencies there and wgpu reaches WebGPU through
`web-sys`; audio is a stub on wasm because no cpal host compiles there (see
`balaur_audio`), and `balaur_webtransport` is left out of that build until it
grows the stub the other two protocols have.

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
[ template executable ][ pack bytes ][ pack length: u64 LE ][ "BPAKSELF" ]
```

ELF, Mach-O and PE all ignore trailing bytes, so the fused file still runs. On
startup the CLI reads its own executable (`balaur_core::fused`); finding a pack
means it is a shipped game, so it boots that and never looks at argv, and
finding nothing means it is the CLI. One binary is therefore the editor, the
CLI, and every game's runtime, which is why the release builds one artifact and
ship it twice.

Templates resolve from `BALAUR_TEMPLATES`, then `templates/` beside the
executable (where the editor download ships its own platform's), then the
per-user cache `<data dir>/balaur/templates/<build id>`. A missing desktop
template is offered for download into that cache from the release this build
came from, verified against the release's `SHA256SUMS` — pinned to the exact
build because a pack must only ever meet the runtime its compiler shipped
with. The prompt requires a terminal or `--download`, so CI never fetches by
surprise; `--no-download` forbids it outright.

The build knows which release it came from because `scripts/package.sh` bakes
an id into the binary (`v0.1.0`, or `nightly-<sha>`; `--version` prints it),
and every release carries a VERSION asset naming the build it holds. That
pair is what catches a Monday nightly meeting Wednesday's template, and what
drives `balaur update`: one command replacing the whole install — binary,
bundled editor, template, header, which only work as a set — from the
published releases. A tagged build follows the latest release, a nightly the
rolling tag, and a source build refuses and points at git.

A *signed* macOS game is `export --app`: a `.app` bundle with the pack in
`Contents/Resources`, because codesign seals resources but can never cover
bytes appended to a flat binary (measured: it rewrites the file and still
fails strict validation). Ad-hoc by default; `--sign <identity>` for a real
certificate.

The alternative — `include_bytes!` and a rebuild per game — needs Rust on the
machine doing the shipping, which rules out anyone who downloaded the editor.
That route still exists for people building their own binary.

Two consequences worth stating. Appending invalidates a macOS code signature,
so signing happens after export, not before. And a template that came through a
zip or a CI artifact store has usually lost its executable bit, so the exporter
sets the execute bits on its output rather than inheriting them — inheriting
produces a game that cannot be launched.

Export is also where a statically-resolved language shows. Rune resolves
`input::just_pressed` while compiling, so validating an
export needs the modules the plugins register: `balaur::build_pack` boots the
app the game would boot (`AppConfig::export`) and compiles through its host.
Compiling against a bare `rune::Context` instead — which is what it used to do
— rejects every script that touches the engine.

## Game UI: widgets are nodes

A `widget` is a component on a scene node, so a menu is a subtree and the
editor edits it the way it edits anything. What the layer did not do until
now was *read* that tree: every widget was anchored to a screen corner on its
own, and a menu meant placing each entry by pixel offset.

`row`, `column` and `panel` lay their widget children out — along the
container's direction, `gap` apart, `padding` inside its edge, `align`ed
across it. A child's own `anchor`, `x` and `y` are ignored: it is placed by
its parent, and a menu that moved when you nudged one entry would not be a
menu. A widget's parent is its nearest *widget* ancestor rather than its
parent node, so a grouping node between a panel and its buttons changes
nothing.

```toml
[nodes.widget]
kind = "panel"
anchor = "bottom_left"
padding = 6
gap = 6

# ... with button children; none of them says where it goes.
```

The layout is egui's own — `left_to_right`, `top_down`, item spacing, frame
margins — rather than a layout crate. The renderer is egui either way, this
is the same arithmetic the editor's own panels use, and it is one fewer
dependency to vet against determinism. Layout is presentation and never
touches the digest; a wrapping or percentage-based layout is what would
justify reaching for `taffy`, and nothing asks for one yet.

Only containers adopt what is under them, so a label with nodes beneath it
still leaves them anchored on their own, and a panel with no children is the
panel it always was — which is what every existing scene's panels are.

### Focus

One focused widget per screen — the thing an *accept* would activate — held
as a resource rather than on a widget, so moving it is one write. Focus walks
the widgets in the order the scene declares them and wraps, because a menu is
a ring. Whether a widget is a stop is **derived, not declared**: focus exists
to activate something, so a widget with nothing to activate is never on the
way to one. `focusable = false` can take a candidate out; it cannot put one
in. A widget that is hidden, freed or made unfocusable releases focus rather
than holding it invisibly.

An accept is a click by another name — the same `clicked`, the same
`on_click` — so a menu written for the mouse works on a pad without changing
a line. `on_focus` fires only when focus *arrives*, since a handler running
every frame focus merely stayed would be a different event and not a useful
one.

The keyboard drives focus through egui, which is where the widget layer's
input already comes from: arrows and Tab move, Enter and Space accept, and no
dependency on the input plugin is needed for a menu to work. A pad reaches
focus through `ui.focus_next()`, `ui.focus_previous()` and
`ui.activate_focused()`, and `standard_app` maps the actions `ui_next`,
`ui_previous` and `ui_accept` onto them for a project that declares those
names. That wiring lives in the assembling crate on purpose: `balaur_ui`
reads egui, which has keys but no pads, and `balaur_input` knows nothing
about widgets. The crate that knows about both is the one that joins them.

### Themes

What a widget owns is what it says and where it sits. What a `widget_theme`
asset owns is how its *kind* is drawn — `fill`, `stroke`, `stroke_width`,
`radius`, `padding`, under a table named for the kind — which is exactly the
set that was hardcoded until now.

```toml
type = "widget_theme"

[panel]
fill = "#1b1f27d0"
radius = 10
padding = 10
```

A widget takes the theme of the nearest ancestor that names one, so a screen
is themed by its root and a dialog can differ inside it. A kind the file
leaves out keeps the built-in look, so a theme that restyles buttons alone is
three lines and says only what it changes. Parsing never fails: a bad colour
is reported and dropped, because a game that would not start over one is
worse than a game that starts plain.

The split is what keeps the two from fighting. A theme cannot say what a
button reads, and a widget cannot say what every button looks like — so
neither has a value the other could contradict, and no widget property needed
a "unset" sentinel to make room for one.

## Audio buses and events

A **bus** is a gain a sound plays through, and buses form a tree:

```toml
[audio.buses]
sfx = { volume = 0.9 }
ui = { volume = 1.0, parent = "sfx" }
music = { volume = 0.6 }
```

A sound's gain is its own volume times every bus's up to the root, so pulling
the music slider moves every piece of music at once and nothing else.
`master` exists whether or not the project declares it, because there has to
be a name for "everything". `audio.set_bus_volume` re-applies to what is
*already* sounding — the difference between a mixer and a default; a handle
remembers the bus it plays on and the volume it started at so the two can be
recomputed from each other. An undeclared bus is unity rather than silence, so
a typo leaves a sound audible and findable; a parent cycle is cut and reported
rather than refused, since the rest of the mix is fine.

An **event** is a named sound, in `audio/events.toml`:

```toml
[hit]
files = ["sfx/hit1.wav", "sfx/hit2.wav", "sfx/hit3.wav"]
bus = "sfx"
volume = 0.9
```

`audio.play_event("hit")` is what a script says; which file, at what level,
through which bus is what a sound designer says, and neither has to touch the
other to be tuned. **Variations are taken in turn, not drawn at random** —
a rotation never plays the same sample twice running, which is what variations
exist to avoid, and drawing from the engine's RNG would put what a player
hears into the simulation's random stream, so a muted game and a loud one
would diverge. Where a rotation has got to is presentation: not snapshotted,
not in the digest, because which of three footsteps played is not something a
replay has to agree about.

## Positional audio

A **listener** is a node carrying the `listener` component, and it is the
ears: distance to it sets a sound's volume, and the offset across its right
sets the pan. A sound is placed either by its own component —

```toml
[nodes.Torch.sound]
file = "sfx/fire.ogg"
loop = true
positional = true
min_distance = 2.0
max_distance = 40.0
```

— or per call, `audio.play(path, #{ position: [x, y, z] })`, which is what an
impact or a one-shot at a point wants; `audio.play_event` takes the same
`position`, with the carry declared in the events file beside the files.
`audio.set_listener` covers a game whose ears are not a node, and a
`listener` node takes the position back on the next frame.

Attenuation is inverse-distance clamped at both ends: full volume inside
`min_distance`, halving with every doubling beyond it, cut at `max_distance`,
where for any sane pair it is already inaudible. Pan is equal-power amplitude
between the two channels, computed with square roots rather than the usual
sine pair — the platform's trigonometry is not the same everywhere
(`docs/DETERMINISM.md`), and this shape needs no exemption. Doppler is
OpenAL's model over velocities measured from how far the ends moved between
frames, **off unless a sound asks for it** and clamped to an octave either
way: physical doppler on a position that jitters is a warble, and on one that
teleports a screech.

Placement multiplies the bus chain rather than replacing it, so a positional
sound still answers the `sfx` slider. It runs in `SceneSync`, after the
transforms settle, so the ears and the emitters are placed from the poses the
frame will draw — and a sound is placed as it starts rather than a frame
later, or a sound the far side of the level would be heard at full volume for
one frame. rodio's own `Spatial` player is not what plays it: that folds
distance into the pan through a fixed inverse square and a pair of ear
positions, with no way to say what a game's unit is. `ChannelVolume` under the
engine's own arithmetic gives the same two channels with the numbers the scene
chose, which also makes a positional sound mono — a sound in one place has one
direction.

None of it reaches the simulation, and none of it is read back off a sink:
what the frame decided is kept on the emitter, so `AudioState::placement_of`
answers the same on a CI runner with no sound card as on a machine with
speakers, and `audio.distance_gain` / `audio.pan` give a debug overlay the
numbers actually applied.

## Localization

One `strings/<locale>.toml` per language, keys to strings, and
`strings.tr("menu.play")` to read one.

```toml
"menu.play" = "Play"
"menu.items" = { one = "{n} item", other = "{n} items" }
```

A key missing from the current locale is looked for in the project's fallback
— one hop, not a chain, because two means two files to hold in mind and the
second is always the language the game was written in. A key neither has comes
back **as itself**: visible in the game, which is how a missing string gets
noticed, where an empty label is a bug that hides.

`{name}` is replaced by the argument called `name`; a placeholder nothing was
passed for is left alone so the hole is visible to whoever fills it. An `n`
argument also picks the plural form. The rules are a named handful — English,
Romanian, the Slavic shape, French, and the languages with one form — rather
than the whole CLDR table, which is a generated artefact of some size; a
language nobody listed gets the English rule, which is also the right rule for
most of the table. A form the file does not carry falls to `other`, so a
translator who wrote two forms for a three-form language still reads sensibly.

A widget takes part through `text_key`, resolved every frame, so
`strings.set_locale("ro")` shows on the next one without anything having to be
told. Saving a strings file forgets the catalogues, so a translation edit hot
reloads like a script.

## Save games

`save.write(slot, data)` and `save.read(slot)`, per user rather than per
project, under the same `user_data_dir()` a rebinding goes to. Nothing here is
engine state — a save is whatever the game puts in the table — so what the
engine decides is only three things.

**Where it lives.** A slot is a name, and the name is checked rather than
trusted: letters, digits, `-` and `_`, so a save called `../../id_rsa` is
refused rather than written.

**That a half-written file cannot replace a good one.** The file is written
beside its target and renamed over it. The save a game writes at a checkpoint
is the one it cannot afford to find truncated.

**What version wrote it.**

```toml
[save]
version = 3                     # what this build writes
migrate = "scripts/saves.rn"    # brings an older file forward
```

`read` stamps every file with that version and brings an older one forward by
calling the migration script's `migrate_save(version, data)` once per version
step — 1→2, then 2→3 — so a migration only ever has to know how the shape
changed between two adjacent versions, never how to get from any version to
any other. A file written by a *newer* build is refused: an older build cannot
guess at it, and handing the game half a save is worse than saying so.

`ScriptHost::call_in(path, function, args)` is the seam that makes the
migration hook possible — calling a public function in a script *file*, with
no instance, for the case where the engine knows which file to ask and there
is no node to ask it of.

## Store and platform services

`docs/PLAN-apple.md`, `docs/PLAN-google.md` and `docs/PLAN-steam.md` are one
plan per store. Two things are shared, and both are decided here.

**Two modules, not one.** `platform.*` carries the verbs every store has —
sign in, the current player, unlock, progress, submit a score, read scores,
cloud read and write, presence. `apple.*` carries what only that platform has,
like Game Center's identity signature. The portable module is not a lowest
common denominator, because the native one is always beside it, and a verb the
loaded store lacks answers `unsupported` rather than pretending: Game Center
has no presence, and saying so beats an empty success.

`balaur_platform` owns the seam and one backend fills it, registered by the
store's plugin (which therefore loads after it). With none — the editor, a
desktop dev run, a replay — every call still answers, so a script written
against `platform.*` runs everywhere.

**Delivery is the engine's usual one.** Every store SDK is asynchronous and
answers on a queue the engine does not own, which is the shape `balaur_http`
already solved: the call returns an id, the answer crosses a channel, and
`ExternalIo` lands it at `Stage::First` of a later tick — recorded in that
tick's snapshot, replayed from the file with no store present, and dispatched
to the node's `on_platform` method and to whoever awaits the id.

**A write waits for its tick to settle.** `ExternalIo::start` already refuses
to reach the outside world while a recording plays or a tick is being
re-simulated. That is right for a socket and not enough for an achievement:
the run that awarded it may be the *predicted* one, and a rollback cannot take
an achievement back. So a call that changes the outside world is held until
the tick that made it can no longer be re-run, which is the tick falling out of
the rollback session's snapshot ring; `rollback::Clock` is that horizon, and a
game with no session leaves it at `u64::MAX`, where every tick is final the
moment it happens. Reads are never held — repeating one costs a round trip and
nothing else.

**One place needs Swift, and it is fenced off.** StoreKit 2 has no
Objective-C interface, so there is nothing for objc2 to bind and no Rust
crate that fits — the one that wraps it needs an Xcode project, which this
engine's export does not have. `crates/balaur_apple/swift` is therefore a
Swift package of about a hundred lines, linked by `swift-rs` (whose whole job
is the runtime search paths that differ between a Mac, a device and the
simulator), and the boundary across it is a request id out and a JSON object
back. Nothing else in the engine is Swift, and the shape of that boundary is
why nothing else has to be: a field the App Store adds reaches a script
without a line of Rust changing.

**Arrivals nobody asked for carry request 0.** A player signing out in the
OS, a subscription renewing on another device, a notification tapped, a URL
opened. They cannot go straight down the channel — a replay is answered from
the file, and anything that reached the channel meanwhile would be taken for
recorded input — so they wait in a queue until a pump that is allowed to
touch the outside world moves them across, and are thrown away by one that is
not. `Engine::next_token` starts at 1, which leaves 0 free to mean "nobody
asked", and a script hears them by subscribing a node rather than by awaiting
an id. The push token and an opened URL reach a game through the application
delegate and no other way, and the window layer owns that delegate, so the
engine proxies it — forwarding every selector it does not answer, and only
from the first call that needs it, because a delegate nobody needed is a
lifecycle bug waiting for a device to find it.

**Capabilities are export-side.** What a store grants is decided by the bundle,
not by the code: `[apple]` in `project.toml` names the identifier, the team,
the deployment target and the capabilities, and `balaur export` writes the
`Info.plist` and the `.entitlements` file from it, refusing a capability the
declared minimum OS cannot satisfy. Signing stays the developer's. The
frameworks themselves are linked into the Apple templates unconditionally,
because a template is prebuilt and a game exported onto one cannot add a
framework afterwards; an app that never calls them pays bytes and no
entitlement.

## Timings

`App::tick` measures each stage and publishes the frame whole, so a reader
never sees half of one. `engine.timings()` hands a script the last frame in
seconds — `{ frame, fixed_steps, stages, spans }` — and the editor's Profiler
dock draws it as a bar per stage against the 16.7 ms a 60 Hz frame has.
Headless runs print the same numbers as a summary with `balaur run --timings`:
mean, worst and share of a frame, which is what a budget is set against and
what `scripts/bench.py` already speaks.

Stages are coarse on purpose — they cost nine `Instant::now()` calls a frame,
beneath the noise of what they measure. Anything finer is a **named span**:
`timings::measure(eng, "physics/step", || ...)` files a duration under a name,
and only a plugin that asks for one pays for it. Core names its own
(`scripts/update`, `scripts/fixed_update`, `scripts/reload`,
`scene/transforms`), which is what turns "fixed_update costs 9% of a frame"
into "and almost none of it is script".

`fixed_steps` is reported beside the stages because `fixed_update` reading as
free usually means the accumulator had nothing to drain, not that the
simulation is cheap.

**Timings are an observer, exactly as rendering is.** Wall time is not
reproducible, so a `fixed_update` that branched on it would desync — and the
digest would say so at the first tick that parted, since no timing is
recorded, replayed or hashed. Reading them from `update`, a tool or a dock is
what they are for.

## Determinism

Cross-platform determinism is a core feature: identical inputs must produce
bit-for-bit identical simulation on every platform.

**Is Rune compatible with that? Yes, with specific measures.** Rune numbers
are IEEE-754 doubles and 64-bit integers that never mix; `+ - * /` and
`sqrt` are exactly specified by IEEE-754 and reproducible everywhere. The VM
is a single-threaded interpreter with no native codegen, so evaluation order
is fixed. The hazards, and how Balaur addresses or will address them:

| Hazard | Status |
| --- | --- |
| `f64::sin/cos/exp/pow/...` call the platform libm; results differ across OS/libc | **Done:** the engine's `math` module (`math::sin`, `math::pow`, `math::PI`, ... — `docs/generated/script-api.md` has the list) is implemented on pure-Rust `libm` (MUSL algorithms, bit-identical everywhere). Rune has no transcendentals of its own — its `f64` module stops at `sqrt`, `abs`, `floor`, `ceil`, `round`, `min`, `max`, `powf` and `powi`, and only the last two are inexact. Rune installs its stdlib wholesale and a function cannot be overridden from outside, so **our fork puts both on libm** (`patch.crates-io`); `crates/balaur_script_rune/tests/pow.rs` asserts the bits. |
| Iteration order over an object (`for (k, v) in obj`) is the hash map's, not insertion order | **Done:** the fork hashes with `XxHash64` at its default seed, so the order is the same on every target and every run — `object_iteration_order_does_not_move_between_runs` pins it. It is still not *insertion* order, so a reader wanting that sorts the keys. Upstream's `ahash` was unusable twice over: its seed comes from `getrandom` once per process, and its AES and software paths disagree. |
| A random source seeded from entropy | **Done:** Rune has no random of its own; the `rng` module (`rng::seed/random/range/int`) is an engine-owned PCG32 stream with a fixed default seed, so a fresh run is reproducible by construction. |
| Wall-clock (`engine::time()`, variable `dt`) leaking into simulation | **Done.** `Stage::FixedUpdate` runs on one app-owned accumulator at `FIXED_DT`; scripts get a `fixed_update(dt)` callback there and physics steps there, so simulation code never sees the frame's measured time. `App::set_fixed_dt` (`balaur run --fixed-tick`) additionally pins the frame itself, making an interactive run reproduce a headless one tick for tick given the same inputs. Physics and animation take the constant from `balaur_core::FIXED_DT` rather than each declaring their own 60th of a second. Input is captured once per frame into a snapshot (replayable). |

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
  curves, euler↔quaternion conversion and slerp are all written out on it,
  and glam's are libm's too because the workspace pins `glamx/libm` and
  `glamx/scalar-math`. That closes the hole `Quat::from_euler` at scene load
  (`project.rs`, `node_api.rs`) used to leave open. `scripts/house_lints.py`
  fails the build on a bare `.sin()` or `f32::sin(x)` in Rust; on the script
  side the rune fork puts `f64::powf`/`powi` on libm, so there is nothing left
  for a script to reach for.
- Dev-mode and shipped bytecode come from one compiler configuration.
- `crates/balaur_physics/tests/determinism.rs` asserts two runs match bit for
  bit, per tick rather than only at the end.
- Physics runs at `f32`. The `f64` feature was dropped on 2026-09-04: nothing
  needed the precision, and a width CI never built had already rotted twice.
- The solver threads on rayon, always — there is no feature to turn it off.
  `tests/threads.rs` asserts one thread and eight produce the same digest,
  which is what rapier's colouring buys and what lets the default thread count
  follow the machine. That default is one less core than
  `available_parallelism` reports, capped at eight, so the frame's own thread
  is not competing with the solver; `physics.set_threads` overrides it.
- The price was the three script physics hooks. rapier compiles its threaded
  pipeline only under `not(unsync-callbacks)`, so a hook may not hold an
  `Engine`: `filter_contact` and `filter_overlap` are gone, replaced by the
  `layers`/`mask` and `solver_layers`/`solver_mask` groups that already did
  that work in the broad phase, and `modify_contacts` is gone with no
  replacement. One-way platforms survive, because their axis was always data
  on the collider rather than a call into a script.
- CI records a per-tick digest on Linux, macOS and Windows and diffs the three
  (`scripts/determinism_trace.sh`, the `simulation-matches` job). `macos-latest`
  has been arm64 since macOS 14, so aarch64 is in that matrix — the runner
  label should be pinned to say so, because the architecture that could
  contract `a*b+c` into one FMA where x86-64 SSE2 does not is the one the
  claim rests on.

### The digest

A determinism claim nobody measures is a wish, and comparing whole worlds is
impractical. `balaur_core::digest` folds the simulation into one 64-bit number
per tick, and that one number is what a test asserts, what CI diffs across
platforms and what a networked peer would exchange to detect desync.

`entries` hashes in labelled slices rather than as one opaque blob, so a
mismatch is a *location*: `n_ball_7/body2d` and not "the digest changed".
`first_divergence` reports the first slice two runs disagree on, including a
node present on one side only. Floats enter as `to_bits`, because a digest that
compared them numerically would call two runs equal after they had already
drifted an ulp — which is exactly the drift that becomes a desync ten seconds
later.

Two design points carry the weight:

- **Labels are stable ids, not paths.** A slice is named by the node's
  `StableId`, which survives rename and reparent, so two peers agree on the
  name of the thing that diverged. This is the structural difference from
  Godot's `MultiplayerSynchronizer`, which resolves replicated properties by
  `NodePath` relative to the synchronizer and therefore needs a negotiated
  path cache, breaks on reparent, and fails to resolve for nodes spawned at
  run time.
- **Components are not the whole simulation.** A component's `get` reports
  what a scene author set: `body3d` reports its kind and nothing about velocity.
  Two peers can agree on every position and still be one tick from parting.
  Plugins therefore contribute what a step computes through
  `App::add_digest_source`; physics adds linear and angular velocity and sleep
  state for every body, in both dimensions.

The walk is scene-tree order, not sorted-by-id: two peers that agree have the
same tree, and a reparent is itself simulation state worth catching.

### Record and replay

A digest says two runs diverged. A recording says what they were *fed* when
they did, which is the other half of the debugger.

`balaur run <project> --record session.blr` writes JSON Lines: a header
(format, project, RNG position) then one frame per tick carrying that tick's
external input, the step it ran at, and the digest it produced. Line-oriented
so it greps, diffs and streams, and so a session that crashes still leaves
every tick up to the crash — the recorder flushes per frame.

`balaur replay session.blr --verify` re-feeds the input and re-checks the
digest, stopping at the first tick that disagrees and exiting non-zero. To go
from a tick to a *slice*, `--entries-at <tick>` prints every labelled digest
component at that tick: run it on both machines' recordings and diff, and the
answer is `n_ball_7/body2d` rather than a tick number.

Eight sources register today — `input` and `gamepad` from `balaur_input`,
`http`, `websocket`, `gamend`, `platform` and `apple` from the crate that owns
each, and `session` from core's own `netsession` — plus one setup,
`input_bindings`. Most serialize by derive; `Pad` needs a hand-written
conversion because an axis name is a `&'static str`, which serializes but has
nowhere to deserialize *to*.

Three design points:

- **Restore re-enters the real path.** A recorded `NetEvent` is pushed back
  down the same channel the worker threads use, so `pump_net_system`
  dispatches it exactly as it did when it was real — same handler lookup,
  same await-wake, same arrival order. There is no second copy of the
  dispatch to drift.
- **Capture and restore are symmetric only for passive sources.** Input and
  the gamepad receive and nothing more, so `App::add_replay_resource::<T>` is
  the whole registration — one line. A socket is not symmetric: replaying a
  session that made a request must not make the request again. Those
  subsystems hold a `replay::ExternalIo<E>`, which owns the worker channel,
  the tick's arrivals and their serialization, and hands out the `Sender`
  only inside `ExternalIo::start` — which does nothing while a replay is
  playing. The check cannot be forgotten because there is no other way to
  reach the outside. The handler is still registered either way, so the
  recorded reply has somewhere to land. `balaur_http`'s tests bind a listener
  and assert it never accepts.
- **The step is recorded per tick, not assumed.** A recording made at a
  variable frame rate still replays exactly, because each frame carries the
  `dt` it ran at as bits. Rounding it would introduce a divergence that has
  nothing to do with the bug being chased.
- **The restore is core's own first system.** It runs at the very top of
  `Stage::First`, ahead of every plugin, because the net pump also runs in
  `First` and would otherwise dispatch this tick's live traffic before the
  recording landed. A driver sets the `ReplayFeed` resource before the tick
  rather than adding its own system, which is what makes that ordering
  possible.
- **Sources are registered, not hardcoded.** `App::add_replay_source` pairs a
  capture with a restore, the same shape as `add_digest_source`. Core knows
  nothing about input; `balaur_input` registers `"input"` and serializes its
  own snapshot. A source missing from an older file is left alone rather than
  reset, so recordings outlive the subsystems they predate.

### What determinism is still missing

Ordered by what would break a networked session first:

1. **Nothing forces gameplay into `fixed_update`.** The callback exists and
   physics shares its accumulator, but a game is still free to simulate from
   `update` and see a variable `dt`. A lint is the likely answer.
2. **Hot reload is a determinism hazard.** Swapping code mid-session changes
   behaviour by design. A verified or networked run has to either disable it
   or record the reload as an event in the input trace.
3. **A run-time `scene.instantiate` reuses the file's ids.** A node a script
   spawns now gets `<authority>:<counter>` from `ids::mint`, but instantiating
   one scene twice gives both copies the ids the file declares, so they
   collide. The fix is a minted prefix per instance; it is held back because
   the editor mirrors the game scene through that call and addresses the
   result by id.
4. **Nothing forces a new subsystem to use `ExternalIo`.** Within it the
   guarantee is structural — the worker `Sender` is unreachable except through
   `start`, which does nothing while replaying — but a subsystem that opens
   its own channel bypasses all of it. A lint on `std::sync::mpsc::channel`
   outside core is the obvious next guard.

### Rollback

A snapshot is the world at one tick, captured so it can be put back: the
piece netcode needs when a late input arrives and the last few ticks have to
be re-simulated from before it. `balaur_core::snapshot` keeps a registry of
sources, each a `save` returning JSON and a `load` taking it, and a
`SnapshotRing` that holds the last N ticks.

The design mirrors the digest, on purpose. Core snapshots only what it owns --
`Transform`s and the RNG -- and every subsystem registers its own source
(physics saves its rapier worlds through serde; scripts go through the host's
`save_state`/`load_state`). Core does not snapshot components, because a
component's `apply` can have side effects: re-adding a `body3d` rebuilds the
rigid body and discards its velocity, so each subsystem restores the state it
owns instead of core replaying its inputs.

Scripts follow a hybrid contract. A script that defines `save_state` says what
matters and `load_state` puts it back; one that does not gets its plain fields
captured -- functions and foreign userdata are skipped, nodes are kept as
entity bits, and `self.node` is never captured because the host owns that
binding and reinstates it. A snapshot is therefore never trusted about which
node an instance sits on.

Restore puts the node set back, not only the state of nodes that survived.
The `nodes` source records every node's id, name, parent, components and
script, and registers first, so it frees what was spawned since and respawns
what was freed before any other source writes into an entity. A respawned
node's components go back through the same `apply` a scene file uses, which
is why core records the declaration rather than whatever a subsystem derived
from it. Everything is keyed by `StableId` rather than entity index, since a
respawned node is a new entity; the index survives as the fallback for a
node with no id, which is a tree built by hand in a test.

The id counter is snapshotted with the rest, so a rollback that re-simulates
a spawn mints the id the first run minted instead of the next one. Nothing
forces a new subsystem to register a source, the same shape of gap the digest
has.

`balaur_core::rollback` is what drives all of it. A `Session` owns the tick
number, a ring of recent snapshots and a journal of who pressed what when.
Each tick it captures the world, decides every player's input, and steps. An
input that has not arrived is predicted by repeating that player's last one,
which is right for a held button and wrong exactly when the button changed.
When the real input turns up and disagrees with what was predicted, the
session restores the tick before it and re-runs every tick since; one that
agrees costs nothing, so a healthy connection never rolls back at all.

Two things make the re-run invisible. The clock is a snapshot source, so a
re-run of tick 3 reports tick 3 rather than whatever the counter had reached.
And `rollback::is_resimulating` is set for the duration, which
`ExternalIo::start` checks alongside `is_playing` — the same choke point that
keeps a replay off the network keeps a second run of a tick off it. A
subsystem that reaches the outside by some other route has the same gap it
already has for replay.

Scripts take part through a `rollback` module with two calls: `input(player)`
for what that player is doing on the tick being simulated, real or predicted,
and `is_resimulating()` for whether this tick is a repeat. A script's own
fields come back with everything else, through the host's `save_state` and
`load_state` — the half of a snapshot core cannot see into.

The session steps the app at `App::fixed_step` and takes no `dt` of its own.
That is structural, not advisory: the substep accumulator lives outside the
snapshot, so a variable step would restore the world without restoring how
much time was owed, and a re-run could take a different number of fixed steps
than the run it repeats. At the fixed step the accumulator is back at zero
every time, so the question cannot arise.

`core::netsession` puts this session behind a `Transport`, so two engines
exchange inputs and digests over a wire. Every datagram carries the last
twelve ticks of that player's input rather than only the newest: inputs go
unreliably and are never retransmitted, so one sent once and dropped would be
lost for good, and a tick simulated on a prediction nobody ever corrects is a
permanent divergence rather than a recoverable one. Repeating costs a few
bytes and lets one packet arriving repair every gap behind it. The gap was
invisible against loopback and surfaced the moment `transport::Faulty` put
five percent loss on the link.

Payloads reach the session through `PeerTraffic`, an `ExternalIo` behind a
`session` replay source, so a recorded session replays with nothing on the
other end and a desync is reproducible from a file. `NetSession::stats`
measures each link — round trip, loss counted from sequence gaps, bytes each
way — and publishes it as `SessionStats`: an observer in the sense
`engine.timings()` is, never recorded, replayed or hashed.

An input older than the ring still cannot be answered — the tick it belongs
to is gone, so this peer keeps a prediction it now knows was wrong.
`Session::stale_inputs` counts those, because that is a divergence the caller
has to resync out of, not a log line.

What it does not do yet: replicate state. Inputs are all that cross, and a
game that cannot have every peer simulate everything needs the model below.

## Settings

Every setting the engine, its plugins and a game declare lives in one registry
(`balaur_core::settings`), addressed by a path the way Godot addresses one:
`physics/solver_iterations`, `netcode/faults`, `editor/appearance/theme`. The
first segment is the category the editor groups under, the last is the key,
and the segments between nest into headings. The path is also the storage:
`editor/appearance/theme` is `[editor.appearance] theme` in the file, so what
you read in TOML is what you write in code, and there is no second registry
of tables to keep in step with the registry of names. The manifest itself
follows the rule: what the game is lives under `[application]`, so
`application/name` is `[application] name` rather than a top-level key the
settings screen would have to special-case.

A setting is declared with the same property spec a component uses — type,
default, range, options, help — so the editor renders a settings row and an
inspector row with the same code, and a plugin adds settings the way it adds
components: from `build`, with `settings::define_group`. A game does the same
from a script with `settings.define`, and its settings appear beside the
engine's; nothing distinguishes them afterwards.

Two scopes, and the distinction is load-bearing. A `Project` setting is the
game's: written to `project.toml`, shipped, version-controlled. An `Editor`
setting is the person's: written to the editor's own data directory and never
to the project. Fault injection is editor-scoped for exactly this reason — a
developer turning on packet loss to test rollback must not be able to commit
that into a shipped manifest, and a test asserts the editor scope can never
reach `project.toml`. Writing back touches only the paths a scope declares,
so a hand-written comment or an unrelated table in the manifest survives.

Editor settings take effect as they change rather than on save, since a theme
you have to save to preview is not a preview; project settings do not, since
they belong to the game. A setting the engine only reads while starting says
`applies = "restart"` and the editor shows it.

## Networking and state sync

Transport, session and rollback are built; replication is not.
`docs/PLAN-networking.md` has the steps, in order, and says which are done.
The rest is written down because the determinism work above was done *for*
it, and because the shape of the engine already decides most of the answers.

### What the engine already gives a replication layer

Three of the four primitives exist and were not built for networking:

| Primitive | Where |
| --- | --- |
| Cross-machine identity | `StableId` — survives rename, reparent, reload |
| Generic property read/write | `ComponentRegistry`'s `get` / `patch` hooks |
| A wire schema | Component schemas: every property already declares a `type` from the closed `PROPERTY_TYPES` set, with defaults |
| A tick clock | `Engine::tick`, `FIXED_DT`, `App::set_fixed_dt` |

The third is the one worth noticing. Godot ships a separate
`SceneReplicationConfig` resource because its nodes have no property schema to
replicate against; Balaur's schemas are already a description of every
replicable property and its datatype, and the editor's inspector is already
generated from them. A delta encoder generated off the registry replicates
third-party plugin components with no code in the plugin — the same way those
components already get inspector rows for free.

### Which architecture

Two, and the engine should not have to choose for the game:

- **Deterministic lockstep / rollback.** Only inputs cross the wire, so cost is
  O(players) and independent of world size — an RTS with 2000 units costs what
  pong costs. Needs the digest for desync detection and full state save/restore
  for rollback; rapier's `serde-serialize` feature snapshots a whole physics
  world, which is the hard half. Any nondeterminism is total desync, and every
  client simulates everything, so hidden information cannot be hidden.
- **Server-authoritative delta replication.** The server simulates and sends
  state deltas; clients predict and reconcile. Tolerates nondeterminism,
  supports join-in-progress, resists cheating, and costs bandwidth that scales
  with the world.

Build the second, factored so the first falls out: both need the same identity,
the same property access and the same clock. Determinism is not wasted on the
authoritative model — it is what makes its corrections rare and small.

### Delta encoding

`components::patch` writes through hooks and nothing tracks that it did, so
change detection is the missing piece: a generation counter per
`(entity, component)`, following the pattern `sprite` already uses to avoid
rebuilding a renderable when only its UVs moved. Per tick and per observer,
send the `(StableId, component, changed properties)` set since that observer's
last acked tick, against a ring of recent states as baselines. Quantisation
comes off the schema, where a `float` property already declares its range.

### Predicting and reconciling

Rollback hides latency by re-running a tick; replication hides it by letting
the client run ahead of the server and correcting it after. The client
applies its own input to its own node on the tick it is pressed and holds
that input pending until a delta acks the tick it ran on. The correction
restores that node to what the server had and replays what is still pending
— the journal and the snapshot ring `rollback` already owns, over one node
instead of the world. Nodes the client does not own are not predicted at
all: they are drawn a send interval or two behind the newest state and
interpolated between the two that bracket the render time, so a late delta
reads as motion rather than a stall.

A correction that would move a node visibly decays out of the render
transform over a few frames instead of snapping it. That is a rendering
offset and never feeds back into simulation state, which is the rule the
three run modes rest on. A server testing a hit rewinds to the tick the
shooter saw, bounded by how far back it will look; the snapshot ring is that
history already. `docs/PLAN-networking.md` §1 "Hiding latency" names every
technique and where it sits.

### Transport

Today: HTTP (`balaur_http`), WebSocket (`balaur_websocket`) and WebTransport
over QUIC (`balaur_webtransport`, off by default), plus the Gamend backend.
One crate per protocol, because a QUIC stack is a large
dependency and a game that only wants `http.request` should not compile one;
nothing shared sits under them, since the shared parts — `ExternalIo`,
`Transport`, `Handler`, the await-token space — are already in core.
WebSocket runs over TCP, so one lost packet stalls every update behind it —
fine for turn-based play and for lockstep at low tick rates, wrong for
anything twitchy. The websocket worker speaks frames itself (tungstenite's
upgrade request and frame codec, its own protocol loop) so that
`permessage-deflate` can set the reserved bit tungstenite's message API
refuses, and so a listener can unmask the client-to-server frames tungstenite
leaves masked. Compression, upgrade headers and the request timeout are
per-call options over the `[websocket]` and `[http]` tables of
`project.toml`.

`balaur_core::transport::Transport` is the seam all of this sits behind: one
reliable ordered channel, unreliable datagrams, and a `receive` that is polled
once per tick rather than awaited, because that is how every other arrival in
the engine enters the simulation. It is shaped by QUIC rather than by what a
websocket offers, so a transport missing a guarantee fakes it — the websocket
implementation sends a datagram reliably, which costs latency and is never
wrong. Opening a link goes through `ExternalIo::start`, so a replay or a
re-simulated tick opens no socket at all.

`balaur_webtransport` is the one where a datagram is genuinely a datagram,
putting `quinn` behind that trait through `web-transport-quinn`. The crate is
async and the frame loop is not, so a link owns a worker thread with a
current-thread runtime and speaks the same two channels the websocket worker
does; no runtime goes near the tick. Its reliable channel is one bidirectional
stream, and a QUIC stream is bytes rather than messages, so a reliable payload
travels behind a four byte length — a datagram needs no framing, which is half
the reason to use one. QUIC is always TLS, so a server generates a short-lived
self-signed certificate and the client pins it by hash, which is also what a
browser accepts; a shipped server passes real files instead.

The target is QUIC/WebTransport as the *single* transport (`web-transport-quinn`,
decided over `wtransport` for a smaller server side): reliable-ordered streams and unreliable datagrams in
one protocol, native and in-browser, TLS included. The point is not raw
performance over raw UDP; it is not maintaining a native UDP path and a
separate browser WebRTC path. WebRTC data channels stay a fallback for browser
P2P without a relay, and raw UDP is never exposed to scripts: QUIC datagrams
are unreliable delivery with encryption, congestion control and a browser
story, none of which a UDP socket brings. `docs/PLAN-networking.md` §2 has
the decision per protocol. Note the wasm shape this has to fit: the emscripten
backend is thread-free and the non-emscripten wasm backend is a stub that
errors, so a browser transport is a JS bridge like `shim/emscripten_net.c`.

### The script-facing shape

Replication is declarative in the scene file and signal-shaped in scripts,
matching how components, widgets and `http.request` already work:

```toml
[[nodes]]
name = "Player"
id = "n_player"
replicate = { authority = "owner", components = ["transform", "body2d"], mode = "on_change" }
```

RPCs address a node by `StableId`, never by path — which is the whole reason
the identity work came first.

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
`ui.code_editor` widget is an editable, syntax-highlighted buffer
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
screen-space script in `editor/scripts/gizmo.rn`, and the backend inhibits
the orbit camera while the gizmo is hot. The rail tools remain as
drag-anywhere fallbacks.
The scene tree draws connector rails with collapse arrows and colored
type icons, and every row has a right-click context menu (add child,
attach script, duplicate, delete) via `ui.pill`'s `menu` callback and
`ui.menu_item`.

The Animate persona edits an engine clip. `editor/scripts/anim.rn` is an
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

The clip belongs to the *player*: the selection, or its nearest ancestor
carrying `animation` (`anim.player`). Keying a bone three levels under a
character therefore writes a track on the character's clip, addressed by the
bone's path, and a bone keys its rotation alone. Rigging is
`editor/scripts/rig.rn`: bones draw as Godot's diamonds (to the first
child bone, or `length` along `angle` for a tip), the **Rig bone** tool grows
a chain by clicking, and the inspector's Skeleton section carries the two
rest-pose verbs. Skins are `editor/scripts/polygon.rn`: Godot's UV editor
window became four viewport modes — Points (click adds an outline vertex,
⌥-click an interior one, drag moves, right-click removes), Polygons (click
vertices to close a loop), UV (the points over a translucent copy of the
texture, a helper sprite that lives under the mirror) and Weights (paint the
active bone's weight within a brush; markers shade white to black by it).
Every edit records history first and then pushes the document's polygon onto
the live node, so undo and the saved scene see the same table. The editor's
engine resolves files against the editor's own root, so `model.build_mirror`
makes a game-relative file path absolute for the mirror and `doc_value`
makes it relative again on the way back — which is what lets a texture show
in the editor at all. `balaur edit <game> --state
"anim,select:Limb,tool:polygon,mode:weights,shot=out.png"` screenshots any
such state offscreen, and `scripts/e2e.sh` runs the `rigdemo` and `polydemo`
self-tests headless on every example.

Note: the workspace patches `kiss3d` to a git fork
(`github.com/Ughuuu/kiss3d`, branch `mobile-fixes`) for the macOS
⌘-modifier fix in its egui integration and for iOS and Android support; drop
that `[patch.crates-io]` entry once both ship upstream. The `rune` patch
beside it is not temporary: it is what puts `powf` and `powi` on `libm`.

## Input

`balaur_input` owns a backend-agnostic `InputSnapshot` (keys by name, mouse,
scroll, per-frame edges) and the `input` module (`is_down`,
`just_pressed`, `just_released`, `mouse_position`, `mouse_delta`,
`is_mouse_down`, ...). The kiss3d backend pumps OS events into it once per
frame; headless runs read neutral state. Because the whole frame's input is
one snapshot, recording it per tick is all a replay system needs.

### Actions

A game asks for `"jump"`, not for `Space`. Actions are declared in
`project.toml`:

```toml
[input.actions]
jump = ["Space", "gamepad:South"]
move_x = ["keys:A,D", "axis:LeftStickX"]
fire = ["mouse:left"]
```

Five binding forms, each spelled the way the constants are: a bare key name
(`input.KEY_SPACE` is `"Space"`), `mouse:left`, `gamepad:South`,
`axis:LeftStickX` — with `axis:LeftStickY+` and `-` for one direction of a
stick — and `keys:A,D`, two keys as one axis with the first negative.

Scripts read `input.action_value` (-1..1), `action_pressed`,
`action_just_pressed` and `action_just_released`. One action serves a key, a
stick and a d-pad at once: every binding contributes a value and the action
takes the one furthest from rest, so a keyboard reads ±1 through the same
call that reads a stick's 0.4. An axis has a deadzone; past half throw an
action counts as pressed, and the edges come from comparing this frame's
value with the last, so a stick pushed past the threshold fires
`just_pressed` exactly as a key does.

**The raw snapshot stays underneath, and that is the point.** Actions are
recomputed in `Stage::First` from the snapshot and the pads — after the
gamepad poll, and after a replay has restored the recording's own snapshot —
so a recording holds the keys that were pressed and the actions are derived
from them again on the way back. Nothing about an action is recorded, and
nothing about it is state a rollback has to carry.

The binding table itself *is* recorded, in the header beside the RNG seed.
Bindings are loaded state, not simulation: a player who rebinds jump after
recording a session would otherwise replay it with a different action firing
and nothing would say so. `App::add_replay_setup` is the seam — a once-at-start
twin of `add_replay_source` for anything a plugin loads rather than simulates
— and a recording made before a plugin declared its setup still plays.

`input.bind("jump", "gamepad:North")` rebinds one action and saves every
rebinding to `input.toml` in the user data directory, where the next boot
reads it; `input.reset_bindings()` goes back to the project's. An action
nobody declared reads 0 and warns once, the same neutral answer a headless
run gives, so a script asking for one is never the thing that fails.

### Gamepads, motion and rumble

Pads are polled inside the tick (`Stage::First`), not by the windowed backend:
a controller is not a window event, and polling here means a headless run on a
desk with a pad plugged in sees it too. Not while replaying — the recorded
pads were restored moments earlier and the hardware would overwrite them.

**Two readers, one pad, no overlap.** gilrs reads buttons and axes and has no
notion of a sensor, so gyro, accelerometer and touchpad come from
`sensors.rs`, which opens the same pad over raw HID and decodes the report
gilrs discards — DualSense and DualShock 4 today, USB and Bluetooth, with the
offsets taken from Linux's `hid-playstation.c`. They are matched by the
vendor and product gilrs already reports, and two identical pads are told
apart by order. The split is deliberate and the rule is that a backend
covering both readings replaces both rather than joining them (Steam Input is
the case that will): two readings of one value is a bug with a name.

Rumble runs the other way, through `gilrs::ff`. It is output, so a recording
never carries it — the script that asked re-runs on replay and asks again.
What *is* recorded is `can_rumble`, because a script may branch on whether a
pad has motors and a replay has to take the same branch on a machine whose pad
has none. A pad with no gyroscope, a platform with no HID and an absent pad
all read the same zero.

## Showcase: the manual's pictures are a test

Every image and clip on the website comes out of `scripts/showcase.sh`, which
drives the editor offscreen with `--state` and never a hand on the mouse. A
still is `shot=<png>` at frame 60; a clip is `show:<name>` — a sequence in
`editor/scripts/showcase.rn`, one per clip, of the calls the UI would make
plus the input a person would feed, paced by frame — captured by
`frames=<dir>` every other frame and encoded by ffmpeg at 30 fps. Offscreen
frames advance on the fixed step, so a clip is the same clip every time.

A sequence runs in two phases. Input is fed from `update`, before the game
reads the frame's snapshot; anything a click on the editor's chrome would do
runs from `draw_ui`, after it. Driving the editor from `update` rebuilt the
scene underneath the game's own `update`, which then ran against half a world.

The input a sequence feeds goes through `input.feed_key`, `feed_mouse` and
`feed_mouse_button`, which call the snapshot's own feeders: a fed frame is
indistinguishable from a window's, so the recorder records it and a replay
reproduces it — the determinism clip records a fed slingshot pull and then
replays it, which is the feature it shows. The pointer is drawn by the input
overlay (`editor/scripts/inputview.rn`) — a cursor image at the snapshot's
position and a ring that spreads from every press — which reads the snapshot
rather than the OS cursor for the same reason: during a replay it is the
recording's pointer that moves. A sequence therefore puts the cursor on the
control it is about to drive and presses there, because egui takes its input
from the window and never sees the fed one: the click is drawn, and the verb
under it is called. The gizmo stands down while a sequence runs, since the
fed pointer is the game's and not the editor's.

A sequence that edits an example's files restores them on its last step, and
the script restores its own backup of every file a sequence may write after
each take — including the example scene files, since a save that lands on the
document re-serialises the TOML.

Taking the clips found two engine-side bugs worth naming: a material handed
in by absolute path resolved its shader against the editor's root rather than
the game's, so a project's materials never drew in the editor; and stopping a
play session cleared the physics world while the game's script instances were
still attached, so the next `update` queried a body that had just been
removed. `node.detach_script()` is the second one's fix — the editor lets go
of the scripts before it clears the world. A third: actions are read from the
running project's `project.toml`, which under the editor is the *editor's*, so
every action a played game asked for read zero; `input.declare_actions` lets a
host declare the actions of the project it is running, and the editor does it
when it loads the game.

## Post-processing

`camera.post` names which screen-space passes run over the 3D view — `bloom`,
`ssao`, `ssr`, `dof` — with `bloom_threshold` and `bloom_intensity` beside
them, because bloom is unusable without the two numbers and the rest need
none. It is a `flags` property: a list of names from a closed set, so a scene
says what it wants rather than carrying a boolean per effect that a later
pass would have to add to.

The chain is the backend's, not the scene's: the camera says which passes run
and the renderer decides in what order, since the order is a property of how
the passes compose rather than of the game. A user shader on that chain is
`docs/PLAN-shaders.md`'s remaining phase, and the open question there is
exactly this one — where a user pass sits among the built-in ones, and
whether the material or the camera says so.

Post-processing is an observer like the rest of rendering: it reads the
rendered image and never writes simulation state, so a headless run computes
the same world as one with bloom on.

## Camera and screenshots

The `render` module exposes `render.set_camera(ex, ey, ez, tx, ty, tz)`
backed by a `CameraConfig` resource; the kiss3d backend applies changes and
keeps its interactive orbit controls in between. `render.screenshot(path)`
saves a frame's framebuffer to PNG, for debugging, CI golden images, and
editor thumbnails later.

Capture is a binding rather than a CLI flag because *when* to capture is the
caller's business: a tool screenshots once the level has finished loading or
after the hit lands, not at a frame number chosen before the game started.
The flag it replaced could only say "frame 60". The `ScreenshotRequest`
resource behind it stays public, so a Rust embedding can insert one directly.

### Three run modes, not two

| Mode | GPU | Window | What it is for |
| --- | --- | --- | --- |
| headless | no | no | tests, CI, servers, determinism runs |
| offscreen | yes | no | screenshots, automation, visual CI |
| windowed | yes | yes | playing and editing |

Headless builds no renderer at all. That is the point of it: `cargo test` and
`scripts/e2e.sh` stay fast, and the engine runs where there is no GPU adapter
to be had. It is also what makes the determinism claim checkable — a headless
run computes exactly what a windowed run computes, and the pack digests in
e2e prove it.

Offscreen (`--offscreen`, `balaur::run_offscreen`) is the same renderer
against a hidden window: kiss3d opens a surface-less wgpu context, so there is
no OS window and no display server, but there is real GPU output and
`snap_image` can read it back. It shares one frame body with the windowed
loop rather than keeping a second copy in step, and it advances at a fixed
1/60 step — there is nothing to pace against without a display, and an
automation client asking for frame 90 should get the same frame every time.

A screenshot needs a GPU, not a window, so both rendering modes serve one.
Headless cannot, and says so: a request nobody consumed used to leave no
file, no message and a zero exit code, which reads as success to a script.

Which mode runs is a *launch* decision — the loop is built before any script
executes — so it stays a CLI flag (`--headless`, `--offscreen`) while what to
capture stays an API. Nothing at runtime can promote a headless run to a
rendering one.

Rendering stays a pure observer in every mode. Nothing a backend draws, and
nothing a screenshot reads, may feed back into simulation state — that is
what lets the three modes agree bit for bit.

## Roadmap

What does not exist yet, each with the plan that says how it will. A row with
no plan names what is already in the tree to build on, which is where its plan
starts. The website's roadmap is the short form of this list, and
`CHANGELOG.md` is what each release added.

The engine is at **0.1.0**. One version for the whole workspace; a release is
a `v*` tag whose notes are that version's changelog section, and cutting one
means moving `Unreleased` into a dated section, bumping
`[workspace.package] version`, and striking from this table whatever the
release finished.

| Item | Plan |
| --- | --- |
| A green `main`, and the holes the 2026-09-04 audit found: widget clicks and the animation player outside the digest and the snapshot, colliders that outlive their node, a bus mix that never reaches `master`, an `fs` module with no root, a `f64` switch that leaves `f32` on | `docs/PLAN-hardening.md`. Phase 0 is CI: every push since 2026-09-03 has failed it, for six independent reasons, and nothing blocks a red push |
| Tilemap editor, curve editor and onion skin | `docs/PLAN-editor.md` §6 |
| The editor in a browser: the same wasm build with a canvas under it, files in the browser's origin-private filesystem, and a project kept on Gamend | `docs/PLAN-web-editor.md`. The editor is a Balaur project and the engine already builds for `wasm32-unknown-emscripten` headless, so the missing halves are a surface on a canvas and a backend under `fs` |
| `#[export]` on a script constant, in place of the `exports` table | `docs/PLAN-scripting.md` |
| Stable asset ids, rename refactoring, sprite-sheet import | `docs/PLAN-scenes-and-assets.md` phases 3, 5, 6 |
| Asset streaming: a load that runs off the tick, a scene added to one already running, and an asset dropped when nothing names it | no plan yet; a pack is held whole in memory today. `ExternalIo` already lands background work on a tick boundary and records it for replay, `assets` caches by reference, and `pack` hashes every entry |
| Extensions tier two: components, systems, calling back into scripts | `docs/PLAN-c-api.md` "What Tier 1 does not do" |
| Soft bodies, tearing, fluids, granular materials | `docs/PLAN-physics.md` — waiting on the solvers landing in Rapier itself |
| Named collision layers, solver tuning carried in a recording's header, and a solver that actually threads | `docs/PLAN-rapier.md`. Everything else rapier has is built. A `f64` world is **not planned**. `parallel` is built but inert until the physics hooks stop calling scripts, which §4 costs out |
| Animation blending, blend trees, state machines | `docs/PLAN-animation-and-resources.md`. 3D IK exists for a reduced-coordinates joint chain (`physics3d.solve_ik`); an animation-side solver over a rig does not |
| A sequencer: cutscenes and cameras on a timeline, with tracks that call something rather than only move it | no plan yet; `balaur_anim` already samples clips onto nodes, with twelve easing curves in four modes, and the editor has the timeline dock |
| Tile-map occluders, and normal-mapped lit sprites | `docs/PLAN-rendering.md`. 2D lights and shadows are built; these are the two that plan deferred |
| Shadow maps in 3D, and baked lightmaps | no plan yet; directional, point and spot lights and fog already reach the 3D material shader, with no shadow pass behind them |
| Frustum and occlusion culling, level of detail, instancing of repeated meshes | no plan yet; the renderer draws every node it is handed, and `Renderable` + `GlobalTransform` is where a visibility pass would sit |
| Drawing terrain: a mesher for the `heightfield` and `voxels` assets, and tools to paint them | no plan yet; both asset types exist and physics builds colliders from them — nothing draws either |
| More than one view: split screen, a camera rendered to a texture, picture-in-picture | no plan yet; the `camera` component drives one 2D and one 3D view, and the offscreen path already renders with no OS window |
| Video playback: a movie on a texture with its audio on a bus | no plan yet; nothing decodes a container, and a frame would ride the texture path sprites use. Render-side only — a video never feeds simulation state |
| Post-process materials on `camera.post`, and Balaur's shader helpers published as a package | `docs/PLAN-shaders.md` |
| More widget kinds, as games ask for them | no plan yet, and demand-driven by design: the `widget` tree, its theme and its focus order are built, so a kind is a schema and a draw |
| Accessibility: a screen reader over the widget tree, text scaling, captions, colour-blind-safe defaults | no plan yet; the retained `widget` tree already carries text, `focusable` and a focus order, which is what a reader walks; egui can emit an AccessKit tree, localization is there for captions, and actions already rebind |
| Navigation: a navmesh baked from the scene, A* over it or over a grid, agents that avoid each other | no plan yet; nothing in the tree pathfinds. Behaviour over it stays a script's job |
| Motion and haptics beyond a PlayStation pad: Switch Pro and Joy-Con gyro, per-unit sensor calibration, adaptive triggers and light bars, waveform haptics, device motion on a phone, gamepads on iOS and Android | `docs/PLAN-input.md` §2 and steps 2-7. Rumble over `gilrs::ff`, and gyro, accelerometer and touchpad decoded from DualSense and DualShock 4 HID reports, are built |
| Gamend sessions and matchmaking, WebTransport in the browser, WebRTC data channels for browser peer-to-peer; never raw UDP, never ENet | `docs/PLAN-networking.md` §2 and steps 8, 13, 14. Native QUIC datagrams, binary frames, run-time stable ids and rollback (one machine and over a socket) are built |
| Replication and RPC, client-side prediction, server reconciliation, interpolation of unowned nodes, lag compensation, interest management, a bandwidth budget; join in progress; a client-authoritative hit is never planned | `docs/PLAN-networking.md` §1 "Hiding latency" and steps 9 to 12 |
| Voice in a session | no plan yet; the transport carries unreliable datagrams and audio has buses to mix a stream into. What is missing is a codec |
| A crash report that reproduces itself: the recording, the log and the build id in one file | no plan yet; `replay` already writes a file that re-runs a session bit for bit and `logbuf` holds the log — nothing packages the two when a game falls over |
| Store and platform services on Steam and Google Play: sign-in, achievements, leaderboards, cloud saves, rich presence, in-app purchase | `docs/PLAN-steam.md` and `docs/PLAN-google.md`. Apple already built (`docs/PLAN-apple.md`) and built what both plug into: the `platform` script module, the `PlatformBackend` seam, and the rule that a store write waits for its tick to settle. Both remaining plans start with work that is worth doing anyway — Steam's redistributable beside the executable, and an Android package id, app bundle and 16 KB page alignment |
| Signed binary releases, published benchmarks | `docs/PLAN-release.md`. The benchmarks are measured and written down: `examples/benchmark` runs the Godot suites' cases and `scripts/bench_compare.py` writes `docs/BENCHMARKS.md` (`docs/PLAN-benchmarks.md`). What is missing is a pinned runner per tag |
| Node destruction that is not quadratic in siblings, and a sibling-reorder operation | no plan yet; `docs/BENCHMARKS.md` measures the first at fifty times Godot's, and the second is why `move_child` has no row |
| Web export: a wgpu surface on a canvas, a shell page, and the pack fetched beside the wasm | `docs/PLAN-mobile-export.md` "Web". The headless wasm template already links and is packaged on every push (`scripts/package_template.sh web`); the window is what is missing, and it blocks `docs/PLAN-web-editor.md` too |
| One-click deploy: a game on a URL or on a phone from one command or one button, rather than a bundle in `dist/` | `docs/PLAN-deploy.md`. `balaur export` builds and signs; nothing sends the result anywhere, and the three store plans each end at a file on the developer's disk |
| Signing on every target, GitHub Actions a game's repository reuses (`setup`, `export-game`, `build-engine`), and an Export sheet in the editor | `docs/PLAN-actions.md`. Only the macOS `.app` signs today; the reusable workflows serve this repo's own CI; the editor's one "export" copies a replay session |
| Console export: Switch, PlayStation, Xbox | no plan yet, and not a target flag: `balaur export --target` and the pack shape travel, but each console's graphics, input and store layer is an NDA SDK that is not wgpu, winit or gilrs |
| XR: OpenXR on desktop and standalone headsets, WebXR in the browser — stereo views, tracked poses, controller and hand input | no plan yet; the wgpu device, the offscreen path that renders with no OS window, and input actions (the shape an OpenXR action set wants) are the reusable half. kiss3d owning the window and the swapchain is what is in the way, and a 60 Hz fixed tick against a 90 Hz display is the open question |
| Parallel system execution, once profiling demands it | no plan yet; the gameplay tick is serial by design |

