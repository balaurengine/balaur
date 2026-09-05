> **Status:** not started. Written down on 2026-09-05 with
> `docs/PLAN-3d-rendering.md`, from the same comparison against Spline. The
> order is what a designer reaches for first: the primitives a palette
> starts with, then glyph outlines, because a 3D title is an extruded one,
> then paths and what they extrude into, then the compound objects —
> booleans, cloners — and last the instanced draw path clones and particles
> share. Everything builds a `mesh` asset headless, so colliders, picking and
> tests share the triangles the screen draws.

# Plan: objects

Parametric primitives, glyph outlines as meshes, bezier paths with extrude,
lathe and sweep, a boolean node, a cloner, the instanced draw path, vertex
colours and morph targets, and the 2D vector shapes a canvas expects. What
draws them is `docs/PLAN-3d-rendering.md`; what the editor does with them is
`docs/PLAN-editor-ergonomics.md`. Three neighbours draw through paths this
plan opens and own the components themselves: `docs/PLAN-text.md` has
`text2d` and `text3d`, `docs/PLAN-particles.md` has `particles3d`, and
`docs/PLAN-views-and-culling.md` has automatic instancing and `multimesh`
over the seam the cloner opens.

## 0. Where the tree is today

Built, and not built for this:

| Have | Where |
| --- | --- |
| Six 3D shapes: ball, cuboid, capsule, cylinder, cone, plane; four 2D: circle, rect, capsule, polyline | `shape.rs`, `words::SHAPES`, `SHAPES_2D` |
| A `mesh` asset that carries its own vertices, so a script builds one at run time | `balaur_core::mesh::MeshData`: positions, indices, normals, uvs, skin |
| OBJ, GLB and glTF import, with skins and animations | `balaur_core::mesh`, `glb.rs`, `balaur import` |
| 2D booleans, hulls, triangulation | `geometry2d` over `i_overlay`, and `balaur_core::triangulate` (ear clipping, one ring, `f32` only) |
| 3D hulls, decomposition, intersect, split, pieces, voxelise | `geometry3d` over parry3d with `spade` |
| A 2D polygon with a rig and painted weights, and the tool that traces it | `polygon`, `editor/scripts/polygon.rn` |
| Polyline strips with width, gradients and textures | `polyline_strip.rs` |
| Shaped text with bidi, fallback and CJK breaks, for widgets | `balaur_ui::text` over cosmic-text with `swash` |
| A 2D particle emitter, textured, one-shot, observer-only | `particles.rs` |
| Bezier curves and surfaces, a pipe along a polyline, instancing, morph targets — all in the fork and none reached | `kiss3d::procedural::{bezier, path}`, `set_instances`, `builtin/deform.rs` |
| Clip tracks addressing `<component>/<property>` | `balaur_anim` |

Missing:

- **Parametric primitives.** No torus, pyramid, prism, tube or rounded
  cuboid, and no `segments` or `corner_radius` on the six that exist.
- **Text as geometry.** `docs/PLAN-text.md` draws text as quads from an
  atlas; nothing turns a glyph's outline into a mesh a title extrudes from
  or a collider fits to.
- **Paths.** No bezier asset, no editable path, no extrude, lathe or sweep.
  The fork's `bezier_curve` lives in a crate a headless build does not link.
- **A boolean node.** `geometry3d.intersect` is a script call that returns a
  mesh; nothing in a scene says "this minus that".
- **A cloner.** The renderer draws every node it is handed; the fork's
  instancing is never used.
- **An instanced draw.** Nothing draws one mesh many times in one call;
  `docs/PLAN-particles.md`'s billboards and the cloner both need it.
- **Vertex colours and morph targets** in `MeshData`, so a glTF that carries
  either loses it.
- **2D vector shapes.** No ellipse, rounded rect, star or regular polygon,
  and no path stroke.

## 1. Design

**Every object is a `mesh` built in `balaur_core`.** A new `shape3d` kind is
a function from its parameters to a `MeshData`, written in core with `libm`,
never taken from the fork's `procedural` module: a collider, an occluder
outline, `pick_ray` and a headless test all read the same triangles, and the
digest covers a mesh a script builds. The existing six move onto the same
path so there is one mesher, and `segments` and `corner_radius` arrive with
it.

