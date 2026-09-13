> **Status:** built 2026-09-07, every step. The prerequisites in
> `docs/PLAN-3d-rendering.md` (lights, environment, the material contract)
> and `docs/PLAN-interactivity.md` (hooks, states, bindings) were built
> alongside, because four of the eight steps here stood on them. What is
> not built is named in §6.

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
| A selection set: `S.sels` ordered, `S.sel` its head | `editor/scripts/selection.rn` |
| Move, rotate and scale gizmos in 3D and 2D, snapping, the tool rail | `gizmo.rn`, `gizmo2d.rn`, `viewport.rn`, `defs::tools` |
| Undo with labels, dirty tracking | `history.rn` |
| The palette, and every command as a script | `palette.rn` |
| Duplicate, reparent keeping the world pose, open the prefab | `model::duplicate_selected`, `node.reparent` |
| Node `visible`, `z_index`, `tags` | `balaur_core` |
| One search rule for every list, and the tree filtering by it | `search.rn`, `left.rn` |
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

Built since, in the order below: the selection set, group, align, distribute,
hide, lock and isolate; the outliner's facet chips; drag-in through
`dropin.rn` and a new `import.*` script module; light and camera gizmos, a
view-mode chip and four camera bookmarks; the Pen; the material panel over
`render.material_params`; the Events view; the Cost dock over `render.stats`;
and `editor/library`.

Still missing, and why:

- **Image-based lighting, SSAO and the shadow reads in the shader.**
  `package::pbr` is GGX over the frame's lights; the fork's IBL and SSAO
  buffers are not bound. `docs/PLAN-3d-rendering.md` steps 2 and 3.
- **Layer stacks.** `shaders/layers.wesl` is that plan's step 8.
- **Overdraw as a view mode.** It wants a counter per pixel, not
  `render.stats`.
- **A `path3d` pen.** The Pen edits `path2d`; a curve in space wants a
  plane to draw on, which is a design question rather than a missing call.

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
`[[nodes.bindings.rows]]` rows, one per line: an event dropdown from the hook
list, a target picked by clicking a node, an action dropdown, a value editor
from the target's schema, a `when` field. "Convert to script" writes the file
and opens it in the code pane.

**A cost dock.** `render.stats` from `docs/PLAN-embed.md` — draw calls,
triangles, texture bytes per node — beside `engine.timings` and the pack's
size per asset from `balaur export`, as a tab of the Profiler dock.

**The library is files.** `editor/library/` holds `material` assets over the
stock shaders, three gradient skies written by `scripts/make_skies.py`, three
models built from primitives, lighting setups, and project templates for
`balaur new --template`. Nothing in it is photographed or scanned, so it is
kilobytes and carries no third-party licence. A library dock lists them with
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
| Pen tool | Step 4, over the `path2d` and `path3d` assets already built |
| Material panel, preview sphere, layers | Step 5, with `docs/PLAN-3d-rendering.md` steps 3 and 8 |
| Events view authoring | Step 6, with `docs/PLAN-interactivity.md` step 3 |
| Cost dock | Step 7 |
| Library dock, stock content, templates | Step 8 |
| Clicking exactly what is drawn | *Built 2026-09-06.* `pick` sorts by the box, then casts against the node's triangles through parry, so a click between the spokes of a wheel misses it. A `Bvh` over the boxes is `docs/PLAN-views-and-culling.md` step 1, for when the linear pass stops being enough |
| Snap to vertex, edge, face | Not planned; grid snapping stays the one snap |
| Mesh editing, sculpting | Not planned: they are a modeller, and a `.glb` from Blender is the answer |
| A community library | `docs/PLAN-collaboration.md` |

## 3. Steps

All eight built, 2026-09-07, in this order, with the engine work each stood on
built beside it.

1. **Selection.** *Built.* `selection.rn`, `arrange.rn`, box select in both
   viewports, a gizmo drag over the set. The `seldemo` state asserts a two-node
   align and its undo.
2. **Finding and dropping.** *Built.* Facet chips in `left.rn`, `dropin.rn`,
   and `import_api.rs` for the seam `balaur import` never had. `dropdemo`.
3. **Seeing.** *Built.* `overlays::lights3d`, the view-mode chip and four
   camera bookmarks in `center.rn`, over `light3d` and `environment`.
4. **Pen.** *Built.* `pen.rn`, over `path2d`. Two engine defects came out of
   it: an empty path did not parse, and an inline asset table was registered
   as the type its schema named rather than the one it declared, so a polyline
   naming an inlined `path2d` was read as a mesh.
5. **Materials.** *Built.* `Param::Texture` and `package::pbr`; the panel is
   the inspector's material rows, which resolve an inline material too, with a
   shader picker and the `@if` flags as toggles. No preview sphere.
6. **Events.** *Built.* `bindings.rs`, `states.rs`, `variables.rs` and the
   hook dispatch in `balaur::interact`; `events.rn` authors them. `eventsdemo`.
7. **Cost.** *Built.* `render.stats` and the Cost dock.
8. **Library.** *Built.* `editor/library`, the dock, and
   `balaur new --template`. `librarydemo`.

## 4. What CI can prove, and what it cannot

- Every command runs headless in `selftest.rn` on every example, as the
  editor's tests do today; a `--state` per new surface joins
  `scripts/uiaudit.sh` and `docs/EDITOR-SCREENS.md`.
- Drag-in is a function per extension with a test per extension, no window.
- The library's thumbnails are regenerated offscreen and diffed.
- What it cannot: the feel of a box select or a handle drag on a real
  pointer. That is the small-window pass a release already gets.

## 5. Open questions

1. **The inspector over a mixed selection.** *Settled:* the banner, and a
   property edit reaching every selected node that carries the component.
2. **How much library ships in the download.** *Settled:* only what is
   written rather than captured. Four materials, three gradient skies, three
   primitive models, two lighting setups and three templates, together under
   twenty kilobytes. A photographic catalogue is fetched, not shipped.
3. **Whether isolate and lock persist.** *Settled:* nowhere.

## 6. What is not built

- Image-based lighting, SSAO, `shaders/layers.wesl`, and overdraw as a view
  mode. Each is named in §0 with the plan it belongs to.
- Photographic skies and scanned models. The library's three skies are
  gradients and its three models are primitives, so the whole library is
  kilobytes and carries no third-party licence. A CC0 catalogue is fetched,
  not shipped: `docs/PLAN-collaboration.md`.
- The material panel's preview sphere. It wants a frame drawn offscreen into a
  texture the interface can show, which nothing else in the editor does yet.
- A `path3d` pen. The Pen edits `path2d`.
- `when` is a comparison over the scene's variables, not a Rune expression:
  a condition is data in a scene file, so the editor reads it, shows it and
  diffs it, and anything a comparison cannot say is a script.
- Pointer hooks need a window. `docs/PLAN-interactivity.md` §4 says why.
