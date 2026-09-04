# Plan: gaps in scripting, nodes, widgets and the editor

> **Status:** most of §1–§4 is built; each item's own status line says which.
> What is left is §4.3's screens, §5's docs, and the three items marked
> *partly done*. Written 2026-09-04 from a read of `balaur_ui`,
> `balaur_script_rune`, the node and component API in `balaur_core`, and
> `editor/scripts`, at `8b44a17` plus the shell-on-widget-nodes and
> `Registry` work in flight in the working tree. Each item says what is
> wrong, where, and the shape of the fix. An item that extends another plan
> names it, so it can move there when it is picked up. §7 is the order.

## 1. Scripting

### 1.1 A patch verb for components

**Now.** `node.set_component(name, params)` and the handle's
`node.widget.set(params)` both reach `components::add`, which merges
`params` over the *schema defaults*: every property the call leaves out goes
back to its default (`crates/balaur_core/src/node_api.rs:419-431`,
`components.rs:578-585`). `components::patch` (`components.rs:599-608`)
merges over what the component currently reports and is what animation
tracks and prefab overrides use, but no script can reach it. So the natural
`node.set_component("widget", #{ text: "Score 5" })` also resets the label's
anchor, offset and font size. `examples/angrynerds/scripts/game.rn:62-72`
works around it with a read-modify-write helper, and the shell code in
flight (`editor/scripts/layout.rn`, `size`) does the same dance.

**Shape.** One entry in `NODE_OPS`, `patch_component`, calling
`components::patch`; `patch` beside `get`, `set`, `has` and `remove` in the
handle's `GENERIC` table (`crates/balaur_script_rune/src/value/component.rs:53-59`),
so `node.widget.patch(#{ text: "Score 5" })` behaves the way the scene's
`overrides` key already does. `set_component` keeps its meaning, and its
docstring says "describes the component whole; `patch_component` changes one
property". Declared once, it reaches every language. The two workarounds
become one call each.

Status: **done** — `node.patch_component`, and `patch` on the component handle.

### 1.2 Exports as spec tables

**Now.** `exports()` returns `#{ name: default }` and the host infers the
type from the default (`crates/balaur_script_rune/src/inspect.rs:181-194`):
bool, int, float, vec2, vec3, colour, else string. A string cannot say it
names a node or an asset, a number has no range or step, an enum is
impossible, there is no help line, and declaration order is lost because a
Rune object keeps none (`inspect.rs:291-295`). The inspector fakes a spec
from the default to reach its editors (`editor/scripts/inspector.rn:276-286`).

**Shape.** Beside the bare form, a value that is an object carrying `type`
is a spec, in the component schema's own vocabulary:

```rune
pub fn exports() {
    #{
        // `type` and `default` are reserved words in Rune, so the keys are
        // quoted; the rest are plain identifiers.
        speed: #{ "type": "float", "default": 2.0, min: 0.25, max: 32.0, order: 1 },
        target: #{ "type": "node", "default": "" },
        mode: #{ "type": "enum", "default": "spin", options: ["spin", "wobble"] },
        clockwise: true,
    }
}
```

`RuneHost::exports` validates a spec with the rule `ComponentDef::parse_schema`
applies per property (`components.rs:190-250`) and reports a bad one at
attach; `attach_with_props` writes the spec's `default`. `ScriptHost::exports`
keeps its signature: the `Value` per name is the spec map, with a bare
default lifted into `#{ type, default }` so every reader sees one shape. The
inspector's `prop_row` already dispatches on `spec["type"]`
(`inspector.rn:706-725`), so an exported node path gets the picker, an enum
the dropdown and an asset the reference row with no editor change; `order`
sorts the rows the way settings already do. `#[export]`
(`docs/PLAN-scripting.md`) stays the phase after: it lowers to this table.

Status: **done** — a spec table beside the bare default, validated against the schema vocabulary and sorted by `order`.

### 1.3 `hot_reload` is never dispatched

**Now.** `ARCHITECTURE.md:137` and `:252` name a `hot_reload` hook for
migrating state shapes, and `editor/scripts/editor.rn:355` defines one, but
`RuneHost::reload` (`crates/balaur_script_rune/src/lib.rs:797-824`) swaps the
unit, refreshes the required module, and calls nothing. No instance has ever
been told its code changed.

**Shape.** After a reload lands, call `hot_reload(this)` on every instance of
that key through `invoke`, in instance order, reporting a throw the way
`on_free` does. Add `a_reload_calls_hot_reload_on_every_instance` beside
`a_reload_keeps_instance_state` in `tests/backend.rs`. The editor's stub,
which re-applies the theme, is the first use.

