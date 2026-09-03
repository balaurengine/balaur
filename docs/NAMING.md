> **Status:** §4 landed on 2026-08-30 — Tables A, B and C in full, together
> with the additive script functions listed with Table B and the `UiState`
> split, the `install_physics2d_api` split, the `DebugLineBuffer` drain
> system and the `parse_schema` validator each of them required. The rows
> aimed at the then-unbuilt asset and animation layers were applied to that
> plan rather than to code; both have shipped since under the names decided
> here. §5's exemptions hold
> as written, and §6's lint rules are still a proposal: nothing in
> `scripts/house_lints.py` enforces any of this yet, and
> `scripts/api_lints.py` does not exist. The tables below are kept in the
> present tense as the record of what was decided and why; read them as the
> specification, not as a to-do list. This file governs the other docs —
> where `ARCHITECTURE.md`, `README.md` or a plan file disagrees with it,
> this file wins.
>
> §4's prerequisite 1 — an e2e step that boots `editor/` headless — **landed
> on 2026-08-31**: `scripts/e2e.sh` now opens every example in the editor and
> fails on a logged ERROR *or* on any scene node the editor cannot resolve in
> its mirror. It came a commit too late to guard the renames below, and it
> immediately caught a live defect (see §4's note). Still open: the
> `--update-baseline` sequencing in §6, which only matters once the rules
> gather debt.

# Balaur naming

Names are the part of the engine that cannot be refactored later without
breaking someone's project, so they are decided here once and checked
mechanically where possible.

This file exists because three questions kept recurring and none of them had
a written answer: *is that a resource or a system?*, *why `CameraConfig` but
`PhysicsState`?*, *what is `DetRng`?* Each has a defensible answer; none of
them was written down, so the tree drifted — 23 typemap types across four
suffixes and 14 bare, two spellings of `2d`, and two words for the same
tagged-union discriminant.

The rule for adding a naming rule is the rule `scripts/house_lints.py` states
for itself: *when the same bad pattern shows up twice, it becomes a lint.*
Nine of the sixteen rules below are mechanically checkable and belong in that
file or its script-API sibling. The rest are prose, and prose decays.

**Three scopes, three costs.** Every rule and every rename is tagged with one.

| Scope | Where the name appears | Cost of changing it |
| --- | --- | --- |
| `rust-internal` | Rust items inside `crates/` | Compiler-caught. Free in-tree, semver-major for the `balaur` facade (`crates/balaur/src/lib.rs:12` re-exports `balaur_core::*` and every plugin crate, so `balaur::DetRng`, `balaur::render::DebugLines` and `balaur::ui::UiState` are published API). Pre-1.0, so: free now, not free later. |
| `script-api` | Module and function names scripts call | Breaks existing projects; a script fails at `balaur export`, at compile time. |
| `scene-file` | Component keys, property names, enum option strings | Breaks existing `.toml` scenes and the editor's inspector, which is generated from the same schemas. |

---

## 1. The four questions, answered

| Question | Answer |
| --- | --- |
| Is `Engine::resource::<T>()` a resource or a system? | A **resource** — an engine-global singleton in a typemap, keyed by `TypeId` (`crates/balaur_core/src/resources.rs:10`). A **system** is a closure run once per frame in a stage (`App::add_system`). Resources are *what exists*; systems are *what happens*. Neither word may take on a third meaning: see D1 and N15. |
| Why `CameraConfig` but `PhysicsState`? | Because they are different kinds of thing, and the suffix says which. `CameraConfig` (`crates/balaur_render/src/lib.rs:155`) is a value the *caller* owns and a backend applies — kiss3d reads it and clears its `changed` flag at `kiss3d_backend.rs:88-98`. `PhysicsState` (`crates/balaur_physics/src/lib.rs:28`) is a rapier world the physics plugin owns outright and steps at `step_system` (`:216`); every writer goes through that crate's own bindings. See D3. |
| What is `DetRng`? | The engine's PCG32 stream (`crates/balaur_core/src/rng.rs:55`). The `Det` prefix is doing nothing there — there is exactly one RNG in the workspace and `Pcg32` is integer-only and bit-identical on every platform by construction, so *every* Rng is the deterministic one. It becomes **`RngState`**. `DetHashMap`/`DetHashSet` keep the prefix, because there the prefix is the whole point. See D4. |
| What are the rules? | §3, sixteen of them, with a Lintable column. |

---

## 2. Decisions

### D1 — The typemap stays `resource`. Game content is an `asset`.

`Engine::resource::<T>()` is a `HashMap<TypeId, Rc<dyn Any>>` of engine-global
singletons. Bevy calls this a Resource. Godot calls game *content* a Resource.
The animation layer needs a word for content shared between nodes, and the
words collide.

**Decision:** the typemap keeps `resource`. Content is `assets` —
`balaur_core::assets`, the `assets` script module, `assets.load(path)`. Zero
renames to existing code; the collision never happens.

The collision is only forced if content takes Godot's word, and it does not
have to. Five of six surveyed engines call disk-loaded shared content an
Asset; only Godot says Resource, and Unity's deprecated `Resources.Load()` is
a live demonstration of how that word ages when it means two things.
The asset-layer plan had already reached this conclusion, so this ratifies
it rather than inventing it. The measured cost of
the alternative: 89 `.resource::<`, 42 `try_resource`, 30 `insert_resource`,
3 `remove_resource` and 6 `Resources` — ~170 compiler-caught sites across
8 crates, to buy a noun.

The real pain behind "is that a resource?" is not the word. It is that
`Engine::resource::<CameraConfig>()` and `Engine::resource::<PhysicsState>()`
read identically at the call site. That is D3's job.

**Rejected.** `Singleton` — accurate, collides with nothing, ~170 sites of
churn to free a word we have decided not to use. `Subsystem` (Unreal) and
`Server` (Godot) — wrong for `ScreenshotRequest`: a one-shot screenshot
request is not a subsystem. `Global` — `global` already means world-space here
(`GlobalTransform`, `node:global_position`), so it trades one collision for
another.

**Consequences the plan must absorb.** The asset-layer plan named the type
`AssetServer` — a `Server` suffix D1 has just rejected and one that is outside
N2's closed set. Decide it before
phase 1 writes it into every future doc: the cache becomes **`AssetState`**
(a `DetHashMap<Key, Rc<Asset>>` owned by the asset layer) and the table of
`register_asset_type` parsers becomes **`AssetTypeRegistry`**. Free today,
~40 sites in three months.

Also: `crates/balaur_core/src/resources.rs:6-8` advertises "(physics state,
render backend, audio device, ...)" and there is no render-backend entry —
kiss3d owns the OS event loop outside the map. Fix that doc line.

### D2 — Module names are the noun for what the module owns.

All 17 script modules besides `assets` are singular: `audio`, `engine`, `fs`,
`gamend`, `http`, `input`, `json`, `log`, `node`, `physics`, `physics2d`,
`render`, `rng`, `scene`, `toml`, `ui`, `websocket`.
`assets` is the one plural, and that is deliberate: a module is plural
only when it is a **keyed store** of many things. `assets` qualifies; nothing
else does today. Do not add a second plural without adding a line here.

