> **Status:** phase 0 and the plugin seam built on 2026-09-02. Every §1
> refactor landed except the two that needed engine surface, which landed
> with it: `util` vector helpers, `style.rn`, `viewport.rn`,
> `model::set_transform`, module-owned state, the property-editor and
> start-state tables, the schema cache, and `ui::central_rect` in place of
> `center::viewport_rect`'s arithmetic. Plugins load from `editor/plugins/`
> and `<game>/editor/`, and `editor/plugins/counter.rn` is the worked example
> the e2e suite runs as `--state counterdemo`. Phase 3 is under way: the
> filesystem verbs (`fs::remove`, `mkdir`, `rename`, `mtime`) and the Assets
> dock's create, rename and delete; node copy and paste over the clipboard;
> `ui::color` for a schema colour; and `render::pick_ray`, which the 3D
> select tool now uses in place of the nearest projected origin. Drag and
> drop, world-anchored text, the export and import verbs and the async
> tokens are not started. See [generated/](generated/) for what the code
> actually does.
>
> **Where the implementation decided differently:**
>
> 1. **A plugin returns a description; it is not handed an `api` object.**
>    In Rune `api.dock(..)` is an instance-function call, not a call of a
>    closure stored on a field, so the `api.*` shape in §4 cannot work as
>    written. `pub fn register()` returns `#{ docks, windows, commands,
>    sections, overlays, editors, states }` instead, which is also
>    inspectable before anything runs.
> 2. **`script::shared(f, arity)` takes the arity.** A native wrapper is
>    typed and a `Function` value does not carry its own. Rune's
>    `Function::new` stops at five arguments, so a property editor receives
>    the component and property it edits as one `#{ comp, prop }` rather than
>    as the six arguments §1 assumed.
> 3. **`ui::central_rect()` measures the surface being drawn into**, not the
>    context's leftover rect: egui 0.36 has no public accessor for the
>    latter. Called inside `central_panel` it is the hole the panels left,
>    which is what the editor needed. It is 0.8 design px taller than the
>    arithmetic it replaced.
> 4. **`model::set_transform` does not record history.** A drag records once
>    at its start and then writes every frame, so recording inside the writer
>    would either duplicate or depend on the coalescing window.
> 5. **Picking takes the ray, and keeps the old fallback.** The backend's
>    `ViewportSnapshot` is empty without a window and every e2e run of the
>    editor is headless, so a picker that read the mouse itself could not be
>    tested; `render::pick_ray` takes the ray and the select tool passes it
>    `render::mouse_ray()`. A mesh keeps its vertices in an asset, so the
>    component carries no bounds for one — those still pick by nearest
>    origin, which is what everything used to do.
> 6. **`script::functions` did not replace `highlight::analyze`.** The
>    editor's host is rooted at the editor project and the sources it shows
>    belong to the game, so the parser stays for those; `script::functions`
>    is what the plugin loader uses to check a file declares `register`
>    before requiring it.

# Plan: the editor after the port

Three questions, one answer each: what in the editor should be refactored,
reused or rewritten; where a language feature would serve better than the
pattern in use; what the engine still has to expose. Then the question those
lead to: how a project adds a window, a tool or a persona of its own.

## 0. Where the tree is today

