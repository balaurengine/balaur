# Engine API for Balaur Script: what the editor calls

> **Status:** audit, 2026-10-20, of the tree before step 4 of [PLAN-engine-in-bs.md](PLAN-engine-in-bs.md), which closed its gaps. `scripts/bs_editor_calls.py` is the live check. How each number was taken is in §11.

The editor (`editor/**/*.rn`, 161 files, 41.6k lines) is scanned for every `a::b` path. Each is one of: an editor
module of its own (ported, not bound), an engine binding, or a constant of an engine module. This file says, for each
engine binding the editor uses, whether a `.bs` script can call it today and with what types.

## 1. Result in numbers

|  | distinct | references | note |
| --- | ---: | ---: | --- |
| editor's own modules (`util::`, `model::`, ...) | 832 | 4019 | 64 modules; ported, not bound (§3) |
| Gamend addon modules (`gamend::core::` ...) | 23 | 317 | files under `editor/library/addons/gamend`; ported (§3) |
| engine functions | 266 | 1923 | 32 modules (§4) |
| engine constants | 78 | 231 | `input::KEY_*`, `math::PI`, ... (§6) |
| Rune type tests `x is balaur::Node` | 2 | 24 | language, not API (§9) |
| **total `module::fn`** | 1201 | 6514 | own + engine functions alone: 1098; the plan says about 1,000 |

What a `.bs` script gets for the engine functions the editor uses (verified against the compiler, §11):

|  | functions | references | share of refs |
| --- | ---: | ---: | ---: |
| reachable, every argument and the result typed | 44 | 228 | 12% |
| reachable, an `any` in an argument or the result | 193 | 1486 | 77% |
| reachable, awaited (`task::wait`) | 1 | 8 | 0% |
| **not reachable** | 28 | 201 | 10% |

Not reachable besides these: all 78 constants (231 refs), and the editor's node-method forms the
node ops do not take (§7).

## 2. How a `.bs` script reaches an engine function

`crates/balaur_script_bs/src/api.rs` `build` makes one `HostSignature` per entry of the binding registry, by this
order (checked on the current tree):

| Registered as | `.bs` gets | Condition |
| --- | --- | --- |
| typed seam (`BindingsExt::function`), Rust types recorded | those types | every argument and the result must map: `f32 f64 i* u* usize` → `float`/`int`, `bool`, `String &str SmolStr` → `str`, `NodeId` → `Node`, `Value` → `any`, `Vec<T>` → `[T]`, `Option<T>` → `T` (**not** optional), `()` → none. Any other Rust type in an argument or the result (in this API: a tuple result, 38 fns; `CallbackId`, 14 fns, all `ui`) → the function is **absent**, and it does **not** fall back to its docs |
| raw (`function_raw`) with a documented signature in `describe` | the docs' types | `string int float bool node vec2 vec3 color` map; `table map list any`, `T?`, `a \| b` and an untyped name → `any`; `[T]` → `[T]`; `fn` → absent. No `-> T` in the docs → the result is `any` |
| node op (`node` module) | `node_ops.rs` `OPS` table, called as a method: `self.node.get_node("a")` | 56 ops; the component an op `acts_on` makes `node.transform.global_position()` |
| `task::seconds/frames/wait` | `waits.rs`, `await task::seconds(1.0)` | async only |
| `script::*` (Rune host, not a binding) | nothing | `BsHost` registers no `script` module |
| constants (`Bindings::constant`) | nothing | `api.rs` `Module::constant` is empty; `input::KEY_SPACE` fails with ``expected `(` after a module function`` |

Across the whole API (906 functions in 44 modules): 835 reachable, 71 absent (19 `script`, 38 with a tuple result, 14 with a `CallbackId`).
`any` values answer these methods only: `kind is_nil as_int as_float as_bool as_str as_node len get(str) at(int) has(str) keys text with(str, any) set(str, any) push(any)`
(`anys.rs`); an `any` cannot be indexed (``any cannot be indexed``).

Consequences seen in the compiler, all probed:

- `any` flows into nothing typed: ``expected bool, got any``. A typed value flows into `any`.
- An optional argument must still be passed. `HostSignature` has no arity range: ``takes 4 argument(s), 3 given``. A Rust `Option<f32>` is a required `float`; a documented `float?` is a required `any` (`nil()`). A Rust `Option<NodeId>` is a required `Node`, and `nil()` is an `any` (``expected Node, got any``), so `events::listen(node, name)` without its third argument has no spelling.
- `node.animation.play(..)` is not a form: ``Node has no field `animation` ``. Only the node ops that `acts_on` a component (`transform`, `states`) are called on it; every other component function is `animation::play(node, ..)`.
- `get_component` takes exactly one argument in `.bs` (``takes 1 argument(s), 2 given``), `call` exactly two, `set_component` exactly two.
- The checker stops at the first error in a file, so one probe file per function was used (§11).

## 3. (a) The editor's own modules: ported, not bound

Counted as references from other editor files (`use crate::x;`, `crate::x::`, or the bare name in `editor.rn`, whose `mod` lines declare them).
`util` alone is 1300 of 4019.

| module | file | lines | distinct fns called | refs |
| --- | --- | ---: | ---: | ---: |
| `util` | `scripts/util.rn` | 425 | 48 | 1300 |
| `model` | `scripts/model.rn` | 2021 | 73 | 501 |
| `signals` | `scripts/signals.rn` | 198 | 11 | 245 |
| `shell` | `scripts/shell.rn` | 1230 | 43 | 210 |
| `icons` | `scripts/icons.rn` | 192 | 6 | 166 |
| `selection` | `scripts/selection.rn` | 296 | 21 | 117 |
| `defs` | `scripts/defs.rn` | 136 | 23 | 102 |
| `history` | `scripts/history.rn` | 130 | 10 | 100 |
| `docks` | `scripts/docks.rn` | 922 | 28 | 95 |
| `polygon` | `scripts/polygon.rn` | 1165 | 32 | 73 |
| `theme` | `scripts/theme.rn` | 293 | 23 | 72 |
| `selftest` | `scripts/selftest.rn` | 4594 | 59 | 61 |
| `manager` | `scripts/manager.rn` | 980 | 28 | 60 |
| `viewport` | `scripts/viewport.rn` | 273 | 10 | 60 |
| `anim` | `scripts/anim.rn` | 649 | 25 | 59 |
| `pool` | `scripts/pool.rn` | 68 | 5 | 51 |
| `window` | `scripts/window.rn` | 206 | 14 | 44 |
| `arrange` | `scripts/arrange.rn` | 291 | 12 | 40 |
| `tiles` | `scripts/tiles.rn` | 1029 | 19 | 38 |
| `session` | `scripts/session.rn` | 416 | 16 | 36 |
| `dropin` | `scripts/dropin.rn` | 403 | 10 | 32 |
| `settings` | `scripts/settings.rn` | 540 | 11 | 32 |
| `lint` | `scripts/lint.rn` | 246 | 10 | 30 |
| `center` | `scripts/center.rn` | 586 | 19 | 30 |
| `rig` | `scripts/rig.rn` | 736 | 14 | 29 |
| `inspector` | `scripts/inspector.rn` | 2045 | 21 | 28 |
| `style` | `scripts/style.rn` | 52 | 7 | 27 |
| `dock` | `scripts/dock.rn` | 1019 | 24 | 27 |
| `newnode` | `scripts/newnode.rn` | 246 | 7 | 27 |
| `importer` | `scripts/importer.rn` | 400 | 5 | 26 |
| `themewin` | `scripts/themewin.rn` | 801 | 10 | 24 |
| `pen` | `scripts/pen.rn` | 426 | 12 | 22 |
| `left` | `scripts/left.rn` | 414 | 15 | 22 |
| `recipes` | `scripts/recipes.rn` | 749 | 10 | 21 |
| `layout` | `scripts/layout.rn` | 588 | 14 | 19 |
| `chrome` | `scripts/chrome.rn` | 523 | 10 | 17 |
| `events` | `scripts/events.rn` | 454 | 9 | 14 |
| `search` | `scripts/search.rn` | 41 | 4 | 13 |
| `complete` | `scripts/complete.rn` | 234 | 8 | 11 |
| `plugins` | `scripts/plugins.rn` | 261 | 9 | 11 |
| `library` | `scripts/library.rn` | 266 | 7 | 11 |
| `exporter` | `scripts/exporter.rn` | 209 | 5 | 10 |
| `populate` | `scripts/populate.rn` | 246 | 5 | 10 |
| `showcase` | `scripts/showcase.rn` | 893 | 6 | 9 |
| `bonemap` | `scripts/bonemap.rn` | 256 | 6 | 9 |
| `weights` | `scripts/weights.rn` | 344 | 8 | 8 |
| `refactor` | `scripts/refactor.rn` | 144 | 6 | 7 |
| `gizmo` | `scripts/gizmo.rn` | 952 | 7 | 7 |
| `overlays` | `scripts/overlays.rn` | 450 | 7 | 7 |
| `textedit` | `scripts/textedit.rn` | 116 | 6 | 6 |
| `physics` | `scripts/physics.rn` | 300 | 4 | 6 |
| `statemachine` | `scripts/statemachine.rn` | 585 | 5 | 5 |
| `gizmo2d` | `scripts/gizmo2d.rn` | 587 | 3 | 5 |
| `about` | `scripts/about.rn` | 117 | 2 | 5 |
| `docs` | `scripts/docs.rn` | 98 | 4 | 4 |
| `inputview` | `scripts/inputview.rn` | 94 | 4 | 4 |
| `kit` | `scripts/kit.rn` | 555 | 3 | 4 |
| `suite` | `scripts/suite.rn` | 138 | 2 | 2 |
| `commands` | `scripts/commands.rn` | 114 | 2 | 2 |
| `fresh_state` | `scripts/editor.rn (root fn)` | - | 1 | 2 |
| `menu` | `scripts/menu.rn` | 72 | 1 | 1 |
| `backfill` | `scripts/editor.rn (root fn)` | - | 1 | 1 |
| `boot` | `scripts/editor.rn (root fn)` | - | 1 | 1 |
| `palette` | `scripts/palette.rn` | 274 | 1 | 1 |

