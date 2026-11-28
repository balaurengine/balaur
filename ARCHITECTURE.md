# Balaur architecture

The decisions, one line each. What is not built yet is `docs/ROADMAP.md`, and
how each of those gets built is `docs/PLAN-*.md`; `docs/NAMING.md` governs the
names.

```
┌────────────────────────────────────────────────────────────┐
│  scripts (game code, and the editor itself)  .bs           │
├────────────────────────────────────────────────────────────┤
│  balaur_script_bs: host, hot reload, `use` files, debugger │
├────────────────────────────────────────────────────────────┤
│  balaur_script: the seam — Module, ScriptHost, Value.      │
│  Traits only. No language; anyhow and serde.               │
├────────────────────────────────────────────────────────────┤
│  modules declared once, reaching every language:           │
│  engine, scene, node, assets, animation, input, physics, ..│
├──────────┬─────────┬────────┬────────┬───────┬─────────────┤
│ physics  │ render  │ audio  │ input  │ anim  │ your plugin │
│ (rapier) │ (kiss3d)│ (rodio)│ (gilrs)│ (clip)│             │
├──────────┴─────────┴────────┴────────┴───────┴─────────────┤
│  geometry · asset · net · project · debug: shapes, models  │
│  and pictures, sessions, packs, the debugger               │
├────────────────────────────────────────────────────────────┤
│  balaur_core, the kernel: hecs ECS + scene tree + assets + │
│  scheduler. Names no scripting language, decodes no image. │
└────────────────────────────────────────────────────────────┘
```

Backends depend on core; core never depends on a backend. The compiler enforces
the direction, and it is what keeps core language-free. The five crates around
the kernel depend on it and on nothing above it,
and a test keeps the kernel's manifest free of them and of the image, glTF and
mesh crates they took. `balaur::builtins()` composes an app: every crate's
registration steps and script modules, in one order, handed to `App::new` as
a `Builtins`; a bare app gets the kernel's own.

## Foundations

- **ECS: hecs 0.11** — minimal, archetype-based, the most used maintained ECS
  that is not `bevy_ecs`. Scheduling, hierarchy and resources are Balaur's own,
  and so is what `bevy_ecs` adds over hecs: one door for
  every structural write (`Engine::spawn`, `insert`, `remove_one`,
  `despawn`, …; the `world-write` lint fails a `world_mut()` outside the
  kernel); change ticks stamped by the registry's writes
  (`components::ticks::changed_since`); lifecycle calls and required
  components declared on the derive (`#[component(on_add = f, requires =
  [..])]`); commands made at each stage's end (`Engine::defer*`); typed
  messages (`App::add_messages`, `MessageCursor`); observers inside
  `events::announce`; relationships (`relationships`, the scene tree's
  `Parent` and `Children`); schedules (`schedule::System`, `Set`); typed systems
  (`system_param`, `App::add_typed`); and batches on the thread pool
  (`Query::par_for_each`).
- Change stamps are not in a snapshot. A restore clears them, so a reader
  that last ran before it hears every component as changed.
- **Math: glamx** (glam + `Pose3`/`Rot3`), shared with rapier 0.35 and kiss3d
  0.46, so there are no conversion layers. It re-exports glam whole. The
  workspace pins `libm` and `scalar-math`, the features parry's
  `enhanced-determinism` turns on, so a build without physics matches one with.
- **parry** ships in every build, so depending on it costs a game no bytes. Two
  constraints keep core backend-free: no parry type crosses a public API, and
  every crate reaching parry turns `enhanced-determinism` on, by name or through
  rapier. Taken for picking, exact ray and point queries, and the 2D shape
  tools; `Bvh` culling is planned (`docs/PLAN-views-and-culling.md`).