Status: **done** — `reload` calls `hot_reload` on every instance of the file.

### 1.4 Public functions are read line by line

**Now.** `public_functions` (`inspect.rs:30-67`) finds a `pub fn` by
scanning each source line and needs its `(`...`)` on that same line. A
signature broken over two lines is invisible to `script::functions`, to
`script::require` (`lib.rs:850-881`), to the editor's hooks panel
(`editor/scripts/highlight.rn`), and to plugin loading, which then reports
"no register(); skipped" (`editor/scripts/plugins.rn:71-87`). Worse, `invoke`
decides whether to run through the breakpoint executor by finding the
function in that list (`pause.rs:45-53`): one it cannot find is treated as
async and runs with its breakpoints ignored. Separately, `trampoline`
(`lib.rs:247-282`) and `script::shared` cap at five arguments.

**Shape.** Read the unit, not the text: `Unit::debug_info().functions`
carries every function with its `DebugArgs::Named` list, which the debugger
already reads for locals (`debugger.rs:239`), and `Unit::is_immediate` says
whether it is synchronous. A packed script keeps the list the pack encodes
today, computed from the dev unit at export. Lift the arity cap by building a
shared function as a raw handler over `Memory` and an argument count, the
shape `RuneModule::function_raw` already uses (`bindings.rs:184-214`).

Status: **done** for the scan, which now spans lines, and for the stepping
decision, which asks the unit rather than the scan. The five-argument cap
**cannot** be lifted here: Rune's `permute!` generates `Function::new` impls
up to arity 5 and no further, so a six-argument callback needs a change in the
fork. Bundling the extra arguments into one object is the workaround, and
`editor/scripts/plugins.rn` already takes it.

### 1.5 Warts

| What | Where | Shape |
|---|---|---|
| `engine.tick()` answers a float, so Rune refuses `tick % 60` against an int | `crates/balaur_core/src/engine_api.rs:628-630` | `Value::Int`; the doc already calls it "an exact integer" |
| `node.call` answers nil for a missing method, the same as a method that returned nothing | `node_api.rs:477-485` | `node.has_method(name)`, from the host's cached `resolve` |
| A misspelled option key or role does nothing, silently (`colour:`); the one test asserts only that nothing crashes | `crates/balaur_ui/src/widgets.rs:47-52`, `tests/pass.rs:164-173` | each widget lists its keys in `describe`; `Opts::with_roles` warns once per widget and key in a dev build; the test asserts the warning |
| `obj.field = a \|\| b` overwrites the local `a` (AGENTS.md); a code-generation bug in a compiler this repo already forks | `balaurengine/rune`, branch `deterministic-pow` | fix it in the fork with a test; until then a `.rn` pass in `scripts/house_lints.py`, which today reads only `*.rs` |

Status: **partly done** — `engine.tick()` is an integer and `node.has_method` exists. The options-typo warning and the Rune fork are open; the two Rune traps are now caught by `scripts/house_lints.py`.

## 2. Nodes

### 2.1 Finding nodes

**Now.** `NODE_OPS` reaches a node by path, and its parent and children
(`node_api.rs:27-149`). There is no recursive find, no "every node carrying
`sprite`", and no groups. `examples/angrynerds/scripts/game.rn:38-60` walks
the tree by hand to collect its level; the editor walks its document rows
instead of asking.

**Shape.** Two declared-once operations. `node.descendants()`: every node
under this one in tree order (`scene::collect_subtree` exists but visits
siblings last-first, `scene.rs:223-233`; `propagate_recursive` walks the
order wanted). `scene.with_component(name)`: every node whose registry `get`
answers, in tree order, through the hook `components::present_on` already
calls. Groups are not planned: a `tags` component with a `flags` property
would be the shape if one is ever wanted, and `with_component` then covers
"all enemies".

Status: **done** — `node.descendants()` and `scene.with_component(name)`, both in tree order.

### 2.2 Events between scripts

**Now.** An engine event reaches a script as a named method on one node
(`ARCHITECTURE.md`, "The seam"), and one script reaches another through
`node.call` on a node it already holds. Nothing lets a script announce
something to whoever cares: a player dying has to know every listener by
path.

**Shape.** An `events` module declared once in core, in the frame-scoped
style everything else uses. `events.subscribe(node, name)` records the pair
in a `DetHashMap`; `events.emit(name, payload)` appends to the tick's queue;
a core system at the top of `Stage::Update` calls `on_<name>(payload)` on
each subscriber in subscription order, skips freed nodes, and clears the
queue. `events.poll(name)` is the asking twin, the pair
`animation.just_finished` already has. Delivery a frame later at a fixed
stage is what keeps it deterministic and out of the middle of a script batch;
emits come from scripts, so a replay reproduces them with no recording
source. `unsubscribe` and detach both release.