Engine-named editor files (`window`, `settings`, `events`, `physics`) shadow an engine module of the same name only
where `use crate::name;` (or `editor.rn`'s `mod name;`) says so; elsewhere `settings::get` is the engine's. This audit
resolves each file that way.

Files that nothing calls as `module::fn`: `editor.rn` (the root), the panel files in `scripts/panels/` (the scenes in `editor/scenes` attach them),
`library/scripts`, `library/templates`, `plugins/`.

Gamend addon (`editor/library/addons/gamend/*.rn`, 49 files): 23 distinct paths reach it through the engine's `gamend::`
prefix and are its own modules, not bindings:

| path | refs |
| --- | ---: |
| `core::given` | 112 |
| `core::missing` | 112 |
| `core::query` | 63 |
| `core::ok` | 4 |
| `core::body` | 4 |
| `core::error_of` | 3 |
| `events::user::KV_DELETED` | 2 |
| `core::unpack` | 2 |
| `authentication::list_auth_providers` | 1 |
| `authentication::oauth_request` | 1 |
| `authentication::oauth_session_status` | 1 |
| `authentication::link_provider_request` | 1 |
| `authentication::link_session_status` | 1 |
| `events::decode` | 1 |
| `events::user::KV_UPDATED` | 1 |
| `kv::get_kv` | 1 |
| `users::get_current_user` | 1 |
| `lobbies::get_lobby` | 1 |
| `logs::setup` | 1 |
| `logs::pump` | 1 |
| `logs::flush` | 1 |
| `logs::report` | 1 |
| `logs::settle` | 1 |

## 4. (b) Engine bindings by module

`typed` = no `any` in the `.bs` signature; `any` = at least one argument or the result is `any`; `absent` = the compiler says ``the engine has no `x::y` ``.

| module | fns used | refs | typed | any | absent | refs absent | fns in module |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| [`log`](#log) | 7 | 301 | 0 | 7 | 0 |  | 7 |
| [`gamend`](#gamend) | 19 | 299 | 0 | 19 | 0 |  | 19 |
| [`fs`](#fs) | 9 | 183 | 0 | 9 | 0 |  | 9 |
| [`scene`](#scene) | 16 | 144 | 0 | 16 | 0 |  | 22 |
| [`script`](#script) | 19 | 144 | 0 | 0 | 19 | 144 | 19 |
| [`render`](#render) | 32 | 142 | 16 | 11 | 5 | 36 | 73 |
| [`math`](#math) | 15 | 135 | 0 | 15 | 0 |  | 24 |
| [`input`](#input) | 16 | 95 | 11 | 2 | 3 | 13 | 53 |
| [`ui`](#ui) | 20 | 76 | 10 | 9 | 1 | 8 | 74 |
| [`events`](#events) | 1 | 70 | 1 | 0 | 0 |  | 6 |
| [`engine`](#engine) | 16 | 57 | 0 | 16 | 0 |  | 28 |
| [`replay`](#replay) | 17 | 39 | 0 | 17 | 0 |  | 17 |
| [`settings`](#settings) | 10 | 35 | 0 | 10 | 0 |  | 12 |
| [`project`](#project) | 13 | 33 | 0 | 13 | 0 |  | 13 |
| [`assets`](#assets) | 6 | 32 | 0 | 6 | 0 |  | 11 |
| [`toml`](#toml) | 3 | 32 | 0 | 3 | 0 |  | 3 |
| [`json`](#json) | 2 | 18 | 0 | 2 | 0 |  | 2 |
| [`save`](#save) | 3 | 14 | 0 | 3 | 0 |  | 6 |
| [`import`](#import) | 6 | 10 | 0 | 6 | 0 |  | 7 |
| [`release`](#release) | 5 | 9 | 1 | 4 | 0 |  | 5 |
| [`physics`](#physics) | 5 | 9 | 5 | 0 | 0 |  | 14 |
| [`debugger`](#debugger) | 6 | 9 | 0 | 6 | 0 |  | 9 |
| [`task`](#task) | 1 | 8 | 0 | 0 | 0 |  | 3 |
| [`skeleton`](#skeleton) | 3 | 8 | 0 | 3 | 0 |  | 3 |
| [`export`](#export) | 5 | 6 | 0 | 5 | 0 |  | 6 |
| [`animation`](#animation) | 4 | 6 | 0 | 4 | 0 |  | 20 |
| [`http`](#http) | 1 | 2 | 0 | 1 | 0 |  | 2 |
| [`window`](#window) | 1 | 2 | 0 | 1 | 0 |  | 8 |
| [`geometry2d`](#geometry2d) | 2 | 2 | 0 | 2 | 0 |  | 12 |
| [`strings`](#strings) | 1 | 1 | 0 | 1 | 0 |  | 6 |
| [`physics2d`](#physics2d) | 1 | 1 | 0 | 1 | 0 |  | 132 |
| [`physics3d`](#physics3d) | 1 | 1 | 0 | 1 | 0 |  | 142 |

`task`: its one function, `wait`, is awaited (`await task::wait(token)`, inside an `async fn`), so it is counted in none of the three middle columns.

Registered where: all of these come from `Bindings` registrations in the engine crates, except `project`, `export` and `import`,
which the CLI binary installs (`crates/balaur_cli/src/main.rs` `own_modules`), not `balaur::standard_app`,
and `script`, which the Rune host installs itself.

## 5. Per module: every function the editor calls

Columns: `refs` = occurrences; `result used` = occurrences where the result is not discarded as a statement (approximate); `.bs signature` as the compiler sees it; `not typed / not callable` names every `any` with the type it was recorded as, or why the function is absent.

### `log`
<a id="log"></a>

7 of 7 functions used, 301 references. The `info`, `warn` and `error` levels a script writes at, and the buffer behind them. Lines go through the engine's own `tracing` stream.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `info` | 183 | 1 | docs | `(str) -> any` | any in result: docs give no `->` |
| `warn` | 98 | 2 | docs | `(str) -> any` | any in result: docs give no `->` |
| `error` | 13 | 0 | docs | `(str) -> any` | any in result: docs give no `->` (unused) |
| `since` | 3 | 3 | docs | `(any) -> any` | any in arg 1 `cursor: int?`, result: docs give no `->` |
| `file` | 2 | 2 | docs | `() -> any` | any in result: docs give no `->` |
| `clear` | 1 | 0 | docs | `() -> any` | any in result: docs give no `->` (unused) |
| `recent` | 1 | 1 | docs | `(any) -> any` | any in arg 1 `n: int?`, result: docs give no `->` |

### `gamend`
<a id="gamend"></a>

19 of 19 functions used, 299 references. The Gamend backend: session, REST API and realtime socket. Each call returns an id to await; the result also reaches the node's `on_gamend_event` (or `on_event`) as a map whose `kind` is an `EVENT_*` constant.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `rest` | 260 | 260 | seam | `(any, str, str, any) -> any` | any in arg 1 `Value`, arg 4 `Option<Value>`, result `Value`; 156 pass 3 of 4: every arg must be passed |
| `connection` | 6 | 6 | seam | `() -> any` | any in result `Value` |
| `server` | 4 | 4 | seam | `() -> any` | any in result `Value` |
| `session` | 4 | 4 | seam | `() -> any` | any in result `Value` |
| `configure` | 3 | 1 | seam | `(str) -> any` | any in result `Value` |
| `login` | 3 | 3 | seam | `(any, any, any) -> any` | any in arg 1 `Value`, arg 2 `Option<Value>`, arg 3 `Option<Value>`, result `Value`; 3 pass 2 of 3: every arg must be passed |
| `push` | 3 | 3 | seam | `(int, str, str, any) -> any` | any in arg 4 `Option<Value>`, result `Value` |
| `restore` | 3 | 0 | seam | `(any) -> any` | any in arg 1 `Option<Value>`, result `Value` (unused) |
| `activity` | 2 | 2 | seam | `() -> any` | any in result `Value` |
| `reply` | 2 | 2 | seam | `(int) -> any` | any in result `Value` |
| `call_hook` | 1 | 1 | seam | `(int, str, str, any) -> any` | any in arg 4 `Option<Value>`, result `Value` |
| `clear_activity` | 1 | 0 | seam | `() -> any` | any in result `Value` (unused) |
| `close` | 1 | 1 | seam | `(int) -> any` | any in result `Value` |
| `connect` | 1 | 1 | seam | `(any, any) -> any` | any in arg 1 `Value`, arg 2 `Option<Value>`, result `Value`; 1 pass 1 of 2: every arg must be passed |
| `interrupt` | 1 | 1 | seam | `(int) -> any` | any in result `Value` |
| `join` | 1 | 1 | seam | `(int, str, any) -> any` | any in arg 3 `Option<Value>`, result `Value`; 1 pass 2 of 3: every arg must be passed |
| `leave` | 1 | 1 | seam | `(int, str) -> any` | any in result `Value` |
| `register` | 1 | 1 | seam | `(any, any, any) -> any` | any in arg 1 `Value`, arg 2 `Option<Value>`, arg 3 `Option<Value>`, result `Value`; 1 pass 2 of 3: every arg must be passed |
| `run_id` | 1 | 1 | seam | `() -> any` | any in result `Value` |

### `fs`
<a id="fs"></a>

9 of 9 functions used, 183 references. Files on disk, project-relative unless the path is absolute. A packed build's contents are reached through `assets` and `scene.source`.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `exists` | 62 | 62 | docs | `(str) -> any` | any in result: docs give no `->` |
| `remove` | 32 | 1 | docs | `(str) -> any` | any in result: docs give no `->` |
| `write` | 29 | 1 | docs | `(str, str) -> any` | any in result: docs give no `->` |
| `read` | 28 | 28 | docs | `(str) -> any` | any in result: docs give no `->` |
| `list` | 15 | 15 | docs | `(str) -> any` | any in result: docs give no `->` |
| `create_directory` | 7 | 0 | docs | `(str) -> any` | any in result: docs give no `->` (unused) |
| `copy` | 5 | 0 | docs | `(str, str) -> any` | any in result: docs give no `->` (unused) |
| `rename` | 3 | 0 | docs | `(str, str) -> any` | any in result: docs give no `->` (unused) |
| `modified_time` | 2 | 2 | docs | `(str) -> any` | any in result: docs give no `->` |

### `scene`
<a id="scene"></a>

16 of 22 functions used, 144 references. The node tree: its root, lookup by path and instancing, plus the component and preset vocabulary an editor's palette lists.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `get_node` | 100 | 100 | docs | `(str) -> any` | any in result: docs give no `->` |
| `root` | 7 | 7 | docs | `() -> any` | any in result: docs give no `->` |
| `preset_info` | 6 | 6 | docs | `(str) -> any` | any in result: docs give no `->` |
| `warnings` | 6 | 6 | docs | `(Node) -> any` | any in result: docs give no `->` |
| `component_tags` | 4 | 4 | docs | `(str) -> any` | any in result: docs give no `->` |
| `bindable_events` | 3 | 3 | docs | `(any) -> any` | any in arg 1 `node: node?`, result: docs give no `->`; 2 pass 0 of 1: every arg must be passed |
| `component_properties` | 3 | 3 | docs | `(str, any) -> any` | any in arg 2 `params: any`, result: docs give no `->`; 1 pass 1 of 2: every arg must be passed |
| `presets` | 3 | 3 | docs | `() -> any` | any in result: docs give no `->` |
| `binding_actions` | 2 | 2 | docs | `() -> any` | any in result: docs give no `->` |
| `component_types` | 2 | 2 | docs | `() -> any` | any in result: docs give no `->` |
| `instantiate` | 2 | 0 | docs | `(str, any, any) -> any` | any in arg 2 `parent: node?`, arg 3 `opts: any?`, result: docs give no `->` (unused); 1 pass 2 of 3: every arg must be passed |
| `variables` | 2 | 2 | docs | `() -> any` | any in result: docs give no `->` |
| `apply_preset` | 1 | 0 | docs | `(Node, str) -> any` | any in result: docs give no `->` (unused) |
| `component_expects` | 1 | 1 | docs | `(str) -> any` | any in result: docs give no `->` |
| `component_schema` | 1 | 1 | docs | `(str) -> any` | any in result: docs give no `->` |
| `node_by_id` | 1 | 1 | docs | `(str, any) -> any` | any in arg 2 `under: node?`, result: docs give no `->` |

### `script`
<a id="script"></a>

19 of 19 functions used, 144 references. Loads other scripts, inspects what they declare and calls into them; a failure stays contained.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `attempt` | 81 | 73 | Rune host | absent | no `script` module in `.bs` |
| `shared` | 22 | 22 | Rune host | absent | no `script` module in `.bs` |
| `store` | 13 | 0 | Rune host | absent | no `script` module in `.bs` |
| `stored` | 5 | 5 | Rune host | absent | no `script` module in `.bs` |
| `format` | 3 | 3 | Rune host | absent | no `script` module in `.bs` |
| `symbols` | 3 | 3 | Rune host | absent | no `script` module in `.bs` |
| `check` | 2 | 2 | Rune host | absent | no `script` module in `.bs` |
| `complete` | 2 | 2 | Rune host | absent | no `script` module in `.bs` |
| `find` | 2 | 2 | Rune host | absent | no `script` module in `.bs` |
| `rename` | 2 | 2 | Rune host | absent | no `script` module in `.bs` |
| `api` | 1 | 1 | Rune host | absent | no `script` module in `.bs` |
| `definition` | 1 | 1 | Rune host | absent | no `script` module in `.bs` |
| `exports` | 1 | 1 | Rune host | absent | no `script` module in `.bs` |
| `functions` | 1 | 1 | Rune host | absent | no `script` module in `.bs` |
| `hover` | 1 | 1 | Rune host | absent | no `script` module in `.bs` |
| `references` | 1 | 1 | Rune host | absent | no `script` module in `.bs` |
| `replace` | 1 | 1 | Rune host | absent | no `script` module in `.bs` |
| `require` | 1 | 1 | Rune host | absent | no `script` module in `.bs` |
| `signature` | 1 | 1 | Rune host | absent | no `script` module in `.bs` |

### `render`
<a id="render"></a>

32 of 73 functions used, 142 references. What a frame is made of: the shape, sprite, mesh or emitter a node draws, the 2D and 3D cameras, the backdrop, debug lines and screenshots. `window` holds the window itself.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `draw_line_2d` | 35 | 0 | seam | `(float, float, float, float, float, float, float, float, any)` | any in arg 9 `Option<Value>`; 35 pass 8 of 9: every arg must be passed |
| `camera_pose` | 20 | 20 | seam | absent | result `(f32, f32, f32, f32, f32, f32, f32, f32)` has no `.bs` type |
| `draw_line_3d` | 15 | 0 | seam | `(float, float, float, float, float, float, float, float, float, float, bool, bool)` | 15 pass 10 of 12: every arg must be passed |
| `camera_2d` | 9 | 9 | seam | absent | result `(f32, f32, f32)` has no `.bs` type |
| `set_debug_view` | 6 | 5 | seam | `(str)` |  |
| `set_camera` | 5 | 0 | seam | `(float, float, float, float, float, float)` |  |
| `set_camera_2d` | 5 | 0 | seam | `(float, float, float)` |  |
| `bounds` | 4 | 4 | seam | `(Node) -> any` | any in result `Value` |
| `draw_lines` | 4 | 0 | seam | `(any)` | any in arg 1 `Value` |
| `pick_ray` | 4 | 4 | seam | `(float, float, float, float, float, float) -> Node` |  |
| `pick_ray_at` | 3 | 3 | seam | `(float, float, float, float, float, float) -> any` | any in result `Value` |
| `texture_size` | 3 | 3 | seam | absent | result `(u32, u32)` has no `.bs` type |
| `camera_matrix` | 2 | 2 | seam | `() -> [float]` |  |
| `debug_view` | 2 | 2 | seam | `() -> str` |  |
| `debug_views` | 2 | 2 | seam | `() -> any` | any in result `Value` |
| `mouse_ray` | 2 | 2 | seam | absent | result `(f32, f32, f32, f32, f32, f32)` has no `.bs` type |
| `mouse_world_2d` | 2 | 2 | seam | absent | result `(f32, f32)` has no `.bs` type |
| `set_grid` | 2 | 0 | seam | `(bool, float, int, int)` |  |
| `set_shader_preview` | 2 | 0 | seam | `(str, int)` |  |
| `stats` | 2 | 2 | seam | `() -> any` | any in result `Value` |
| `trace_texture` | 2 | 2 | seam | `(str, any) -> any` | any in arg 2 `Option<Value>`, result `Value` |
| `begin_retained` | 1 | 0 | seam | `(str)` |  |
| `check_material` | 1 | 1 | seam | `(str) -> any` | any in result `Value` |
| `clear_retained` | 1 | 0 | seam | `(str)` |  |
| `draw_text_2d` | 1 | 0 | seam | `(float, float, str, any)` | any in arg 4 `Option<Value>` |
| `draw_text_3d` | 1 | 0 | seam | `(float, float, float, str, any)` | any in arg 5 `Option<Value>` |
| `end_retained` | 1 | 0 | seam | `()` |  |
| `material_params` | 1 | 1 | seam | `(str) -> any` | any in result `Value` |
| `screenshot` | 1 | 0 | seam | `(str)` |  |
| `set_background` | 1 | 0 | seam | `(float, float, float)` |  |
| `set_grid_colors` | 1 | 0 | seam | `(float, float, float, float, float, float)` |  |
| `texture_pixels_per_unit` | 1 | 1 | seam | `(str) -> float` |  |

### `math`
<a id="math"></a>

15 of 24 functions used, 135 references. Deterministic float maths backed by pure-Rust `libm`: the same inputs give the same bits on every platform.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `sin` | 33 | 33 | docs | `(float) -> any` | any in result: docs give no `->` |
| `cos` | 32 | 32 | docs | `(float) -> any` | any in result: docs give no `->` |
| `sqrt` | 21 | 21 | docs | `(float) -> any` | any in result: docs give no `->` |
| `floor` | 10 | 10 | docs | `(float) -> any` | any in result: docs give no `->` |
| `round` | 9 | 9 | docs | `(float) -> any` | any in result: docs give no `->` |
| `atan` | 6 | 6 | docs | `(float, any) -> any` | any in arg 2 `x: float?`, result: docs give no `->` |
| `ceil` | 5 | 5 | docs | `(float) -> any` | any in result: docs give no `->` |
| `tan` | 5 | 5 | docs | `(float) -> any` | any in result: docs give no `->` |
| `to_radians` | 4 | 4 | docs | `(float) -> any` | any in result: docs give no `->` |
| `clamp` | 3 | 3 | docs | `(float, float, float) -> any` | any in result: docs give no `->` |
| `pow` | 3 | 3 | docs | `(float, float) -> any` | any in result: docs give no `->` |
| `acos` | 1 | 1 | docs | `(float) -> any` | any in result: docs give no `->` |
| `asin` | 1 | 1 | docs | `(float) -> any` | any in result: docs give no `->` |
| `atan2` | 1 | 1 | docs | `(float, float) -> any` | any in result: docs give no `->` |
| `to_degrees` | 1 | 1 | docs | `(float) -> any` | any in result: docs give no `->` |

`.bs` also has float builtins with no module prefix and a `float` result: `sqrt abs floor ceil min max sin cos tan asin acos atan atan2 pow exp ln` (`atan` takes one argument there). They cover 112 of these 135 references (`sin cos tan asin acos atan2 pow sqrt floor ceil`); `round`, `clamp`, `to_radians`, `to_degrees`, 2-argument `atan` (23 refs) and the constants `PI` and `INFINITY` exist only as `math::`.

### `input`
<a id="input"></a>

16 of 53 functions used, 95 references. One frame of input: keyboard, mouse, touch screen and gamepads, plus what went down or came up this frame. A headless run answers every query neutrally.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `mouse_just_pressed` | 17 | 17 | seam | `(int) -> bool` |  |
| `mouse_down` | 14 | 14 | seam | `(int) -> bool` |  |
| `key_down` | 13 | 13 | seam | `(str) -> bool` |  |
| `feed_key` | 12 | 6 | seam | `(str, bool)` |  |
| `feed_touch` | 8 | 0 | seam | `(int, float, float, str)` |  |
| `mouse_position` | 7 | 7 | seam | absent | result `(f32, f32)` has no `.bs` type |
| `feed_mouse` | 6 | 0 | seam | `(float, float)` |  |
| `feed_mouse_button` | 6 | 0 | seam | `(int, bool)` |  |
| `mouse_delta` | 4 | 4 | seam | absent | result `(f32, f32)` has no `.bs` type |
| `scroll_delta` | 2 | 2 | seam | absent | result `(f32, f32)` has no `.bs` type |
| `action_just_pressed` | 1 | 1 | seam | `(str) -> bool` |  |
| `action_value` | 1 | 1 | seam | `(str) -> float` |  |
| `feed_scroll` | 1 | 0 | seam | `(float, float)` |  |
| `key_just_pressed` | 1 | 1 | seam | `(str) -> bool` |  |
| `pan` | 1 | 1 | seam | `() -> any` | any in result `Value` |
| `pinch` | 1 | 1 | seam | `() -> any` | any in result `Value` |

### `ui`
<a id="ui"></a>

20 of 74 functions used, 76 references. Immediate-mode UI redrawn from a script's `draw_ui` every frame: panels, layout containers and widgets. HUD elements in the scene tree are the `widget` component.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `widget_rect` | 24 | 24 | seam | `(Node) -> any` | any in result `Value` |
| `request_repaint` | 12 | 0 | seam | `(any)` | any in arg 1 `Option<Value>`; 7 pass 0 of 1: every arg must be passed |
| `window_size` | 8 | 8 | seam | absent | result `(f32, f32)` has no `.bs` type |
| `set_clipboard` | 4 | 0 | seam | `(str)` |  |
| `edit` | 3 | 0 | seam | `(Node, any) -> bool` | any in arg 2 `Value` |
| `fill_rows` | 3 | 1 | seam | `(str, any, any, bool) -> bool` | any in arg 2 `Value`, arg 3 `Value` |
| `fill_strip` | 3 | 1 | seam | `(str, any) -> bool` | any in arg 2 `Value` |
| `set_focus` | 3 | 0 | seam | `(Node)` |  |
| `contrast_pairs` | 2 | 2 | seam | `(any, str) -> any` | any in arg 1 `Value`, result `Value` |
| `reread` | 2 | 0 | seam | `(Node) -> bool` |  |
| `set_load_progress` | 2 | 0 | seam | `(float, str)` |  |
| `set_widget_layer` | 2 | 0 | seam | `(bool, float, float, float, float)` |  |
| `complete_theme` | 1 | 1 | seam | `(any) -> any` | any in arg 1 `Value`, result `Value` |
| `contrast` | 1 | 1 | seam | `(str, str) -> any` | any in result `Value` |
| `finish_loading` | 1 | 0 | seam | `()` |  |
| `set_lazy` | 1 | 0 | seam | `(bool)` |  |
| `set_scale` | 1 | 0 | seam | `(float)` |  |
| `set_theme` | 1 | 0 | seam | `(any)` | any in arg 1 `Value` |
| `set_widget_surface` | 1 | 0 | seam | `(str, bool, float, float, float, float)` |  |
| `takes_pointer_at` | 1 | 1 | seam | `(float, float) -> bool` |  |

### `events`
<a id="events"></a>

1 of 6 functions used, 70 references. Named events between scripts. A node listens for a name and hears it as its `on_<name>` method; `node.emit` and `events.emit` send one.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `listen` | 70 | 0 | seam | `(Node, str, Node)` | 10 pass 2 of 3: every arg must be passed |

### `engine`
<a id="engine"></a>

16 of 28 functions used, 57 references. The running app: its clock, command-line `args`, `user_data_dir`, loaded `plugins` and `quit`.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `unix_time` | 21 | 21 | docs | `() -> any` | any in result: docs give no `->` |
| `open_url` | 5 | 0 | docs | `(str) -> any` | any in result: docs give no `->` (unused) |
| `target` | 5 | 5 | docs | `() -> any` | any in result: docs give no `->` |
| `user_data_directory` | 5 | 5 | docs | `() -> any` | any in result: docs give no `->` |
| `dark_mode` | 3 | 3 | docs | `() -> any` | any in result: docs give no `->` |
| `timings` | 3 | 3 | docs | `() -> any` | any in result: docs give no `->` |
| `args` | 2 | 2 | docs | `() -> any` | any in result: docs give no `->` |
| `device_id` | 2 | 2 | docs | `() -> any` | any in result: docs give no `->` |
| `reload_script` | 2 | 2 | docs | `(str) -> any` | any in result: docs give no `->` |
| `reveal` | 2 | 1 | docs | `(str) -> any` | any in result: docs give no `->` |
| `set_script_profiling` | 2 | 0 | docs | `(any) -> any` | any in arg 1 `on`, result: docs give no `->` (unused) |
| `delta` | 1 | 1 | docs | `() -> any` | any in result: docs give no `->` |
| `function_costs` | 1 | 1 | docs | `() -> any` | any in result: docs give no `->` |
| `quit` | 1 | 0 | docs | `(any) -> any` | any in arg 1 `code: int?`, result: docs give no `->` (unused) |
| `script_costs` | 1 | 1 | docs | `() -> any` | any in result: docs give no `->` |
| `user_data_directory_of` | 1 | 1 | docs | `(str) -> any` | any in result: docs give no `->` |

### `replay`
<a id="replay"></a>

17 of 17 functions used, 39 references. Records what a running game is fed and plays it back. A recording holds each tick's input, network arrivals and events; `balaur run --record` writes one.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `position` | 8 | 8 | docs | `() -> any` | any in result: docs give no `->` |
| `state` | 8 | 8 | docs | `() -> any` | any in result: docs give no `->` |
| `events` | 4 | 4 | docs | `(int, int) -> any` | any in result: docs give no `->` |
| `divergence_tick` | 2 | 2 | docs | `() -> any` | any in result: docs give no `->` |
| `length` | 2 | 2 | docs | `() -> any` | any in result: docs give no `->` |
| `marks` | 2 | 2 | docs | `(str, any) -> any` | any in arg 2 `key: string?`, result: docs give no `->` |
| `play` | 2 | 0 | docs | `() -> any` | any in result: docs give no `->` (unused) |
| `seek` | 2 | 0 | docs | `(int) -> any` | any in result: docs give no `->` (unused) |
| `header` | 1 | 1 | docs | `() -> any` | any in result: docs give no `->` |
| `info` | 1 | 1 | docs | `(str) -> any` | any in result: docs give no `->` |
| `load` | 1 | 1 | docs | `(str) -> any` | any in result: docs give no `->` |
| `pause` | 1 | 0 | docs | `() -> any` | any in result: docs give no `->` (unused) |
| `record` | 1 | 1 | docs | `(str, any) -> any` | any in arg 2 `options: any?`, result: docs give no `->` |
| `recording` | 1 | 1 | docs | `() -> any` | any in result: docs give no `->` |
| `recording_name` | 1 | 1 | docs | `() -> any` | any in result: docs give no `->` |
| `stop` | 1 | 0 | docs | `(any) -> any` | any in arg 1 `reason: string?`, result: docs give no `->` (unused) |
| `unload` | 1 | 0 | docs | `() -> any` | any in result: docs give no `->` (unused) |

### `settings`
<a id="settings"></a>

10 of 12 functions used, 35 references. Every setting the engine, its plugins and the game declare, by path: `physics/solver_iterations` is `[physics] solver_iterations` in `project.toml`. `define` adds your own; `[override.<platform>]` tables answer per platform.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `get` | 11 | 11 | seam | `(str) -> any` | any in result `Value` |
| `set` | 7 | 0 | seam | `(str, any) -> any` | any in arg 2 `Value`, result `Value` (unused) |
| `all` | 5 | 5 | seam | `() -> any` | any in result `Value` |
| `base` | 3 | 3 | seam | `(str) -> any` | any in result `Value` |
| `merge_toml` | 3 | 0 | seam | `(str) -> any` | any in result `Value` (unused) |
| `to_toml` | 2 | 2 | seam | `(str, str) -> any` | any in result `Value` |
| `clear` | 1 | 0 | seam | `(str) -> any` | any in result `Value` (unused) |
| `known_tags` | 1 | 1 | seam | `() -> any` | any in result `Value` |
| `overrides` | 1 | 1 | seam | `(str) -> any` | any in result `Value` |
| `revision` | 1 | 1 | seam | `() -> any` | any in result `Value` |

### `project`
<a id="project"></a>

13 of 13 functions used, 33 references. The projects this machine has opened, the templates a new one starts from, and the folder picker the OS provides. What the editor's start screen is made of.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `forget` | 6 | 1 | seam | `(str) -> any` | any in result `Value` |
| `home` | 4 | 4 | seam | `() -> any` | any in result `Value` |
| `create` | 3 | 3 | seam | `(str, str) -> any` | any in result `Value` |
| `editor_data_directory` | 3 | 3 | seam | `() -> any` | any in result `Value` |
| `recent` | 3 | 3 | seam | `() -> any` | any in result `Value` |
| `copy_example` | 2 | 2 | seam | `(str, str) -> any` | any in result `Value` |
| `examples` | 2 | 2 | seam | `() -> any` | any in result `Value` |
| `in_tab` | 2 | 2 | seam | `() -> any` | any in result `Value` |
| `pick_directory` | 2 | 2 | seam | `() -> any` | any in result `Value` |
| `templates` | 2 | 2 | seam | `() -> any` | any in result `Value` |
| `use_data` | 2 | 0 | seam | `(str) -> any` | any in result `Value` (unused) |
| `open` | 1 | 1 | seam | `(str) -> any` | any in result `Value` |
| `version` | 1 | 1 | seam | `() -> any` | any in result `Value` |

### `assets`
<a id="assets"></a>

6 of 11 functions used, 32 references. Asset definitions by reference: a project path, `file#entry` for one entry, `#id` for a scene block, or `id://<id>` from `assets/index.toml`. A script gets the definition table.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `load` | 13 | 13 | docs | `(str) -> any` | any in result: docs give no `->` |
| `save` | 8 | 7 | docs | `(str, any) -> any` | any in arg 2 `definition: any`, result: docs give no `->` |
| `duplicate` | 4 | 4 | docs | `(str) -> any` | any in result: docs give no `->` |
| `reload` | 3 | 3 | docs | `(str) -> any` | any in result: docs give no `->` |
| `directory` | 2 | 2 | docs | `(str) -> any` | any in result: docs give no `->` |
| `invalidate` | 2 | 0 | docs | `() -> any` | any in result: docs give no `->` (unused) |

### `toml`
<a id="toml"></a>

3 of 3 functions used, 32 references. TOML text to and from script tables, the format of scene files, asset definitions and component properties.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `parse` | 16 | 16 | docs | `(str) -> any` | any in result: docs give no `->` |
| `encode` | 15 | 15 | docs | `(any) -> any` | any in arg 1 `value: any`, result: docs give no `->` |
| `patch` | 1 | 1 | docs | `(str, any) -> any` | any in arg 2 `table: any`, result: docs give no `->` |

### `json`
<a id="json"></a>

2 of 2 functions used, 18 references. JSON text to and from script values. Its `null` keeps nil through a round trip.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `encode` | 9 | 9 | docs | `(any) -> any` | any in arg 1 `value: any`, result: docs give no `->` |
| `parse` | 9 | 9 | docs | `(str) -> any` | any in result: docs give no `->` |

### `save`
<a id="save"></a>

3 of 6 functions used, 14 references. Save games: a table in, a table out, stored per user. `[save] version` stamps each file; `[save] migrate` names the script whose `migrate_save(version, data)` upgrades older ones.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `remove` | 5 | 3 | docs | `(str) -> any` | any in result: docs give no `->` |
| `write` | 5 | 1 | docs | `(str, any) -> any` | any in arg 2 `data: any`, result: docs give no `->` |
| `read` | 4 | 4 | docs | `(str) -> any` | any in result: docs give no `->` |

### `import`
<a id="import"></a>

6 of 7 functions used, 10 references. Imports a `.glb` or `.gltf` model, an `.aseprite` sprite, or a `.tmx` or `.ldtk` level into the project being edited, with the importers `balaur import` runs.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `cancel` | 3 | 0 | seam | `() -> any` | any in result `Value` (unused) |
| `running_count` | 2 | 2 | seam | `() -> any` | any in result `Value` |
| `start` | 2 | 2 | seam | `(str, str) -> any` | any in result `Value`; 1 pass 1 of 2: every arg must be passed |
| `choose` | 1 | 0 | seam | `() -> any` | any in result `Value` (unused) |
| `handles` | 1 | 1 | seam | `(str) -> any` | any in result `Value` |
| `listen` | 1 | 0 | seam | `(Node, any)` | any in arg 2 `Option<Value>` |

### `release`
<a id="release"></a>

5 of 5 functions used, 9 references. Which build of the engine this is, what the channels are publishing, and replacing this install with another. The editor's Engine tab and About sheet; `balaur update` is the same code.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `installed` | 4 | 4 | seam | `() -> any` | any in result `Value` |
| `channels` | 2 | 2 | seam | `() -> any` | any in result `Value` |
| `check` | 1 | 1 | seam | `() -> bool` |  |
| `install` | 1 | 1 | seam | `(str, str, any) -> bool` | any in arg 3 `Option<Value>` |
| `listen` | 1 | 0 | seam | `(Node, any)` | any in arg 2 `Option<Value>` |

### `physics`
<a id="physics"></a>

5 of 14 functions used, 9 references. What spans both physics worlds: pausing, sleeping, tuning and clearing. Bodies and colliders live in `physics2d` and `physics3d`.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `is_paused` | 3 | 3 | seam | `() -> bool` |  |
| `set_paused` | 3 | 0 | seam | `(bool)` |  |
| `clear` | 1 | 0 | seam | `()` |  |
| `set_sleeping_allowed` | 1 | 0 | seam | `(bool)` |  |
| `sleeping_allowed` | 1 | 1 | seam | `() -> bool` |  |

### `debugger`
<a id="debugger"></a>

6 of 9 functions used, 9 references. Breakpoints, pausing a script and resuming it. The editor's Debugger dock and the Debug Adapter Protocol server drive the same machinery.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `break_on_error` | 2 | 2 | docs | `() -> any` | any in result: docs give no `->` |
| `request_break` | 2 | 1 | docs | `() -> any` | any in result: docs give no `->` |
| `set_breakpoints` | 2 | 2 | docs | `(str, [int]) -> any` | any in result: docs give no `->` |
| `resume` | 1 | 0 | docs | `(any) -> any` | any in arg 1 `mode: string?`, result: docs give no `->` (unused) |
| `set_break_on_error` | 1 | 0 | docs | `(bool) -> any` | any in result: docs give no `->` (unused) |
| `set_scope` | 1 | 0 | docs | `(any) -> any` | any in arg 1 `node: node?`, result: docs give no `->` (unused) |

### `task`
<a id="task"></a>

1 of 3 functions used, 8 references. Waiting inside an async handler: `init` and event handlers may await; `update` is synchronous.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `wait` | 8 | 8 | wait | `await task::wait(int)` |  |

### `skeleton`
<a id="skeleton"></a>

3 of 3 functions used, 8 references. Bones under a rig node: their rest pose and the order a skin numbers joints in. A bone is any node carrying `bone2d` or `bone3d`.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `bones` | 6 | 6 | docs | `(Node) -> any` | any in result: docs give no `->` |
| `apply_rest` | 1 | 0 | docs | `(Node) -> any` | any in result: docs give no `->` (unused) |
| `overwrite_rest` | 1 | 0 | docs | `(Node) -> any` | any in result: docs give no `->` (unused) |

### `export`
<a id="export"></a>

5 of 6 functions used, 6 references. Exports the project being edited. `targets` lists what this install can build; `start` runs one off the frame and reports to `on_export_event`.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `preview` | 2 | 2 | seam | `(str, str) -> any` | any in result `Value` |
| `listen` | 1 | 0 | seam | `(Node, any)` | any in arg 2 `Option<Value>` |
| `output` | 1 | 1 | seam | `(str) -> any` | any in result `Value` |
| `start` | 1 | 0 | seam | `(str, any) -> bool` | any in arg 2 `Option<Value>` |
| `targets` | 1 | 1 | seam | `() -> any` | any in result `Value` |

### `animation`
<a id="animation"></a>

4 of 20 functions used, 6 references. Clip playback on a node's `animation` component: play, pause, seek and query. `tween` builds a short clip from a table of steps and returns a handle.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `is_tween_running` | 2 | 2 | seam | `(any) -> bool` | any in arg 1 `Value` |
| `tween` | 2 | 1 | seam | `(Node, any) -> int` | any in arg 2 `Value` |
| `ease_names` | 1 | 1 | seam | `() -> any` | any in result `Value` |
| `stop` | 1 | 0 | seam | `(any)` | any in arg 1 `Value` |

### `http`
<a id="http"></a>

1 of 2 functions used, 2 references. HTTP requests off the frame: `method`, `headers`, `body`, `timeout_seconds` and `save_to` options. The reply reaches `on_response` as a map whose `kind` is `response`, with `status`, `headers` and `body`, or `error`; `save_to` downloads report to `on_progress` with `kind` `progress`, and a body over 256 KB going out with `kind` `upload`, natively; a browser's fetch does not report one. Each kind is an `EVENT_*` constant.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `request` | 2 | 2 | seam | `(any, any, any) -> any` | any in arg 1 `Value`, arg 2 `Option<Value>`, arg 3 `Option<Value>`, result `Value` |

### `window`
<a id="window"></a>

1 of 8 functions used, 2 references. The OS window and the display under it: mode, cursor, app icon, keep-awake, the safe area and the refresh rate.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `set_app_icon` | 2 | 0 | seam | `(str, any)` | any in arg 2 `Option<Value>`; 1 pass 1 of 2: every arg must be passed |

### `geometry2d`
<a id="geometry2d"></a>

2 of 12 functions used, 2 references. Polygons on the plane as lists of `[x, y]` points in outline order: triangulation, booleans, hulls and containment. Every answer is identical on every platform.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `is_clockwise` | 1 | 1 | seam | `(any) -> bool` | any in arg 1 `Value` |
| `triangulate` | 1 | 1 | seam | `(any) -> any` | any in arg 1 `Value`, result `Value` |

### `strings`
<a id="strings"></a>

1 of 6 functions used, 1 references. Localization: one `strings/<locale>.toml` per language, keys to strings. `[locale]` in `project.toml` sets the starting locale and the fallback; a key neither has comes back as itself.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `set_root` | 1 | 0 | docs | `(str) -> any` | any in result: docs give no `->` (unused) |

### `physics2d`
<a id="physics2d"></a>

1 of 132 functions used, 1 references. The 2D rigid-body world: bodies and colliders on nodes, their velocities, raycasts and overlap queries. `physics` holds what spans both worlds.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `ragdoll` | 1 | 0 | seam | `(Node, any) -> any` | any in arg 2 `Option<Value>`, result `Value` (unused) |

### `physics3d`
<a id="physics3d"></a>

1 of 142 functions used, 1 references. The 3D rigid-body world: bodies and colliders on nodes, their velocities, raycasts and overlap queries. `physics` holds what spans both worlds.

| fn | refs | result used | source | .bs signature | not typed / not callable |
| --- | ---: | ---: | --- | --- | --- |
| `ragdoll` | 1 | 0 | seam | `(Node, any) -> any` | any in arg 2 `Option<Value>`, result `Value` (unused) |

## 6. Constants

78 distinct constants, 231 references, **none reachable**: `Bindings::constant` is a no-op in `api.rs`, and the compiler has no constant lookup (a bare `input::KEY_SPACE` is a syntax error).

| module | constants | refs | most used |
| --- | ---: | ---: | --- |
| `input` | 17 | 71 | `MOUSE_BUTTON_LEFT` 26, `MOUSE_BUTTON_RIGHT` 8, `KEY_LEFT_META` 5, `KEY_D` 4, `KEY_A` 4, `KEY_SHIFT` 4, `KEY_W` 3, `KEY_S` 3, ... |
| `render` | 14 | 43 | `SHAPE_SPHERE` 5, `SHAPE_CIRCLE` 5, `POPULATE_SURFACE` 5, `SHAPE_BOX` 4, `POPULATE_GRID` 4, `SHAPE_RECTANGLE` 4, `LIGHT_DIRECTIONAL` 3, `POPULATE_ROW` 3, ... |
| `math` | 2 | 35 | `PI` 18, `INFINITY` 17 |
| `animation` | 18 | 33 | `EASE_OUT_CUBIC` 5, `ADVANCE_MODE_ENABLED` 4, `ADVANCE_MODE_AUTO` 3, `LOOP_LINEAR` 2, `PROPERTY_POSITION` 2, `LOOP_NONE` 2, `MODIFIER_LOOK_AT` 2, `STATE_END` 2, ... |
| `physics2d` | 9 | 14 | `SHAPE_CONVEX_DECOMPOSITION` 3, `BODY_STATIC` 2, `SHAPE_CIRCLE` 2, `SOFT_VOLUMETRIC` 2, `BODY_DYNAMIC` 1, `SHAPE_RECTANGLE` 1, `SOFT_GRID` 1, `SOFT_CIRCLE` 1, ... |
| `debugger` | 4 | 13 | `STEP_OVER` 5, `CONTINUE` 4, `STEP_INTO` 2, `STEP_OUT` 2 |
| `physics3d` | 10 | 12 | `BODY_STATIC` 2, `SHAPE_CONVEX_HULL` 2, `BODY_DYNAMIC` 1, `SHAPE_TRIANGLE_MESH` 1, `SHAPE_SPHERE` 1, `SHAPE_BOX` 1, `SOFT_VOLUMETRIC` 1, `SOFT_SPHERE` 1, ... |
| `replay` | 2 | 8 | `STATE_PAUSED` 6, `STATE_PLAYING` 2 |
| `ui` | 2 | 2 | `FONT_HEADING` 1, `WIDGET_ROW` 1 |

## 7. Node methods and component methods

Not `module::fn`, but engine calls too. Counted as `.name(` on any receiver in the editor, for names that are node ops; a few may be other objects' methods (`name`, `path`, `call`), so read the counts as upper bounds.

| node op | sites | .bs signature (after the node) | call-site arity | missing |
| --- | ---: | --- | --- | --- |
| `get_node` | 121 | `(str) -> Node` | 121×1 arg |  |
| `call` | 116 | `(str, any) -> any` | 68×1 arg, 43×2 args, 5×3 args | call sites pass 68×1 arg, 43×2 args, 5×3 args; the op takes 2 |
| `patch_component` | 115 | `(str, any)` | 115×2 args | `any` in argument |
| `get_component` | 68 | `(str) -> any` | 22×1 arg, 46×2 args | call sites pass 22×1 arg, 46×2 args; the op takes 1 |
| `name` | 41 | `() -> str` | 41×0 args |  |
| `path` | 31 | `() -> str` | 31×0 args |  |
| `global_position` | 29 | `() -> Vec3` | 29×0 args |  |
| `children` | 17 | `() -> [Node]` | 17×0 args |  |
| `global_rotation_euler` | 16 | `() -> Vec3` | 16×0 args |  |
| `global_scale` | 12 | `() -> Vec3` | 12×0 args |  |
| `parent` | 11 | `() -> Node` | 11×0 args |  |
| `set_component` | 11 | `(str, any)` | 1×1 arg, 10×2 args | call sites pass 1×1 arg, 10×2 args; the op takes 2 |
| `component_names` | 10 | `() -> [str]` | 10×0 args |  |
| `is_valid` | 8 | `() -> bool` | 8×0 args |  |
| `has_component` | 6 | `(str) -> bool` | 6×1 arg |  |
| `add_child` | 5 | `(str) -> Node` | 5×1 arg |  |
| `has_method` | 4 | `(str) -> bool` | 4×1 arg |  |
| `queue_free` | 4 | `()` | 4×0 args |  |
| `set_visible` | 3 | `(bool)` | 3×1 arg |  |
| `descendants` | 2 | `() -> [Node]` | 2×0 args |  |
| `set_name` | 1 | `(str)` | 1×1 arg |  |
| `set_sibling_index` | 1 | `(int)` | 1×1 arg |  |
| `remove_component` | 1 | `(str)` | 1×1 arg |  |
| `stable_id` | 1 | `() -> str` | 1×0 args |  |
| `emit` | 1 | `(str, any)` | 1×2 args | `any` in argument |
| `attach_script` | 1 | `(str, any)` | 1×2 args | `any` in argument |
| `detach_script` | 1 | `()` | 1×0 args |  |
| `material` | 1 | `() -> str` | 1×0 args |  |

Component functions called as `node.<component>.fn(..)` in the editor, which `.bs` does not take in that form
(the function is reachable as `module::fn(node, ..)`):


| method | sites | reaches as | component | .bs signature |
| --- | ---: | --- | --- | --- |
| `.move_character(..)` | 5 | `physics2d::move_character` | character2d | `(Node, float, float) -> any` |
| `.move_character(..)` | 5 | `physics3d::move_character` | character3d | `(Node, float, float, float) -> any` |
| `.is_on_floor(..)` | 4 | `physics2d::is_on_floor` | character2d | `(Node) -> bool` |
| `.is_on_floor(..)` | 4 | `physics3d::is_on_floor` | character3d | `(Node) -> bool` |
| `.pause(..)` | 3 | `animation::pause` | animation | `(Node)` |
| `.seek(..)` | 2 | `animation::seek` | animation | `(Node, float)` |
| `.seek(..)` | 2 | `audio::seek` | sound | `(Node, float)` |
| `.set_terrain(..)` | 1 | `render::set_terrain` | tilemap | `(Node, int, int, int)` |
| `.add_clip(..)` | 1 | `animation::add_clip` | animation | `(Node, str, any)` |
| `.play(..)` | 1 | `animation::play` | animation | `(Node, str, any)` |
| `.play(..)` | 1 | `audio::play` | sound | `(Node) -> int` |
| `.resume(..)` | 1 | `animation::resume` | animation | `(Node)` |
| `.time(..)` | 1 | `animation::time` | animation | `(Node) -> float` |
| `.current_state(..)` | 1 | `animation::current_state` | state_machine | `(Node) -> any` |
| `.populate(..)` | 1 | `render::populate` | multimesh2d, multimesh3d | `(Node, any) -> any` |
| `.outline(..)` | 1 | `render::outline` | occluder2d | `(Node) -> [float]` |
| `.instance_count(..)` | 1 | `render::instance_count` | multimesh2d, multimesh3d | `(Node) -> int` |

Property reads and writes (`node.transform.position = ..`) are typed from component schemas (`HostApi.components`) and are not `module::fn`; this audit does not count them.

## 8. Optional arguments the editor leaves out

Call sites that pass fewer arguments than the `.bs` signature has (the engine call itself accepts them; `.bs` demands every one):

| fn | sites | passes | .bs signature |
| --- | ---: | --- | --- |
| `gamend::rest` | 156 | 156 pass 3 of 4 | `(any, str, str, any) -> any` |
| `render::draw_line_2d` | 35 | 35 pass 8 of 9 | `(float, float, float, float, float, float, float, float, any)` |
| `render::draw_line_3d` | 15 | 15 pass 10 of 12 | `(float, float, float, float, float, float, float, float, float, float, bool, bool)` |
| `events::listen` | 10 | 10 pass 2 of 3 | `(Node, str, Node)` |
| `ui::request_repaint` | 7 | 7 pass 0 of 1 | `(any)` |
| `gamend::login` | 3 | 3 pass 2 of 3 | `(any, any, any) -> any` |
| `scene::bindable_events` | 2 | 2 pass 0 of 1 | `(any) -> any` |
| `gamend::register` | 1 | 1 pass 2 of 3 | `(any, any, any) -> any` |
| `gamend::connect` | 1 | 1 pass 1 of 2 | `(any, any) -> any` |
| `gamend::join` | 1 | 1 pass 2 of 3 | `(int, str, any) -> any` |
| `window::set_app_icon` | 1 | 1 pass 1 of 2 | `(str, any)` |
| `scene::instantiate` | 1 | 1 pass 2 of 3 | `(str, any, any) -> any` |
| `import::start` | 1 | 1 pass 1 of 2 | `(str, str) -> any` |
| `scene::component_properties` | 1 | 1 pass 1 of 2 | `(str, any) -> any` |

235 sites in 14 functions, plus the node ops in §7 (`get_component` 46, `call` 73, `set_component` 1).

## 9. Rune type tests

`x is balaur::Node` (23) and `balaur::Color` (1): language, not an engine function. `.bs` has `Node` and `Color` as types; the test itself is a translator matter.

## 10. Top gaps by call count

Ranked by references in the editor. Each row is something missing from the `.bs` API, with the count of editor references it blocks, breaks or leaves untyped. `not callable`: the compiler rejects the name. `won't compile as written`: callable, but not with the arguments the editor passes. `callable, result any`: compiles, but the result is an untyped `any`.

| # | kind | gap | refs | detail |
| ---: | --- | --- | ---: | --- |
| 1 | callable, result `any` | result is `any` and the editor reads it | 979 | 142 fns, listed below |
| 2 | won't compile as written | an optional argument is left out (engine fns) | 235 | 14 fns: `gamend::rest` 156, `render::draw_line_2d` 35, `render::draw_line_3d` 15, `events::listen` 10, ... |
| 3 | not callable | constants (`Bindings::constant` is empty) | 231 | 78 names: `input::MOUSE_BUTTON_LEFT` 26, `math::PI` 18, `math::INFINITY` 17, `input::MOUSE_BUTTON_RIGHT` 8, `replay::STATE_PAUSED` 6, ... |
| 4 | not callable | `script::*`: no `.bs` module | 144 | 19 fns: `attempt` 81, `shared` 22, `store` 13, `stored` 5, `symbols` 3, `format` 3, ... |
| 5 | won't compile as written | node op `call(method, payload)` takes exactly two args | 73 | 68 sites pass no payload, 5 pass 3 args |
| 6 | not callable | tuple result has no `.bs` type | 57 | 9 fns: `render::camera_pose` 20, `render::camera_2d` 9, `ui::window_size` 8, `input::mouse_position` 7, `input::mouse_delta` 4, `render::texture_size` 3, ... |
| 7 | won't compile as written | node op `get_component(name, key)` takes one arg | 46 | reading one field of a component |
| 8 | won't compile as written | node op `set_component(name, data)` called with one arg | 1 |  |

The `any`-result functions the editor uses most (result used, so the script reads it through the `any` methods of §2):

| fn | refs | result used | .bs signature | source |
| --- | ---: | ---: | --- | --- |
| `gamend::rest` | 260 | 260 | `(any, str, str, any) -> any` | seam |
| `scene::get_node` | 100 | 100 | `(str) -> any` | docs |
| `fs::exists` | 62 | 62 | `(str) -> any` | docs |
| `math::sin` | 33 | 33 | `(float) -> any` | docs |
| `math::cos` | 32 | 32 | `(float) -> any` | docs |
| `fs::read` | 28 | 28 | `(str) -> any` | docs |
| `ui::widget_rect` | 24 | 24 | `(Node) -> any` | seam |
| `engine::unix_time` | 21 | 21 | `() -> any` | docs |
| `math::sqrt` | 21 | 21 | `(float) -> any` | docs |
| `toml::parse` | 16 | 16 | `(str) -> any` | docs |
| `toml::encode` | 15 | 15 | `(any) -> any` | docs |
| `fs::list` | 15 | 15 | `(str) -> any` | docs |
| `assets::load` | 13 | 13 | `(str) -> any` | docs |
| `settings::get` | 11 | 11 | `(str) -> any` | seam |
| `math::floor` | 10 | 10 | `(float) -> any` | docs |
| `json::encode` | 9 | 9 | `(any) -> any` | docs |
| `json::parse` | 9 | 9 | `(str) -> any` | docs |
| `math::round` | 9 | 9 | `(float) -> any` | docs |
| `replay::state` | 8 | 8 | `() -> any` | docs |
| `replay::position` | 8 | 8 | `() -> any` | docs |
| `scene::root` | 7 | 7 | `() -> any` | docs |
| `assets::save` | 8 | 7 | `(str, any) -> any` | docs |
| `gamend::connection` | 6 | 6 | `() -> any` | seam |
| `skeleton::bones` | 6 | 6 | `(Node) -> any` | docs |
| `math::atan` | 6 | 6 | `(float, any) -> any` | docs |

Where the `any` comes from, for the 142 functions above: 82 are documented raw ops whose docs give no `-> type` (572 used results; adding `-> T` to the `describe` string types them); 60 are typed-seam functions returning `Value` (407 used results; they need a typed Rust return, or a struct type to describe the table they hold).

## 11. Method

- **Extraction**: every `a::b[::c]` path in `editor/**/*.rn` after removing comments and string literals (template-string `${..}` code kept). Resolved per file: `crate::x::` and names in that file's `use crate::x;` (or `editor.rn`'s `mod x;`) are the editor's own; a head that is a module of `balaur api` is an engine binding or constant. Nothing was left unresolved.
- **Engine API**: `balaur api` (the list of modules, functions, constants and recorded signatures) from a debug build of the tree as it was at 16:23 on 2026-10-20: `HEAD` 7ce824b2 plus the uncommitted work (the whole `crates/balaur_script_bs` crate was untracked, the documented-ops change in `api.rs` included). The build used its own target directory; `balaur check` of that build is what the probes ran against. The tree compiled.
- **Reachability and types**: `crates/balaur_script_bs/src/api.rs` `build` was reproduced on the dump (typed seam first, then docs, no fall-back from a seam signature that does not map), then checked against the compiler three ways:
  1. one `.bs` file per editor-used function and constant calling it with no arguments: 344 probes. The compiler's answer (``the engine has no `x::y` ``, ``takes N argument(s), 0 given``, or none) matched the reproduction in all 344.
  2. every reachable engine function in the API (776 non-node, non-async of 906): one call with arguments of the predicted types, the result passed to a parameter of the predicted type (or, for `any`, to a `float` parameter, which must fail with ``expected float, got any``): 1,196 probes, no disagreement.
  3. every typed parameter position (965): that one argument replaced by `nil()`; the compiler's ``expected T, got any`` named the predicted `T` each time.
- **Not checked**: the types of values the editor passes (a dynamic Rune script may pass an `int` where `float` is declared; that is a `.bs` compile error at the call site), `ui::` callbacks (no `ui::` function the editor still calls takes one), and component property access.
- **Counting**: a reference is one occurrence of the path; the 2 non-call references are function values. "Result used" is a text heuristic (the call is not a statement ending in `;`).

Found along the way, outside the audit's scope: a `.bs` function parameter with an unknown type name (`fn w(x: nosuchtype) {}`, or `string` for `str`) makes the compiler panic at `crates/balaur_bs/src/codegen/mod.rs:122` (`no entry found for key`) instead of reporting an error; and a file reports only its first error.