| | Status |
|---|---|
| Structure | One unit, `mod` per panel or tool, one state object `S` (60 fields, all declared in `editor.rn`) handed to every function. `draw_ui` lays the shell out in a fixed order; `update` runs the viewport tools. Sound for an immediate-mode editor, and every module can read and write every field. |
| Repetition | Two gizmos (`gizmo.rn` 790 lines, `gizmo2d.rn` 517) share their colours, snapping, hot-part logic, drag bookkeeping and the four rail-tool fallbacks, each written twice. `hover_color`, `world_per_px`, `orange`, `snapping`, `hot_part` and `manipulates` exist in two to four copies across `gizmo`, `gizmo2d`, `rig` and `polygon`. A transform is written to the document and to the live node as a pair at 29 sites; a `Vec3` is unpacked to `[v.x, v.y, v.z]` at 15. A mono label's option object (`#{ size: 11.5, font: "mono", color: k.faint }`) is spelled out 58 times; the section heading exists in four variants. |
| Extension points | None. Personas, tools and chips are literal lists in `defs.rn`; dock tabs in `dock.rn`; document tabs in `center.rn`; the inspector's per-persona layout is an `if` chain in `inspector.rn`; palette commands are a literal list plus the engine's component registry. A plugin cannot add a window, a dock tab, a tool, an inspector section or a command without editing those files. |
| What already crosses the seam | Engine extensions (`extensions/*.dylib`, `docs/PLAN-extensibility-and-quality.md`) reach the editor with no editor code: a registered component gets an inspector section from its schema, a palette entry from `scene::component_types`, and a preset from `presets.toml`. The gap is editor-side (UI) plugins, not engine-side ones. |
| Language | Rune 0.14: `#{}` objects everywhere, no structs, closures, `match`, `Option`. `pub async fn` and `task::wait` exist in the host but the editor uses neither. `script::require(path)` loads another file's `pub fn`s as an object and hot-reloads it in place; a `Function` value from one unit called inside another unit's VM dispatches into the wrong unit, which is why `require` routes every call through a Rust trampoline (`SHARED_FNS` in `crates/balaur_script_rune/src/lib.rs`). |
| Engine surface the editor leans on | `fs` (`exists`, `list`, `read`, `write`: no mtime, no remove, no mkdir, no watch), `ui` (panels, overlay, modal, 30 widgets; no floating window, no drag-and-drop, no clipboard, no colour picker), `render` (lines, camera pose and matrix, mouse ray; no picking, no text), `task` (`wait` only), `scene`, `assets`, `skeleton`, `debugger`. |

## 1. Refactor, reuse, rewrite

Ordered by what it removes per line changed. Each is mechanical and self-tests
(`--state undodemo,rigdemo,polydemo,animdemo,breakdemo`) cover it.

| What | Where | Change |
|---|---|---|
| **Viewport tool commons** | `gizmo`, `gizmo2d`, `rig`, `polygon` | One `viewport.rn`: `world_per_px`, `mouse_design_px`, `over_viewport(rect, mx, my)`, the two colours, `snapping`, `hot_part`, `manipulates`, `end_drag`. The rail-tool fallbacks (move, rotate, scale, zoom by mouse delta) become one function taking the axis mapping; the 2D and 3D gizmos keep only their geometry, hit-test and drag math. About 200 lines removed. |
| **One transform writer** | 29 sites | `model::set_transform(S, i, #{ position, rotation_euler, scale })` writes the document and the live node together, records history with the caller's key, and is the only place that spells the pair. The gizmo's rotate fallback reads the document's `rotation_euler` where every other writer reads the live node; one writer ends that drift. |
| **Vector helpers** | `gizmo.rn` (`vsub`…`vnorm`), `rig.rn` (hand-rolled cross products), `overlays.rn`, `anim.rn` | Move to `util` as `xyz(v)`, `vadd`, `vsub`, `vscale`, `vdot`, `vcross`, `vnorm`, `dist`. `[p.x, p.y, p.z]` becomes `util::xyz(p)`. |
| **A style module** | every panel | `style.rn` with the recurring option objects as functions of the theme: `mono(k)`, `faint(k)`, `heading(k)`, `pill_primary(k)`, `pill_ghost(k)`, `field(k)`, plus `util::merge(a, b)` for the per-call overrides Rune has no spread syntax for. Cuts the 58 mono labels and the four headings to one definition each; a theme change becomes one edit. |
| **Module-owned state** | `editor.rn` `init` | Each tool declares its own slice: `polygon::state()` returns `#{ hover, drag, loop, mode, texture, bone, radius, strength, backdrop }`, stored as `S.polygon`; likewise `S.gizmo`, `S.rig`, `S.anim`, `S.debug`. The root object keeps only shell state. This is also what gives a plugin a place of its own (`S.plugins[id]`). |
| **Property editors as a table** | `inspector::prop_row` | The `if datatype == …` chain becomes `EDITORS[datatype](S, k, comp, prop, spec, value)`. Same code, but a plugin can register an editor for a datatype or an asset type (a curve, a colour ramp) without touching the inspector. |
| **Start states as a table** | `shell::apply_start_state` | `STATES[name]` instead of the `if` chain; self-tests register theirs. |
| **Layout from egui, not arithmetic** | `center::viewport_rect` | The viewport rect is derived from seven panel sizes that are also literals in seven `ui::*_panel` calls. With `ui::central_rect()` (§3) the derivation goes, and a plugin dock of any height cannot break the gizmo's hit-testing. |
| **Hooks from the host** | `highlight::analyze` | The host already reads every script's `pub fn` signatures (`PublicSignature`) and, with breakpoints, its line table. `script::functions(path)` (§3) returns name, arity, async flag and line; the editor's parser and its `hooks` cache go. |
| **Schema cache** | `inspector`, `palette`, `selftest` | `scene::component_schema` is called per component per frame. Schemas are fixed for a session: cache in `S.schemas` on first use. |

