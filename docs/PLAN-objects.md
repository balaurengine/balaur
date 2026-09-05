> **Status:** not started. Written down on 2026-09-05 with
> `docs/PLAN-3d-rendering.md`, from the same comparison against Spline. The
> order is what a designer reaches for first: the primitives a palette
> starts with, then text, because every demo has a word in it, then paths
> and what they extrude into, then the compound objects — booleans, cloners
> — and last 3D particles, which are their own renderer. Everything builds a
> `mesh` asset headless, so colliders, picking and tests share the triangles
> the screen draws.

# Plan: objects

Parametric primitives, text in 2D and 3D, bezier paths with extrude, lathe
and sweep, a boolean node, a cloner, 3D particles, vertex colours and morph
targets, and the 2D vector shapes a canvas expects. What draws them is
`docs/PLAN-3d-rendering.md`; what the editor does with them is
`docs/PLAN-editor-ergonomics.md`.

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
- **Text in the world.** A widget draws text on the screen layer only;
  nothing draws a word at a node's pose in 2D, and nothing extrudes one in
  3D.
- **Paths.** No bezier asset, no editable path, no extrude, lathe or sweep.
  The fork's `bezier_curve` lives in a crate a headless build does not link.
- **A boolean node.** `geometry3d.intersect` is a script call that returns a
  mesh; nothing in a scene says "this minus that".
- **A cloner.** The renderer draws every node it is handed; the fork's
  instancing is never used.
- **3D particles.** `particles` is 2D, in logical pixels, on the 2D layer.
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

**Text is a mesh from glyph outlines, shaped by the engine widgets use.**
`text2d` and `text3d` (D5: both marked, since both exist) take `text`,
`font` (a file under `fonts/`), `size`, `align`, `line_height` and
`letter_spacing`; `text3d` adds `depth` and `bevel`. cosmic-text shapes the
run, `swash` yields each glyph's outline, the curves flatten at a tolerance
in world units, and the outline becomes a polygon with holes. The ear clipper
takes one ring, so triangulation with holes is `i_triangle`, the sibling of
the `i_overlay` core already depends on, which scales to integers as it
does. `text2d` is a filled mesh on the 2D layer, tinted and textured like a
polygon; `text3d` extrudes the caps and closes the sides. Editing the string
rebuilds the mesh; the cache is by `(font, size, text)`.

**A path is an asset; extrude, lathe and sweep are `shape3d` kinds over it.**
`path2d` and `path3d` are lists of cubic segments with a `closed` flag,
inline or under `paths/`. `shape2d = { kind = "path", path = "#outline",
width = 0.05 }` strokes one through the polyline strip; `shape3d = { kind =
"extrude", path = "#outline", depth = 0.5, bevel = 0.05, segments = 4 }`
fills and extrudes; `lathe` revolves a `path2d` profile around y; `sweep`
runs a profile along a `path3d`. Sampling is a core function on `libm`, so a
path's mesh is the same everywhere. The pen tool that edits one is
`docs/PLAN-editor-ergonomics.md`.

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
`set_instances`, one draw call per mesh. Physics, scripts and the tree see
one node; `seed` scatters position, rotation and scale within a `random`
range. "Bake to nodes" in the editor makes real children when a game needs
to touch one. 2D clones instance sprites the same way.

**3D particles are a renderer, not a system.** `particles3d` mirrors
`particles`: rate, lifetime, speed, spread, gravity as a vec3, size in world
units, `color_end`, `size_end`, `texture`, `one_shot`, plus an emitter
`shape` (point, sphere, box, cone) and `lit`. Drawn as instanced billboards
through the `set_instances` path the cloner uses, so there is one instancing
seam. Observer-only, as the 2D one is: the live particles never enter the
simulation. `particles` becomes `particles2d`, with the old key accepted for
one release (NAMING D5, Table C).

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
| `text2d`, `text3d` | Step 2, the components `docs/PLAN-text.md` specifies; that plan tries atlas quads from the widget layer's shaper first and outlines through `i_triangle` second |
| `path2d`, `path3d`, stroke, extrude, lathe, sweep | Step 3, sampling in core. The fork's `bezier_curve` and `polyline_path` pipe are not used |
| `boolean2d` | Step 4, `geometry2d` |
| `boolean3d` intersection | Step 4, `geometry3d.intersect` (parry) |
| `boolean3d` union and difference | Step 4, `csgrs`; fallback an in-tree BSP |
| `cloner`: linear, radial, grid, seed | Step 5, fork `set_instances` |
| Cloner along a path or over a surface | Not planned until a scene asks; the path kind is the natural third mode |
| `particles3d` | Step 6, instanced billboards. The fork's `PointRenderer3d` is not used: no texture or size per point. The emitter's surface — shapes, randomness, attractors, colliders, trails, sub-emitters — is `docs/PLAN-particles.md`; this step is the draw path |
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
2. **Text.** `text2d`, `text3d`, the glyph cache. Ends with: the hello
   example saying hello.
3. **Paths.** The two assets, stroke, extrude, lathe, sweep.
4. **Booleans.** `boolean2d`, then `boolean3d`, then the bake command.
5. **Cloner.** With bake.
6. **3D particles.** With the `particles2d` rename.
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
2. **Where the built-in font lives.** Widgets have theme fonts; a `text3d`
   with no `font` needs one, and the fork ships Work Sans. One default under
   `editor/fonts` reused, or one shipped in the runtime, is a size question
   for `docs/PLAN-embed.md`.
3. **Booleans on moving children.** Recomputing per tick for an animated
   operand is the naive rule; a `static` flag that computes once is the
   likely answer.
4. **A cloner's clones in physics.** One collider per clone is the honest
   choice when a scene asks; until then the cloner is visual, like particles.