| Kind | Parameters |
| --- | --- |
| `torus` | `radius`, `tube_radius`, `segments`, `rings` |
| `pyramid` | `half_extents`, `sides` (4 by default) |
| `prism` | `radius`, `height`, `sides` |
| `tube` | `radius`, `inner_radius`, `height`, `segments` |
| `cuboid` | gains `corner_radius` |
| `plane` | gains `segments` |

**Glyph outlines are a mesh path, not a component.** `docs/PLAN-text.md`
owns `text2d` and `text3d` and draws them as atlas quads first; what it
borrows from here, for a `text3d` that takes outlines, is the path from a
shaped run to a mesh. cosmic-text shapes the run, `swash` yields each
glyph's outline, the curves flatten at a tolerance in world units, and the
outline becomes a polygon with holes. The ear clipper takes one ring, so
triangulation with holes is `i_triangle`, the sibling of the `i_overlay`
core already depends on, which scales to integers as it does. The caps are
the 2D mesh; step 3's extrude closes the sides. The cache is by `(font,
size, text)`.

**A path is an asset; extrude, lathe and sweep are `mesh` kinds over it.**
`path2d` and `path3d` are lists of cubic control points with a `closed`
flag, inline or under `paths/`. `shape2d = { kind = "polyline", mesh =
"#outline", width = 0.05 }` strokes one through the polyline strip, the
`mesh` key taking a path as readily as a mesh; `[[assets]] type = "mesh"`
with `kind = "extrude"`, `path = "#outline"`, `depth`, `bevel` and
`segments` fills and extrudes; `lathe` revolves a `path2d` profile around y;
`sweep` runs a profile along a `path3d`. Sampling is a core function on
`libm`, so a path's mesh is the same everywhere. The pen tool that edits one
is `docs/PLAN-editor-ergonomics.md`.

> Written as `shape3d` kinds when this was planned. They are `mesh` kinds
> instead because `Shape` is `Copy` and an asset reference is not, which is
> the same reason `Shape::Mesh` already keeps its reference beside the enum.
> A path-built solid is therefore a `mesh` asset a node draws, which is what
> the torus in `examples/hello` does too.

**A boolean is a component on the parent, and its operands are its
children.** `boolean3d = { op = "difference" }` on a node takes the meshes of
its children in order, computes the result into a private `mesh` asset, and
draws that in place of them; the children stay in the tree, hidden, editable,
and moving one recomputes. `intersection` is `geometry3d.intersect`, which
parry provides; union and difference are not in parry. `csgrs` — pure Rust,
BSP-based, `f32` — is the candidate, with a small in-tree BSP as the fallback
if its arithmetic cannot be pinned to `libm`. `boolean2d` over `geometry2d`'s
three operations is nearly free and comes first as the proof of the shape. A
"bake" command in the editor writes the result out as a plain `mesh` under
`models/`.

**A cloner draws copies, not nodes.** `cloner = { mode = "grid", counts =
[5, 1, 5], step = [1.2, 0, 1.2] }` — also `linear` with `count` and `step`,
and `radial` with `count`, `radius` and `angle` — takes the node's child
subtree as the template and draws it that many times through the fork's
`set_instances`, one draw call per mesh. The seam it opens is the one
`docs/PLAN-views-and-culling.md` step 3 fills with automatic instancing and
step 8 with `multimesh`, the cloner's scripted twin. Physics, scripts and
the tree see one node; `seed` scatters position, rotation and scale within a `random`
range. "Bake to nodes" in the editor makes real children when a game needs
to touch one. 2D clones instance sprites the same way.

**The instanced draw path is one seam.** The fork's `set_instances` is
reached once, in `shader_material_3d.rs`, and three things draw through it:
the cloner here, `docs/PLAN-views-and-culling.md`'s automatic instancing and
`multimesh`, and `docs/PLAN-particles.md`'s billboards. Per-instance data is
a model matrix and a colour; a material sees an instance index and nothing
else, so a fire shader is a material and not a feature. The emitter, its
shapes and the `particles2d` rename are `docs/PLAN-particles.md`'s.

**`MeshData` grows colours and morphs.** `colors: Option<Vec<[f32; 4]>>` and
`morphs: Vec<MorphTarget { name, positions, normals }>`; `glb.rs` keeps
both; the fork's deform path draws them. A `vertex_color` feature in the
material contract multiplies the albedo; morph weights are
`mesh/morph.<name>` properties, so a clip animates a smile with the tracks
it already has.

**2D shapes complete the canvas.** `shape2d` gains `ellipse`
(`half_extents`), `star` (`points`, `inner_radius`, `radius`) and `ngon`
(`sides`, `radius`), and `rect` gains `corner_radius`. Each is one mesher in
core.

## 2. The surface

| Piece | Decision |
| --- | --- |
| Torus, pyramid, prism, tube, rounded cuboid, segmented plane | Step 1, core meshers |
| Sphere, cuboid, capsule, cylinder, cone, quad, circle, hemisphere in the fork's `procedural` | Not used: the same shapes move into core so headless and drawn geometry agree |
| Glyph outlines to a mesh, for `docs/PLAN-text.md`'s `text3d` when it takes outlines; that plan tries atlas quads first | Step 2, `swash` + `i_triangle` |
| `path2d`, `path3d`, stroke, extrude, lathe, sweep | Step 3, sampling in core. Extrude, lathe and sweep are `mesh` kinds rather than `shape3d` ones; the fork's `bezier_curve` and `polyline_path` pipe are not used |
| `boolean2d` | Step 4, `geometry2d` |
| `boolean3d` intersection | Step 4, `geometry3d.intersect` (parry) |
| `boolean3d` union and difference | Step 4, `csgrs`; fallback an in-tree BSP |
| `cloner`: linear, radial, grid, seed | Step 5, fork `set_instances` |
| Cloner along a path or over a surface | Not planned until a scene asks; the path kind is the natural third mode |
| The instanced draw path | Step 6, over the fork's `set_instances`; `docs/PLAN-particles.md`'s billboards and `docs/PLAN-views-and-culling.md`'s `multimesh` draw through it. The fork's `PointRenderer3d` is not used: no texture or size per point |
| `particles3d`, its emitter shapes, and what both emitters lack | `docs/PLAN-particles.md` |
| GPU-simulated particles | Not planned (`docs/PLAN-shaders.md` question 6) |
| Vertex colours, morph targets | Step 7 |
| Ellipse, star, ngon, rounded rect | Step 1, beside the 3D primitives |
| Subdivision surfaces, mesh editing, sculpting, bevel on arbitrary meshes | Not planned. They are a modeller, and a game engine that grows one regrets it; a `.glb` from Blender is the answer |
| Hair, cloth | Not planned; cloth is `docs/PLAN-physics.md` |

## 3. Steps

1. **Primitives.** The mesher module in core, the six existing kinds moved
   onto it, the new kinds in 2D and 3D, `segments` and `corner_radius`. Ends
   with: a torus in `examples/hello` with a collider fitted from the same
   mesh.
2. **Glyph outlines.** The outline-to-mesh path and its cache. Ends with: a
   word as a mesh with a collider fitted from it.
3. **Paths.** The two assets, stroke, extrude, lathe, sweep.
4. **Booleans.** `boolean2d`, then `boolean3d`, then the bake command.
5. **Cloner.** With bake.
6. **The instanced draw.** One seam in `shader_material_3d.rs`, the cloner
   moved onto it. Ends with: a thousand clones as one draw.
7. **Colours and morphs.** `MeshData`, `glb.rs`, the material feature, the
   animation property.

## 4. What CI can prove, and what it cannot

- Every mesher is a pure function: vertex counts, closedness (each edge
  shared by two triangles), winding and bounds are unit tests with no GPU.
- A text mesh for a fixed font and string has a fixed triangle count and
  digest on every platform; that is the determinism test for `i_triangle`.
- A boolean's result volume against the analytic answer for two cubes.
- Golden frames for each example through `scripts/showcase.sh`.
- What it cannot: how a bevel reads at a glance, and font licensing for
  anything shipped in the library.

## 5. Open questions

1. **`i_triangle`'s arithmetic.** It scales to integers, which is how it
   stays exact; whether its scaling is identical across platforms needs one
   test before it is trusted.
2. **The default font** is `docs/PLAN-text.md`'s question; the outline path
   reads whichever it picks.
3. **Booleans on moving children.** Recomputing per tick for an animated
   operand is the naive rule; a `static` flag that computes once is the
   likely answer.
4. **A cloner's clones in physics.** One collider per clone is the honest
   choice when a scene asks; until then the cloner is visual, like particles.
