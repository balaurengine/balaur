> **Status:** phase 0, the registries and the plugin seam shipped on
> 2026-09-02, and most of §3 with them — the filesystem verbs, the Assets
> dock's create, rename and delete, node copy and paste, `ui::central_rect`,
> `ui::color`, `script::functions`, `script::shared` and `render::pick_ray`.
> Plugins load from `editor/plugins/` and `<game>/editor/`, with
> `editor/plugins/counter.rn` as the worked example the e2e suite runs. What
> is left is below; see [generated/](generated/) for what the code does.

# Plan: the editor after the port

The numbering is the original plan's, so what is left keeps the names
ARCHITECTURE.md and the website roadmap already use.

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
module-owned state slices (they are constructed in one place each) and leave
`#{}` for document data, which is TOML-shaped and must stay so.

**Generators for multi-click tools.** The polygon loop (`S.polygon_loop`
plus three branches in `polygon::update`) and the bone chain are
click-sequences; a Rune generator that `yield`s between clicks would hold its
own place in the sequence. Nice, not necessary; listed so it is not
rediscovered.

## 3. What the engine still has to expose

Each of these is something the editor works around today.

| Missing | Editor workaround now | Shape |
|---|---|---|
| A change feed | `model::source` caches with a 0.5 s TTL and re-reads; the Assets dock lists directories on the same timer | `fs::changed()` returning the paths the host's own `notify` watcher saw this frame, the way `input::dropped_files` reports drops. `engine::watch(dir)` adds a root, so a game's directory hot-reloads inside the editor (today only the editor's root is watched; play-in-editor reloads on ⌘S by calling `engine::reload_script`). |
| `ui::window(id, opts, body)` | none: the only floating surfaces are `overlay` (positioned by the caller) and `modal` | egui `Window`: title, closable, resizable, position remembered per id; the surface a plugin gets when it asks for "a window" |
| `ui::drag_source` / `ui::drop_target` | none: nothing can be dragged from the Assets dock onto a node or a property | egui's DnD, payload a string |
| `task::frames(n)`, `task::seconds(t)`, `task::paused()` | flags in `S` | tokens the pump and the debugger wake |
| `render::draw_text_2d` / `draw_text` | none: overlays cannot label a bone or a node | a world-anchored label in the line layer |
| `engine::export(root)`, `assets::import(path)` | CLI only | the two verbs an Export button and an Import command need, async-capable |

## 5. Phases

Phase 3 is what remains, and it lands as the tools that need it do:
drag-and-drop from the Assets dock, `draw_text` for bone names,
`engine::export` / `assets::import` behind an Export button and an Import
command, and the async self-tests once the tokens exist.

## 6. Tools not yet built

One tool left. Each is a persona tool or a dock registered like the
built-ins, and each lands with a `--state` self-test.

### Tilemap editor — built, 2026-09-05

The Scene persona has a Tiles tool (`editor/scripts/tiles.rn`, `tilesdemo`).
The palette dock cuts the tile set's texture by `tile_size` and picks a tile,
left-drag paints it, right-drag erases, and a Rectangle mode fills between
two corners. A layer is a sibling `tilemap` node: the Add layer button
duplicates the map and empties it. Edits write `cells` through `history`, so
undo is free, and a map authored as text stays text while every tile it holds
still has a character.

The palette is what asked for `ui.image_button` and `region` on `ui.image`:
an atlas has to be shown a tile at a time and clicked. Both are general
widgets, not a tile-set feature.

Two things it does not do. **Autotiling** — a rule table on the tile set
(`[[autotile]]` with a bitmask per tile) the painter consults after each
stroke — is not built; it is a `tileset` asset change before it is a tool
change, and the tool has no rule to consult until the asset carries one.
And the map grows to the right and down only: it is centred on its node, so
the painter moves the node by half of what it added to hold the tiles
already down still, and a cell left of column zero has nowhere to go. Both
are `docs/PLAN-tilemap.md`'s, behind the tileset metadata a terrain brush
needs before it can paint.

### Curve editor and onion skin

The Animate persona's timeline shows keys as dots on a lane. A curve view
under it draws each track's channels as curves against time, with the
easing of a segment editable by dragging a handle at the key — the twelve
named easings stay the storage, and a handle drag picks the nearest one,
so the file stays readable. Onion skin ghosts the posed rig at the previous
and next keys behind the viewport, from the same sampler the preview uses.

The profiler that was to measure both shipped on 2026-09-03.