Status: **done** — the `events` module, pumped at the top of `Stage::Update`, with `emitted` as the asking twin.

## 3. Widgets

### 3.1 Keyboard focus is opt in

**Now.** `keyboard_move` reads arrows, Tab, Enter and Space on every frame
the layer draws (`crates/balaur_ui/src/widget_layer.rs:478-496`), `advance`
applies them whenever a visible button exists, and the focused button gets a
ring (`:756-765`). A game that moves with arrows and jumps with Space clicks
its own HUD button, and the ring appears the first time an arrow is pressed.

**Shape.** `WidgetLayerConfig.keyboard`, off by default. `standard_app`
turns it on for a project that declares `ui_next`, `ui_previous` or
`ui_accept`, beside `drive_ui_focus` (`crates/balaur/src/lib.rs:280-300`),
and `ui.set_keyboard_focus(on)` covers a menu that opens at run time. While
egui reports a text field focused, the focus keys stand down. The focus
tests (`tests/widget_layer.rs:758-889`) set the flag.

Status: **done** — `WidgetLayerConfig::keyboard`, off by default, turned on by `standard_app` for a project declaring the `ui_*` actions and by `ui.set_keyboard_focus`.

### 3.2 Label wrap and alignment; an `image` kind

**Now.** A `label` draws one unwrapped line at the container's start
(`widget_layer.rs:835-837`); there is no `wrap`, no text alignment, and as a
root its `width` is ignored. There is no image kind and no progress kind: a
heart, a portrait or a health bar is a `draw` node with a script behind it.
`ARCHITECTURE.md`'s roadmap calls more kinds demand-driven; these two are
what every HUD asks for.

**Shape.** `wrap` (bool) and `text_align` (`start | center | end`) on the
component, honoured by the label and button arms and by `Measure::text`
(`widget_measure.rs:166-180`), which measures a wrapping label against the
width its parent assigned. `kind = "image"` with `source`, a project-relative
PNG loaded through `ProjectFiles` and cached in `UiState.textures` the way
`ui.image` already does (`widgets.rs:756-829`), sized by `width` and
`height` or the native size. `kind = "bar"` with `value` in 0..1, drawn from
the theme's fill and stroke, comes after if asked for.

Status: **partly done** — `wrap`, `text_align` and an `image` kind. A `bar` kind is still open.

### 3.3 Integer margins

**Now.** egui's `Margin` is whole device pixels, so a padding of 14 design
px at the editor's 1.25 scale is 17.5 and lands on 17. The working tree fixes
the row and column path by shrinking a float rect instead
(`widget_arrange.rs`, `contain`); the panel arm (`widget_layer.rs:768`), the
scroll box and `themed_frame` (`widget_arrange.rs:79,115`) still cast to
`i8`, which also saturates at 127.

**Shape.** The same technique in the three remaining places: keep the
`Frame` for fill, stroke and radius with a zero margin, and hand the
children a child `Ui` at the rect shrunk by the float padding.

Status: **done** — the panel, the scroll box and the themed frame all take their padding off in floats.

### 3.4 The per-frame walk

**Now.** `forest` walks the entire world every frame and clones every
widget, eight strings each, into the arena (`widget_layer.rs:432-469`);
`draw` clones them all again to settle clicks (`:625-628`) and writes
`clicked` on every widget whether or not anything was clicked. A scene with
no widgets still pays the walk.

**Shape.** Return before the walk when `world.query::<&Widget>()` is empty;
settle clicks and focus from the arena rather than a second copy; write
`clicked` only where it changes. Not a measured problem yet; listed so it is
not rediscovered at a thousand nodes.

Status: **partly done** — the walk returns before doing anything when a scene has no widgets. The second clone and the unconditional `clicked` write stand.

## 4. The editor

### 4.1 Saving keeps the document whole

**Now.** `model::load` keeps only the `nodes` array of the parsed scene
(`editor/scripts/model.rn:559-562`), `save` writes `#{ nodes }` back through
`toml.encode` (`:761-766`), and `open_scene` and the mirror do the same
(`:939-950`, `:643`). Three things are lost on the first save: every comment
(every example scene has some), key order, and any top-level table,
including the `[[assets]]` blocks a scene declares for `#id` references
(`ARCHITECTURE.md`, "Assets"). The mirror drops those blocks too, so such a
reference fails inside the editor. `toml.encode` is `toml::to_string`
(`crates/balaur_core/src/file_api.rs:126`), which has no comments to keep;
the settings writer (`settings.rs:237,265`) keeps unrelated tables but loses
comments the same way.