### D3 — Six suffixes, chosen by ownership and lifetime — never by subject.

This is the direct answer to "camera config but physics state?". Both names
turn out to be correct; the rule is what makes that legible.

The axis is **not** "who calls the setter" — scripts write both
(`physics.set_gravity`, `physics.apply_impulse` and `audio.play` all mutate a
`*State`). The axis is two questions:

1. **Is the value a message, or durable data?** A message is consumed and
   disappears (`*Request`), or is drained by exactly one consumer each frame
   (`*Buffer`), or is republished wholesale each frame (`*Snapshot`).
2. **If durable, does the subsystem that reads it also own it?** If yes,
   `*State` — every writer goes through that subsystem's own API. If no, the
   value's owner is outside and the reader merely applies it: `*Config`.

| Suffix | Contract | Members today |
| --- | --- | --- |
| `*Config` | Written by callers (scripts, editor, CLI). A consumer applies it and does not own the truth. Durable. Pending changes are flagged by a field named `changed`. | `CameraConfig`, `GridConfig`, and after §4: `ClearColorConfig`, `AppIconConfig`, `CameraInputConfig`, `CameraConfig2d`, `WidgetLayerConfig`, `UiConfig` |
| `*Snapshot` | A backend writes it once per frame; everyone else reads and must never write. **Absent or `Default` under a headless backend** — that trap is the reason the suffix exists. | after §4: `InputSnapshot`, `ViewportSnapshot`, `ViewportSnapshot2d` |
| `*Buffer` | Anyone appends during the frame; its consumer drains it; empty at frame start. | after §4: `DebugLineBuffer`, `DebugLineBuffer2d` |
| `*Request` | One caller inserts it; its handler consumes it and removes it. | `ScreenshotRequest` |
| `*Registry` | Appended during plugin build only; read-only afterwards. | `ComponentRegistry`, and after §4 `SceneKeyRegistry`, `AssetTypeRegistry` |
| `*State` | Owned and mutated by exactly one subsystem. Lives across frames. Every writer goes through that subsystem's API. | `PhysicsState`, `AudioState`, `UiState` (post-split), `PhysicsState2d`, `RngState` |
| *(none)* | Immutable after insertion. | `ProjectRoot`, `ScriptArgs`, `ProjectManifest` |

Every category has at least two members; none is invented for symmetry.

The payoff is that the suffix becomes actionable. `*Buffer` is not
theoretical: `DebugLines` is pushed to at `crates/balaur_render/src/lib.rs:395`
and `:478` and drained **only** inside the kiss3d backend
(`kiss3d_backend.rs:191` and `:205`), so a headless run grows a `Vec` that
nothing ever empties. Nothing in the name `DebugLines` warns you.
`DebugLineBuffer` names the missing owner. (It does not empty the `Vec` — ship
the drain system in the same commit or the rename is 8 sites of diff against a
still-live leak.)

`*Snapshot`'s headless clause is equally concrete: `ViewportSnapshot2d`
derives `Default` with `zoom: 0.0`, while `CameraConfig2d::default()` is
`zoom: 60.0`. A script reading `render.camera_2d()` headless gets `(0, 0, 0)`,
and only `editor/scripts/viewport.rn:20`'s `util::max(zoom, 0.01)` keeps
that from being a division by zero.

**Rejected.** A four-suffix set (State/Config/Registry/Request + bare): it has
no word for the three backend-published readback values or the two drain-me
buffers, so those keep landing on `State` — which is how `InputState`, whose
`begin_frame()` clears over half the struct every frame
(`crates/balaur_input/src/lib.rs:36-43`), ended up in the same bucket as
`PhysicsState`, which is never reset. A `*Settings`/`*Descriptor` tier: correct
prior art (Unity `QualitySettings`, wgpu `InstanceDescriptor`) but zero members
in this tree. **`Settings` is reserved** for user-authored, disk-persisted
config when that tier arrives, and must not land on `Config`.

**One type spans two categories and needs a split, not a rename.** `UiState`
(`crates/balaur_ui/src/lib.rs:37-45`) holds `theme` / `theme_dirty` / `scale`
— script-written, applied at `lib.rs:88-93` — alongside `text_buffers` /
`textures` / `focused_once` / `fonts_installed`, which are plugin cache. The
split is by **ownership, not mechanism**: `fonts_installed` and `theme_dirty`
are the same mechanism (both booleans flipped by `run_pass` ten lines apart at
`lib.rs:85-93`), but nothing outside the crate ever sets `fonts_installed`, so
it goes to `UiState`. `theme_dirty` is `UiConfig`'s pending flag and is renamed
`changed`.

### D4 — Abbreviations: banned where users read them, standardised where they don't.

The two halves of this are different rules, and the scope tag is what
separates them.

- **`script-api` and `scene-file`: no abbreviations.** These are read by
  people who did not write the engine. `str` becomes `string`; `ui.hslider`
  becomes `ui.slider` (there is no `vslider` for the `h` to distinguish it
  from — it is the only abbreviated widget name in a 140-function API).
- **`rust-internal`: one spelling per concept, chosen by what already
  dominates.** `eng: &Engine` leads `engine: &Engine` 164 to 24. Standardise
  on `eng`. `m` for the binding module, as `ARCHITECTURE.md:174` already
  documents.

`Det` is not an abbreviation problem, it is a *contrast* problem, and it gets
its own rule (N3). On the collections it is load-bearing: `DetHashMap`
(`crates/balaur_core/src/collections.rs:14`) is the sanctioned substitute for
the exact `HashMap`/`HashSet` that `scripts/house_lints.py`'s
`nondeterministic-iteration` rule makes a CI **error** to iterate, so the name
teaches "this is the one you are allowed to use". 19 occurrences workspace-wide
— it would be cheap to change and it should not be. On `DetRng` there is
nothing to contrast with, so the prefix costs a decode and buys nothing.

Stated this way the rule also tells a future author when to reach for it: a
`DetInstant` would earn the prefix; a `DetPcg32` would not. Photon Quantum —
the most determinism-obsessed engine surveyed — names its RNG plainly
`RNGSession` inside a `Photon.Deterministic` namespace, because the module
already carries the promise. `crates/balaur_core/src/rng.rs:1` already opens
"The engine's deterministic random stream."

### D5 — Dimension: lowercase, terminal, and on both sides.

Three questions, decided together.

**Casing.** Rust RFC 430 / C-CASE treats a compound token as one word, every
user-facing key is lowercase (`body2d`, `shape2d`, `collider2d`, `physics2d`),
and the crates are `rapier2d`/`rapier3d`.

**Position: terminal, not infix.** `PhysicsState2d` sorts next to
`PhysicsState`. `Physics2DState` sorts nowhere near it, which is why no single
grep finds the 2D half of a subsystem.

**Both dimensions are marked where a user reads the name.** Every user-facing
key with a 2D sibling carries its dimension: `shape3d`/`shape2d`,
`body3d`/`body2d`, `collider3d`/`collider2d`, the presets
`rigid_body3d`/`rigid_body2d`, and the script modules `physics3d`/`physics2d`.
The earlier decision left 3D unmarked, borrowing Unity's asymmetry; it was
reversed because a reader of the component list should not have to know that
"bare means 3D". What that reversal cost, and what it deliberately did not do:

- The `physics` script module could not simply become `physics3d`, because
  `set_paused`, `is_paused`, `set_sleeping_allowed`, `sleeping_allowed` and
  `clear` span **both** worlds. So `physics` now holds exactly those, and the
  per-dimension calls (`add_body`, colliders, impulses, velocities, overlaps,
  `set_gravity`, the `BODY_*`/`SHAPE_*` constants) live in `physics3d` and
  `physics2d`. A module name is no longer a lie about what is in it.
- A component with no sibling keeps its plain name: `camera`, `mesh`,
  `tilemap`, `sprite`. Marking a dimension that nothing contrasts with would be
  noise, not consistency.
- Rust-internal types (`Shape`, `Renderable`, `PhysicsState`) are not renamed
  here. They are compiler-caught and free to follow (N4); the user-facing layer
  is what this decision is about.

**The snake_case carve-out costs zero renames**, which is what makes it worth
stating. In snake_case `_2d` is its own word — *unless* the segment quotes a
component key or module name, where the key is spelled as the key is spelled.

## 3. The rules

| # | Rule | Example | Counterexample | Scope | Lintable |
| --- | --- | --- | --- | --- | --- |
| **N1** | One word, one meaning **within a scope**. `resource` is a typemap entry and nothing else; `asset` is game content loaded from a file; `load` produces a live object from a path (reading raw text is `source`); `components` on a node means the components it has. Synonyms *across* the Rust/script boundary are allowed when each word is native to its side — the ban is on homonyms. | `Engine::resource::<PhysicsState>()` (`crates/balaur_core/src/engine.rs:67`) is the typemap and only the typemap. `scene.spawn` (one empty node) and `scene.instantiate` (a whole scene file) are two verbs for two concepts, correctly. | `scene.load(path)` (`crates/balaur_core/src/engine_api.rs:73`, impl `:236`) does not load a scene — it returns the file's raw TOML via `host.scene_source(rel)`, or nil. `assets.load` is about to mean a genuine cached load one module over. | all | REPORT |
| **N2** | Every typemap type ends in a suffix from D3's closed set, or none. A type that spans two categories is **split**, not renamed. A `*Config`'s pending flag is named `changed`. A `*Snapshot`'s doc comment must state what it is when no windowed backend is running. | `ScreenshotRequest` — inserted at `crates/balaur_cli/src/main.rs:152`, consumed and removed at `crates/balaur_render/src/kiss3d_backend.rs:391-417`. | `ClearColor` (`crates/balaur_render/src/lib.rs:35`) has the `changed: bool` that defines Config and is applied-and-cleared at `kiss3d_backend.rs:139-148`, but reads as a plain colour value. `CameraInputEnabled(pub bool)` (`:149`) is a settable knob named as a predicate. | rust-internal | REPORT + denylist ERROR |
| **N3** | `Det` marks exactly one thing: a collection whose iteration order is fixed, standing in for a std type the house lint forbids. Nowhere else. Determinism is otherwise carried by the module and its doc comment. | `DetHashMap` / `DetHashSet` (`crates/balaur_core/src/collections.rs:14-15`) — the sanctioned substitutes for the `HashMap`/`HashSet` that `nondeterministic-iteration` makes a CI failure to iterate. | `DetRng` (`crates/balaur_core/src/rng.rs:55`) — one RNG in the workspace, `Pcg32` integer-only and platform-identical, so the prefix contrasts with nothing. | rust-internal | ERROR |
| **N4** | In CamelCase the dimension is a lowercase `2d`/`3d` at the **end** of the identifier. Rust-internal 3D types may stay unmarked until renamed; user-facing keys never (D5). SCREAMING_SNAKE consts are exempt. | `Shape2d`, `Renderable2d` (`crates/balaur_render/src/lib.rs:185,192`), `Slot2d`. | `DebugLines2D` (`:97`) sits 95 lines from `Renderable2d` in the same file with the other casing. `Physics2DState`, `Camera2DConfig` and `Viewport2D` put the dimension mid-name, so none sorts next to its 3D twin. `SHAPE_KINDS_2D` (`crates/balaur_physics/src/lib.rs:388`) is exempt. | rust-internal | ERROR |
| **N5** | In snake_case `_2d` is its own word — unless the segment quotes a user-facing component key or script module name, in which case it stays glued. Requires no renames; it explains why both spellings are correct. | Separated: `node_pose_2d` (`crates/balaur_physics/src/dim2.rs:50`), `sync_2d`, `draw_line_2d`, `camera_2d`, `mouse_world_2d`. Glued: `register_shape2d_component` (`crates/balaur_render/src/lib.rs:604`, key `shape2d`), `install_physics2d_api` (`dim2.rs:329`, module `physics2d`). | Nothing in the tree violates it once stated — which is the point. Before it was written down, `render.get_shape2d` and `render.set_camera_2d` sitting 40 lines apart in the same install function read as a coin flip. | all | ERROR |
| **N6** | Component schema vocabulary is fixed. The tagged-union discriminant is always the property `kind` — never `shape`, `type`, `mode` or `variant`. The meta key declaring a property's datatype is `type`, never `kind`. Type names are spelled out and come from a closed set that `parse_schema` **rejects departures from**: `float`, `bool`, `string`, `enum`, `vec2`, `vec3`, `color` (plus `asset` when the asset layer lands). A colour property is `type = "color"`. A property never repeats its own component's name or reuses another component's name for a different type. | `body`, `body2d`, `shape`, `shape2d` and `widget` all name the discriminant `kind` (`crates/balaur_physics/src/lib.rs:405` and four others). The `color` component correctly names its property `rgba` (`crates/balaur_render/src/lib.rs:679`). | `collider` and `collider2d` name it `shape` (`crates/balaur_physics/src/lib.rs:451`, `dim2.rs:435`), so `examples/hello/scenes/main.toml:16` says `shape = "cuboid"` under `[nodes.collider]` directly above `:20`'s `kind = "cuboid"` under `[nodes.shape]`. `widget.color = { kind = "str" }` (`crates/balaur_ui/src/widget_layer.rs:61`) holds a hex string where the `color` component holds a float array. And today `parse_schema` (`crates/balaur_core/src/components.rs:59-61`) is a bare `toml::from_str(...).expect(...)` that validates nothing — the closed set is the target, not the current state. | scene-file | ERROR (needs the validator) |
| **N7** | A script function that only reads state is named for what it returns: no `get_` prefix, and `is_` **only** for a boolean. | `ui.scale` / `ui.set_scale` (`crates/balaur_ui/src/widget_bindings.rs:295,300`); `node.position`, `node.parent`, `node.children`, `render.camera_pose`, `input.mouse_position`, `input.is_down`. 40 of 46 readers already comply. | `render.get_shape`, `render.get_shape2d`, `render.get_color` (`crates/balaur_render/src/lib.rs:412,516,423`) — zero call sites in `editor/` or `examples/`, so the cheapest fixes available. Exemptions are listed in §5 with their reasons, so they stop being cited as precedent. | script-api | ERROR |
| **N8** | Every `set_x` on a `*Config` or `*State` has a reader; a setter without one carries a justification comment at its declaration. `*Snapshot` and `*Buffer` readers take no setter and must not be given one. Where the setter writes a Config and the reader reads a Snapshot, both declarations say so — that pair is **command in, truth out**, not a round trip. | `physics.set_paused` / `physics.is_paused`. `render.set_camera_2d` writes `Camera2DConfig` while `render.camera_2d` reads `Viewport2D`, and `editor/scripts/editor.rn:193-194` reads-modifies-writes across the two deliberately. | Ten setters have no reader: `render.set_camera_input`, `set_background`, `set_grid`, `set_grid_colors`, `set_app_icon`, `physics.set_gravity`, `physics2d.set_gravity`, `physics.set_sleeping_allowed`, `ui.set_theme`, `ui.set_widget_layer` — every one writes a typemap entry that already holds the value. `editor/scripts/editor.rn:101` shadows `set_sleeping_allowed`'s value in `S` because it cannot read it back. | script-api | REPORT |
| **N9** | Never encode a **flag or mode** in a function name. If the value is an optional flag, put it in the trailing options table; if it comes from a fixed set of same-arity choices, take a `FAMILY_VALUE` constant as an argument. This does **not** extend to constructors whose argument lists differ in length and meaning — in a dynamically typed API those stay separate functions. | `scene.instantiate(path, parent, { scripts = false })` (`crates/balaur_core/src/engine_api.rs:226`) and `ui.horizontal(opts, cb)` — the options-table idiom, established in three crates. | `audio.play_looping` (`crates/balaur_audio/src/lib.rs:108`) is literally `state.play(&path, volume, true)` against `play`'s `false`. `render.set_ball` / `set_cuboid` are **not** violations: their arities differ (radius vs three half-extents), and `balaur_render` has no `balaur_physics` dependency to reach `SHAPE_KINDS` through (`crates/balaur_render/Cargo.toml:16-27`). | script-api | REPORT |
| **N10** | The verb is bound to the parameter type. `install_*(m: &mut dyn Bindings<Engine>)` declares script functions. `register_*(app: &mut App)` registers scene components. `build_*(app: &mut App)` inserts typemap entries and adds systems. No bare `install(` or `register(` at that seam, and neither word gets a third meaning. | `install_body_api(m: &mut dyn Bindings<Engine>)` (`crates/balaur_physics/src/lib.rs:328`) against `register_physics_components(app: &mut App)` (`:400`); the 20 `install_*` functions in `crates/balaur_ui/src/widget_bindings.rs` all comply. | `balaur_audio::register(m: &mut dyn Bindings<Engine>)` (`crates/balaur_audio/src/lib.rs:96`) and `balaur_input::register` (`crates/balaur_input/src/lib.rs:361`) do the `install_*` job under the `register_*` name. `install_physics2d_api(app: &mut App)` (`dim2.rs:329`) inserts a resource and adds a system before touching bindings. `theme::install_fonts` (`crates/balaur_ui/src/theme.rs:126`) and `logbuf::install` (`crates/balaur_core/src/logbuf.rs:126`) are a third and fourth meaning of `install`. | rust-internal | ERROR |
| **N11** | An `install_*` group name must be true of every function it registers. Where a house line limit forces a split with no honest name, say so in a comment at the split — a stranded trailing comment is the signal that nobody did. | `install_world_controls` (`crates/balaur_physics/src/lib.rs:267`) and `install_constants` (`:390`) are each exactly what they say. `crates/balaur_ui/src/widget_bindings.rs:3-5` states its splits exist to satisfy `MAX_FILE_LINES`. | `install_shape_api` (`crates/balaur_render/src/lib.rs:411`) registers `set_camera` (`:435`); `install_2d_api` (`:449`) registers the 3D `set_ball` (`:488`) and `set_cuboid` (`:492`); `install_camera_api` (`:288`) registers `set_app_icon` (`:291`), an OS/dock concern; `install_scene_api` (`:344`) registers nothing from the real `scene` module. Stranded trailing comments at `:407`, `:445`, `:528`. | rust-internal | no |
| **N12** | The `Fn` suffix is reserved for type aliases of a boxed or pointer callable; a struct or enum never ends in `Fn`. No `pub` item is named `*Inner`. A const is named for its contents, not for the fact that it is a list. | `SystemFn` (`crates/balaur_core/src/app.rs:35`), `ApplyFn` / `RemoveFn` / `GetFn` (`components.rs:40-44`), and `ScriptHostFactory` (`app.rs:55`), where the descriptive noun says more than `Fn` would. | `pub struct NodeFn { name, call }` (`crates/balaur_core/src/node_api.rs:21`) is not callable — its own doc comment one line above says "One node operation". `pub struct EngineInner` (`engine.rs:24`) publishes eight fields that `Engine`'s accessors exist to mediate, and appears in `docs/generated/crates.md` as if it were API. `DECLARATIONS` (`node_api.rs:27`, `engine_api.rs:25`) says nothing about what is declared. | rust-internal | ERROR (partial) |
| **N13** | One local name per concept, in every crate: `eng` for `&Engine`, `m` for the binding module. `scripts` names script files or their compiled bytes; the running host is `script_host`; the thing that builds one is a `script_backend`. | `fn apply_collider(eng: &Engine, ...)` (`crates/balaur_physics/src/lib.rs:77`) — 164 `eng: &Engine` sites against 24 `engine: &Engine`. | `crates/balaur_ui/src/widget_layer.rs` uses both spellings 54 lines apart (`:64` `\|eng, entity, params\|`, `:118` `fn draw(engine: &Engine, ...)`). `scripts` names four things: the running host (`engine.rs:30,85`), the factory that builds it (`app.rs:70`), the compiler passed to `Pack::build` (`pack.rs:31`), and the compiled bytes (`pack.rs:24`) — so `AppConfig { scripts: None }`, in 13 test files, reads as "this project has no scripts". | rust-internal | ERROR |
| **N14** | Enum option strings in a scene file are Balaur's user vocabulary, not the backend crate's. When the editor, the docs or a test already carries a translation for an option, that option has the wrong name. | `"dynamic"` and `"kinematic"` (`crates/balaur_physics/src/lib.rs:405`) — rapier's words, but also Godot's and Unity's, so nothing translates them anywhere in the tree. | `"fixed"` in the same list is rapier's `RigidBodyType::Fixed`; Godot says StaticBody and Unity says static, and `editor/scripts/model.rn:31` and `:37` both test `kind == "fixed" or kind == "static"` even though `static` is not a legal option — a dead branch recording what the author expected the word to be. | scene-file | no |
| **N15** | Frame work has two vocabularies and they must not blur. `*_system` is reserved for anything passed to `App::add_system`. A backend loop step takes a verb bound to the N2 category it touches: `apply_*` reads a Config into the backend, `publish_*` writes a Snapshot, `flush_*` drains a Buffer, `pump_*` fills a Snapshot from the OS, `sync_*` mirrors ECS state into the backend. Verb and suffix cross-check each other. | `step_system` (`crates/balaur_physics/src/lib.rs:62`, `dim2.rs:331`) — the only two named systems. `apply_camera` / `publish_camera` / `flush_debug_lines` / `pump_input` / `sync` in `crates/balaur_render/src/kiss3d_backend.rs:89,316,204,348,422` already follow it exactly. | `draw_grid` (`kiss3d_backend.rs:153`) reads `GridConfig` and so is an `apply_*` under the rule; `take_screenshot_if_due` (`:392`) consumes and removes a `*Request` and has no verb for it. Seven of nine `add_system` call sites are anonymous closures, so most of the system tier has no names at all. | rust-internal | ERROR (partial) |
| **N16** | A registered component key names what the scene author manipulates, and its registration says in a doc comment what state it writes — because the mapping from key to storage is neither one-to-one nor total. | `register_component("widget", ...)` (`crates/balaur_ui/src/widget_layer.rs:50-51`) writes exactly one `Widget` component. | `shape` (`crates/balaur_render/src/lib.rs:533`), `shape2d` (`:605`) and `color` (`:675`) are three keys backed by the **same two** structs `Renderable`/`Renderable2d`, so `node:set_component("color", ...)` and `node:set_component("shape", ...)` mutate one struct. `body`, `collider` (`crates/balaur_physics/src/lib.rs:401,447`) and `body2d`, `collider2d` (`dim2.rs:385,431`) are backed by **no** component type at all — they write into `PhysicsState`. Nothing says so anywhere. | scene-file | REPORT |

