> **Status:** done, 2026-09-04. All five phases: `light2d`, `occluder2d`,
> `camera.ambient` and the light-map pass; the 3D skinning material;
> `camera.post`; the editor's light gizmos and occluder outlines.
> Written 2026-09-02.

# Plan: 2D lights and shadows, GPU skinning in 3D

## Where the tree is today

- 2D draws shapes, sprites, tile maps, particles and skinned polygons
  unlit, through kiss3d's 2D path with one custom WGSL material
  (`balaur_render::skinned_2d`).
- 3D meshes imported from glTF are skinned every frame on the CPU
  (`modify_vertices` / `modify_normals`), which is also what the headless
  test asserts against.
- Rendering is an observer: nothing here touches the digest.

## 2D lights and shadows

Two components, both render-only:

```toml
[nodes.light2d]
kind = "point"          # point | directional
color = "#ffd28a"
radius = 6.0
intensity = 1.2
shadows = true

[nodes.occluder2d]
polygon = [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]]   # default: the node's collider2d shape
```

The pass: lights render additively into a light map at the camera's
resolution; each shadow-casting light extrudes every occluder edge away
from itself into a shadow polygon and subtracts it; sprites, polygons and
tiles multiply by the light map plus an ambient colour on the camera. All
of it is one extra material and one extra target in the backend. A
`camera` gains `ambient` and, later, `post = ["bloom"]` for the effects
that read the same target.

## GPU skinning in 3D

The 2D skinning material already takes a joint palette; the 3D path does
the same in a vertex shader: joints and weights as vertex attributes, the
palette in a uniform buffer, the deformed position and normal computed on
the GPU. `joint_matrices_3d` stays where it is; only the consumer changes.

The CPU path stays as the reference: the headless test keeps asserting the
CPU-deformed vertices, and a windowed test reads back one frame and
compares within a tolerance, so the GPU path cannot drift from the numbers
a digest would see.

## What shipped

Both 2D phases, in one pass in the kiss3d backend (`balaur_render::light`
resolves the scene, `light_map` rasterises it). Two departures from the
sketch above:

- The multiply is one full-screen draw added last to the 2D scene, not a
  material on every sprite. It reaches tiles, polygons and anything else
  already drawn, and a scene with no `light2d` adds no draw at all — an
  unlit frame is byte-for-byte the frame it was before.
- A shadow-casting light gets a render pass of its own with a stencil,
  rather than subtracting its shadow polygons from the accumulated map.
  Subtracting eats the other lights wherever two occluders overlap.

`occluder2d` takes its outline from an authored `mesh`, else from the node's
`collider2d`, else from its circle, capsule, rect or sprite shape.

The 3D skinning material (`balaur_render::skinned_3d`) draws against the same
`package::mesh` contract a project's material does, so a skinned mesh is lit
and fogged like every other one. It is the built-in look only: a node naming a
`material` asset, or a frame under a channel view, keeps the CPU path, because
those shaders know nothing about a joint palette. `blend` in the shader is
`@const`, so a headless test evaluates it against `skeleton::blend_3d` — the
GPU path is held to the numbers a digest would see without needing a GPU.

`camera` gained `post`, a flags list over the screen-space effects kiss3d's
window can switch — `bloom`, `ssao`, `ssr`, `dof` — plus `bloom_threshold`
and `bloom_intensity`. Unlike `ambient` it is not per-dimension: the effects
run over the whole film, so the last current camera of either kind sets them.

The editor draws a `light2d`'s reach and aim and an `occluder2d`'s resolved
outline behind a Lights chip, and the Polygon tool traces an `occluder2d`'s
mesh as well as a `polygon`'s — in Points mode only, an outline having no
triangles, UVs or weights.

## Phases

1. ✅ `light2d` without shadows and `ambient` on the camera; the light map
   pass.
2. ✅ `occluder2d` and shadow polygons; default occluders from `collider2d`.
3. ✅ The 3D skinning material; the CPU path kept for headless and as the
   reference in tests.
4. ✅ Post-processing on the camera: bloom first, since the light map already
   gives it what it needs.
5. ✅ Editor: light gizmos, an occluder editor on the Polygon tool.

## Open questions

1. **Tile-map occluders.** A tile set could mark tiles as occluders; the
   occluder polygon is then built from the map. Second cut.
2. **Normal maps on sprites.** Lit sprites with normal maps are one more
   texture on `sprite`; worth it when a game asks.