Not worth changing: whole-document history (simple, correct, coalesces
drags; a scene is small); the hand-drawn tree (rows are pills, which is
what gives them menus and icons); `selftest.rn` (it is the editor's CI and
reads well).

## 2. What the language would do better

**Async for sequences that span frames.** Four places hold a state machine
in `S` to do something "next frame" or "after N frames": `anim_rebind`,
`shot` at frame 60, `breakdemo`'s alternation, `polygon::idle`'s backdrop.
Each is a flag, a check in `update`, and a reset. As `pub async fn` they
read as what they are:

```rune
pub async fn shot(this, path) {
    task::frames(60).await;
    render::screenshot(path);
}

pub async fn break_demo(this) {
    loop {
        let p = task::paused().await;
        debugger::resume(if p.reason == "breakpoint" { debugger::STEP_OVER } else { debugger::CONTINUE });
    }
}
```

That needs two tokens the `task` module does not mint yet: `task::frames(n)`
/ `task::seconds(t)` (woken by the frame pump) and `task::paused()` (woken by
the debugger). Both are small: the pump already wakes `http` tokens the same
way. The self-tests are the other natural async: `undo(S)` mutates, then
waits a frame for the mirror, then asserts, and today it cannot wait, which
is why `mirror_resolves` asserts on the synchronous rebuild only.

Async does not belong in draw code: `draw_ui` is immediate-mode and a frame
is a frame. Long operations do belong there (export, `balaur import
model.glb`, a texture size read, a scene reload): they need an engine op that
runs on a thread and a token to await (`task::wait(engine::export_async(root))`),
so the shell keeps drawing.

**`match` over `if let` shadowing.** Rune rejects `if let Some(x) = x` when
the name is bound on both sides, and the port left about 35 `let x_some = x;`
workarounds. `match` with a `None => return` arm is what the same sites read
as elsewhere in the tree; the workaround should not spread. Worth an upstream
issue.

**Structs, sparingly.** `S`, a drag, a gizmo geometry and a history frame are
shapes that never vary. A Rune `struct` with an `impl` gives a compile error
for a missing field at construction and a named type in a stack trace; it
does not give field checks on access, so the gain is modest. Take it for the
module-owned state slices in §1 (they are constructed in one place each) and
leave `#{}` for document data, which is TOML-shaped and must stay so.

**Generators for multi-click tools.** The polygon loop (`S.polygon_loop`
plus three branches in `polygon::update`) and the bone chain are
click-sequences; a Rune generator that `yield`s between clicks would hold its
own place in the sequence. Nice, not necessary; listed so it is not
rediscovered.

## 3. What the engine has to expose

Each of these is something the editor works around today.