---

## 4. The rename plan

Ordered so the free renames land first. Everything in Table A is compiler-caught
and touches no project file; do it in one pass before anything in B or C.

Two prerequisites, both from things this audit turned up:

1. ~~**Nothing in CI exercises `editor/`.**~~ **Done 2026-08-31**, after the
   renames rather than before them. `scripts/e2e.sh` now runs a fourth pass
   per example — `edit` — booting the editor headless against that project.
   It fails on a logged ERROR and on `did not resolve in the mirror`, an
   invariant the editor states at WARN: the document and the mirror are built
   from the same TOML, so every document node must resolve to a mirror node.
   That second check is the one that earns its keep — when it broke, nested
   nodes silently had no ref, so no inspector, no gizmo and no transform read,
   and nothing failed. Headless means this covers loading, mirroring,
   resolution and asset rebinding, but not drawing, which needs a window.
2. `ARCHITECTURE.md` and `README.md` name script functions by hand and nothing
   regenerates or `--check`s them (`scripts/gen_docs.py` only writes
   `docs/generated/`). `ARCHITECTURE.md:136`, `:288`, `:289`, `:328` and
   `README.md:84` are on the checklist for Table B.

### Table A — free (rust-internal, compiler-caught)

| From | To | Sites | Rule |
| --- | --- | --- | --- |
| `Physics2DState` | `PhysicsState2d` | 20 (in-crate + `crates/balaur_physics/tests/api.rs`) | N4 |
| `Camera2DConfig` | `CameraConfig2d` | 8 | N4 |
| `DebugLine2D` (alias) | `DebugLine2d` | 2 | N4 |
| `Viewport2D` | `ViewportSnapshot2d` | 6 | N2, N4 |
| `ViewportCamera` | `ViewportSnapshot` | 9 | N2 |
| `DebugLines` | `DebugLineBuffer` **plus a headless drain system in the same commit** | 16 | N2 |
| `DebugLines2D` | `DebugLineBuffer2d` | 8 | N2, N4 |
| `ClearColor` | `ClearColorConfig` | 7 | N2 |
| `AppIcon` | `AppIconConfig` | 4 | N2 |
| `CameraInputEnabled(pub bool)` | `CameraInputConfig { enabled: bool }` — the newtype's `.0` is the real defect; a suffix alone would make it worse | 9 | N2 |
| `WidgetLayer` | `WidgetLayerConfig` — frees "widget layer" to mean the feature | 6 | N2 |
| `InputState` | `InputSnapshot`; rename `crates/balaur_input/tests/input_state.rs` with it | 30 | N2 |
| `UiState` | split into `UiConfig { theme, changed, scale }` and `UiState { fonts_installed, text_buffers, focused_once, textures }`. Costs `run_pass` a second typemap lookup and borrow per frame (`crates/balaur_ui/src/lib.rs:81-100`) | 14 → ~20 | N2 |
| `DetRng` | `RngState`. The duplicate seed reset and the cross-crate reach into the resource lived in the Luau backend and left with it, which is what makes it a legal `*State`. | 11 | N3, N2 |
| `SceneVocab` | `SceneKeyRegistry` — structurally identical to `ComponentRegistry`, and `scene_key` is already the word `App::scene_key_handler` uses | 5 | N2 |
| `NodeFn` | `NodeOp` — file-local, zero external references | 29 (all in `node_api.rs`) | N12 |
| `Decl` | `EngineOp` — file-local; its own doc says "One engine operation" | 25 (all in `engine_api.rs`) | N12 |
| `node_api::DECLARATIONS` | `node_api::NODE_OPS` | 7 external | N12 |
| `engine_api::DECLARATIONS` | `engine_api::ENGINE_OPS` | 3 external | N12 |
| `pub struct EngineInner` | `pub(crate) struct EngineInner` — visibility, not spelling; removes it from `docs/generated/crates.md` | 3 | N12 |
| `Engine::scripts()` / `set_scripts()` | `script_host()` / `set_script_host()` | 38 | N13 |
| `AppConfig.scripts` | `AppConfig.script_backend` — the field holds a `ScriptHostFactory`, not a host, and `app.rs:69`'s own doc comment already says "Which script backend to run" | ~25 (13 `scripts: None`, 8 `scripts: Some(..)`, plus `app.rs:70,132` and two sites in `crates/balaur/src/lib.rs`) | N13, N1 |
| `Pack::build(project_root, scripts)` | `Pack::build(project_root, compiler)` — the parameter is a `&dyn ScriptCompiler`; the field `Pack.scripts` (the compiled bytes) keeps the word | 1 signature, 2 calls | N13 |
| `engine: &Engine` | `eng: &Engine` | 24 | N13 |
| `let mut module = app.script_module("ui")?` | `let mut m = ...` (`crates/balaur_ui/src/widgets.rs:142`) | 1 | N13 |
| `balaur_audio::register` | `install_audio_api` | 1 | N10 |
| `balaur_input::register` | `install_input_api` | 1 | N10 |
| `widgets::install` | `install_ui_api` | 1 | N10 |
| `widget_layer::register` | `register_widget_component` | 1 | N10 |
| `node_api::install` | `install_node_api` | 1 | N10 |
| `engine_api::install` | `install_engine_api` — takes `&Engine` because it creates the modules on the host rather than filling one; note that at the declaration | 1 | N10 |
| `theme::install_fonts` | `load_fonts` — reads TTFs and calls `ctx.set_fonts`; nothing is installed into a module or registry | 1 | N10 |
| `logbuf::install` / `install_for_test` | `logbuf::capture` / `capture_for_test` — a tracing subscriber, not a binding seam | 3 (tests) | N10 |
| `install_physics2d_api` | split into `build_physics2d(app: &mut App)` (resource + system) and `install_physics2d_api(m: &mut dyn Bindings<Engine>)`, mirroring `PhysicsPlugin::build` at `lib.rs:60-72` | 1 | N10 |
| `install_scene_api` | `install_backdrop_api` — it registers `set_background`, `set_grid`, `set_grid_colors`, `draw_line`, and `scene` is a real and different module owned by `balaur_core` | 1 | N11 |
| *(contents, not names)* | move `set_camera` (`:435`) into `install_camera_api`; move `set_ball` / `set_cuboid` / `set_color` (`:488,492,497`) into `install_shape_api`; move `set_app_icon` (`:291`) into a new `install_window_api`; what remains of `install_2d_api` becomes `install_camera_2d_api`. Re-check `MAX_FN_LINES` after the moves — the current shape was cut by line count. | 5 fns, one file | N11 |
| planned `AssetServer` | `AssetState` (the cache) + `AssetTypeRegistry` (the parser table) — decided before either was written, and what the asset layer shipped as | — | D1, N2 |

