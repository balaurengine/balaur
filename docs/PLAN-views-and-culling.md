> **Status:** not started. Written 2026-09-05 from the Godot parity
> investigation: the renderer draws every node it is handed, once, from one
> camera, into the window.

# Plan: more than one view, culling, level of detail and instancing

## 0. Where the tree is today

- One 2D and one 3D view, each from the last current `camera`; the backend
  draws the whole scene graph into the window every frame, and offscreen
  into a hidden surface.
- kiss3d's fork has `RenderTarget`s for its post chain and the offscreen
  output, and no way to render the graph a second time with another camera
  or into a texture a material can read.
- `Renderable` and `GlobalTransform` are where a visibility pass would sit;
  `visible` propagates in `SceneSync`; nothing computes bounds.
- The 3D material pipelines are Balaur's own (`shader_material_3d.rs`);
  every mesh is one draw. The fork's `set_instances` on a node is unused.
- The 3D camera is the fork's orbit camera built with its default frustum:
  `camera` has no `projection`, `fov`, `near` or `far`, though every fork
  camera takes them through `new_with_frustum`.
- `docs/PLAN-3d-rendering.md` step 1 gives `light3d` and every renderable a
  `layers` mask; nothing masks a camera.
- The offscreen path already renders with no OS window at a fixed step.

## 1. Design

**A view is a node.** `viewport` is a component: a size, a rect of the window
or a texture to draw into, the camera it draws from, a layer mask, its own
clear colour and MSAA, and how often it updates. The window itself is the
default view, so a scene with none draws as today. A view that draws to a
texture is referenced as `"view:<node path>"` wherever a texture string is
accepted — a `sprite`, an `image` widget, a material's `[textures]` — which
is a minimap, a mirror and a security camera in one rule.

**The fork renders the graph; the engine says how many times.** One hook in
kiss3d: render the scene graph with this camera, this mask, into this target.
Everything above it — which views, in what order, what depends on what — is
`balaur_render`'s. A view drawing a texture another view samples renders
first; a cycle is refused with a message.

**Culling is a pure function the simulation may ask.** Bounds are computed
per renderable at attach — a shape's extent, a mesh asset's box, a sprite's
quad — and `render.in_view(node)` answers whether the current camera's
frustum meets them, computed from the `camera` component and
`GlobalTransform` alone, so a headless run answers exactly as a windowed
one. The backend uses the same answer to skip a draw. What makes that safe
to expose is that it reads nothing the backend produced; a script that
writes the answer into state has put a camera in the digest, which the
reference says on the function.

**Instancing is automatic.** Renderables sharing a mesh asset and a material
draw as one call with per-instance model matrices in a storage buffer,
grouped in the material pipeline the engine already owns. A script wanting a
hundred thousand instances gets `multimesh`: the mesh, a count, and
`set_instance(i, pose, color)` on the handle, which is Godot's
`MultiMeshInstance` without the resource.

**Level of detail is the mesh asset's.** `lods = [{ source, distance }]` in
the definition, so an imported model carries its own chain, and `balaur
import` generates one through `meshopt` when asked. A renderable's
`range = [near, far]` and `range_fade` hide it past a distance whatever its
mesh.

## 2. The surface

| Need | Decision |
| --- | --- |
| Perspective and orthographic cameras, field of view, clip planes | Step 1: `camera.projection`, `fov`, `near`, `far`, `size`, applied through the fork's `new_with_frustum`; `render.set_camera` keeps taking eye and target |
| Skipping nodes outside the camera | Step 1: frustum culling in 3D, rect culling in 2D, from bounds; `render.in_view`, `on_view_entered` / `on_view_exited` |
| Visibility layers | Step 2: `cull_mask` on `camera` and `viewport`, over the `layers` `docs/PLAN-3d-rendering.md` step 1 puts on lights and renderables |
| Repeated meshes in one call | Step 3: automatic instancing in `shader_material_3d.rs`, over the fork's `set_instances` — the seam `docs/PLAN-objects.md` step 5 opens for its authored `cloner`; this is the same seam applied to whatever the scene repeats |
| Anti-aliasing | Step 4: `viewport.msaa`, and `[render] msaa` in `project.toml` for the window's own view; FXAA and sharpening are `docs/PLAN-3d-rendering.md` step 5 |
| Split screen | Step 4: `viewport` with `rect`, `camera`, `cull_mask`, `clear`, `msaa`, `update = "always" \| "once" \| "visible"`; input per view through `render.mouse_ray(view)` |
| A camera on a texture | Step 5: `viewport.target = "texture"`, `size`, referenced as `view:<path>` |
| Picture-in-picture | Step 5: a `viewport` on an `image` widget |
| Level of detail | Step 6: `lods` on the mesh asset, `lod_bias` on `mesh`, `range` and `range_fade` on renderables, `balaur import --lods` through `meshopt` (C bindings, the constraint) |
| Many sprites in one call | Step 7: 2D batching by texture and material in the sync |
| Scripted mass instancing | Step 8: `multimesh`, the scripted twin of `docs/PLAN-objects.md`'s `cloner`: a count and `set_instance(i, pose, color)` where the cloner has a mode and a seed |
| Occlusion culling | **Not planned** until a scene asks; frustum and distance first, and a software depth rasteriser is its own plan |
| Render scale and sharpening | Step 4: `viewport.scale`, `sharpen` through the fork's CAS pass |
| Stereo views for XR | The roadmap's XR item; step 4's hook is the half it reuses |

## 3. Steps

1. Camera projection; bounds, frustum culling, `render.in_view`.
2. Layers and cull masks.
3. Automatic instancing.
4. `viewport` on a window rect, and the fork hook.
5. `viewport` to a texture.
6. Level of detail and ranges.
7. 2D batching.
8. `multimesh`.

## 4. What CI can prove, and what it cannot

The headless-versus-offscreen digest job proves culling and instancing
touch nothing; a headless test asserts `in_view` against a hand-computed
frustum. Offscreen renders a split screen and a minimap into the showcase.
The benchmark project gains a case of ten thousand crates, so instancing is
a number in `docs/BENCHMARKS.md` rather than a claim. CI cannot prove a
view order deadlock is impossible; the cycle check is a test.

## 5. Open questions

1. **Who owns the frame loop.** Every step above fits under kiss3d's window
   with one hook; XR does not, and neither would a view per monitor. Whether
   `balaur_render` takes the loop from the fork is a decision this plan
   defers and the XR item cannot.
2. **A view's texture as an asset.** `view:<path>` is a string that names a
   node, not a file; it cannot be inlined or saved. That is right for a live
   image and wrong the day someone wants to bake one, which `render.screenshot`
   already covers.
