> **Status:** not started. Written down on 2026-09-05, from what a Spline
> user does in the first ten minutes that the editor does not let them:
> select two things, line them up, hide one, drop an image in, pick a
> material from a list. The order is selection first, because group, align
> and every multi-node command stand on it; then the gestures around the
> viewport; then the panels that author what `docs/PLAN-3d-rendering.md`,
> `docs/PLAN-objects.md` and `docs/PLAN-interactivity.md` add; then the
> library, which is content more than code.

# Plan: editor ergonomics

Multi-select and box select, group and ungroup, align and distribute, hide,
lock and isolate, an outliner filter, drag-in import, light and camera
gizmos, view modes, a pen tool, a material panel, an Events view that
authors, a cost dock, and a library of materials, skies, models and
templates. `docs/PLAN-editor.md` §6 holds the curve editor, and
`docs/PLAN-tilemap.md` what the Tiles tool still lacks; neither is repeated
here.

## 0. Where the tree is today

Built, and not built for this:

| Have | Where |
| --- | --- |
| One selection: `S.sel` indexes `S.doc` | `editor/scripts/model.rn` |
| Move, rotate and scale gizmos in 3D and 2D, snapping, the tool rail | `gizmo.rn`, `gizmo2d.rn`, `viewport.rn`, `defs::tools` |
| Undo with labels, dirty tracking | `history.rn` |
| The palette, and every command as a script | `palette.rn` |
| Duplicate, reparent keeping the world pose, open the prefab | `model::duplicate_selected`, `node.reparent` |
| Node `visible`, `z_index`, `tags` | `balaur_core` |
| One search rule for every list | `search.rn` |
| Files dropped on the window | `input.dropped_files` |
| A `.glb` brought in as models, a scene and clips | `balaur import` |
| A colour editor, sliders, dropdowns and folds in the inspector | `ui`, `inspector.rn` |
| Component presets and tags | `scene.presets`, `apply_preset` |
| The Polygon tool, which already edits a mesh in the viewport | `polygon.rn` |
| The Tiles tool: a palette dock, paint and erase, rectangle fill, layers as sibling maps | `tiles.rn`, `ui.image_button` |
| The Export sheet, off the frame with progress | `export_api.rs`, `exporter.rn` |
| Wireframe, normals and UV materials in the fork | `kiss3d::builtin::{wireframe, normals_material, uvs_material}` |
| Timings per system, and a profiler dock | `engine.timings` |
| The editor's own self-test with named states | `selftest.rn`, `--state` |

Missing:

- **More than one selected node.** Every command reads `S.sel`.
- **Group, align, distribute, isolate, lock.** None; `visible` exists but no
  shortcut toggles it.
- **An outliner filter.** `search.rn` exists and the tree does not use it;
  no filter by component.
- **Drag-in.** A dropped file is an event a game may read; the editor does
  nothing with it.
- **Gizmos for what is not a shape.** A `camera` and, once it exists, a
  `light3d` draw nothing in the viewport.
- **View modes.** The viewport draws the game's look and nothing else.
- **Panels for the new content.** A material is a TOML file edited by hand;
  a path has no pen; the Events view lists hooks and authors nothing.
- **A library.** No stock materials, skies, models or templates to start
  from.

## 1. Design

**Selection is a set, and the first element is the active one.** `S.sel`
becomes `S.sels`, ordered, with `S.sel` kept as its head so every existing
command works on the active node unchanged; commands that make sense over a
set — delete, duplicate, hide, lock, group, align, set a shared property —
iterate. Shift-click and ⌘-click extend, a drag on empty viewport box-selects
through the projection `gizmo::project` already uses, and the inspector shows
the active node's rows with a "N selected" banner and applies a property edit
to every node that has the component. Undo records the set.

**Group is a node.** Group creates an empty parent at the selection's
centroid and reparents into it keeping world poses, which `node.reparent`
already does; ungroup is the reverse. Align and distribute are six commands
over world positions along one axis, in the palette and a toolbar. Hide,
lock and isolate are editor state on the mirror: hidden writes `visible`,
locked is a set the gizmos and box select skip, isolate hides everything not
selected until toggled.

**The outliner filters with the search everyone else uses.** A field at the
top of the tree using `search.rn`, plus a component filter chip — cameras,
lights, meshes, scripts — built from `scene.component_types`.

**A dropped file becomes a node.** An image drops as a `sprite` at the
pointer in 2D or a textured `plane` in 3D and is copied under `art/`; a
`.glb` runs `balaur import` and instantiates the result; a `.wesl` becomes a
material; an `.hdr` sets the environment's sky; a `.toml` scene instantiates
as a prefab; a font lands under `fonts/`. The rule is one function per
extension in a `dropin.rn` module, and every one goes through `history`.