### Table B — breaking the script API

Every row here also needs `docs/generated/script-api.md` regenerated and the
hand-written mentions in `ARCHITECTURE.md` / `README.md` updated.

| From | To | Call sites in `editor/` + `examples/` | Rule |
| --- | --- | --- | --- |
| `render.get_shape` / `get_shape2d` / `get_color` | `render.shape` / `shape2d` / `color` | **0** | N7 |
| `scene.load(path)` | `scene.source(path)` — `source` because it contrasts with `fs.read` (a plain disk read with no pack fallback) and because the host method it calls is already `scene_source` | **0** | N1 |
| `audio.play_looping(path, volume)` | `audio.play(path, { volume = 1.0, loop = true })` — `play` already takes an optional second argument; stops the combinatorial explosion when fade/pitch/bus arrive | **0** | N9 |
| `ui.hslider` | `ui.slider` | 2 (`editor/scripts/dock.rn:166`, `inspector.rn:173`) + `crates/balaur_ui/tests/pass.rs:132` | D4, N9 |
| `scene.components()` | `scene.component_types()` — sits correctly beside `scene.component_schema(name)`, and stops reading as "the components in this scene" | 1 (`editor/scripts/palette.rn:38`) | N1 |
| `node.add_component` | delete it; keep `node.set_component`. Both entries share `call: add_component` (`crates/balaur_core/src/node_api.rs:99-106`); `set_component` has 7 in-tree call sites to `add_component`'s 1, and the family reads `set_ / get_ / has_ / remove_`. | 1 (`editor/scripts/model.rn:253`, the editor's only Add-component path) + 4 lines in `crates/balaur/tests/components.rs:34-37` + `ARCHITECTURE.md:136` + `crates/balaur_core/src/components.rs:9` | N1 |