| Missing | Editor workaround now | Shape |
|---|---|---|
| `fs::mtime(path)` and a change feed | `model::source` caches with a 0.5 s TTL and re-reads; the Assets dock lists directories on the same timer | `fs::mtime`; and `fs::changed()` returning the paths the host's own `notify` watcher saw this frame, the way `input::dropped_files` reports drops. `engine::watch(dir)` adds a root, so a game's directory hot-reloads inside the editor (today only the editor's root is watched; play-in-editor reloads on ⌘S by calling `engine::reload_script`). |
| `fs::remove`, `fs::mkdir`, `fs::rename` | none: the Assets dock cannot create, rename or delete | plain wrappers, rooted at the project like `fs::write` |
| `ui::central_rect()` | `center::viewport_rect` re-derives the hole between panels from seven literals | the rect egui has after the panels, in design px |
| `ui::window(id, opts, body)` | none: the only floating surfaces are `overlay` (positioned by the caller) and `modal` | egui `Window`: title, closable, resizable, position remembered per id; the surface a plugin gets when it asks for "a window" |
| `ui::drag_source` / `ui::drop_target` | none: nothing can be dragged from the Assets dock onto a node or a property | egui's DnD, payload a string |
| `ui::clipboard()` / `ui::set_clipboard(text)` | none: no copy/paste of nodes | egui's clipboard |
| `ui::color(value)` | four drag values | egui's colour picker; the inspector's `color` editor and a plugin's |
| `script::functions(path)` | `highlight::analyze` parses `pub fn` lines by hand | `[#{ name, arity, is_async, line }]` from the host |
| `script::shared(f)` | none: a closure handed across units cannot be called | wraps any `Function` in the `require` trampoline; what lets a plugin pass `\|\| …` to the registry |
| `task::frames(n)`, `task::seconds(t)`, `task::paused()` | flags in `S` | tokens the pump and the debugger wake |
| `render::pick(x, y)` | the select tool projects every node's origin and takes the nearest within 40 px; 2D tests half extents | a ray or point test against what is drawn (shapes, sprites, polygons, meshes); returns the node. **The ray is an argument, not the mouse**: the backend's `ViewportSnapshot` is all zeros without a window, so a picker that read it could not be tested in a headless run — and every e2e run of the editor is headless. `render::pick_ray(o, d)` tests `Renderable` + `GlobalTransform`, which is engine data the renderer never sees, and the select tool passes it `render::mouse_ray()`. |
| `render::draw_text_2d` / `draw_text` | none: overlays cannot label a bone or a node | a world-anchored label in the line layer |
| `engine::export(root)`, `assets::import(path)` | CLI only | the two verbs an Export button and an Import command need, async-capable |

## 4. Editor plugins

### Two kinds, one of which exists

An **engine plugin** is Rust (or C through the C API): it registers
components, script modules and assets, and the editor already shows all of
it. A **editor plugin** is Rune: it adds UI and tools. That is the missing
kind, and everything it needs except `ui::window` and `script::shared` is
in the tree.

### Where a plugin lives

- `editor/plugins/<name>.rn` ships with the editor and is on for every project.
- `<game>/editor/<name>.rn` belongs to one project and is on there only.

`editor.rn` lists both directories with `fs::list` at start-up and loads each
file with `script::require(absolute_path)`; the host resolves an absolute key
as it is and hot-reloads the module in place, so editing a plugin is editing
the editor. A plugin file declares `pub fn register(api)` and, optionally,
`pub fn state()`.

### What `register` gets

`api` is an object of functions, each pushing into a registry on `S`:

| Call | Adds |
|---|---|
| `api.dock(id, name, draw)` | a dock tab beside Output / Assets / Timeline / Debugger; `draw(S, k)` |
| `api.window(id, title, draw)` | a floating `ui::window`; opened from the palette |
| `api.persona(id, name, icon, tools)` | a persona, with its tools and chips |
| `api.tool(persona, id, name, icon, handlers)` | a rail tool; `handlers.update(S, over_viewport, mx, my)`, `handlers.draw(S)` |
| `api.inspector(section)` | `section(S, k, node)`, drawn after the built-in sections; may draw nothing |
| `api.editor(datatype, draw)` | a property editor for a schema type or an asset type (§1) |
| `api.command(label, icon, key, run)` | a palette entry |
| `api.chip(persona, key, label, default)` | a viewport chip |
| `api.overlay(draw)` | per-frame viewport lines |
| `api.state(name)` | a start-up state, so a plugin can carry its own self-test |
| `api.on(event, handler)` | `selection`, `play`, `stop`, `pause`, `save`, `reload` |

Every callback is a `Function`; `api` wraps each with `script::shared` so the
editor's VM can call it. The built-in panels move onto the same registries
first (phase 1), so Timeline is the first registered dock and the Polygon
tool the first registered tool, and a plugin is not a second-class path.

### What a plugin sees

`S` in full, the same as a built-in module: the document, the mirror, the
selection, history, the theme. `history::record(S, label, key)` before a
mutation is the one rule, and `model::*` is the API for the document. A
plugin's own state is `S.plugins[id]`, from its `state()`.

Nothing is sandboxed: a plugin runs in the editor's host with the editor's
capabilities. That is the right first cut for a tool a project writes for
itself; a marketplace would need more, and nothing here precludes it.

### An example

```rune
// <game>/editor/counter.rn: how many nodes carry a script, as a dock tab
// and a palette command.

pub fn state() {
    #{ shown: 0 }
}

pub fn register(api) {
    api.dock("counter", "Counter", draw);
    api.command("Count scripted nodes", "Σ", "", count);
}

pub fn count(S) {
    let (_, scripts) = crate::model::stats(S);
    S.plugins.counter.shown = scripts;
    log::info(format!("{} scripted nodes", scripts));
}

pub fn draw(S, k) {
    ui::label(format!("{} scripted nodes", S.plugins.counter.shown), style::mono(k));
}
```

(`crate::model` is the editor's; a required module reaches the editor's
modules through the object `api.editor_modules` hands it, since `crate` is
per unit.)

## 5. Phases

| Phase | What | Size |
|---|---|---|
| 0 | §1 refactors: `viewport.rn`, `style.rn`, `model::set_transform`, vector helpers, module-owned state, the two tables. No behaviour change; self-tests stay green. | 2 days |
| 1 | Registries: docks, tools, personas, chips, commands, inspector sections, property editors, start states on `S.registry`; built-ins registered through them. `ui::central_rect`, `script::functions`, `script::shared`, `task::frames`. | 2 days |
| 2 | Plugin loading from the two directories; `api`; `ui::window`; `fs::mtime`/`changed`/`engine::watch`; `docs/manual/editor.mdx` gains "Extending the editor" with the example above. | 2 days |
| 3 | The rest of §3 as the tools that need them land: DnD and clipboard (Assets dock, node copy), `render::pick` (select tool), `draw_text` (bone names), `engine::export` / `assets::import` (an Export button, an Import command), the async self-tests. | as needed |

## 6. Tools not yet built

Three tools the plugin seam now makes cheap to add. Each is a persona tool
or a dock registered like the built-ins, and each lands with a `--state`
self-test.

### Tilemap editor

A `tilemap` component draws today; nothing paints one. The Scene persona
gains a Tiles tool: a palette dock showing the tile set's texture cut by
`tile_size`, click and drag to paint the selected tile, right-drag to
erase, a rectangle fill, and layers as sibling `tilemap` nodes. Autotiling
is a rule table on the tile set (`[[autotile]]` with a bitmask per tile) the
painter consults after each stroke. Edits write the `cells` string through
`history`, so undo is free.

### Curve editor and onion skin

The Animate persona's timeline shows keys as dots on a lane. A curve view
under it draws each track's channels as curves against time, with the
easing of a segment editable by dragging a handle at the key — the twelve
named easings stay the storage, and a handle drag picks the nearest one,
so the file stays readable. Onion skin ghosts the posed rig at the previous
and next keys behind the viewport, from the same sampler the preview uses.

### Profiler

The engine records a duration per stage and per system each frame, plus
rapier's counters and the script host's time per hook; `engine::timings()`
returns the last frame's table. A Profiler dock draws them as a stacked bar
per frame against a budget line at 16.7 ms and lists the top entries; a
budget table in the project marks a row red when it is over. Headless runs
print the same table with `--timings`, which is what the benchmark suite
already wants.

Phases: profiler first (it measures the other two), then the tilemap
editor, then the curve view and onion skin.