**Shape.** Two steps. First, the editor keeps the whole parsed document,
writes `nodes` into it, and hands the whole document to `scene.instantiate`,
so `[[assets]]` and unknown tables survive and `#id` resolves in the mirror.
Second, one comment-keeping writer for both files: `toml_edit` is already in
the dependency graph under `toml`, and a `toml.patch(existing_text, table)`
binding that edits a `DocumentMut` in place would serve the scene save, the
settings save and `assets.save`. A self-test: open an example, save with no
edits, assert the bytes are unchanged.

Status: **done** — `toml.patch` keeps comments and unmodelled tables, and the mirror is built from the whole document.

### 4.2 Addressing the mirror

**Now.** `build_mirror` resolves every document row to a live node by its
name path (`model.rn:647-655`), and `find_node` answers the first sibling
with that name (`scene.rs:92-113`); two siblings sharing a name map to one
live node, and the inspector edits the wrong one. Every edit deep-copies the
document into history (`history.rn:12`) and every undo rebuilds the whole
mirror with a physics clear (`shell.rn:25-44`).

**Shape.** Address by `StableId`: the engine stamps one on every node it
instantiates and prefixes instance ids (`project.rs:398-399`), and every
document row carries `id`. A declared-once `scene.node_by_id(id)` and
`node.stable_id()` let `S.refs` and `index_of_live` resolve by id, and the
name-path walk goes. History and the rebuild on undo stay as they are:
whole-document frames were chosen for a reason, and the cost is fine below a
few hundred nodes. Noted here so a future "the editor is slow on a big
scene" report has its answer.

Status: **partly done** — the mirror resolves by stable id through `scene.node_by_id(id, under)`. Whole-document history stands, deliberately.

### 4.3 Screens

| What | Where | Shape |
|---|---|---|
| In the Interface persona the axis pill and the zoom pill draw over the HUD's bottom labels: the HUD surface is the whole stage rect, and the pills sit inside it | `editor/scripts/center.rn:206-234`, `editor.rn:207-209` | inset the widget surface by the HUD band, or draw the pills in the tab row |
| D7 and D17 in `docs/EDITOR-SCREENS.md` are fixed in code (`dock.rn:14-41` hint table, `:132-138` padded columns) but still listed open; D10, D14 and D15 still stand | `docs/EDITOR-SCREENS.md` §8 | mark them, and re-capture |

Status: open.

## 5. Docs and lints

| What | Where | Shape |
|---|---|---|
| `docs/generated/script-api.md` and `api.json` list the settings module as `pages`, `project_toml`, `editor_toml`; the working tree has `all`, `define`, `load`, `to_toml`. The docs job on CI diffs these | `docs/generated/` | `scripts/gen_docs.py` before that commit |
| The determinism table says object iteration order is the hash map's and an ordered map is planned; the fork now hashes with XxHash64 at a fixed seed (`7be8013`, "Deterministic hashing") and `object_iteration_order_does_not_move_between_runs` pins the order | `ARCHITECTURE.md`, "Determinism" table | say so: stable across runs and platforms, not insertion order |
| `hot_reload` is documented and not dispatched | `ARCHITECTURE.md:137`, `:252` | §1.3, or strike the two sentences |
| No lint reads Rune; the `a \|\| b` bug cost four defects | `scripts/house_lints.py` | a `.rn` pass for `= <local> \|\|` and `&&`, and for `if let Some(x) = x` |

Status: open.

## 6. What this does not change

- The seam. Every item above is one more declared-once operation or one
  more component property; nothing needs a language-specific type.
- Whole-document history and a mirror rebuilt from the document. The costs
  in §4.2 are known and accepted at the scene sizes the examples have.
- The `#[export]` fork. §1.2 is what it will lower to, and it is worth
  having whether or not the fork happens.

## 7. Order

| Phase | What | Why here |
|---|---|---|
| 1 | §1.1 patch verb, §1.3 `hot_reload`, §1.5 tick and `has_method` | a day each, and the footguns scripts hit today |
| 2 | §4.1 the save keeps the document whole | data loss |
| 3 | §1.2 exports as spec tables | the inspector gains pickers, and it fixes what `#[export]` would lower to |
| 4 | §3.1 focus opt in, §3.2 wrap and `image`, §3.3 margins | a game's HUD |
| 5 | §2.1 finding nodes, §2.2 events, §4.2 ids | the node API a second game needs |
| 6 | §1.4 functions from the unit, §3.4 the walk | robustness, then cost |
| 7 | §5 docs and lints | keep it from regressing |