**Additive, non-breaking, same pass:**

| Add | Why |
| --- | --- |
| `physics.sleeping_allowed()` | `editor/scripts/editor.rn:101` shadows the value in `S` because it cannot read it back — the one setter with a demonstrated missing reader. The other nine wait for a caller; N8 is a REPORT, not a mandate to ship nine functions nobody calls. |
| `input.mouse_just_released(button)` | The keyboard family is complete (`is_down` / `just_pressed` / `just_released`, `crates/balaur_input/src/lib.rs:373-385`); the mouse family stops at two, so drag-release has no binding and `examples/angrynerds` infers it by hand. |
| `physics2d.add_body`, `physics2d.add_collider` | `add_body` already exists as a private Rust fn at `crates/balaur_physics/src/dim2.rs:64` and is simply never bound. `physics2d` has 7 functions to `physics`'s 11 and no constructors at all. |
| `node.rotation_degrees` / `set_rotation_degrees` | `editor/scripts/inspector.rn:106-111` authors in degrees against `rotation_euler`'s radians. Godot-style, additive, breaks nothing. |
| planned `assets.duplicate(path)` | The plan said `assets.instance(path)`. `instantiate` is spoken for by `scene.instantiate`, which builds nodes; the private-copy operation is Godot's `duplicate()`, which is what shipped. |
| planned `animation` module, `animation.stop(id)` | The plan says `anim` and `anim.kill(id)` (`:257-290`). Every existing module is a full word or an established initialism, the plan's own scene key is `[nodes.animation]`, and `kill` would be a third destruction verb beside `node.queue_free` and `physics.clear`. |

### Table C — breaking scene files

Two of these touch no `.toml` at all — do those first.

| From | To | Sites | Rule |
| --- | --- | --- | --- |
| schema meta key `kind` | `type` | 6 Rust literals (`crates/balaur_physics/src/lib.rs:405,451-453`; `dim2.rs:389,435-440`; `crates/balaur_render/src/lib.rs:537-539,609-611,679`; `crates/balaur_ui/src/widget_layer.rs:55-62`), the module doc at `crates/balaur_core/src/components.rs:18`, and **one** editor line, `editor/scripts/inspector.rn:248` `local kind = spec.kind`. **No `.toml` changes.** Do it before the asset layer: today every discriminant reads `kind = { kind = "enum", ... }`, and the asset-layer plan proposed `library = { kind = "asset", type = "animation_clip" }`, pinning a third meaning onto `type` at the level where `kind` already means the datatype. | N6 |
| schema type `str` | `string` | `crates/balaur_ui/src/widget_layer.rs:56,61,62` plus test schemas. **No `.toml` changes.** | N6, D4 |
| `collider.shape` / `collider2d.shape` | `collider.kind` / `collider2d.kind` | 13 scene sites (`examples/hello/scenes/main.toml:16`, `examples/hello_rune:16`, `examples/angrynerds` ×11), `crates/balaur_physics/tests/api.rs:29,74`, and **three editor readers the first pass missed**: `editor/scripts/editor.rn:271` `if c.shape == "circle"` (no fallback — after the rename every 2D circle collider silently draws as a box in the Show-colliders overlay), `gizmo.rn:162` `shape.kind == "ball" or shape.shape == "ball"`, `gizmo2d.rn:68` `shape.kind == "circle" or shape.shape == "circle"`. Those last two are `kind or shape` disjunctions — dead branches recording the same confusion. | N6 |
| body/body2d option `"fixed"` | `"static"`; `BODY_FIXED` → `BODY_STATIC` | 3 scene files, all e2e targets (`examples/hello/scenes/main.toml:7`, `hello_rune:7`, `angrynerds:59`); the forward maps `crates/balaur_physics/src/lib.rs:161` and `dim2.rs:67`; the reverse maps `lib.rs:437` and `dim2.rs:421`; the schemas `lib.rs:405`, `dim2.rs:389`; the constant `lib.rs:380`; tests `determinism.rs:28,106`, `crates/balaur/tests/components.rs:41,42`, `api.rs:38`, and `constants.rs`, which asserts `BODY_KINDS` against the live registry — so constant and schema must move in the same commit. Unblocks the dead branch at `editor/scripts/model.rn:31,37`. | N14 |
| `widget.color = { type = "string" }` | `widget.text_color = { type = "color" }` | 5 `[nodes.widget]` blocks across **three** example projects (`examples/hello/scenes/main.toml:53`, `hello_rune:53`, `angrynerds:17,31,45`), used at `crates/balaur_ui/src/widget_layer.rs:160`. Stops shadowing the `color` component with an incompatible type, and gets the editor's colour picker instead of a text field (`inspector.rn:284` vs `:306`). Requires `type = "color"` to also parse `#rrggbb`, which additionally fixes the silent-grey bug where `color = "#ff0000"` on a node falls through to 0.8/0.8/0.8 because `apply` calls `as_array()` (`crates/balaur_render/src/lib.rs:681-687`). | N6 |
| `widget.size` | `widget.font_size` | Same 5 blocks. It is read as a font height at `crates/balaur_ui/src/widget_layer.rs:161` — and `panel` is already a shipped option (`:55`), so a shipped widget kind has a property that means the wrong thing for it. | N6 |
| *(nothing)* | declare widget's `clicked`: `clicked = { type = "bool", default = false, readonly = true }` | Additive. `get` emits it at `crates/balaur_ui/src/widget_layer.rs:102` but the schema (`:55-62`) does not declare it, so it is invisible to the inspector, to `scene.component_schema("widget")` and to any generic round trip. Enforce with a test asserting every component's `get` key set is a subset of its schema key set. | N6 |
| planned `library = { kind = "asset", type = "animation_clip" }` | `library = { type = "asset", asset = "animation_clip" }` | Decided before the asset layer was written. | N6 |