**Gizmos for lights and cameras.** A camera draws its frustum and gets a
"look through" command; a light draws its kind — an arrow, a sphere at its
radius, a cone with both angles — with the radius and cones draggable. Both
are `gizmo.rn` handlers like the transform ones.

**View modes are the fork's materials applied to the mirror.** Shaded,
wireframe, normals, UVs, unlit, and overdraw once `render.stats` exists, as
a viewport chip; the fork already draws the first four and the editor sets
the material on every mirrored node. Never a game-visible feature.

**The pen tool edits a `path2d` or `path3d` in place.** Click adds an anchor,
drag pulls its handles, alt breaks them, with the Polygon tool's picking and
history pattern reused; the path is written back to the asset, inline or in
its file.

**A material panel is the inspector over `material_params`, plus layers.**
For a `material` asset the inspector already derives rows from the shader's
`Params`; the panel adds a preview sphere drawn offscreen, a shader picker
over `shaders/` and the library, `features` as toggles, and — once
`layers.wesl` exists — a layer stack with add, remove and reorder as folds.

**The Events view authors bindings.** `docs/PLAN-interactivity.md`'s
`[[nodes.bindings]]` rows, one per line: an event dropdown from the hook
list, a target picked by clicking a node, an action dropdown, a value editor
from the target's schema, a `when` field. "Convert to script" writes the file
and opens it in the code pane.

**A cost dock.** `render.stats` from `docs/PLAN-embed.md` — draw calls,
triangles, texture bytes per node — beside `engine.timings` and the pack's
size per asset from `balaur export`, as a tab of the Profiler dock.

**The library is files.** `editor/library/` holds `material` assets over the
stock shaders, a handful of CC0 skies from Poly Haven at a modest resolution
with their licence in `THIRD-PARTY-NOTICES.md`, a few `.glb` models, and
project templates for `balaur new --template`. A library dock lists them with
thumbnails rendered offscreen at build time; dragging one copies the file
into the project and drops it as above. Nothing at runtime references the
library. A Gamend-hosted catalogue with the same manifest is
`docs/PLAN-collaboration.md`'s, later.

## 2. The surface

| Piece | Decision |
| --- | --- |
| Multi-select, box select, shift and ⌘ | Step 1 |
| Group, ungroup, align, distribute | Step 1 |
| Hide, lock, isolate, with shortcuts | Step 1 |
| Outliner filter and component chips | Step 2 |
| Drag-in of image, glb, wesl, hdr, toml, font | Step 2 |
| Light and camera gizmos, look through | Step 3 |
| Camera bookmarks | Step 3, four slots on the viewport chip |
| View modes: shaded, wireframe, normals, UVs, unlit, overdraw | Step 3 |
| Pen tool | Step 4, with `docs/PLAN-objects.md` step 3 |
| Material panel, preview sphere, layers | Step 5, with `docs/PLAN-3d-rendering.md` steps 3 and 8 |
| Events view authoring | Step 6, with `docs/PLAN-interactivity.md` step 3 |
| Cost dock | Step 7 |
| Library dock, stock content, templates | Step 8 |
| Snap to vertex, edge, face | Not planned; grid snapping stays the one snap |
| Mesh editing, sculpting | Not planned (`docs/PLAN-objects.md`) |
| A community library | `docs/PLAN-collaboration.md` |

## 3. Steps

1. **Selection.** The set, box select, group, align, hide, lock, isolate.
   Ends with: `selftest.rn` states for a two-node align and an undo of it.
2. **Finding and dropping.** The filter, the chips, drag-in.
3. **Seeing.** Gizmos, view modes, bookmarks.
4. **Pen.**
5. **Materials.**
6. **Events.**
7. **Cost.**
8. **Library.**

## 4. What CI can prove, and what it cannot

- Every command runs headless in `selftest.rn` on every example, as the
  editor's tests do today; a `--state` per new surface joins
  `scripts/uiaudit.sh` and `docs/EDITOR-SCREENS.md`.
- Drag-in is a function per extension with a test per extension, no window.
- The library's thumbnails are regenerated offscreen and diffed.
- What it cannot: the feel of a box select or a handle drag on a real
  pointer. That is the small-window pass a release already gets.

## 5. Open questions

1. **The inspector over a mixed selection.** The intersection of components,
   or the active node's rows with a banner. The banner is cheaper and is what
   the design above says; a mixed edit may never be asked for.
2. **How much library ships in the download.** Skies are megabytes each; the
   desktop editor can carry a few, the browser editor should fetch them.
3. **Whether isolate and lock persist.** Editor state in a sidecar, or
   nowhere. Nowhere first.
