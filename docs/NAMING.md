# Balaur naming

Names are the part of the engine that cannot be refactored later without
breaking someone's project, so they are decided here once and checked
mechanically where possible. This file governs the other docs: where
`ARCHITECTURE.md`, `README.md` or a plan disagrees with it, this file wins.

The rule for adding a rule is the one `scripts/house_lints.py` states for
itself: when the same bad pattern shows up twice, it becomes a lint.
`house_lints.py` enforces the Rust half, `scripts/api_lints.py` the script API.

## Scopes

| Scope | Where the name appears | Cost of changing it |
| --- | --- | --- |
| `rust-internal` | Rust items inside `crates/` | Compiler-caught. Free in-tree, semver-major for the `balaur` facade, which re-exports core and every plugin crate. Pre-1.0, so free now, not later |
| `script-api` | Module and function names scripts call | Breaks existing projects, at `balaur export` |
| `scene-file` | Component keys, property names, enum options | Breaks existing `.toml` scenes and the inspector generated from the same schemas |

## Decisions

**D1 — The typemap stays `resource`; game content is an `asset`.**
`Engine::resource::<T>()` is a `HashMap<TypeId, Rc<dyn Any>>` of engine
singletons (Bevy's word); Godot calls disk-loaded content a Resource, and five
of six surveyed engines call it an Asset. Content takes `asset`, so the
collision never happens and no existing code is renamed. Rejected: `Singleton`
(~170 sites to free a word we decided not to use), `Subsystem` and `Server`
(wrong for `ScreenshotRequest`), `Global` (already means world-space here). The
cache is `AssetState`, the parser table `AssetTypeRegistry`.

**D2 — A module is the noun for what it owns, and singular unless it is a keyed
store.** The four plurals are `assets`, `settings` (pages by table, values by
name), `strings` (one translation per key per locale) and `events`
(subscribers by name). Do not add a fifth without adding a line here.

**D3 — Six suffixes, chosen by ownership and lifetime, never by subject.** Is
the value a message or durable data? If durable, does the subsystem that reads
it also own it?

| Suffix | Contract |
| --- | --- |
| `*Config` | Written by callers (scripts, editor, CLI); a consumer applies it and does not own the truth. Pending changes are flagged by a field named `changed` |
| `*Snapshot` | A backend writes it once per frame; everyone else reads. Its doc comment must say what it is under a headless backend |
| `*Buffer` | Anyone appends during the frame; its consumer drains it; empty at frame start |
| `*Request` | One caller inserts it; its handler consumes and removes it |
| `*Registry` | Appended during plugin build only; read-only afterwards |
| `*State` | Owned and mutated by exactly one subsystem, across frames; every writer goes through that subsystem's API |
| *(none)* | Immutable after insertion (`ProjectRoot`, `ScriptArgs`, `ProjectManifest`) |

The suffix is actionable, not decorative: `DebugLineBuffer` names the owner that
must drain it (as `DebugLines`, a headless run grew a `Vec` nothing emptied),
and `*Snapshot`'s headless clause is why a script reading `render.camera_2d()`
headless gets zeros rather than the config's defaults. A type spanning two
categories is **split**, not renamed. `Settings` is reserved for user-authored,
disk-persisted config and must not land on `Config`.

**D4 — Abbreviations: banned where users read them, standardised where they do
not.** `script-api` and `scene-file` spell words out (`string`, not `str`;
`ui.slider`, not `hslider`). `rust-internal` picks one spelling per concept by
what already dominates: `eng: &Engine`, `m` for a binding module. `Det` is a
contrast rule, not an abbreviation rule — see N3.

**D5 — Dimension: lowercase, terminal, and on both sides.** `body2d`, not
`Body2D` or `Physics2DState`: a terminal `2d` sorts next to its 3D twin, and one
grep finds both. Every user-facing key with a sibling carries its dimension
(`shape3d`/`shape2d`, `physics3d`/`physics2d`), because a reader should not have
to know that bare means 3D; a key with no sibling stays plain (`camera`, `mesh`,
`sprite`). `physics` keeps only what spans both worlds — `set_paused`,
`is_paused`, `set_sleeping_allowed`, `sleeping_allowed`, `clear` — so a module
name is not a lie about what is in it. In snake_case `_2d` is its own word
unless the segment quotes a key or module name (`register_shape2d_component`).

## Rules

| # | Rule | Scope | Lint |
| --- | --- | --- | --- |
| N1 | One word, one meaning **within a scope**. `resource` = typemap entry; `asset` = game content; `load` = a live object from a path (raw text is `source`). Synonyms across the Rust/script boundary are fine; homonyms are not | all | REPORT |
| N2 | Every typemap type ends in a D3 suffix or none. A type spanning two categories is split. A `*Config`'s pending flag is `changed`. A `*Snapshot` documents its headless value | rust-internal | REPORT + denylist ERROR |
| N3 | `Det` marks exactly one thing: a collection with fixed iteration order standing in for a std type the house lint forbids. Determinism is otherwise carried by the module and its doc comment | rust-internal | ERROR |
| N4 | In CamelCase the dimension is a lowercase `2d`/`3d` at the **end**. SCREAMING_SNAKE is exempt | rust-internal | ERROR |
| N5 | In snake_case `_2d` is its own word, unless the segment quotes a component key or module name | all | ERROR |
| N6 | Component schema vocabulary is fixed: the tagged-union discriminant is always `kind`; the meta key declaring a datatype is always `type`; type names come from the closed set `parse_schema` rejects departures from. A property never repeats its component's name or reuses another component's name for a different type | scene-file | ERROR |
| N7 | A reader is named for what it returns: no `get_` prefix, and `is_` only for a boolean | script-api | ERROR |
| N8 | Every `set_x` on a `*Config` or `*State` has a reader, or a justification comment. `*Snapshot` and `*Buffer` readers take no setter. Where a setter writes a Config and the reader reads a Snapshot, both say so — command in, truth out, not a round trip | script-api | REPORT |
| N9 | Never encode a flag or mode in a function name: optional flags go in the trailing options table, fixed choices take a `FAMILY_VALUE` constant. Constructors whose argument lists differ in length and meaning stay separate functions | script-api | — |
| N10 | The verb is bound to the parameter type: `install_*(m: &mut dyn Bindings<Engine>)` declares script functions, `register_*(reg: &mut Registry)` registers components, `build_*(reg: &mut Registry)` inserts typemap entries and systems | rust-internal | ERROR |
| N11 | An `install_*` group name is true of every function it registers. Where a line limit forces a split with no honest name, say so at the split | rust-internal | — |
| N12 | `Fn` is reserved for type aliases of a boxed or pointer callable; no struct or enum ends in `Fn`, and no `pub` item is named `*Inner`. A const is named for its contents | rust-internal | ERROR |
| N13 | One local name per concept: `eng` for `&Engine`, `m` for a binding module. `scripts` = script files or compiled bytes, `script_host` = the running host, `script_backend` = what builds one | rust-internal | ERROR |
| N14 | Enum option strings are Balaur's vocabulary, not the backend crate's. If the editor, docs or a test carries a translation for an option, that option has the wrong name | scene-file | — |
| N15 | `*_system` is reserved for anything passed to `App::add_system`. A backend loop step takes a verb bound to the N2 category it touches: `apply_*` (Config in), `publish_*` (Snapshot out), `flush_*` (drain a Buffer), `pump_*` (fill a Snapshot from the OS), `sync_*` (mirror ECS into the backend) | rust-internal | ERROR |
| N16 | A component key names what the scene author manipulates, and its registration says in a doc comment what state it writes — the mapping from key to storage is neither one-to-one nor total | rust-internal | REPORT |

## Deliberate exemptions

Recorded so each stops being cited as precedent for the next.

| Name | Why it stays |
| --- | --- |
| `resource` for the typemap | D1 |
| `DetHashMap` / `DetHashSet` | The prefix is the whole job: it says which one the house lint wants you to use |
| `node.get_component` / `get_node` | N7 exemption. Dropping the prefix gives `node:component(name)` beside `node:components()` — two functions one character apart with unrelated return types, and `node:node(path)` |
| `input.is_mouse_down` | `is_down(key)` and `is_mouse_down(button)` are one question about a held button and must agree |
| `render.set_camera` / `camera_pose` | Not an accessor pair: the setter writes `CameraConfig`, the reader reads the published `ViewportSnapshot`. Command in, truth out — fixed by a doc line under N8 |
| `render.camera_2d`, `set_camera_2d`, `mouse_world_2d`, `draw_line_2d` | Correct under N5; none quotes a key or module name |
| `render` as one large module | Fixable by moving functions between `install_*` fns at zero user cost; a `render2d` split costs ~23 breaking call sites for a boundary `ui` manages without. Revisit past ~30 functions |
| `"ball"` / `"cuboid"` | parry's words, but nothing in the tree translates them and no bug traces to them. 2D's `circle`/`rect` are already design words |
| `render.set_ball` / `set_cuboid`, `physics.add_ball_collider` | N9 does not reach them: `balaur_render` has no physics dependency, and in a dynamic API a function whose argument count and meaning differ stays its own function |
| `rotation_euler` | The Rust field is a quaternion, so bare `rotation` becomes ambiguous the day a quaternion accessor lands. Degrees are additive (`set_rotation_degrees`) |
| `widget.x` / `widget.y` | Anchor-relative offsets against five anchor corners, not a position vector |
| `SHAPE_KINDS_2D` | SCREAMING_SNAKE has no lowercase to be consistent with (N4) |
| The editor's `S` and `k` | 1054 sites threaded as a consistent pair through every draw function, in hot-reloaded code with no compiler behind it. Documented at the top of `editor/scripts/editor.rn` instead |
| The editor's display types (`RigidBody3D`, `MeshInstance2D`, …) | A deliberate affordance for Godot refugees; renaming five of nine would mix vocabularies in one inspector header |
| `scale`, four times over | `node:scale()`, `ui.scale()`, `ViewportSnapshot.scale_factor` and the 2D camera's `zoom` are four scopes, not one. N1 bans a word meaning two things in one scope |
| `scene.spawn` vs `scene.instantiate` | Not synonyms: one empty node against a whole scene file |

## Glossary

`resource` = typemap entry. `asset` = game content from a file. `system` = a
closure in a stage. `load` = produce a live object from a path. `source` = raw
file text. `spawn` = one empty node. `instantiate` = a scene file. `duplicate` =
a private copy of an asset. `kind` = a tagged-union discriminant. `type` = a
schema property's datatype. `scale` = qualified by its module, above.