**Migration, and why "one release of deprecation" does not work here.** The
editor's save is a verbatim round-trip of the parsed document
(`editor/scripts/model.rn:155-163` writes `S.doc`, which came from
`toml.parse` at `:111`); only components the user actually edits are
normalised through `get` (`:243-245`, reached from `:254` and `:276`). A scene
whose collider is never touched saves the old key back forever, so a
deprecation window never expires on its own. And the `options = [...]` list is
served to the inspector through `scene.component_schema`, so a window where
`apply` accepts `"fixed"` but `options` advertises only `"static"` gives an old
scene a dropdown that cannot round-trip its own current value.

So: **flip in one commit.** Rewrite `examples/` and `editor/` in the same
commit, keep the old spelling accepted in `apply` (and listed in `options`)
until the next release, and delete the alias on a date rather than on a
condition.

---

## 5. What deliberately stays as it is

| Name | Why it stays |
| --- | --- |
| `resource` for the typemap | D1. ~170 sites to buy a noun we have decided not to use. |
| `DetHashMap` / `DetHashSet` | 19 sites, and the name is right about both halves — hash-based lookup (FxHasher) with fixed iteration order. No prior-art name conveys "this is the one `house_lints.py` wants you to use", which is the whole job. |
| `PhysicsState`, `CameraConfig`, `GridConfig`, `ScreenshotRequest`, `ComponentRegistry`, `ProjectRoot`, `ScriptArgs`, `ProjectManifest` | Already correct; they are the reference members of their categories. `GridConfig` has no `changed` flag — the backend polls `enabled` every frame at `kiss3d_backend.rs:153` — and is still a Config, because the suffix describes ownership, not mechanism. `ProjectManifest` is inserted after boot (`app.rs:281`), so readers keep using `try_resource`. |
| `AudioState` | Correct as a `*State`. Separately, drop its `project_root: PathBuf` field and read the `ProjectRoot` entry, as `crates/balaur_ui/src/theme.rs:132` already does. |
| `SceneKeyHandler` | It *is* a boxed callable, correctly aliased, and "Handler" says more than `Fn` would — N12 tolerates a descriptive noun for exactly that reason (`ScriptHostFactory`). The real finding is that it is byte-identical to `ApplyFn` (`crates/balaur_core/src/components.rs:40`) and `App::register_component` wraps one into the other at `app.rs:216-226`. That is deduplication, not naming: `pub type SceneKeyHandler = ApplyFn;`. |
| `node.get_component` / `node.component_names` | Renaming both gives `node:component("shape2d")` returning one property table beside `node:components()` returning a list of name strings — two functions one character apart with unrelated return types. Today's pair is ugly and unmistakable. 9 breaking call sites to make it worse. **N7 exemption, recorded here so it stops being cited as precedent.** |
| `node.get_node` / `scene.get_node` | Dropping the prefix gives `node:node(path)`. **N7 exemption**, for that reason and no other. |
| `input.is_mouse_down` | Under N7's boolean clause, `is_` marks a boolean reader — and `input.is_down(key)` and `input.is_mouse_down(button)` are the same question about a held button, so they must agree. De-prefixing only the mouse one would put both spellings of one predicate in one module. |
| `render.set_camera` / `render.camera_pose` | They are not an accessor pair and must not be made to look like one: the setter writes `CameraConfig`, the reader reads the published `ViewportSnapshot` (eye, target, fov, `scale_factor`, `view_proj`, picking ray). Command in, truth out. Fix is a doc line on both under N8, not a rename. |
| `render.camera_2d`, `set_camera_2d`, `mouse_world_2d`, `draw_line_2d` | Correct under N5 — none quotes a component key or module name. Gluing them would break 23 call sites, 14 of them `draw_line_2d` (12 in `editor/scripts`, 2 in `examples/angrynerds/scripts/game.luau`), the most-called render function in the tree. |
| `render` as one 22-function module | The module-scope complaint is real, but it is fixable by moving functions between `install_*` fns (N11, Table A) at zero user cost, whereas a `render2d` split costs ~23 breaking script call sites for a boundary the 38-function `ui` manages without. Revisit past ~30 functions. |
| `physics` / `physics3d` / `physics2d` | D5. `physics` holds only what spans both worlds (`set_paused`, `is_paused`, `set_sleeping_allowed`, `sleeping_allowed`, `clear`); everything per-dimension is in `physics3d` or `physics2d`. |
| `"ball"` / `"cuboid"` | parry's words, and `ball` is odd on a *render* component — but nothing in the tree translates them and no bug traces to them, unlike `"fixed"`. Renaming costs 13 scene sites, the `SHAPE_BALL`/`SHAPE_CUBOID` constants that `crates/balaur_physics/tests/constants.rs` asserts on, and the script setters. Churn for taste. 2D's `circle`/`rect` are already design words. |
| `render.set_ball` / `set_cuboid` / `set_rect` / `set_circle`, `physics.add_ball_collider` / `add_cuboid_collider` | N9 explicitly does not reach them. `balaur_render` has no `balaur_physics` dependency (`crates/balaur_render/Cargo.toml:16-27`) and adding one inverts the layering and drags rapier into every headless render build; and in a dynamically typed API a function whose argument *count and meaning* depend on the runtime value of argument 2 is worse than four fixed-arity functions. `physics.add_ball_collider` also has a live call site (`examples/hello/scripts/ball.rn`), and Rune resolves engine calls at compile time, so the Rune one would fail `balaur export`, not just play. Consequence to accept: `render.shape()` returns `("ball", r, r, r)` and does not round-trip into a single write. Separately, `physics.SHAPE_BALL` / `SHAPE_CUBOID` are exported, documented and test-asserted with no function that accepts them — a REPORT-level oddity; if they still have no consumer in a year, delete the constants. |
| `rotation_euler` | `euler` is not a unit, but the Rust field it exposes is a quaternion (`crates/balaur_core/src/scene.rs:24 pub rotation: Quat`), so bare `rotation` becomes ambiguous the day a quaternion accessor lands. The real defect — radians in the API, degrees in the inspector — is fixed additively (Table B). |
| `widget.x` / `widget.y` | They are anchor-relative offsets applied as `ox`/`oy` against five different anchor corners (`crates/balaur_ui/src/widget_layer.rs:145-151`), not a position vector like `half_extents`. A scene-format break plus a compatibility read to save one inspector row. |
| `SHAPE_KINDS_2D` | SCREAMING_SNAKE has no lowercase to be consistent with. N4 exemption. |
| The editor's `S` and `k` | 710 uses of `S` and 344 of `k` across 13 files, threaded as a consistent `(S, k)` pair through every draw function, with zero collisions against the `for (k, v) in obj` idiom. Renaming 1054 sites in dynamically-typed, hot-reloaded code with no CI coverage buys readability worth less than the risk. Document the pair at the top of `editor/scripts/editor.rn` instead. `editor/scripts/theme.rn:42`'s `local t = M.current` is four uses inside one eight-line function and stays too. |
| `editor/scripts/model.rn:25-49` display types (`RigidBody3D`, `MeshInstance2D`, …) | `M.node_type` returns nine labels; renaming five of them would mix vocabularies in one inspector header, which is worse than today's uniform Godot flavour. It is a deliberate affordance for Godot refugees, rendered as an 11px mono subtitle (`inspector.rn:399`) — its own comment calls it "a friendly type name". |
| `scale`, four times over | `node:scale()` is `Transform.scale`; `ui.scale()` is the design-pixel multiplier; `ViewportSnapshot.scale_factor` is the HiDPI factor; `Camera2DConfig.zoom` is logical pixels per world unit. N1 bans a word meaning two things *in one scope*; these are four scopes and `ARCHITECTURE.md` already disentangles them. Pinned in the glossary below rather than renamed — every alternative (`ui.set_zoom`) collides with one of the other three. |
| `scene.spawn` vs `scene.instantiate` | Not synonyms: `spawn` creates one empty named node, `instantiate` builds a scene file into the tree (`crates/balaur_core/src/engine_api.rs:63,226`). Two verbs, two concepts. |

