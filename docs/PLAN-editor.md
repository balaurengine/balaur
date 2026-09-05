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

The curve editor, and the rigging panels below it. Each is a persona tool
or a dock registered like the built-ins, and each lands with a `--state`
self-test.

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

### Rigging panels

Measured on 2026-09-05 against what Godot 4's Skeleton2D, Polygon2D and
Skeleton3D editors and Spine's setup mode show. Built: the Rig tool (a
chain grown by clicking, picking in 2D and 3D, Reset and Overwrite Rest
Pose), the Polygon tool's Points, Polygons, UV and Weights modes with a
brush, `modifier2d` as inspector rows, the timeline dock, and `bone3d`
from `balaur import`. What a rigger still reaches for, in the order it
pays off; every entry is editor work over data the engine already has,
except where it names the animation plan.

- **A weight table.** Spine's Weights view. A dock for the Weights mode
  listing the selected vertices with one number per bone, editable in
  place, with Bind and Unbind, Normalise, Auto (weights by distance to
  each bone's segment, the same segment `rig::geometry` draws) and Smooth
  (average with the vertices sharing an edge). Writes `mesh.skin.weights`
  through `history`, as the brush does. Planned.
- **Modifier gizmos.** Godot's SkeletonModification2D editor shows the
  target and the elbow; here `look_at` and `two_bone_ik` are inspector
  rows. Planned: a handle at the target node, a line from the driven bone,
  and a `flip` toggle on the chain, drawn by `rig::draw` from the same
  `modifier2d` table. Chain solvers and jiggle are the engine's first —
  `docs/PLAN-animation-and-resources.md` "Rig tooling".
- **Bone names in the viewport.** Needs `render.draw_text_2d` (§3); until
  then the inspector's Skeleton row is the only place a bone is named.
  Planned with §3.
- **Mirror and symmetry.** Godot has neither; Spine mirrors bones, vertices
  and weights across an axis. Planned as one verb, Mirror, in the Rig and
  Polygon tools: reflect the selection's rest pose or vertices over the
  node's x or y axis, and map each bone's weights to the bone whose name
  differs by a `left`/`right` or `l_`/`r_` prefix.
- **A mesh traced from the texture's alpha.** Spine's Generate. Planned:
  a Trace button in the Polygon section that walks the image's alpha at a
  threshold, simplifies the outline to a tolerance in pixels, and writes
  `positions`; the image is already read headless for `natural_half_extents`,
  so the tracer is CPU code in `balaur_core::geometry2d` with a test.
- **Deform keys.** Spine's free-form deformation: a clip animating vertex
  positions without a bone. The engine half is a `polygon/deform` track
  (`docs/PLAN-animation-and-resources.md` "Rig tooling"); the editor half is
  the Points mode recording a key when the Animate persona is armed, the
  way a transform edit does today. Planned after the engine track.
- **A bone map for retargeting.** Godot's BoneMap and SkeletonProfile
  panel: an imported humanoid rig mapped to a canonical one so a clip
  plays on another rig. The engine has no `bone_map` asset yet; when it
  does, the panel is a two-column list of canonical names against the
  rig's bones with a guess by name. Planned after the asset.
- **Physical bones.** Godot's Create Physical Skeleton: a body and a
  joint per bone, for a ragdoll. Engine first (`docs/PLAN-physics.md`);
  the button is one command once a rig can be walked into bodies.
- **Not planned:** slots, attachments and skins as Spine has them — a
  node with a `sprite` and `visible` is the same thing here; a separate
  Skeleton node — a rig is the bones under a node, by design
  (ARCHITECTURE.md "Skeletons and skins"); 3D weight painting — weights
  come from the file, as in Godot.