- Hand-written instead: `primitive` (parry returns no UVs or normals and lacks
  half the shapes), `csg` (parry has intersection only), `geometry2d`'s booleans
  on `i_overlay`, the 2D convex hull. Triangulation is `i_triangle`, named in
  `balaur_geometry::triangulate` alone (parry's ear clipper is `pub(crate)`),
  with two forms: one over the caller's own points, which refuses a loop that
  crosses itself, and one that returns the vertex a crossing or a hole needed.
- **`Engine`** is a clonable `Rc` handle over world, resources, command queue
  and script host, with interior mutability and short borrows: Rust systems and
  script bindings see one state, unmarshalled. Single-threaded by design;
  parallelism goes inside a system.
- A native thread sleeps until its work arrives: a socket on `mio` with a
  `wake::Commands` waker, blocking work on a `task::Pool`, a deadline through
  `wake::at`. It never wakes on a timer to look.
- HTTP shares one `ureq::Agent` per engine, and `[http] max_parallel` workers,
  six by default, take requests from a queue. A websocket handshake blocks;
  the socket then turns non-blocking, and a woken thread reads until
  `WouldBlock`.
- The door, a query and a get are hecs's own, so they run at its speed. A
  system costs 2.7 ns a run, a typed one 5.3 ns (Apple M1, release,
  2026-10-31). A property written through the registry, as a script, a scene
  or the editor writes it, costs 117 ns with its change stamp and its log
  entry. A Rust system's write through a typed query's `Tracked<T>` costs
  1.8 ns and stores the system's change tick in the component's tick column
  (`Ticked<T>`). A plain `&mut T` costs 0.4 ns, unseen by change detection. Finding
  what changed scans that column, as bevy's `Changed` does.
- **Scene tree over ECS.** A node is an entity with `Name`, `Parent`,
  `Children`, `GlobalTransform`. Its local `Transform` is the `transform`
  component, `[nodes.transform]` in a file, and a node without one sits at its
  parent. Paths, transform propagation and recursive free are core systems;
  plugins hang components off the same entities, so "node" is an API surface,
  not a cost.
- A scene's `parent` is an id or a path of names; ids resolve first, and a path
  may not climb out with `..`. The editor normalizes paths to ids on load, since a
  replay addresses a node by id.

**Frame schedule.** `First → PreUpdate (reload pump) → Update (scripts,
animation) → FixedUpdate (scripts, physics) → PostUpdate (audio) → SceneSync
(transforms) → Render → Last (deferred destruction)`.

- `queue_free` lands in `Last`, so iteration is never invalidated mid-frame;
  every other deferred write (`Engine::defer`) lands at the end of the stage
  that asked, or of the fixed step. A mode set with `Engine::set_mode`
  changes at the next frame's start, before `First`.
- `FixedUpdate` drains one app-owned accumulator in whole `fixed_dt()` steps: 0
  in a fast frame, up to `max_substeps()` in a slow one. One accumulator, not one
  per plugin, so the step count never depends on wall-clock jitter.
  `DEFAULT_FIXED_DT` is the default rate, and `[time] tick_hz` is what moves it,
  to one value on every platform: a recording replays and a match steps at one
  rate, so loading refuses an `[override.<tag>.time] tick_hz` (`overridable =
  false` in its spec).
- Order inside a stage is registration order unless a system says otherwise
  (`schedule::System::before`, `after`, `in_set`; sorted once, cycles named),
  and core registers the script callback first, so `fixed_update` runs before
  that frame's physics step.
- A debugger's freeze stops the whole stage. A game's own pause does not: the
  stage runs and each per-node subsystem asks `process::ticks`, because
  `process = "always"` has to let one subtree run while its sibling is held.
  Physics is one world and is held whole either way.
- A pause holds every `task` wait, an `always` subtree's too: a wait's
  token carries no node to ask.
- `SceneSync` places only what changed since its last pass: the subtree under
  a `transform` the change ticks name, a node attached or moved, an
  appearance written (`scene::appearance_written`); a restore places all. A
  write past the registry stamps with `transform::written`.
- The backend's mirror redraws the renderables that pass placed and those a
  component was written on, plus the few that move every frame (a rig,
  copies, levels of detail). A freed node reaches it, and physics and audio,
  through `scene::on_freed`.
- `SceneSync` places a node that opted into interpolation between the two
  poses the last fixed steps left it in. Render-side only: `Transform` and
  `composed_global` answer the tick, so no script and no digest sees a blend.

## Scripting

### The seam

`balaur_script` is traits and a neutral `Value`, three dependencies (`anyhow`,
`serde`, `smol_str`), no language. Subsystems declare into a `Module<Engine>`; a backend implements
`ScriptHost<Engine>`. Balaur Script (`balaur_bs`, `balaur_script_bs`) is the
one backend.

- Operations are declared once in core (`node_api.rs` `NODE_OPS`,
  `engine_api.rs` `ENGINE_OPS`) and reach every language. A second language
  costs the call sugar, not the operations.
- A module the editor's binary declares is `editor_only()`, and each function
  it registers carries the mark to the compiler (`HostSignature::editor`). The
  host judges each file by its path (`file_api::editors_script`): a file of
  the `EditorRoot` that `ToolsPlugin` names, or one under a game's or an
  addon's `editor/` folder, may call one. Any other call fails to compile. A
  game's runtime registers none, so there the name is simply missing.
- The same judgment (`Options::editor`) refuses a `use` of the editor's file
  from one that is not, and the scene loader refuses a node's script under an
  `editor/` folder (`project::plugin_attached`). The editor loads its plugins
  with `node.attach_script`, which no scene does, and an export leaves them out
  of the pack. The roots a judgment compares against are resolved once, until
  one is added or removed.
- A `Str` and a `Map` key are `SmolStr`, which holds up to 22 bytes inline.
  Almost every string crossing the seam is a component name, a property key or
  a node path, so a short one never reaches the allocator.
- `Value` is `Nil/Bool/Int/Num/Str/Bytes/Vec2/Vec3/Color/List/Map`, plus
  `Node(u64)` (opaque entity bits) and `Callback(id)`, valid only for the call
  that received it. `Many` is several return values, not a list.
- Nothing subscribes with a closure, since that lifetime is call-scoped. A
  subscription is a node plus a method name: `ScriptHost::call_on(node, method,
  args)`. Persistent callbacks would need an id space with explicit release.
- Input also answers when asked (`input.key_just_pressed(key)`): a press is a
  frame-scoped snapshot.
- `events.listen(node, name)` / `events.emit(name, payload)` deliver
  `on_<name>(payload, source)` at the top of the next `Update`, in emission
  then subscription order. The frame of delay keeps a handler from freeing the
  node being ticked. Not recorded: a replay re-runs the script, which emits again.
- One handler shape: `on_<name>(payload, source)`, `source` the node the event
  was said on. A node hears its own engine events unasked and another's after
  `events.listen`. A method naming fewer parameters is handed fewer.
- A `call` row in `bindings` calls the node its `target` path names, or its
  own node when it names none, with the event's values and then the row's
  node. A written control names its `target` node beside its `on_click`. Nothing
  searches the tree for a method, so a panel declaring a method of the same
  name cannot take another's click. A core hook of two values keeps both:
  `on_pointer_drag(dx, dy, source)`.
- A `bindings` row's `when` is a comparison of scene variables with values,
  never a script expression: what a comparison cannot say is a script.
  Convert to script writes the `.bs` a node's rows equal, one method a hook
  and one line a row, and attaches it.
- Every registry write appends its node, component and properties to the
  frame's change log, heard next frame as one `changed` event and kept only
  while someone listens. Adding, freeing, renaming and moving a node record
  themselves in that log and announce `child_added`, `renamed` and the rest
  from the same call; loads stay silent.
- `events::listen(listener, "changed", root)` hears only entries under
  `root`, so the editor hears the scene it edits and not its own widgets.
- A `states` entry is a named table of component properties.
  `node.states.set_state` patches it over the node, then calls
  `on_state_changed(from, to)`.
- A script names the events it sends with `event name;`, an item the compiler
  reads without running the file (`ScriptHost::events`), as a component names
  its own in `ComponentDef::events`. `scene.bindable_events(node)` lists both.
- `balaur check` holds every `on_<name>` method, `events.listen`,
  `events.next` and `emitted:` row to the names something announces, and an
  `emit` to its file's `event` items. The engine's names come from where each
  is declared (`events::names`): the hooks table, each component's `events`,
  and `events::declare` for what a plugin says from no node.
- Awaiting, GDScript's `await`: `task::wait(events.next(name, from?))` wakes
  with the next matching event's payload, and
  `task::wait(node.call_async(method, args))` with that method's return value,
  however many ticks it suspended for. Both wake on the next step, like a timer.
- A scene inits its scripts children before parents, Godot's `_ready` order: a
  child can always reach its parent node, while a parent reading a child's
  script state is the one that would find nothing.

### Model

- A script is a `pub class` named after its file, `door_bell.bs`'s
  `DoorBell`, with a `node: Node` field and an `impl` of its methods:
  `init()`, `update(dt: float)`, `fixed_update(dt: float)`, `on_free()`. One
  object per attached node, in the module's memory; the host holds its
  pointer, so a call copies nothing in or out, and a call back into the same
  node sees the same object. A frame's `update` runs once for each run of
  nodes in scene order that share a script (`T.update$batch`).
- `use name;` links `name.bs`, found beside the file or in a folder above it,
  and its items read as `name::item`; `use a::b;` is `a/b.bs`. A file with no
  class named after it is a module others use. Disk in a dev run, export pack when shipped.
- Each file compiles once, alone, as a unit, and every unit of the project
  is made in one instance, one memory, each on its first call. A script
  holds another node's script as its
  class, `let tree: scene_tree::SceneTree? = node.script;`, and calls it
  directly. A native build holds those units, translated to C, and links
  them the same way (`balaur_bs::c`, `runtime/native_engine.rs`); a
  debugger's probes, a profiler's counters and a browser's metering are built
  into a unit and link as ordinary ones do. A file from outside the project,
  as the editor reads a game's scripts, is a unit of the instance beside it.
- Turning a debugger or a profiler on makes the whole instance again: a
  filled slot has no way back to a unit's old code.
- A script runs on wasmtime on desktop and Android, on wasmi under a
  debugger, on the browser's own engine, and as native C where `[export]
  native_scripts` builds it. A dev run keeps what wasmtime compiled for each
  module in the game's data folder, under `bs-cache/`, across runs.
- Strings, lists, `any` cells and class objects are reference counted and
  released at every frame exit. No garbage collector runs, so a cycle leaks.
- A module traps on fuel, its memory cap, unbounded recursion, an
  out-of-bounds index and a division by zero; the host canonicalises every
  NaN it reads out.

### Enums

`enum Shape { Circle(float), Rect { w: float, h: float }, Empty }` and
`match` are lowered before any code is made (`balaur_bs/src/enums.rs`), so
neither the code generator, the C translator nor any engine knows them.

- An enum is a value struct no script can see inside: `#tag`, the variant's
  index, then every variant's fields as `Variant#field`. A field is stored
  optional unless its type already holds nil, so a constructor names only
  its own variant's fields; an `any` field of another variant is set to nil.
  The parser writes the struct with the declaration, so linking, renaming
  and a unit's declarations carry it as any struct.
- `Shape::Circle(2.0)`, `Shape::Rect { w: 1.0, h: 2.0 }` and `Shape::Empty`
  are that struct's literal, resolved after linking, so a module's enum is
  `util::Shape::Circle`.
- A `match` is `let match#n: Shape = subject;` and an `if .. else if ..
  else` on the tag, the last arm the `else` once every variant is named. In
  a statement's place, or as a block's value, the `let` and the `if` move
  into the block around it, so the async lowering sees an ordinary `if` and
  an `await` in an arm works.
- Exhaustive: a variant left out, one named twice, or an arm after `_` or a
  bare name is a compile error.
- An enum is a value, so one cannot hold itself; a recursive shape goes
  through a class or an array.

### Properties

Every `pub` field of a script's class is what a scene may tune, whatever its
type; `script.props` overrides only what it names. Visibility is the whole
rule, as Godot's `@export` is: an `internal` or unmarked field stays the
script's own, so there is no second marker to keep in step with it.

- `exports()` answers each `pub` field in declaration order with a spec in the
  component schema's words, held by the same check
  (`node_api::checked_property_spec`), so the inspector draws a script's rows
  with a component's drawers. A number, `bool` or `str` is itself; a `Node`,
  or another script's class, a `node`; `[T]` a `list`; `{str: V}` a `map`; a
  struct a `record` of its `pub` fields; a unit-only enum an `enum` of its
  names; one with fields a `variant`; `Vec2`, `Vec3` and `Color` the vectors;
  a transform a `record` of its axes and origin, as Godot draws one.
- No `pub` field of a script's class holds an `any` or a `fn`, in a list, a
  map or a struct's `pub` fields either: the compiler refuses one
  (`checks::unwritable`), so every property has a row. A struct that is only
  ever a value, a hook's reply, keeps `pub` fields of any type.
- The `///` text above a field is its `description`; the compiler keeps it in
  `Meta.public_fields`, beside each enum's variant names in `Meta.variants`.
- A field with no literal default starts at the schema's zero for its spec.
- A typed map is built in `convert`, wherever it sits, as its keys and values
  with no table of slots; its first lookup indexes it. An enum is written by
  its variant's name, or as `{ kind, ..fields }` for one with fields.
- At attach: the struct is made with its defaults, `props` written over them,
  then `init`, so `init` reads tuned values.
- A prop naming a field that is not `pub`, or no field, is dropped, and warns.
- `props` is sparse: the inspector writes an override only where it differs, so
  changing a default reaches every node that never overrode it. The default's
  type is what the inspector draws, and what keeps `2` from becoming `2.0`.
- It is scene data: packed, digested, replayed. `node.attach_script(path,
  props)` is the same thing at run time.
- A `node` export arrives as the node its path names, resolved from the
  scripted node once the whole scene exists, or nil: Godot's `@export var x:
  Node`. One that also names a `component` arrives as that node's handle for
  it, the way an `asset` property names its asset type.
- A `nodes` export is a list of them, Godot's `Array[Node]`: each path is
  resolved the way one is, and a path naming nothing is nil in its place.
- A module's constants are read where they are declared, and a save of it
  rebuilds its unit, the units built against what it declares when that
  changed, and makes the project's instance again, every object carried
  across by field name.

### Prefabs

A scene has exactly one root, as a Godot scene and a Unity prefab do: it is
what a path, a stable id and an instance all address, and a second root is an
error naming it. `instance = "scenes/crate.toml"` makes the node *be* that
prefab's root: the root's keys, components and script land on the node, under
the node's own, and the root's children become the node's.

- `overrides` is keyed by path from the instance node and holds scene keys,
  including `script.props`.
- **An override patches, it does not replace** (`components::patch`): through
  `add`, overriding a collider's `size` would reset its `kind`.
- Every `StableId` inside an instance is prefixed by the instance's id
  (`n_crate_b/n_lid`) and nests. That is what a replay prints and what
  replication will address.
- A path naming nothing is an error and is kept, so the file loses no work and
  a test run fails; a self-containing prefab is an error naming the cycle.
  Scripts attach when the outermost scene finishes.
- Overrides name paths from the instance node, `.` for the node itself, so a
  Godot path reads the same here.
- In the editor: placed from the palette, opened from its row, drawn one shade
  quieter. Editing a prefab row writes a sparse `overrides` entry, removed again
  when the value returns to the prefab's. Comparison needs
  `scene.component_properties`, since a partial table and the full one are
  one component spelled two ways. Structural edits inside an instance are
  refused, since the file has nowhere to put them.

### Hot reload

- The host watches the project (`notify`); on save the file recompiles. A
  compile error keeps the previous unit running, reported once. `balaur run`
  watches when it opens a window, and a `--headless` or `--offscreen` run only
  with `--watch`: a test's run is not edited under it. The game's data
  folders and its script cache are never heard, however they are linked.
- Writes gather into one burst, over after 60 ms of quiet or 2 s from the
  first, so a checkout of any size is one build. Only files the host
  remembers are read. `engine::watch` follows a directory outside the
  project, which is how the editor hears the game it opens.
- The new module replaces the old, and every instance of the file is carried
  across by field name, so state survives a save; a field the new struct
  dropped goes, one it added starts at its default.
- Content reloads through the same watcher, every file under the project but
  its scripts and its dot folders, in any subfolder. Each saved file's parse is
  forgotten with every entry cut from it, and a strings file forgets the
  catalogues. `assets::changed_on_disk` holds it.
- The asset generation moves when the file is one the engine derives from: a
  texture, sound, font or model, a `.toml`, `.wesl` or `.bin`. Each consumer
  re-derives. Textures are keyed by path and mtime, so saving one image
  re-decodes that image alone.
- A file only a game's script reads, `data/one.level`, moves nothing. The
  burst names it in `files_changed`, and the script reads it again. Paths are
  project-relative in a run, and spelled from the game's root in the editor.
- The editor follows the open scene's file. With nothing unsaved it is read
  again; unsaved edits stay, with a warning. A changed prefab is instanced
  again from the editor's own text, which keeps undo.
- The new text is built under a throwaway node first, in a call of its own
  (`Editor.scene_reread`). A file that does not build logs why and ends that
  call alone, with the open scene as it was.

### Scene switches

`scene.switch(path)` frees the scene the last switch built and builds `path` in
its place, every `init` running. Readying a scene and starting it are two
steps: `scene.preload(path)` readies it, and the switch starts it.
`crates/balaur_core/src/scene_switch.rs` holds it.

- **A switch replaces one scene, not a parent's children.** It remembers the
  root it built under each node and frees that root alone. A node a script made
  beside it under the tree's root stays and keeps running, which is where a
  game's services sit. `under` switches the scene below any node, so an app
  shell keeps its own nodes and swaps a content slot.
- **Every visit is fresh.** A scene's instances, tasks and fields go with its
  nodes, and a switch to the file already there is a reset. No scene is kept
  built for a return: a kept scene needs a second way in beside `init`, and
  every plugin holding state by node has to hold that scene too.
- **A preload is warm caches, not nodes.** It reads and parses the file and its
  prefabs and hands each component value to the plugin that asked, so the UI
  decodes a picture on a worker thread. A replay that never preloaded builds
  the same world. Godot's threaded load answers a status the game polls; a
  status set by a thread's timing would part two runs of one recording, so
  there is none here.
- **A pack warms what it ships.** After the first frame, one scene a frame is read
  and parsed and its pictures decoded off the frame, one a pass, so a first visit
  is no slower than the next; a memory warning ends it.
- **`scene.unload(path)` lets go** of what a preload or a pack warmed.
- **A snapshot records what stands where:** each switch's node, file and root by
  stable id, so a restore across a switch puts `scene.current` back on the
  respawned root.
- **No state outside a node.** A script keeps state in its fields, and a
  module function that needs some finds a node beside the scene, as the Gamend
  addon's `service.bs` does. A value no node owned outlived every reset and
  stood outside every snapshot, so there is no such store.

### Debugger

Breakpoints, stepping and a call stack with locals, for scripts in the editor.

- A pause is a parked coroutine plus an engine flag, never a blocked thread:
  play-in-editor shares the process and the host with the editor.
- `Engine::set_debug_scope` names the game's subtree. While paused, inside that
  scope: no `FixedUpdate`, no `update`/`call_all`/`call_on`. Editor scripts keep
  drawing. Instances the tick had not reached run on resume.
- The seam adds defaulted debugger methods (breakpoints, break-on-error,
  `request_break`, `paused`, `resume(StepMode)`), so a backend without a
  debugger compiles unchanged. The `debugger` module exposes them plus `scope`
  and `set_scope`, so the dock is plain script. `balaur::ToolsPlugin` declares
  it, and no runtime registers that plugin: `balaur_cli` does, on its `editor`
  feature, beside `project`. A game run, played or exported has no `debugger`.
- A file with breakpoints compiles with a probe per statement and runs
  stepwise; one without keeps the plain module, so the no-debugger cost is
  unchanged. A requested line lands on the next line with code, re-applied
  after a reload. Step over/into/out compare frame depth and line.

### Components

`App::register_component` takes a TOML schema and apply, get and remove hooks.
The schema gives each property a `type` from the closed set (`float`, `int`,
`bool`, `string`, `enum`, `flags`, `vec2`, `vec3`, `vec4`, `color`, `asset`,
`node`, `list`, `map`, `record`), a default, options, ranges, `readonly`,
`description`, `unit` and `group`.

- `components::Attached` is one bit per component per node, so a free runs only
  the `remove` hooks whose bits are set. Attaching state by another path warns
  in debug and gets no `remove` in release.
- `parse_schema` validates at registration and panics naming the component and
  property: a bad schema fails at boot, not at the first inspector row.
- A tagged union's discriminant is always `kind`; `type` is always the datatype
  (N6). A `color` takes floats or `#rrggbb[aa]`, expanded before `apply`.
- Three types hold others: a `list` and a `map` name what they hold in `of`, a
  `record` names one spec per field in `fields`, and each of those is a whole
  spec, so `min`, `options`, `asset` and `component` work at any depth. A
  nested spec may leave its `default` out and registration writes the type's
  zero in. A `map`'s `key` is `"string"` or `"int"`, and the file spells a
  number key `"7"` either way. A `record` is a struct a script reads as its
  own type.
- Two verbs: `set_component` merges over schema defaults (whole component),
  `components::patch` merges over the component's own `get` (leaves the rest).
  Animation and the inspector need the second: patching `collider3d/radius` with the
  first would reset `size`.
- A key the schema does not declare is refused by both verbs and by a scene
  file, naming the component and the key, since a typo would otherwise apply
  as nothing. A scene logs the refusal and loads the rest. A component that
  takes free keys says so with `App::accept_keys`: `meta` takes any name,
  `states` a state's, `widget` a screen class's and `mesh` a `morph.`
  weight's.
- A record asset is held to its schema where it is named, inline or from a
  file, so a key it does not declare refuses the component naming it. A file
  that cannot be read only warns, so a tool can still open the scene. A type
  with no schema can be held the same way: `App::register_checked_asset` takes
  a check, which closes the levels that are closed and leaves the free names
  open. `state_machine` and `tileset` have one.
- A record asset (`App::register_record_asset`) holds the rows several
  components repeat: `physics_material`, `soft_material`, `camera_effects`,
  `font` and `text_style`.
- A schema writes its rows in the inspector's order (`Schema::order`): the
  ungrouped ones first, then each group where it is first named. A group's
  name is lowercase and shared across components: `placement`, `shadows`.
- No property shows on a condition. A row only some kinds read is a
  component beside the main one, and a node without it reads the defaults.
- `#[derive(Component)]` writes the schema from a struct: a property per
  field, in field order, its doc comment the description, read and written
  by index.
- The struct holds what was asked for, and what a system works out from it
  is a cache beside it. A save spells it back less its defaults
  (`components::written_of`).
- A scene key nothing handles is an error and is kept for the build that
  handles it. A host without sound, a physics world or an extension it turned
  off inserts `LacksKeyOwner`, and there it only warns: it cannot tell that key
  from a typo.
- `meta` is the one component with no schema, so every key on it is the
  author's: values filed on a node for whoever holds the node rather than for
  its own script (Godot's `set_meta`). A scene writes `[nodes.meta]`, a script
  reads `node.meta["fade"]`, and `node.meta = { … }` replaces the table.
- A handle indexes as well as it reads fields: `node.widget["checked"]` names
  a property at run time, which is the only way to reach a schema-less one.
- One registration buys the scene key, the node API (`set_component`,
  `get_component`, `has_component`, `remove_component`, `component_names`,
  `scene.component_types`, `scene.component_schema`) and the editor: the
  Add-component palette and every inspector row are generated from the registry,
  so a third-party component needs no editor change.
- A component exposes every setting the type it wraps offers; each is a key,
  a reader, or a constraint stated in the code.
- Render components store angles in degrees and name them `_degrees`; physics
  components store radians and declare `unit = "degrees"`, so the inspector
  draws degrees.
- A controller answering the mouse or keys is a script, not a component: the
  camera rigs in `editor/library/rigs/` place a camera through its node.
- **A build may register 128**, one bit each in `components::Attached`, and
  registering the 129th panics naming it. 112 are in tree, so 16 are left for
  plugins. The cap is a `u128` per node that carries any, which makes a
  presence test a shift and a free ask only the plugins it owes.
- **A name is a number at run time.** Registration order is the component's
  index, and `index_of`, `property_at`, `patch_at` and the `Attached` bits all
  take that number. A backend resolves a name once, when it builds its
  handles, the way `MaterialId::intern` does for materials: dispatch costs
  neither a hash nor a string. `transform` registers first and owns bit 0, so
  the node bundle can say a node has one without reaching the registry.
- `docs/generated/components.md` lists those in tree, from `transform` in core
  through physics, render, UI and animation. `shape3d`, `shape2d`
  and `sprite` each carry their own `color` property, since a tint needs
  something to tint.
- Materials split the same way. A renderable's `material` is its own; the
  `material` component goes on any node and names one for the subtree, composed
  beside the tint as `Appearance::material`. That is an interned `MaterialId`,
  so `Appearance` stays `Copy`, and the digest and snapshot store the reference
  since ids follow load order. A shader's contract import (`package::sprite`,
  `package::mesh`) says which dimension it draws, and an inherited material that
  reaches the other one keeps the built-in material.

**Browsing them.** The list is flat: a node is exactly its components.

- **Tags** are facets, several at once (`collider2d` is `2d` *and* `physics`); a
  category path would bury whichever lost. 3D components are tagged `3d` though
  their names carry no marker (D5).
- **Presets** are named recipes over components on one node. Nothing records
  which was used, so components stay free to come and go. They are the editor's
  data, not the engine's: `editor/presets.toml` lists them, a game's
  `editor/presets.toml` and each addon's add more, and the picker builds one
  with `set_component`. Anything spanning nodes is a scene.
- **Expectations** are advisory: a component names others it needs something
  from, and the editor warns while none is present. Nothing blocks. Built-ins
  declare one where a component is inert alone: a body or a bone expects
  `transform`, a joint a body, a character a collider, `tile_collision` a
  `tilemap`, a wheel its vehicle. A `collider2d` with no `body2d` is static
  geometry, so it expects nothing.

### 2D

A second set of components over the same tree, on the same `transform`.

- `shape2d` (six kinds) and `sprite` render through a pan/zoom
  orthographic camera: a `camera2d` node's position and `pixels_per_unit`, in
  logical px per world unit, plus `camera_2d`, `mouse_world_2d`, `draw_polyline_2d`.
- `body2d`/`collider2d` run in a rapier2d world beside the 3D one, with the same
  determinism build, accumulator and ordered collections. Both carry joints,
  character controller, queries, contacts and closest-point queries; 3D adds
  vehicles.
- A sprite sizes from its image (`pixels_per_unit`, default 100) or one cell of
  a `columns`/`rows` grid or a `sprite_sheet` in `sheet`, resolved when set rather than at draw time, so the
  image header is read in every build, headless included.
- Changing the frame moves UVs only and does not bump the renderable's version;
  changing texture or sheet does.
- `physics.set_paused`, `is_paused`, `clear`, `set_sleeping_allowed`,
  `sleeping_allowed`, `set_tuning`, `set_debug_draw` span both
  worlds, so an editor treats physics as one simulation.
- 2D batching draws a run of sprites or shapes as one call, cut wherever
  material, texture, mesh or blend changes, so draw order never moves.
- A material with `features = { screen = true }` reads the frame so far: the
  kiss3d fork splits the 2D pass and copies the HDR film first.
- The editor detects a 2D scene from its components and switches grid, camera,
  gizmo, picking and collider overlays. `examples/angrynerds` is a full 2D game.
- A `convex_decomposition2d` cuts `exact` by default, with parry's
  Hertel-Mehlhorn merge. `overlap` then grows each piece through its seams,
  so a thin body cannot wedge into one.
- `vhacd` and `voxels` cut fewer, approximate pieces for a dense outline. 3D
  has only those two, since nothing decomposes a closed mesh exactly.

**Tile maps.** A `tileset` and a `tilemap`'s cells live in
`balaur_geometry::tiles`, which the renderer and physics both read.

- `tile_collision` makes one parry voxel collider per group from the full
  cells, so a body sliding along a wall cannot catch on a seam.
- Autotiling is one ordered rule table, first match wins, and bitmask
  terrains are sugar over it.
- A tile's `data` is the game's own table. A script finds it through
  `tilemap.cell(x, y)` and `assets::load` of the tileset.
- `balaur import level.tmx` reads Tiled through `tiled` 0.16, behind
  `balaur_cli`'s `import` feature, which no game template has. Its
  `quick-xml` 0.31 and `zstd` 0.13 advisories are ignored in `deny.toml`.
  The XML is a file a developer chose, at author time. `tiled` leaves out
  the seekable and legacy `zstd` decoders and opens a fresh one per layer,
  so no `zstd` advisory is reached. Drop the ignores once `tiled` takes
  `quick-xml` 0.41 and `zstd` 0.14.

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
- `application/ignore` in `project.toml` lists what is not the game's, as
  globs (`art/wip/**`, `**/*.blend1`): the asset index skips it and an export pack
  leaves it out, so nothing there reaches a shipped game. One list rather than
  a marker file per folder, so it is visible and versioned. A script compiles
  against the disk, where an ignored module still is, so the export pack asks the
  compiler what each shipped script links in (`ScriptCompiler::uses`) and
  fails on one it left out, naming the pattern; `balaur check` says the same.
- Sharing is the default, and Rust's `assets::duplicate` opts out; a script's
  `assets.load` is a table of its own already. Cache keys use a
  hand-written FNV-1a over bytes and TOML structure, since `std`'s hashers specify
  nothing about their output, and this key must agree across platforms.
- Resolution reuses `ScriptHost::scene_source` (export pack or disk), with a
  `ProjectRoot` + `std::fs` fallback for Rust-only apps and tests.
- An unresolved reference **warns** and the scene loads; a definition table that
  does not parse is an error. `apply` errors are fatal to `instantiate_scene`,
  so warning is what keeps one bad path from taking a scene down.
- `assets.load` hands back the definition *table*: `Value` has no variant for
  `Rc<dyn Any>`, and per-backend userdata is what the seam exists to avoid.
- `assets.rename` moves a file and rewrites every `.toml` through `toml_edit`,
  comments intact. `assets/index.toml` maps `id = "path"`, and `id://<id>`
  stands in for a path anywhere, resolved before the cache key.
  `asset_index::assign_id` writes a digest of path and content, so a rebuilt index
  gives a file the id it had. Script sources are not rewritten.
- A path stays the reference in a file, readable and diffable; `id://` is
  the escape for content edited outside the editor.
- `toml::patch` writes a table into a document's text through `toml_edit`
  too, so a tool saving a hand-written file keeps its comments and key order.
- `balaur import file.aseprite` writes one atlas page, a `sprite_sheet`
  naming every frame, tag and slice on it, and a clip per tag.
- **Import settings are a sidecar**, Godot's `.import` by another name:
  `art/hero.png.import.toml` states how `art/hero.png` is read, and `[import.<kind>]`
  in `project.toml` sets the default for every file of that kind. A file's kind
  comes from its extension: texture, audio, font, model. `balaur_core::import`
  resolves the two, and the resolved settings ride in the name a texture is
  uploaded under, so changing one image's filter re-uploads that image alone.
- Settings change pixels and samples, never extents: a sprite is sized from its
  image header, so a headless run resolves settings for nothing and computes the
  same world. That is why downscaling is an export key and not an import one:
  `[export] max_size` records the drawn `size` beside what it shrank. An SVG's
  `scale` and a model's `scale` do set sizes, and every build reads them alike.
- Every reader of a picture goes through `balaur_asset::pixels`: a raster
  decoded, an SVG rasterized, then `bleed` and `flip_green` applied. An export
  writes an SVG's raster under its own name, so a game runtime has no
  rasterizer.
- A texture property is asset-typed, `asset = "texture"`: a plain image path
  resolves as `{ source = <path> }`, so the asset layer reads no file, and a
  `textures/*.toml`, `[[assets]]` block or inline table adds settings that win
  over the sidecar. `balaur_asset::texture_asset::source` is the one resolver.
- A model's `lods` are index lists over its own vertices, simplified at load by
  `meshopt-rs` and swapped by camera distance in the renderer alone, so no
  simulation reads which copy is drawn.
- `import.start` runs an import a few files a frame and reports each as
  `import_event`; a replay hands a script the same events without importing
  twice. A cancelled import keeps what it wrote: the files are the output, not
  a transaction.
- An importer takes bytes and writes through a `Sink` one file at a time;
  `ProjectSink` writes through the files backend, so a tab imports with no
  disk.
- Every `fs.*` path resolves `..` and symlinks first, and is refused unless it
  lands inside a root the host declared (`file_api::resolve`).

### Animation

`balaur_animation` is a plugin: clips, state machines, retargeting and modifiers:
four asset types, four components, one script module, two systems in `Update`. It depends on core and no other plugin crate, and a test reads
its `Cargo.toml` and fails if one appears.

- A clip is a length, a loop mode (`none | loop | pingpong`) and tracks; a track
  is a node path, a property, an interpolation (`step | linear | cubic`) and
  keys. A library is the same file with `[clips.<name>]` entries, addressed
  `hero.toml#idle`; the entry inherits the document's `type`, so nothing in core
  knows the word `clips`.
- **Property addressing reuses the component registry**: `position`,
  `rotation_euler`, `scale` are the transform, anything else is
  `component/property` through `patch`. So `shape3d/color`, `collider2d/radius`
  and `widget/x` animate, and a third-party component animates the day it registers.
  A track with no `property` is a method track, calling through `call_on`.
- Rotation keys are authored as euler radians (the spelling
  `transform.rotation_euler` uses, readable in a diff) and interpolated as
  quaternions, the only way past ±180° that takes the short way. A `rotation`
  track takes the quaternion, which is what an imported `.glb` holds.
- The sampler is `(clip, time) -> pose`, pure and reachable with no `Engine`, so
  blend trees can compose samples later. Keys sharing one time are a jump, and
  at that instant the last of them holds, including a track whose every key
  sits at the end, which is what "hide when the fade lands" generates.
- **A tween is a generated clip**: one sampler, two authoring front-ends, no
  second interpolation path. Steps are sequential; `parallel = true` joins the
  previous, and the next non-parallel step waits for the group. `to`, `by`,
  `from`, `target`, `interval`, `call`. A tween dies with its node.
- The spec is data, not a chainable builder: a handle object means new userdata
  in every backend and every future language. A handle travels as an integer,
  which `animation::stop_tween` takes; a clip on a node ends with
  `node.animation.stop()`. Data also makes a tween serialisable, so the editor
  authors one and it hot reloads.
- Easing is Godot's 12 transitions in 4 modes, with its names and shapes. Every
  curve maps 0→0 and 1→1 exactly, asserted with `assert_eq!`; without endpoint
  guards `expo` and `elastic` land a float short.
- Determinism: playback has its own accumulator at `fixed_dt()`, capped at
  `max_substeps()` catch-up steps, so the sampler only sees the fixed step. Folding is floor-and-subtract;
  every transcendental is `libm`'s, glam's included. Players and tweens live in
  `DetHashMap`s and apply in insertion order.
- Scheduling: after the script tick, so `node.animation.play()` lands the same frame;
  before `FixedUpdate`, where physics reads `Transform` for kinematic bodies, so
  an animated platform pushes what stands on it with no wiring. A step records
  deferred effects rather than applying in place, since an `apply` may want the world
  mutably, and a handler may free its own node.
- A crossfade keeps the clips it leaves on a stack, each still advancing, and
  poses every track either side keys: one only the old clip drives follows it
  out. A fade started mid-fade folds over the blend, so it never pops.
- A `state_machine` is Godot's `AnimationNodeStateMachine`, transition field
  for field, decided on the fixed step and in the snapshot. A nested machine
  is flattened at parse into `group/state` names, so the runtime walks one
  graph. An `advance_expression` is a `check`: a script method asked once a
  frame, before the steps, because a script may read what they hold.
- Not now, not precluded: blend trees, and a tween's `loop_finished` and
  `step_finished` signals. A clip that animates a dynamic body fights the
  solver for it, as in Godot.

### Objects: every shape is a mesh

A shape is a function from parameters to `MeshData` in `balaur_geometry::primitive`
on `libm`. So a collider fitted to a torus, a ray picking one, a headless test
and the triangles on screen are the same triangles.

- Ten 3D primitives, six 2D; `path` adds beziers and what they extrude, revolve
  and sweep into; `csg` combines two with a BSP over their faces, written out
  because the candidate crates reach for parry and a BSP has no transcendental
  to pin. `balaur_text::glyph` is the one mesher outside core: shaping a word
  needs that crate's font set.
- A `mesh` asset names a model file, a primitive, a word or a path to thicken;
  the two that reach another asset resolve through `mesh::load_from`.
- A node draws `Shape3d::Solid` (parameters), `Shape3d::Mesh` (an asset) or
  `Shape3d::Built` (what a `boolean3d` settled on), the split `Shape2d::Polygon`
  already used. A `multimesh3d` or `multimesh2d` draws its `multimesh` asset's
  mesh once per instance, and `render::instancing` splits each instance's
  matrix for the shader; automatic instancing draws through the same seam.
  The node copies the asset's instances when it attaches, so a script's edits
  stay on that node.
- There is no cloner: a row, a ring or a grid is a `multimesh` instance list
  an asset holds or a script computes. An instance always carries a
  position, a rotation, a scale, a colour and four custom floats.
- 3D nodes sharing a mesh and a material draw as one instanced call; a
  skinned, boolean or unevenly scaled node keeps its own object.
- A soft body writes its solved mesh onto its node each fixed step,
  `SolvedMesh` in 3D and `SolvedPolygon` in 2D, and the renderer draws that.

### Skeletons and skins

A bone is a node with a rest pose (`bone2d`, registered by `core::skeleton`).
There is no skeleton component: a skin names its rig by node path
(`polygon.skeleton = ".."`), and the rig's bones are that node's descendants
carrying a bone, in tree order, the order a skin numbers them.

- A clip keys a bone by path, `target = "Hip/Thigh"`, so playback needs no
  bone type, and the digest and snapshot ring already cover it. Modifiers and
  retargeting are what name bones. Bones live in
  core so rendering, the editor's registry and future physics reach them. The
  `skeleton` module is `bones`; the editor's `rig.bs` resets and records a
  rest pose over it.
- **A skin is a `polygon`** whose `mesh` asset holds `[x, y]` positions, an
  `internal` count of trailing interior vertices, optional `polygons` index
  loops (with interior vertices the author draws them, since automatic triangulation
  bends badly), `uvs`, and `skin.bones` folded at parse to the four heaviest
  influences, renormalised. Triangulation is `core::triangulate` over
  `i_triangle`, so a headless test asserts the triangle list. Absent UVs centre the texture at `pixels_per_unit`,
  v downward, from the image header.
- **The palette is computed by the engine, uploaded by the backend.**
  `joint_matrices_2d` is a pure function of the tree: global pose × inverse rest
  pose down the rig, carried into the skin's space, so a skin need not sit
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
  image feature: nothing here decodes a texture. GPU skinning is
  `skinned_3d::attach`; the CPU twin stays for a node with its own `material`
  and for the tests. A `.gltf` reads its buffers through a caller-supplied
  `SideReader`, or a `data:` URI decoded in core.
- **Modifiers have the last word.** `modifier2d` and `modifier3d` are Godot's
  skeleton modifiers as one component each: `look_at`, `two_bone_ik` (the
  analytic solve, `flip` choosing the elbow, an out-of-reach target
  straightening the chain), `fabrik`, `ccdik` and `jiggle`. It runs after animation, from local transforms as
  they are now, in entity order, on `libm`.
- There are no Spine slots or skins: a node with a `sprite` and `visible` does
  that. 3D weights come from the file.

### Binding API

One call per entry point; conversions are inferred.

```rust
let m = reg.script_module("physics3d")?;
m.describe(&[("apply_impulse", &["body3d"], "(node: node, impulse: Vec3)", "...")]);
m.function("apply_impulse", |eng, (node, impulse): (NodeId, Value)| {
    ...
});
```

A point, an offset, a size or a velocity crosses as one `Vec2` or `Vec3` and a
colour as one `Color`, never as loose floats. The closure takes it as a `Value`,
and the signature in `describe` names its type for the compiler and the
reference.

A component's scene key comes with `register_component`, applied in plugin
registration order. `balaur_physics` is the reference
implementation; its `scalar.rs` is the one place a number changes width between
the engine's `f32` and rapier's.

### What stays native

What a script can build from the engine's functions is Balaur Script: in the
language's prelude under the engine's own names (`encoding`, `random::at`,
`random::uuid`, the `geometry2d` helpers, `quat`, `audio::play_file`), or in
the editor's scripts. A function stays in Rust where a script cannot reach
what it needs, or where the script version was measured and cost too much.
Each measurement is in instructions retired or milliseconds under wasmtime.

- `hash::sha256_file` and `sha256_text`: 13.3 to 20.4 ms on 1 MiB as a script,
  against 0.75 to 0.88 ms through `sha2`.
- The immediate shapes, `draw_rect_2d` to `draw_capsule_3d`: 500 a frame took
  2.6 times as long for rectangles and 50 times for spheres. A scripted
  rectangle also builds a mesh each frame.
- The `timer` component: 8.9 million instructions a tick for 1,000 timers as
  scripts, against 44 thousand native.
- Breakable joints: 2.5 to 5.1 times the native check for 100 joints a step.
- The tree walks, `node::descendants` to `scene::node_by_id`: 4.2 to 18 times
  native on the editor's 544-node tree and a 2,000-node scene.
- Autotiling: a 32-cell brush stroke at 4.2 times native, and the brush and
  the load share one resolver.
- Tile collision: 431 million instructions for a 256 by 256 map with 30% of
  its cells solid, against 27 million native.
- `screen_notifier2d`: at least 1.8 million instructions a frame for 200
  notifiers as scripts, against 0.26 million native.
- `ui::write`: a control's table routed to the components that hold it,
  written only where it moved, and its `data` handed to its handler, which a
  script cannot do without a call a key.
- `max_contact_impulse`, the character, vehicle and follow controllers, and
  rumble shaping: rapier's contact impulses and a pad's per-frame state are
  out of a script's reach.
- Touch buttons and sticks: hit-tested in the frame's first stage, where a
  script would answer a frame late.
- The splash and the log file: both open before scripts compile.
- `ui::contrast`, `contrast_pairs` and `colors_apart`: they walk roles by the
  engine's own theme inheritance.

Kept as product decisions rather than for reach: scene `bindings` and
`states`, the animation `state_machine`, the `code` widget's language tables,
the `window` widget kind, gestures, the built-in UI focus controller, and
gamepad repeat. `apply_force` stays beside `apply_impulse`, being Godot's and
Unity's word for a push held over time. `panel` stays beside `column`: it is
Godot's `PanelContainer`, a plate its children sit on as in a `stack`. Tweens
stay native, since `then`, `on_tween_finished(handle)` and the step events need
an owner that a generated clip on a child node is not.

### Modules and extensions

A **module** is linked in behind a cargo feature; an **extension** is loaded
from a shared library at run time. Both implement `balaur_plugin::Plugin`
(`manifest()` and `declare(&mut Registry)`), so one source ships either way.

- `Registry` names resources, systems, components, asset types, replay
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
  requirements, never directory iteration, since it decides registration order, which
  decides the simulation.
- `[plugins]` in `project.toml` switches modules off or configures them. Asking
  for one nothing registered is an error naming the feature to rebuild with;
  turning off one every build has is refused: a setting that cannot be honoured
  must not look like it was.
- `balaur_plugin` re-exports every crate its API names (`balaur_core`,
  `balaur_script`, `anyhow`, `toml`) plus a `prelude`, so a plugin outside the
  tree has one dependency.
- Only the desktop builds carry `extensions`. Web, iOS and Android builds leave
  it off, so a game shipping there takes its plugins as modules.
- An exported desktop game carries its extensions. A fused executable has them
  in `extensions/` beside it. A `.app` has them in `Contents/PlugIns`, signed
  with the game's identity before the bundle, since the hardened runtime's
  library validation refuses a library from another team.
  `standalone::extensions_beside` names that place for exporter and runtime
  alike, and the template's own format picks `.so`, `.dylib` or `.dll`.
- Every desktop runtime opens Valve's `steam_api` at run time, from beside
  its executable (`Contents/MacOS` in a `.app`), so one built without it
  beside still starts. A release stages each runtime's library beside it as
  `balaur-runtime-<target>.<library>`, and only `balaur export --steam`
  copies it beside the game, signed with the game before the bundle, with the
  two hardened-runtime entitlements the overlay needs. Any other export ships
  no Valve file, so one game goes to Steam and to other stores.
- A packed boot loads from there, never from the working directory, which is
  `/` for a `.app` Finder opened. `balaur play` loads from beside the export pack. A
  web, iOS or Android export warns and ships without them.

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
  (measured in `crates/balaur_plugin/tests/suite/extension.rs`). An extension may own
  state; it may not reach the engine's.
- **The C path has no ceiling**: no `TypeId`, state behind its own `void*`, four
  symbols, and a table of host function pointers rather than resolving symbols
  back into the executable. The header is committed, with `_Static_assert`s on
  every size and offset and a Rust test asserting the same numbers.
- **No allocation crosses it**: every `BalaurValue` is a borrowed view valid for
  its call, and the host copies before returning. Tier 1 today is script modules
  of functions and constants (`docs/PLAN-c-api.md`).
- A C extension gets no `Engine` and no asynchrony; a service that needs
  either, such as a store, is an engine crate behind a feature. The whole
  `App` and `Engine` surface is never offered across the C ABI, since it would
  freeze internals that still move.

### Export packs

`balaur export` compiles every script to the unit dev mode builds, less the
debug info, so shipped bytecode is what was tested, and bundles the units,
scenes and manifest into a `.bpak`, for the web as for every other target. A
unit for the web counts its own fuel; the project's own empty unit rides at
`.balaur/bs-project.unit`. Only `--keep-sources` ships source, and only a build
with the `compile` feature runs such an export pack: the editor's, never a
game template, which has no compiler, checker or language tools.

- The web game template and the editor's module are two builds of
  `balaur_cli`. The template leaves out `editor` (the exporter, the project
  store, a tab's editor entry points), `compile` and `fallback-fonts` (egui's
  bundled faces, 1.35 MiB); the editor's module adds `import,editor`.
- The web template alone also leaves out `webgl`, the GL backend a page
  without WebGPU would draw with, and `env-filter`, `RUST_LOG`'s regex, and
  builds std with `optimize_for_size` (`WEB_STD=stock` keeps rustup's). Each
  is a switch, so a template for an old browser builds with `webgl` back on.
- `compile` off drops the `.bs` compiler, the C translator an iOS export uses,
  `balaur check`, `lsp` and `fmt`, and the WESL linker. A unit runs plain, so
  a template's scripts take no debugger's probes or profiler's counters.
- Every shader link goes through `balaur_render::prelinked::cached`, keyed by
  what it links. An export runs each one the game can draw with: the engine's
  in every variant, each material morphed or not, in the transparency pass or
  not. It writes the results to `.balaur/shaders.toml` in the export pack, which a template reads.
- A web export may come in two pieces. `[export] later` names scenes whose
  own assets go to `game.bpak.tail`; the head names it in
  `.balaur/tail.toml` with those scenes. The page starts on the head and
  fetches the tail, `pack_tail::deliver` hands it over, and a switch to one of
  those scenes waits for it at the end of the tick. `balaur play` reads a tail
  beside its pack before the first frame.
- kiss3d's Tony McMapface LUT, 864 KiB, is in a build with `compile` alone.
  An export of a game whose scenes, assets or scripts name `tony_mcmapface`
  writes it deflated to `.balaur/tony_mcmapface.lut.deflate`, and a template
  hands it to kiss3d before the window opens.
- kiss3d links its built-in shaders in `build.rs`. Its object shader keeps
  three `@if` flags that change what it declares (8 modules); the other
  fourteen are `override` constants each pipeline sets.
- egui fills each glyph's outline once, into its atlas, with `zeno`, which
  balaur_text links already: a patch on epaint 0.36 (`Ughuuu/egui`) in place
  of `vello_cpu` and its SIMD kernels, 1.4 MB of a web build.
- A runtime variant is a name with a suffix: `-2d` and `-3d` on every target
  keep one physics world, `-bare` neither, and `-server` on a desktop has no
  window or sound. A variant is a game template, with no editor and no
  compiler, takes its platform's tags and is found as `balaur-runtime-<target>-<variant>`. A
  server is a target of its own; `[export] runtime` turns `web` into `web-2d`,
  by default whichever is the smallest the game's scenes and scripts fit,
  and the export refuses a game whose scenes or scripts use the world it
  leaves out, since a script's body is invisible until it runs. CI builds,
  exports and runs every one; `scripts/features.sh` holds the feature sets.
- A physics world is a feature, `physics2d` or `physics3d`. Both compile in
  every build, since the 2D code calls helpers in the 3D files; a world left
  off registers no system, component or verb, and makes no state, so neither
  its solver nor rapier's shape code links. `physics.*` spans the worlds a
  build has, reading each through `state_3d` and `state_2d`.

- `balaur::boot_pack(include_bytes!(...))` makes a self-contained binary. Where
  JIT is banned, iOS included, the export translates the scripts to native C
  (`crates/balaur_script_bs/src/native.rs`). CI cross-compiles to iOS, Android
  and wasm on every push to main.
- The web target is `wasm32-unknown-unknown` with wasm-bindgen: kiss3d declares
  its web dependencies there and wgpu reaches WebGPU through `web-sys`. Audio
  plays through cpal's WebAudio host, its device opened on the first
  `UserActivation` rather than at startup, since a browser refuses to start
  audio before a gesture. `balaur_webtransport` has a browser backend, but no plugin registers
  it and nothing outside its own tests opens a link, so it stays out of the
  default web feature set.
- An export pack is written in sorted key order, so two exports of one source
  tree give the same bytes anywhere; `docs/QUALITY.md` says what checks it.
- An export pack holds each entry as its original bytes with a SHA-256; the
  hash catches a corrupt entry and hides nothing. Extensions reach the host
  through a table of function pointers, so a shipped executable exports no
  symbols of its own.
- A web export ships the engine's `index.html` shell unless the project has
  `web/index.html`; the rest of the project's `web/` folder ships beside it.
- An export refuses to replace an output directory that is neither empty nor
  an earlier export of the game, so `-o .` deletes nothing
  (`bundle.rs`, `replace_export`).

### Dependencies

A dependency is another project imported whole into `deps/<alias>/`,
declared in `[dependencies]` (`docs/PLAN-dependencies.md`).
`balaur_core::dependencies` holds the manifest and lock formats, the hash
both a desktop and a Forge runner check, and what a running engine knows.

- Each installed dependency is a declared root, so `document_paths` reads a
  document under `deps/<alias>/` against that root and spells its paths
  `deps/<alias>/…`, which is also how an export pack keys them.
- A `path` dependency is mounted: `dependencies::Mounted` wraps the file
  backend and maps the mount point to the folder, `canonicalize` included,
  so `file_api`'s root check and the asset loader see one tree. The `.bs`
  host watches the folder and keys its edits under the mount point.
- The `.bs` host names the file of each instance it calls into
  (`dependencies::calling`). `path_of` and `fs.*` read a relative path a
  dependency's script builds under its folder, and one a played game's
  script builds under that game's root. The editor keys the game it plays
  by absolute path and its own scripts relative. Building a scene is the
  game's turn (`as_game`), since its document is already in game spelling.
- A file under `deps/` is refused to `fs.*` writes, `assets.save`,
  `assets.rename` and `asset_index::assign_id`. The `dependency` script module is
  two halves. The engine's is `list` and `of`, which a game reads. `install`,
  `add_path`, `make_local` and `move_into` write a project, so `balaur_cli`
  adds them beside `project` and `forge`, for the editor's docks.
- `Pack::build` gathers each dependency with its own `ignore`, rewrites its
  documents, bakes its `[import.<kind>]` into sidecars, and drops what
  nothing names, reading script literals from the sources. It writes
  `deps/credits.json`.
- `balaur add` and `balaur fetch` (`balaur_cli`'s `deps_cmd`) read a Forge
  project over its HTTP API by tree and blob, each blob checked against its
  git id and kept once under the editor's data folder.
- The lock's sha256 covers a dependency's whole tracked tree less `deps/`:
  each file in path order, as path, newline, length, newline, bytes.
- Installing runs nothing, and a dependency's window, plugins and main-scene
  settings never reach the game.
- Files under `deps/` stay as their project wrote them; a binary asset is
  taken over with Make local, never overridden.

### Fused executables

`--target <platform>` appends the export pack to the runtime template CI publishes:

```text
[ template executable ][ pack bytes ][ pack length: u64 LE ][ "BPAKSELF" ]
```

- ELF, Mach-O and PE ignore trailing bytes, so the fused file runs. At startup
  the CLI reads its own executable (`core::standalone`): an export pack means it is a game
  and argv is never read. One binary is the editor, the CLI and every game's
  runtime.
- Runtimes resolve from `BALAUR_RUNTIMES`, then `runtimes/` beside the
  executable, then `<data dir>/balaur/runtimes/<build id>`. A missing desktop
  template is offered for download from the release this build came from and
  verified against its `SHA256SUMS`, pinned exactly, because an export pack must only
  meet the runtime its compiler shipped with. The prompt needs a terminal or
  `--download`; `--no-download` forbids it.
- `scripts/package.sh` bakes a build id and every release carries a VERSION
  asset, which catches a Monday nightly meeting Wednesday's template and drives
  `balaur update`: one command replacing binary, editor, template and header,
  which only work as a set. A source build refuses and points at git. An update
  follows the channel its own version names, and `--channel` crosses lines
  (`docs/RELEASING.md`). The editor's `release` module runs the same code on a
  thread, so its Engine tab and the command cannot drift.
- A signed macOS game is `export --bundle app`: a `.app` with the export pack
  in `Contents/Resources`, because codesign seals resources but never bytes
  appended to a flat binary. `--sign` signs it, and implies `--bundle app` on
  macOS; `--notarize` staples Apple's ticket.
- Only the editor download is signed. A runtime template is unsigned, because
  appending a pack breaks a signature, so `balaur export` signs the fused
  result.
- An iOS or Android game cannot be fused, so the export pack travels as a
  bundle resource: inside the `.app`, or `assets/game.bpak` in the APK. Its
  scripts are native code, `Frameworks/BalaurScripts.framework` or
  `lib/<abi>/libBalaurScripts.so` for each 64-bit ABI.
- `--bundle aab` packs the base module with the `zip` crate over aapt2's proto
  output, then runs `bundletool build-bundle` and `jarsigner`. The exporter
  finds the Android SDK through `ANDROID_HOME` and never ships it;
  `bundletool.jar` comes from `[android] bundletool` or
  `BALAUR_ANDROID_BUNDLETOOL`. An `[android] min_sdk` below the template's own
  `minSdkVersion` is an export error naming that floor.
- `balaur export` applies every signature, profile, keystore and
  notarization, so a CI action only places the credentials and calls it.
- A password or an API key comes from the environment
  (`BALAUR_APPLE_NOTARY_*`, `BALAUR_ANDROID_KEYSTORE_PASSWORD`); an identity
  or a keystore path may sit in `project.toml`.
- Authenticode appends its certificate table after the pack, so
  `core::standalone` finds the trailer before that table.
- The exporter sets the execute bits on its output: a template from a zip or an
  artifact store has lost them.
- A `.bs` script compiles against the engine's modules, so `balaur::build_pack`
  boots the app the game would boot and compiles through its host.

### Tooling: one provider, two fronts

`LanguageTools` on `BsHost`, over `balaur_bs::analysis`, answers *at this
file, line and column: what completes, what is under the cursor, where it is
defined, what this file's symbols are*. `balaur lsp` maps LSP methods onto it
through `ScriptHost::tools_for`, and the editor's `script` module calls the
same verbs, so a script means one thing to both. `script` is `ToolsPlugin`'s
too, so a game's runtime has no `script::check`.

- Completions and hover text for a module path, `self.`, a component handle
  and a constant come off the `HostApi` a script compiles against, which
  `balaur api` prints as `bs`.
- Each request parses the buffer as it stands, `use`d files found from its
  path, so an unsaved edit is what is answered for.
- Rename and references stay inside the file asked about.
- Formatting is `balaur_bs::fmt`, which `balaur fmt` and the editor's format
  key call too.
- `balaur check` builds every scene file with the run's own loader, so the
  check and a load refuse the same keys, values and overrides. A `.bs` file
  no scene reaches is compiled alone, as the module it is; a nested project
  and `deps/` answer for themselves.

## UI: widget nodes, painted with egui

A `widget` is a component on a node, so a menu is a subtree the editor edits
like anything else. `balaur_ui` lays each subtree out with taffy and paints it
with egui inside the kiss3d window, once a pass. A script fills nodes and
answers their events; it never draws. The `ui` module is what a script asks of
the UI and tells it: where a node drew, focus, the scale, the theme, the
clipboard, when the next pass runs and the control tables a form writes.

- `row`, `column` and `panel` lay out their widget children: along the
  direction, `gap` apart, `padding` inside, `align`ed across. A child's own
  `anchor`/`x`/`y` is ignored: a menu that moved when you nudged one entry would
  not be a menu.
- A widget's parent is its nearest *widget* ancestor, so a grouping node changes
  nothing. Only containers adopt what is under them.
- The layout is `taffy`'s flexbox, solved per root before anything draws.
  Layout is presentation and never touches the digest.

**Focus.** One focused widget per screen, held as a resource so moving it is one
write. It walks widgets in scene order and wraps.

- Whether a widget is a stop is **derived, not declared**: focus exists to
  activate something. `focusable = false` takes a candidate out; it cannot put
  one in. Hidden, freed or unfocusable releases focus.
- A theme entry carries `gap` and `icon_color` as well as its frame and its
  type: Godot's separations and its `icon_normal_color`. A widget's `padding`
  and `gap` below zero take the theme's, and a stated zero is none at all;
  `padding` takes one number for every side or four for left, top, right and
  bottom, which is what a MarginContainer's margins convert to.
- A `button` with `toggle` holds down when clicked and releases on the next,
  flipping `checked` before its `pointer_click` is heard; a `group` makes a
  set of them exclusive, as it does for a `check`.
- An `image` states how it sits in its box with `fit`: `contain`, `cover`,
  `fill` or `none`, CSS's object-fit and Godot's expand and stretch modes.
  Without one the picture's own size decides the box, which is the default a
  scene written by hand wants.
- A button fills the box the layout handed it and never hugs its caption
  inside one: a button in a column is as wide as the column, as it is in a
  browser and in Godot.
- An accept is a click by another name (same `clicked`, same `pointer_click`),
  so a mouse menu works on a pad unchanged. `focus_entered` is said only on arrival.
- **2D draws in one order**: every sprite, polygon and tilemap under the root
  by `z_index`, then z, then tree order. A tilemap's mesh is built by a pass
  of its own, so it is put back in that order after both passes have run.
- **A `stack` lays its children over one another**, each placed in its box by
  its own `anchor`: what a Godot MarginContainer does, and what a plain
  Control's anchored children do. Every other container queues its children
  up, so `anchor` is read on a root and inside a stack, nowhere else.
- **A `viewport` is the screen for what sits in it**, as Godot's SubViewport
  is. Each widget under it is a root of that screen: placed against the
  viewport's box by the rules a root follows against the window, clipped to
  it, and given no focus or clicks past its edge. A viewport with no size
  draws nothing in it. Where a tree is says what its screen is; no widget
  names one, so a scene draws the same in a window and in an editor's stage.
- A viewport keeps what opens from it. A dialog in it dims and holds the
  viewport alone, so the editor around a played game stays clickable. A
  menu, a dropdown's list, a context menu and a tooltip open on the side of
  what they hang off that fits inside it, or slide inside where no side has
  the room (`widget/popups.rs`). A widget's class tables and its
  `hide_narrower` and the rest read its viewport's size where it last drew
  (`widget/screens.rs`), so two viewports of different widths each lay out
  as their own; the theme's tables read the played content's area.
- **A chord is data on a node.** A `shortcuts` table's rows each pair a
  chord with a name, and a press raises `shortcut_pressed` with that name on
  the node, which need not be a widget. A row answers while its node is shown,
  so a scene's keys in a viewport with no size are silent. A widget's own
  `shortcut` clicks it and draws its chord, for a menu row or a button.
- A widget holds no handler name. It announces its events from its node
  (`value_changed`, `submitted`, and a click as `pointer_click`, a world node's name for
  it), and a scene hears one with a `bindings` row: `{ event = "pointer_click",
  action = "call", value = "start" }`.
- A `label`, a `panel` or an `image` senses a click only while one is heard on
  its node: a row, its script's `on_pointer_click`, a listener or a waiter
  naming it, or the `on_click` its control table names (`Widget::hears_click`,
  kept by `input::follow_hearers` from `events::watch_hearers`).
- `ui.write` keeps a control table's `on_click`, `on_change` and `on_submit`
  off the widget and calls each from the event its control announces, with
  the table's `data`, the edit and the control. It writes only what moved,
  but `visible` every time, since `scene.repeat` shows a node it reuses. A
  form's rows are item scenes
  through `scene.repeat`, `scenes/items/control.toml` a control and
  `control_group.toml` a group, written with `ui.write` by `editor/scripts/form.bs`.
- `scene.repeat(host, items)` keeps one instance of an item scene per key
  under a host. A key asked again keeps its node, a new key takes a hidden
  spare of the same scene, and one no item asks for is hidden, not freed.
  Each root is named for its key, so the item scene's `call` row hands over a
  node whose name is its item. Its stable ids sit under `<host id>/<n>/`, as
  a prefab instance's do: a binding row finds its node by id, so instances
  sharing the file's ids would take each other's clicks. The tool rail is a
  `grid` of `scenes/items/rail_mark.toml`, and the viewport's toggles and the
  Polygon tool's modes are `scenes/items/chip.toml`, whose row says which.
  So are a dock's tabs, the brand menu's panels, the recordings, the
  imports' lines, the start screen's projects and cards, the lanes of a
  replay and of the Timeline, the ruler's ticks, the hook pills and the input
  overlay's chips. The picker and the sheet's sidebar are `list`s
  whose rows the screen up hands over as `ui::Item`s, a pick coming back by key.
- The keyboard drives focus until the game shows the keys are its own: an action
  binding an arrow, Tab, Enter or Space, or a script reading one, unless it also
  declares a `ui_*` action. A pad moves focus and accepts as `ui.focus_next`, `focus_previous` and a click do, which `standard_app` maps
  to the actions `ui_next`, `ui_previous`, `ui_accept`, wiring in the
  assembling crate, since `balaur_ui` has no pads and `balaur_input` no widgets.

**Themes.** A `widget_theme` asset owns how a *kind* is drawn: `fill`,
`stroke`, `stroke_width`, `radius`, `padding` under a table named for the kind.

- A widget takes the theme of the nearest ancestor naming one, so a screen is
  themed by its root and a dialog may differ inside it. An omitted kind keeps
  the built-in look.
- Parsing never fails: a bad colour is reported and dropped, because a game that
  would not start over one is worse than a game that starts plain.
- Neither side can contradict the other, so no widget property needed an
  "unset" sentinel.
- A widget, a `widget_theme` and a setting take one override shape: a table
  per class word, merged over the base. The input class is the `touch` tag,
  fixed for a run; the width and height classes are read each frame from the
  recorded `DeviceFacts`.

**Text.**

- A theme's `[fonts]` names fonts by role: `default`, `heading`, `mono` and
  `icon`. A text's `font` row takes a face file as it is, or a `font` asset
  setting features, hinting and snapping. Size, weight and style stay on the
  text.
- Each project keeps its own `ProjectFonts`, so a game's faces never stand
  in for the editor's. A missing glyph tries `fonts/` in path order, the
  bundled face, then the system's.
- `balaur_text` shapes (cosmic-text, swash) for both the widget layer and the
  renderer's world text, so a label and a `text3d` share one font set.
- A character no face of a project has a glyph for asks the operating system
  through `fontique`, for drawing only: a measurement stays on the project's
  faces. That reads every font on macOS, iOS and Android, tens of
  milliseconds, so a worker does it once the first frame is out. A character
  asking sooner waits for the worker and gets the same face, so what a frame
  draws never depends on when it finishes. Text egui lays out itself is noted
  as it goes by (`theme::asked_of_egui`), since egui never says what it could
  not draw.
- The `code` kind is an editable syntax-highlighted buffer with a gutter.
- Every text kind types through `balaur_text::edit`, on the shaper a label
  uses, so Arabic or Thai typed into a field draws as its label does.

**Painting.**

- Dimensions are design pixels. `ui.set_scale(f)` sets `UiConfig.scale`,
  which is egui's one zoom (`set_zoom_factor`), and the queries divide back.
  HiDPI is separate and automatic; `set_scale` is comfort zoom (⌘+/⌘−). A
  touch screen scales the whole shell up rather than flooring each control
  at 44.
- A kind's draw and its measure take their box from one function
  (`widget/sizing.rs`), so a row sized from the measure holds what is painted.
- A change a container makes while drawing, a dragged seam or a chosen tab,
  is written after the pass. No part of a frame lays out against
  half-written numbers.
- A `list`, `tree` or `table` builds only the rows in view
  (`ScrollArea::show_rows`); each row is an item record named by its `key`.
  A row view owns no model: `sort` orders only the drawing, and a reorder drag
  announces `moved` and moves nothing.
- A row that is a part is a `scroll`'s `scene_rows` (`widget/scene_rows.rs`):
  instances of one scene, built only in view and `ahead` rows past it, at
  most `per_frame` a frame, each told its item by its script's `show(data)`
  after `ui::set_rows`. A row sits out of the column's run at its top, the
  rows it holds no instance for keep the height last measured or the mean,
  and a row scrolled away is hidden and bound to the next one in view.
- There is no `popup` kind: a `menu` whose children are widgets is the popup,
  and its `placement` says where it opens.
- While a screen reader listens, each pass builds an AccessKit tree of the
  drawn widgets (`balaur_ui::access`). The kiss3d fork holds the adapter, on
  every platform but the web and Android.
- egui paints and hit-tests. Every shape goes through `widget/paint.rs`,
  which the `epaint-outside-paint` house lint holds; egui keeps scrolling,
  popups, menus, modals, images and the colour picker.
- A widget root's first pass costs egui a second one: egui sizes a new
  `Area` itself and takes no size from outside.

## Audio

**Buses** form a tree (`[audio.buses] ui = { volume_linear = 1.0, parent = "sfx" }`).

- A sound's gain is its own volume times every bus to the root. `master` exists
  whether declared or not.
- `set_bus_volume_linear` re-applies to what is already sounding, the difference
  between a mixer and a default; a handle remembers its bus and starting volume.
- An undeclared bus is unity, not silence, so a typo stays audible and findable.
  A parent cycle is cut and reported.
- **Effects are a bus's, not a sound's**: the filters, distortion, reverb and
  automatic gain control (`low_pass_hz`, `reverb_time`, `auto_gain`, ...) and
  the limiter are keys in `[audio.buses]`. A bus with any of them is mixed on
  its own and passed through them once, before its parent; a sound wanting its
  own plays on a bus of its own. They are built when the device opens.

**A one-shot sound** plays through `audio::play_file(path, options)`, under the
`sound` component's keys: Balaur Script that adds a node carrying `sound`, which
a `bindings` row frees once it plays out. A named table of sounds, or variations in
turn, is a game's own script over it.

**Positional.** A `listener` node is the ears, else the current camera, a 3D one
over a 2D one: distance sets volume, offset across its right sets pan. A sound
is placed by its `sound` component or per call.

- Attenuation is `inverse`, halving per doubling, or `inverse_square`,
  quartering, full inside `min_distance` and cut at `max_distance`. Pan is
  equal-power amplitude computed with square roots rather than the usual sine
  pair, since platform trigonometry differs.
- Doppler is OpenAL's model over frame-to-frame velocities, off unless a sound
  asks, clamped to `max_doppler`, an octave by default: on a jittering
  position it is a warble, on a teleport a screech.
- Placement multiplies the bus chain rather than replacing it, so a positional
  sound still answers the `sfx` slider. It runs in `SceneSync`, so ears and
  emitters use the poses the frame will draw.
- rodio's `Spatial` is not used: it folds distance into pan through a fixed
  inverse square with no notion of a game's unit. `ChannelVolume` under the
  engine's arithmetic gives the scene's numbers, and makes a positional sound
  mono.
- Nothing is read back off a sink: `placement_of` answers the same on a runner
  with no sound card, and `audio.distance_gain` / `audio.pan` give an overlay
  the numbers applied.

## Localization

`strings/<locale>.toml` per language, read with `strings.tr("menu.play")`.

- A missing key falls back one hop to the project's fallback locale. A key
  neither has comes back **as itself**, visible in the game, where an empty
  label would hide.
- `{name}` takes the argument called `name`; an unfilled placeholder is left
  alone so the hole shows. An `n` argument picks the plural form.
- Plural rules are a named handful (English, Romanian, the Slavic shape, French,
  the one-form languages), not the CLDR table; an unlisted language gets the
  English rule, and a missing form falls to `other`.
- A widget takes part through `text_key`, resolved every frame. Saving a strings
  file forgets the catalogues, so a translation hot reloads like a script.

## Save games

The library's `save` addon, in script: `save::write(slot, data)` and
`save::read(slot)` under `saves/` in `engine::user_data_directory()`. A save
is whatever the game puts in the table. The engine supplies the one piece a
script cannot: `fs::write` writes beside the target, syncs the file and
renames it over, so a crash or a power cut mid-save keeps the old file. The
addon decides the rest.

- **Where it lives.** A slot name is letters, digits, `-` and `_`, so
  `../../id_rsa` is refused.
- **No versions.** A slot holds what `write` put there and nothing else: no
  stamp and no migration, as the engine keeps none before 1.0.
- **Whose folder.** A game the editor plays keeps its slots in the folder it
  has when run alone: `project::use_data` points `engine::user_data_directory()`
  there, and the device id with it.
- **A browser tab.** It mirrors `MemoryFs` into IndexedDB under one root, the
  edited project or a game's user directory, so a reload starts where it
  stopped.

## Store and platform services

One plan per store (`PLAN-apple.md`, `PLAN-google.md`, `PLAN-steam.md`); two
things are shared.

- **Two modules, not one.** `platform.*` is the verbs every store has; `apple.*`
  is what only Apple has. Not a lowest common denominator, because the native
  module sits beside it, and a missing verb answers `unsupported` rather than
  pretending. `balaur_platform` owns the seam, one backend fills it, and with
  none every call still answers.
- **What the OS lends is a second seam.** Notifications, the motor, the share
  sheet and the review prompt belong to the OS, not the store: a Mac game
  Steam started still notifies through Apple's centre. So `balaur_platform`
  holds a `DeviceBackend` beside the store's, which Apple's plugin fills on
  iOS and macOS and the crate's own Java shims fill on Android
  (`native.rs`). Its calls are `platform.notify`, `request_notifications`,
  `cancel_notification` and `register_for_push`, beside the store's verbs,
  and `device.*`:
  `vibrate`, `haptic`, `share`, `request_review`. Their answers cross the
  store's channel, so they are recorded and stood in for alike; a tapped
  notification, or one shown in front, is `platform_event` under request 0.
  The motor is output, never recorded and never run on a replayed tick, as
  rumble is.
- **`platform.is_available` is one check.** A service is a `SERVICE_*`
  constant, and the plugin that reaches it answers through
  `balaur_platform::provide`: Apple's from its bundle id and what the OS
  says, Google's from its client id and Credential Manager, the facade's from
  `SteamState::is_loaded`, the platform crate's own for Android and a page.
  None answering is false. The answers are a replay setup source, pinned in
  the header, so a replay takes the branch the recording took.
- **Delivery is the engine's usual one**: an id out, the answer across a
  channel, `ExternalIo` landing it at `Stage::First` of a later tick: recorded,
  replayable with no store present, waking whoever awaits the id; who is playing
  is also the `platform_event` event.
- **A write waits for its tick to settle.** A rollback cannot take an
  achievement back, so an outward call is held until its tick leaves the
  snapshot ring (`rollback::Clock`; `u64::MAX` with no session). Reads are never
  held.
- **One place needs Swift, fenced off.** StoreKit 2 has no Objective-C
  interface, so `crates/balaur_apple/swift` is ~160 lines linked by `swift-rs`,
  with a request id out and a JSON object back, so a field the App Store adds
  reaches a script with no Rust changing.
- **Arrivals nobody asked for carry request 0** (a sign-out, a renewal, a
  notification, an opened URL). They queue until a pump allowed to touch the
  outside moves them, since anything reaching the channel during a replay would
  be taken for recorded input. `Engine::next_token` starts at 1. The window
  layer owns the application delegate, so the engine proxies it, forwarding
  unanswered selectors, from the first call that needs it. A URL the game was
  launched with is held by the window layer and handed to the Apple queue on
  the first frame, as an ordinary URL arrival.
- **A sign-in page is a platform verb.** `platform.web_sign_in` shows a
  provider's page in the OS's own sheet (`ASWebAuthenticationSession`, which
  needs no URL scheme registered), answering the URL it ended on or
  `cancelled`; `close_web_sign_in` shuts it once the game learned some other
  way that the sign-in is done, and `web_sign_in_state` reads how it stands
  without waiting, for a game that polls a server meanwhile. On Android with no
  store loaded it is a Custom Tab (below). Where there is neither it answers
  `unsupported`, and the game opens the page with `engine.open_url`.
- **A store's proof of a player is a platform verb.** `platform.identity`
  answers what a server checks with the store: Steam's Web API ticket, hex,
  as `ticket`. `balaur_steam` fills `platform.*` only where the Steam client
  runs the game, after Apple's, so a script that names only `platform.*`
  runs on every store.
- **Steam answers `platform.*` from its call results.** A call Steam answers
  later hands its handle to an inbox the steam plugin's pump fills, beside the
  callbacks the backend waits on (stats received, stats stored, a Web API
  ticket), and the backend's pump answers the call when its result lands, as
  a GameKit completion handler does: an unlock after `StoreStats`, a score
  after `FindOrCreateLeaderboard` and `UploadLeaderboardScore`, a cloud write
  after `FileWriteAsync`. Achievement calls wait for the player's stats, which
  Steam fetches as the game starts. A test loads a fake library, test
  functions under Valve's symbol names, through the same `Api::load`.
- **Steam is Valve's own description, generated.** `scripts/gen_steam.py`
  turns the SDK's `steam_api.json` into checked-in Rust: every flat function a
  pointer looked up when the library opens, every struct with Valve's packing
  (8 on Windows, 4 elsewhere) asserted at compile time against
  steamworks-sys's bindgen record, and a `steam_*` script module per
  interface. A call that answers at once is recorded per tick and a replay
  answers from the recording; one that answers later returns a token its call
  result wakes through `ExternalIo`, and Valve's callbacks are the
  `steam_event` event. Dispatch is Valve's manual one, pumped at
  `Stage::First`. What does not map is listed with why in
  `crates/balaur_steam/sdk/unmapped.md`, and leaves that list when the
  generator learns its shape, never by hand.
- **Android's Java is a fixed dex, no Gradle.** The template carries one
  `classes.dex`, javac and d8 over every `crates/*/java` against the
  platform's `android.jar` (`package_runtime.sh`), with nothing from Maven. The
  game's thread is a native one whose `FindClass` sees only the platform, so a
  shim is loaded through the activity's class loader and its one native
  method registered on it, answering onto the call's channel
  (`balaur_platform`'s `android.rs`, which `balaur_google` shares). The
  activity is the one android-activity keeps, handed over as the app is
  built: `ndk-context` holds the `Application`, which shows no sheet. It is
  `Game`, NativeActivity declared `singleTop`, so a notification's tap or a
  link reaching a running game is a new intent to it, never a second
  activity under a live native library (FCM's tap carries `CLEAR_TOP` alone,
  which recreates a standard activity). NativeActivity drops a new intent;
  `Game` makes it the activity's own, which a shim reads as the activity
  resumes, as it reads a cold start's.
  `platform.web_sign_in` is a Custom Tab with no AndroidX: a VIEW intent with
  the protocol's session extra, in the default browser when it has Custom
  Tabs, else the first that does, else a plain browser. An unseen activity,
  `WebSignIn`, sits between the game and the tab, as AppAuth's does, and
  hears the tab shut as it resumes. The redirect lands on `Redirect`
  (an intent filter each of `[android] schemes`, added at export), which
  brings `WebSignIn` back with the URL and clears the tab; `WebSignIn`
  resuming with none answers `cancelled`, and `close_web_sign_in` brings it
  back the same way, shutting the tab. A link on those schemes that ends no
  open page goes to `Links` instead, never both, and brings the game
  forward, starting it when it was not running: `Links` holds what lands
  until the game listens on its first tick (`Arrivals`, plain Java a JVM
  test runs, which `Notifications` holds its taps in too), so the launch
  link arrives as one that lands while the game runs does, each once, as
  `url_opened` on `platform_event`. A task reopened from Recents replays
  the intent it began with, which `Redirect` passes over.
  The device shims are unseen activities because NativeActivity forwards no
  permission result or activity result: `Permission` asks for
  POST_NOTIFICATIONS, `Share` hears the chooser shut (`Chosen` hears what
  was picked), and `Opened` takes a notification's tap and brings the game
  forward. A tap that clears one of them off the game answers as it goes, so
  no call waits on an activity that is gone. The chooser's pick is a
  broadcast and its shutting an activity result, which Android orders
  neither way, and the result often lands first: a pick answers `shared` the
  moment it arrives, and a chooser shut with none waits two seconds for one
  before answering `cancelled` (`ShareAnswer`, plain Java a JVM test runs). `AlarmManager` holds the schedule, inexact,
  and `Alarm` posts each one, with the small icon the export wrote for
  `[android] notification_icon` (a white silhouette, named in the
  application's meta-data to `Alarm` and to FCM), else the system's
  reminder, since an adaptive launcher icon as a small icon crashes
  Android 8's status bar. A tap that started the process waits in the
  shim until the game's first tick listens. A shared file goes out as a
  content URI from `Files`, a read-only provider of the app's own files
  whose authority the export names after the application id.
  `google.sign_in` is Credential Manager with the bundle keys Google's
  `googleid` library writes, so Play services answers it with an id token for
  `[google] server_client_id`; below Android 14, off Android, with no id or with
  no Google account on the device it answers `unsupported`.
- **Google's libraries are pinned and picked, not built.** Play Games, Billing
  and Play's review prompt need Google's AARs, so the template also carries
  `play/`: every library the roots in `crates/balaur_google/play/libraries.json`
  pull in, resolved from their POMs once (highest version wins, as Gradle's),
  each file downloaded against its pinned SHA-256 and dexed on its own, so two
  capabilities share a library's dex. Their resources are linked once at
  template build to fix their ids (`play/ids.txt`), and their R classes are
  generated from that link; an export links what it keeps with
  `--stable-ids`, renamed to the game's package, so the prebuilt R classes
  hold the ids the APK has. Each library's manifest entries are merged per
  set of capabilities at template build, one element a name. An export keeps
  what its `[android] capabilities` need as `classes2.dex` on (multidex is
  native from API 21), and a game declaring none ships none of it. One
  template carries every capability, adding 2.7 MB to its download, and an
  export picks; a template per set of capabilities would be seven. A capability's shim
  (`play/<capability>/`) is compiled against its libraries, loaded by name
  through `balaur_platform::start_capability`, and answers through the
  platform's own native `answer`; a class the export left out answers
  `unsupported` naming the capability to add.
- **One purchase API over every store.** `platform.products`, `purchase`,
  `finish_purchase`, `restore_purchases` and `entitlements` answer in one
  shape, `platform::Product` and `platform::Transaction`, which each store's
  shim writes (StoreKit's Swift, Play's Java), so the Rust between them passes
  a map through. A transaction's `token` is what a server verifies (the App
  Store's JWS, Play's purchase token); `finish_purchase` waits for the game
  to say so, since a store keeps an unfinished one coming back. A purchase
  opens the store's sheet, so it is a write and waits for its tick to
  settle. Steam answers `unsupported`: its microtransactions are a Web API a
  server calls.
- **A push token is shaped for the server that sends.**
  `platform.register_for_push` belongs to the OS, not the store, so it sits
  on the device seam: APNs through the application delegate on Apple, FCM through
  `firebase-messaging`'s shim on Android. It answers `push_registered` with
  `token`, `provider` and `platform` spelled as Gamend's `POST
  /me/push_tokens` takes them (`PUSH_PROVIDER_*`, `PUSH_PLATFORM_*`), so a
  game hands the answer over as it is. The token reaches neither OS's caller
  directly: Apple's delegate and FCM's service hand it over, a call waiting
  takes it, and one with none waiting is `push_token_changed`. A push in
  front is `push_received`; a tap is `notification_opened`, the same arrival
  a local notification's tap is, with the push's data, which on Android
  comes off the intent `Game` resumes with: the launch intent, or the new
  one a tap handed the running game. A build without the capability answers `push_failed`
  naming it: the entitlement or the shim is missing, and saying so beats a
  token that never comes.
- **A Mac has two builds: the download and the store's.** Apple grants Sign
  in with Apple, Game Center and in-app purchase to a Mac App Store app
  alone, and push to both. `balaur export --target macos-universal
  --app-store` is the store's build of the same template, as `--steam` is
  Steam's: sandboxed (`com.apple.security.app-sandbox`, with
  `network.client` for a server), claiming `com.apple.application-identifier`
  and `com.apple.developer.team-identifier` from `[apple] team_id`, signed
  with an `Apple Distribution` identity against a Mac App Store profile,
  with no hardened runtime and none of its exceptions, and wrapped by
  `productbuild` as `<output>.pkg` beside the `.app`, signed with the
  installer identity (`--installer-identity`, `[apple]
  macos_installer_identity`, or the distribution identity's team's `3rd
  Party Mac Developer Installer`). It needs `[apple] category`, which the
  store requires, a profile and an installer identity whenever it signs,
  and refuses `--steam` and notarization. Unsigned it needs no profile and
  is what the signing image takes, as a Developer ID export is: the
  sandboxed `.app` with its entitlements beside it, which `docker/sign.sh`
  signs with rcodesign against the Mac App Store profile and packages with
  `docker/pkg.py`, the same flat package `productbuild` writes, made without
  a Mac, then signs with the installer identity. User data already lives in the
  container the sandbox gives it, so no file entitlement is asked for. At
  run time a Mac reads its own signature: Game Center's verbs answer
  `unsupported` without Game Center's entitlement and the purchase verbs
  without the sandbox, and `platform.is_available` says so, so a Developer
  ID build falls back to a site's checkout and Apple's web sign-in.
- **A link that opens the game is a platform arrival.** `[apple] schemes`
  writes `CFBundleURLTypes` and `[apple] associated_domains` the
  `com.apple.developer.associated-domains` entitlement (`applinks:host`),
  which a profile must grant as a capability's must. Either link reaches the
  game as `url_opened` with its `url` on `platform_event`: the application
  delegate's proxy goes up as launch ends (iOS's
  `UIApplicationDidFinishLaunchingNotification`, a Mac's
  `NSApplicationWillFinishLaunchingNotification`) and hears
  `application:openURL:options:`, `application:openURLs:` and a universal
  link's `continueUserActivity`. The iOS launch URL also comes from the
  window layer's launch options, handed over on the first frame, and the two
  are told apart so it is heard once. On Android a link on one of
  `[android] schemes` lands on `Redirect` and arrives the same way, the
  launch link too, unless it ends an open `platform.web_sign_in` page.
- **A test answers from a file.** `BALAUR_STAND_INS` names a JSON object of
  `"<module>.<call>"` to the answer its waiter receives (`null`: never
  answers); such a call never reaches its backend, and its answer crosses the
  same channel, so it is recorded like a store's (`stand_ins.rs`).
- **Capabilities are export-side.** `[apple]` in `project.toml` names identifier,
  team, deployment target and capabilities; `balaur export` writes the
  `Info.plist` and `.entitlements` and refuses what the minimum OS cannot
  satisfy, and a `min_ios` or `min_macos` under the template's own: the iOS
  template's `MinimumOSVersion`, the macOS binary's `LC_BUILD_VERSION`, since
  a flat runtime has no plist. Frameworks link into Apple templates
  unconditionally, since a prebuilt template cannot add one later.
- **What only a profile grants is checked against one.** Every capability but
  `in-app-purchase` writes an entitlement Apple grants through a
  provisioning profile alone, so a bundle embeds the one it is signed
  against (`embedded.mobileprovision` on iOS,
  `Contents/embedded.provisionprofile` on a Mac, from `--provisioning-profile`
  or `[apple] ios_provisioning_profile` / `macos_provisioning_profile`). The
  export reads the profile's plist out of its CMS envelope, without checking
  Apple's signature, which the device does; its `aps-environment` is the one
  the bundle claims. iOS refuses a profile that does not grant an
  entitlement the capabilities write, naming the capability. A Mac leaves
  such a capability out with a warning instead: a Developer ID profile
  grants less than its App ID (no Sign in with Apple), and the game runs
  without it, `apple.sign_in` answering `unsupported` there from the
  signature's own entitlements, so a game falls back to Apple's web page. A
  Mac build signed with an identity and no profile is refused; an ad-hoc
  one, the dev build, signs with the hardened runtime's exceptions alone,
  since macOS kills a process claiming an entitlement no profile grants, and
  keeps the full `.entitlements` beside the bundle for a signer.
  `docker/sign.sh` embeds a mounted `macos_provisioning_profile` and leaves
  out what it lacks the same way.

## Timings

`App::tick` measures each stage and publishes the frame whole, so a reader never
sees half of one.

- `engine.timings()` gives `{ frame, fixed_steps, stages, spans }` in seconds;
  the Profiler dock draws a bar per stage against 16.7 ms, and `balaur run
  --timings` prints mean, worst and share of a frame.
- Stages are coarse on purpose: nine `Instant::now()` calls a frame, beneath
  the noise. Finer is a named span (`timings::measure(eng, "physics3d/step",
  ...)`), paid for by the plugin that asks; work outside the stages files
  itself with `timings::record`. Core names `scripts/update`,
  `scripts/fixed_update`, `scripts/reload`, `scene/transforms`.
- `fixed_steps` sits beside the stages because a free-looking `fixed_update`
  usually means the accumulator had nothing to drain.
- `--timings` opens with the boot, up to the first picture on screen: `device`,
  `renderer` and `shaders` are what opening the window cost, and `later shaders`
  counts pipelines built after it, each a stall on the frame that asked. A driver
  compiles what it has not seen, so a first launch and a later one are different
  runs: `scripts/first_launch.py` measures both on macOS.
- A post pass compiles when settings turn it on (`Window::prepare_post`), not
  on the frame it first draws. The 2D pass is skipped when the 2D scene has no
  children, so a 3D-only project pays no 2D film traffic.
- The app icon reaches the desktop from the third frame, and never on an
  offscreen run (`app_icon.rs`, `AFTER_FRAMES`).
- **Timings are an observer**: never recorded, replayed or hashed, so a
  `fixed_update` branching on one would desync and the digest would say so.

## Determinism

Identical inputs produce bit-for-bit identical simulation on every platform.

Balaur Script fits: it compiles to WebAssembly, whose integer and float
operations are exactly specified, the interpreter canonicalises NaNs, and the
JIT, the native build and the interpreter run the same module. Its `Vec2`,
`Vec3` and `Color` are values, copied where they are bound, as Godot's are.

| Hazard | Status |
| --- | --- |
| `f64::sin/cos/exp/pow/...` call the platform libm | **Done** — the `math` module is pure-Rust `libm`, and a script reaches transcendentals only through it |
| A random source seeded from entropy | **Done** — `random` is an engine-owned PCG32 with a fixed default seed |
| Wall-clock or variable `dt` in simulation | **Done** — `FixedUpdate` runs on one accumulator at `fixed_dt()`; `--fixed-tick` pins the frame too, so an interactive run reproduces a headless one. Input is one snapshot per frame |

Engine-side:

- rapier's `enhanced-determinism` workspace-wide; physics at `f32` only.
- `DetHashMap` (`IndexMap` + unseeded FxHasher) wherever order matters, never
  bare `HashMap`. Scene instantiation and scene-key order are deterministic.
- Rendering and audio are pure observers; a headless run computes what a
  windowed run computes.
- Transcendentals use `libm`, glam's included (`glamx/libm`,
  `glamx/scalar-math`). `house_lints.py` fails a bare `.sin()` in Rust; a
  script has no other path to one.
- Dev-mode and shipped bytecode come from one compiler configuration.
- The solver threads on rayon wherever the default `parallel` feature builds,
  which is every native target. `tests/suite/threads.rs` asserts one thread and
  eight give the same digest, so the default follows the machine: one less than
  `available_parallelism`, capped at eight. rapier compiles its threaded
  pipeline only under `not(unsync-callbacks)`, so a script has no physics
  hooks: the `layers`/`mask` groups filter contacts, and a one-way platform's
  axis is collider data.
- Live particles are backend state seeded from their emitter's entity bits,
  so the engine's `random` stream is untouched and headless runs digest alike.
- `crates/balaur_physics/tests/suite/determinism.rs` asserts two runs match per tick,
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
- **Components are not the whole simulation**: `body3d` reports its kind, not
  its velocity. Plugins add what a step computes through
  `App::add_digest_source`; physics adds velocity and sleep state in both
  dimensions. A soft body's entry hashes every piece's particle velocities and
  its topology version, so two machines that tore differently diverge on that
  step.
- The walk is scene-tree order: peers that agree have the same tree, and a
  reparent is state worth catching.

### Record and replay

A recording is JSON Lines: a header, then one line per tick with that tick's
external input, its step and its digest. Line-oriented so it greps and
streams, flushed per frame so a crash leaves every tick. `docs/DETERMINISM.md`
has the commands.

- Twelve sources register: `input`, `gamepad`, `http`, `websocket`,
  `platform`, `apple`, `multiplayer`, `device`, `wall_clock`, `ui`, `browser`
  and `audio_device`. Setups carry into the header what `init` read: the
  bindings, the platform, the device, the physics world, the audio device and
  animation. Most serialize by derive; `Pad` needs a hand-written conversion
  because an axis name is a `&'static str`.
- The header's `physics_world` carries both worlds' tuning and gravity, and a
  replay restores them before its first tick, over the local manifest.
- **Restore re-enters the real path**: a recorded event goes down the same
  channel the workers use, so dispatch, handler lookup, await-wake and arrival
  order are the originals.
- **How work leaves a tick is `core::task`.** `ExternalIo` is how it reports
  back; the other half is getting off the tick, and `task::step` is that for
  work that can be cut into slices. A desktop hands the job a thread, a browser
  has none, so `task::park` advances one slice per frame from
  `advance_parked_system` at `Stage::First` and `step` is `park` there. A job
  moves to a thread natively, so it is `Send` there and not on the web, and it
  reads the file backend where it runs rather than carrying an `Rc` across.
  Work that waits on a socket or a fetch is not this shape: its two halves are
  different code, a thread blocking in `ureq` against a task awaiting Fetch.
  So each subsystem keeps a `mod backend` per target, and only the event and
  the pump are shared.
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
  nothing about input, and a source the recording build did not have is left
  alone. A change to a source's shape bumps `replay::FORMAT`, and a file of
  another format is refused.

### What determinism is still missing

1. **Nothing forces gameplay into `fixed_update`**: a game may still simulate
   from `update`.
2. **Hot reload is a hazard by design.** A reload ends an open recording, with
   `reload` in its trailer; a networked run must still disable it.
3. **A run-time `scene.instantiate` reuses the file's ids**, so two copies
   collide. A minted prefix per instance is held back because the editor
   builds the game's scene through that call and addresses it by id.
4. **A channel outside core must answer for itself.** `house_lints.py` fails an
   `mpsc` channel in a file naming neither `ExternalIo` nor
   `replay::suppressed`; nothing else forces a new subsystem through it.

### Rollback

`core::snapshot` keeps a registry of sources (`save` to JSON, `load` back) and a
`CheckpointRing` of the last N ticks. `docs/DETERMINISM.md` has how a game
uses it.

- Core snapshots the node set, transforms, appearance, tags, pause, clock,
  timers, each script's plain fields and the RNG; every
  other subsystem registers its own (physics through serde, animation). Core
  does not snapshot components: re-adding a `body3d` would rebuild the body and
  discard its velocity.
- Scripts are hybrid: define the two methods, or get plain fields captured
  (functions and userdata skipped, nodes as entity bits). `self.node` is never
  captured; the host owns that binding.
- **Restore puts the node set back**: the `nodes` source records id, name,
  parent, components and script, and registers first, so it frees what was
  spawned and respawns what was freed before any other source writes.
  Components go back through the same `apply` a scene file uses. Keyed by
  `StableId`, with the entity index as fallback for a hand-built tree. The id
  counter is snapshotted, so a re-run mints the id the first run did.
- `rollback::Session` owns the tick, the ring and a journal of who pressed what
  when. A missing input is predicted by repeating the last one, right for a
  held button and wrong exactly when it changed; a disagreeing real input restores
  and re-runs, an agreeing one costs nothing.
- The re-run is invisible because the clock is a snapshot source, and
  `is_resimulating` is checked by `ExternalIo::start` alongside `is_playing`.
- Scripts see `rollback.input(player)` and `rollback.is_resimulating()`.
- The session steps at `App::fixed_step` and takes no `dt`: the substep
  accumulator lives outside the snapshot, so a variable step could re-run a tick
  with a different number of steps.
- `balaur_net::netsession` puts the session behind a `Transport`. Every
  datagram carries the last twelve ticks of that player's input. Inputs are
  never retransmitted, so one dropped would be a permanent divergence;
  repeating costs a few bytes and lets one packet repair every gap behind it.
  Invisible on loopback, obvious under `balaur_net::transport::Faulty`.
- Payloads arrive through `PeerTraffic`, an `ExternalIo` behind a `multiplayer`
  replay source, so a recorded session replays with nothing on the other end.
  `NetSession::stats` (round trip, loss from sequence gaps, bytes) is an
  observer. `Session::stale_inputs` counts inputs older than the ring: a
  divergence to resync out of, not a log line.
- Not done: replicating state. Inputs cross unreliably, digests reliably for
  the desync check.

## Settings

One registry (`core::settings`) addressed by path:
`physics/solver_iterations`, `editor/appearance/theme`.

- The path is the storage (`editor/appearance/theme` is `[editor.appearance]
  theme`), so there is no second registry of tables. The manifest follows:
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
- An override is a setting's own path under `override/<tag>/`,
  `[override.android.window]` in the file. `settings::get` tries this run's
  tags narrowest first, then the base value.
- Tags come from the facts a recording restores, so a session recorded on a
  phone replays against the phone's settings.
- An asset variant is `name.<tag>.ext` beside the asset. An export packs the
  one its target answers to under the plain name, so nothing downstream
  learns variants exist.

## Networking and state sync

Transport, session and rollback are built, and a script opens a match with
`multiplayer.*` (`crates/balaur_multiplayer`); replication is not built.
`PLAN-networking.md` has the ordered steps, `PLAN-multiplayer.md` the script API,
`PLAN-gamend.md` the server, `PLAN-voice.md` voice.

A match steps the world itself. `App::advance` hands a live frame to the
`FrameDriver` resource when one is installed, and the multiplayer plugin's
driver runs the ticks owed through `NetSession::advance`, waits a frame when
a peer is behind, and skips Render on a re-run tick. A match starts from one
world: the host loads the match scene and sends its snapshot, and each
joiner loads the same scene and restores it. Nodes tagged `local` stay out
of the digest and of every snapshot, since a camera, a HUD or a sign-in
differs per machine on purpose: a join leaves the joiner's own.

| Primitive a replication layer needs | Where it already is |
| --- | --- |
| Cross-machine identity | `StableId` — survives rename, reparent, reload |
| Generic property read/write | a component's `get` / `apply` hooks, which `components::patch` composes |
| A wire schema | Component schemas: every property declares a `type`, with defaults |
| A tick clock | `Engine::tick`, `fixed_dt()`, `App::set_fixed_dt`; a recording's header carries the rate |

The schema is the notable one: Godot ships a separate `SceneReplicationConfig`
because its nodes have no property schema. An encoder generated off the registry
replicates third-party components with no code in the plugin.

- **Which architecture.** Lockstep costs O(players) and nothing for world size,
  but needs full save/restore and cannot hide information; server-authoritative
  delta replication tolerates nondeterminism, allows join-in-progress and
  resists cheating, at bandwidth that scales with the world. Build the second,
  factored so the first falls out, since both need the same identity, property access
  and clock.
- **Delta encoding** reads the change ticks each `(entity, component)` carries
  (`components::ticks`). Per tick and observer, send the changed properties
  since that observer's last ack against a ring of baselines; quantisation
  comes off the schema's range.
- **Predicting and reconciling.** The client applies its own input on the tick
  it is pressed and holds it pending until a delta acks that tick; the
  correction restores and replays what is pending, over one node. Unowned nodes
  are not predicted: they are drawn a send interval behind and interpolated between the
  two states bracketing render time. A visible correction decays out of the
  render transform and never feeds back into simulation. A server testing a hit
  rewinds to the tick the shooter saw.
- **Transport**: HTTP, WebSocket and WebTransport over QUIC (native, and the
  browser's own `WebTransport`). One crate per protocol, so a game wanting
  `http.request` compiles no QUIC stack. `ExternalIo`, `Handler` and the token
  space are in core, and `Transport` in `balaur_net`, so nothing else is
  shared beneath them.
- **Gamend** is a client in Balaur Script, the library's `addons/gamend`,
  generated in the Gamend repository: the session and its refresh over
  `http`, Phoenix Channels over `websocket` on a node of its own. Its
  arrivals are those two modules', so a recording replays it with no server.
  A generated call is async and takes path parameters as text, `params` for
  the body and `options` for the query. Signing in is `client::login_device`,
  `login_email` or `confirm_registration`; the session refreshes inside
  `core::rest`.
- The server a run talks to is `GAMEND_URL`, else the editor's
  `editor/gamend/server` pick between `gamend/local_url` and `gamend/url`.
  `client::configure` overrides it at run time.
- WebSocket is TCP, so one lost packet stalls everything behind it: right for
  turn-based, wrong for twitchy. Its worker speaks frames itself so
  `permessage-deflate` can set the reserved bit tungstenite's message API
  refuses, and so a listener can unmask client-to-server frames.
- `balaur_net::transport::Transport` is the seam: one reliable ordered
  channel, unreliable datagrams, and a `receive` polled once per tick. It is
  shaped by QUIC, so a transport missing a guarantee fakes it: the websocket
  sends a datagram reliably, which costs latency and is never wrong. Opening a
  link goes through `ExternalIo::start`.
- `balaur_webtransport` sends a datagram unreliably, as QUIC does: `quinn` behind the
  trait via `web-transport-quinn`, on a worker thread with a current-thread
  runtime, so no runtime goes near the tick. Its reliable channel is one
  bidirectional stream, so a payload travels behind a four-byte length. QUIC is
  always TLS: a self-signed certificate pinned by hash, which is what a browser
  accepts; a shipped server passes real files. A datagram carries at most 1200
  bytes, the floor every QUIC path guarantees, whatever the path would take.
- The target is QUIC/WebTransport as the *single* transport, native and
  in-browser: not for raw speed, but so there is no native UDP path and
  browser WebRTC path to maintain. WebRTC data channels stay a fallback for
  browser P2P without a relay; raw UDP is never exposed to scripts.
- **The script-facing shape** is declarative in the scene (`replicate = {
  authority = "owner", components = [...], mode = "on_change" }`) and
  signal-shaped in scripts. RPCs address a node by `StableId`, never by path.

## The editor is a game

`editor/` is a regular Balaur project: a tree of `widget` nodes the engine
lays out, each panel a node with one script that fills it. Five workspaces,
docks that fold and move, the command palette, dark and light token sets.

- `balaur edit <project>` puts the game's path in `engine.args()`. The editor
  builds the game's scene under its `Edited` node with `scene.instantiate`,
  `scripts = false` and the game's `root`. The viewport shows the actual scene,
  and physics builds the actual bodies, paused until play.
- The live nodes are the editor's document. `scene.to_toml` writes a scene
  back from its nodes, defaults left out and a prefab instance as its
  overrides.
- Play attaches the game's scripts to the live nodes and unpauses. Play keeps
  the scene's text, and stop builds the scene again from it. Editing an
  editor script hot reloads the editor.
- Undo is a list of changes `scene.keep_changes` kept, each with the value
  it replaced, keyed by stable id.
- Each command the shell runs is one entry in `editor/scripts/commands.bs`:
  its name, label, icon and when it would act. A row of the `shortcuts` in
  `scenes/panels/keys.toml` names one, the palette lists those that would act,
  and the mark menu shows six. Each row writes its key's chord through
  `ui::chord_text`, so a chord is spelled once. Two rows on one chord run
  whichever would act: F5 continues a stopped script, and plays otherwise.
- A shell button is named for the command it runs: the bar's workspaces and
  transport, the bindings tab's head. Its `call` row hands the panel's script
  the button, which runs `Editor::command(name)`.
- A game's editor plugins sit in its `editor/` folder and in each addon's.
  Only those files and the editor's own may call the editor's modules, so a
  game script that calls one fails in Play as it would in an export.
- One process edits one project. `project::open` relaunches the binary on the
  new project and quits this one.
- `layout.bs` computes every shell rect, and a folded dock hands its width
  back to the centre. Focus is a remembered layout: `shell::toggle_focus`
  records which docks were open, shuts them, and a second call restores them.
- The selection is an ordered set whose head is the active node. A file
  dropped or pasted becomes a node by one rule per extension (`dropin.bs`).
- Lock and isolate are editor state, never in the file or the history.
  Autosave writes a scene 0.6 s after its edits settle, a script 1.5 s
  after typing rests.
- An editor theme is a folder holding `dark.toml` and `light.toml`. A
  person's theme names a bundled `base` and states only what differs.
- Under mode `system` the editor wears the half `engine::dark_mode()` names,
  on every platform. `crates/balaur/tests/suite/editor_theme.rs` holds every
  bundled half to WCAG AA contrast.
- An edit's cost is counted in instructions retired over many frames
  (`scripts/edit_cost.sh`), never in wall time, which a loaded machine moves.
- The editor keeps its window size in `window.toml` in its data folder,
  from its own scripts; the engine has no setting for it.
- The editor in a tab is the same `editor/` project on the wasm build. Its
  files are `files::MemoryFs`, which `balaur_cli::web_store` mirrors into
  IndexedDB. Hot reload follows the editor's own write, and a debugged script
  moves to wasmi (`bs-wasmi`), which can park a call.
- A file dropped, pasted or picked on the page is written under
  `/handed/<n>/` and said as dropped, so `dropin.bs` places it as on desktop.
- A rig is a library scene (`kind = "rig"` in `editor/library/catalog.toml`)
  whose nodes run scripts from `editor/library/scripts/`. `follow.bs` trails a
  target through `node.transform.set_global_position`, so a follower under a
  moving parent lands where it should.
- The gizmo follows steadyum's: an orange bounding-box frame, a face drag
  translating in that face's plane, a rectangle at each face centre scaling
  along its normal, corner brackets scaling uniformly, and a rotation ball whose
  depth-biased rings win the pick over the face behind. Hit-testing and drag
  math are screen-space script in `gizmo.bs`; the backend inhibits the orbit
  camera while the gizmo is hot.
- The Animation workspace edits an engine clip: `anim.bs` is an adapter over the
  `assets` and `animation` modules, and the Timeline scrubs the node's actual
  player, so what is previewed is what ships. The clip lives inline
  (`[nodes.animation.library]`) or in `animations/<node>.toml`; **Save as file**
  and **Make inline** are exact inverses because both hold byte-identical
  documents. Preview goes through `animation.add_clip`, so the engine's parser
  reads the table before it is written.
- The clip belongs to the *player*, the selection or its nearest ancestor
  carrying `animation`, so keying a bone writes a track on the character's
  clip, addressed by path.
- `rig.bs` draws bones as Godot's diamonds and grows a chain by clicking;
  `polygon.bs` turns Godot's UV window into four viewport modes (Points,
  Polygons, UV, Weights). Every edit records history and then writes the
  polygon onto the live node, so undo and the saved scene agree.
- `balaur edit <game> --offscreen --state "animation,select:Limb,tool:polygon,mode:weights" --frames N --shot out.png`
  screenshots any state; `scripts/e2e.sh` runs the self-tests headless.
- The workspace patches `kiss3d` to `github.com/Ughuuu/kiss3d` (`balaur-hooks`):
  upstream v0.46.0 plus the opaque web canvas, IME events, a mid-pass screen
  copy for 2D materials and UIKit's safe area.

## Input

`balaur_input` owns a backend-agnostic `InputSnapshot` (keys by name, mouse,
scroll, per-frame edges) and the `input` module. The kiss3d backend pumps OS
events into it once per frame; headless reads neutral state. One snapshot per
frame is all a replay needs.

**Actions.** A game asks for `"jump"`, not `Space`:

```toml
[input.actions]
jump = ["Space", "gamepad:south"]
move_x = ["keys:KeyA,KeyD", "axis:left_x"]
fire = ["mouse:left"]
```

- Five binding forms: a key's W3C code, `mouse:left`, `gamepad:south`,
  `axis:left_x` (`+`/`-` for one direction), `keys:KeyA,KeyD` (two keys as
  one axis, first negative).
- Scripts read `action_value` (-1..1), `action_down`, `action_just_pressed`,
  `action_just_released`. Every binding contributes and the action takes the
  value furthest from rest, so one action serves a key, a stick and a d-pad. An
  axis has a deadzone, counts as pressed past half throw, and takes its edges
  from comparing frames.
- **The raw snapshot stays underneath**: actions are recomputed in `First` after
  the pad poll and after a replay restored the recording's snapshot, so nothing
  about an action is recorded or rolled back.
- The binding table *is* recorded, in the header beside the RNG seed, since a player
  who rebinds after recording would otherwise replay with a different action
  firing. `App::add_replay_setup` is that seam.
- `input.bind` saves every rebinding to `bindings.toml` in the user data directory;
  `reset_bindings` goes back to the project's. An undeclared action reads 0 and
  warns once.
- A held value names its source and is read again when its event arrives;
  an event only says that something moved.
- Losing focus, or a page's `blur`, releases every held key and button, and
  a pointer leaving the window ends hover. The kiss3d fork sends both.

**Gamepads, motion and rumble.**

- gilrs runs on a thread of its own, one per process (`pad_thread.rs`): a
  controller service can hang inside it, as Windows Gaming Input does after a
  resume from sleep (gilrs issue 196), and there it stops the pads, not the
  game. The thread sleeps until a pad changes and wakes the loop.
- The tick takes that thread's changes in `First`, never waiting on it,
  rather than the windowed backend: a controller is not a window event, and a
  headless run sees one. Not while replaying: the recorded pads were just
  restored. What a script reads of a pad is the engine's contract, not
  gilrs's: `docs/PLAN-input.md` §1.
- **Two readers, one pad, no overlap.** gilrs reads buttons and axes; gyro,
  accelerometer and touchpad come from `sensors.rs` over raw HID (DualSense and
  DualShock 4, USB and Bluetooth, offsets from Linux's `hid-playstation.c`),
  matched by vendor and product and told apart by order. A backend covering both
  readings replaces both rather than joining them.
- Rumble is output, so a recording never carries it: the script asks again
  on replay, and the engine works out its shape in tick time. `can_rumble` *is*
  recorded, because a script may branch on it. No gyroscope, no HID and no pad
  all read zero.
- gilrs silences an effect under gain 0.05, so each motor is three effects a
  factor of 20 apart (`haptics.rs`).
- The gamepad thread sleeps inside gilrs and cannot be woken, so motors are
  built when a pad connects and mappings load at first read. A pad backend
  whose waits come back empty 64 times in a row counts as stopped, and its
  pads unplug.

**Touch.**

- Pointer and finger convert into each other at the top of the tick, once per
  frame, not per event: `emulate_mouse_from_touch` (on) makes a finger a left
  click, which is why every widget kind works on a phone;
  `emulate_touch_from_mouse` (off) makes a held button a finger. The snapshot
  records what the conversion produced, so a replay converts nothing.
- `touch_button` and `touch_stick` are components, not widget kinds, as
  Godot's `TouchScreenButton` is a `Node2D`: the widget pass runs in the
  backend's draw, after actions derive and never headless. They are hit-tested
  in `First` against `DeviceFacts`' recorded `screen_size`, `ui_scale`,
  `safe_area` and `game_area`, the `viewport` over the played content, so a
  game played in the editor keeps them in the stage. `balaur_render` only paints
  them.
- A control *feeds* an action (`InputActions::feed`, `input.set_action_value`)
  rather than being bound to one: the furthest from rest wins against the
  bindings, and a fed action answers even if undeclared.
- Pinch, pan, swipe and long press are derived from the recorded touches and
  the fixed step, never recorded, and skipped on a rollback's second run of a
  tick, where their timers would count twice.
- The keyboard height lives on `DeviceFacts`, beside the safe area a layout
  reads with it; every root but a backdrop measures its bottom from its top.

## Showcase: the manual's pictures are a test

`scripts/showcase.sh` drives the editor offscreen with `--state`. A still is
`--shot <png>`, taken on the budget's last frame. A clip is `show:<name>`, a
sequence in `showcase.bs` of the calls the UI would make plus the input a
person would feed. The `frames:<dir>` state captures it, encoded at 30 fps.
Offscreen frames advance on the fixed step, so a clip is the same clip every
time.

- Two phases: input is fed from `update`, before the game reads the snapshot;
  anything a click on the chrome would do runs at the end of the Editor's
  `update`, after it, so the scene is never rebuilt under the game's own
  `update`.
- Input goes through `input.set_key_down`/`set_mouse_position`/`set_mouse_down`, which
  call the snapshot's own feeders, so a fed frame is indistinguishable from a
  window's and the recorder records it. The pointer is drawn by `inputview.bs`
  from the snapshot rather than the OS cursor, for the same reason. egui never
  sees fed input, so a sequence puts the cursor on a control and calls the verb
  under it. The gizmo stands down while a sequence runs.
- A sequence restores the files it edited, and the script restores its own
  backup after each take.
- `balaur edit` hands the game's `[input]` to `actions::declare_manifest`, so
  a played game reads its own actions, not the editor's.

## Post-processing

`camera.post` lists the frame's passes in order, a `strings` property: the
engine's own (`bloom`, `ssao`, `ssr`, `dof`, `fxaa`, `sharpen`, `tonemap`,
`vignette`, `aberration`, `grain`, `pixelate`), or any `material` asset, drawn
over the whole frame. `bloom_threshold` and `bloom_intensity` sit beside it,
since bloom is unusable without the two numbers.

Where a built-in pass runs is fixed by the pipeline; the list orders the
materials. `tonemap` splits the chain: a material before it works in linear
light, one after it on the finished frame, and a list that does not name it has
it at the head. Post-processing is an observer like the rest of rendering.

- A scene with no `light3d` keeps the backend's own sun, so every example
  draws unchanged; the first `light3d` retires it.
- A project material that blends draws in the kiss3d fork's
  order-independent pass; the engine wraps its `fs_main` in a second `fs_oit`
  entry point.

## Colour

A colour is authored as a screen shows it: `#3c4753` on an unlit shape is the
pixel (60, 71, 83), and `render::set_background(color(0.2, 0.4, 0.6, 1.0))` clears to
(51, 102, 153). The film holds linear light, so `balaur_render::film` decodes
each authored colour once, where the backend hands it to the renderer. Lights
multiply light, and the resolve encodes the frame once.

The decode is the inverse of the fork's resolve, a 2.2 power, so an unlit
colour comes back as the byte written. A light's `color`, `ambient_color` and
`fog_color` are colours like a shape's, and `intensity` multiplies in linear
light. An image is decoded by its sampler, which `srgb` in its import settings
turns off. A shader's `[params]` and a mesh's `colors` are numbers, and reach
the shader as written.

## Camera and screenshots

A `camera3d` node's position and rotation write a `CameraConfig3d`: it looks
along the node's -z, as a `light3d` shines, and `transform.look_at(point)` turns
any node to face a world point. The backend applies it, and no mouse or key
moves the camera between. `render.screenshot(path)`
saves a frame to PNG.

Capture is a binding rather than a flag because *when* to capture is the
caller's business: once the level loaded, or after the hit lands. The
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
- `[window] low_processor` lets a windowed run skip the frames nothing asked
  for. The loop blocks in kiss3d's `Window::wait_events` until input, a
  `balaur_core::wake`, or a repaint egui or `ui.request_repaint` scheduled.
  A worker thread reports with `replay::report`, which wakes it; a browser tab
  checks every 16 ms instead. The editor runs this way, so an idle editor runs
  no script and draws nothing.
- Rendering stays a pure observer in every mode, which is what lets the three
  agree bit for bit.