**Glossary — words this file pins.** `resource` = typemap entry. `asset` =
game content from a file. `system` = a closure in a stage. `scale` = qualified
by its module, see above. `load` = produce a live object from a path. `source`
= raw file text. `spawn` = one empty node. `instantiate` = a scene file.
`duplicate` = a private copy of an asset. `kind` = a tagged-union discriminant
in a component. `type` = a schema property's datatype.

---

## 6. Mechanical checks worth adding

`scripts/house_lints.py` globs `*.rs` only (`rust_files()`), so it has never
seen a single script-API or scene-file name. That is the gap. Two files:

**In `scripts/house_lints.py`**, in its existing style — mechanical rules are
ERROR, heuristics are REPORT:

| Rule id | Severity | Check |
| --- | --- | --- |
| `det-prefix-misuse` | ERROR | `\bDet[A-Z]` on any item that is not a type alias in `collections.rs`. N3. |
| `dimension-casing` | ERROR | `[A-Z][A-Za-z]*2D\b` in a type or alias name; and `2d` appearing anywhere but the end of a CamelCase identifier. Skip SCREAMING_SNAKE. N4. |
| `dimension-snake` | ERROR | `_?2d` in a snake_case identifier that is neither `_2d`-separated nor one of the registered keys/module names. Needs the literal list from `register_component("...")` and `script_module("...")` in the same pass. N5. |
| `install-verb` | ERROR | `fn install_\w+\(` whose first parameter is not `&mut dyn Bindings`; `fn register_\w+\(` whose first parameter is not `&mut App`; any `fn install(` or `fn register(` in a crate that has a plugin seam. N10. |
| `engine-param-name` | ERROR | `engine: &Engine` in a parameter list. N13. |
| `fn-suffix-on-struct` | ERROR | `(struct\|enum) \w+Fn\b`. N12. |
| `pub-inner` | ERROR | `pub (struct\|enum) \w+Inner\b`. N12. |
| `system-verb` | ERROR | a named function passed to `add_system` whose name does not end `_system`. N15. |
| `resource-suffix` | ERROR | *denylist only* — a type at an `insert_resource(` site ending in `Manager`, `Info`, `Data`, `Settings`, `Server`, `Enabled`, `Handler`. An allowlist cannot work: "no suffix" is a legal category, so `ClearColor` and `DebugLines` pass a permissive check. N2. |
| `new-resource-type` | REPORT | any type name at an `insert_resource(` site not already recorded. Choosing *which* category is human judgement; the lint's job is to make the choice happen. Note the one non-literal site, `self.engine.insert_resource(manifest.clone())` (`crates/balaur_core/src/app.rs:281`) — the regex must tolerate it rather than assume every site names a type. |
| `component-registration-doc` | REPORT | `register_component("` with no doc comment on the preceding lines. N16. |

**A new `scripts/api_lints.py`**, reading the JSON `balaur api` already emits
(`crates/balaur_cli/src/main.rs:230-245`, shape `{modules:[{name, functions,
constants}]}`), which `scripts/gen_docs.py:92` already consumes:

| Rule id | Severity | Check |
| --- | --- | --- |
| `getter-prefix` | ERROR | any function named `get_*`, with an allowlist of exactly `node.get_node`, `scene.get_node`, `node.get_component` and the reason for each. N7. |
| `is-prefix-nonboolean` | ERROR | `is_*` on a function whose declared return is not a boolean. N7. |
| `setter-without-reader` | REPORT | `set_x` with no `x` or `is_x` in the same module and no justification comment at the declaration. N8. |
| `abbreviation` | ERROR | any function or constant name containing a segment from a small denylist (`str`, `h`+noun, `cfg`, `buf`, `idx`, `pos` where it is not `position`). D4. |
| `module-plural` | ERROR | a module name ending in `s` that is not on the one-entry allowlist (`assets`). D2. |
| `schema-vocabulary` | ERROR | over the registered schemas: every property spec has a `type` key from the closed set and no `kind` meta key; every tagged union's discriminant property is named `kind`; every key in a component's `get` output is declared in its schema. N6. |

**Sequencing.** `scripts/house_lints_baseline.txt` does not exist — the tree
carries zero accepted ERROR debt today, and the ratchet keys on `path\trule`,
which the file moves and renames in Table A would invalidate anyway. So:
**land Table A first, add the rules second, and run `--update-baseline` as its
own commit with the number in the message**, so "accepted 0" becoming
"accepted N" is a visible event rather than a side effect of adding nine rules.
Turning nine new ERROR rules on against the current tree without the ratchet
would block CI on day one, which teaches people to disable the lint.