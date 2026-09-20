> **Status:** the plan this file opened with shipped on 2026-09-04 and its
> text is gone; the manual's Rendering page documents what it built. Of the
> two it deferred, tile-map occluders moved to `docs/PLAN-tilemap.md` on
> 2026-09-05, where the 2026-09-06 rewrite numbers them step 4; the one left is below, not started.

# Plan: what 2D lighting still does not do

## Tile-map occluders

Moved to `docs/PLAN-tilemap.md`: a per-tile `occluder` in the tileset, and
one merged outline per run of occluding tiles, beside the collision compound
that merges the same runs.

## Normal maps on sprites

Lit sprites with normal maps are one more texture on `sprite`: the light map
would have to carry a direction as well as a colour, so a sprite could be
shaded per pixel rather than multiplied flat.

That is a real change to the pass, not a property. The current light map is
one RGB target and the composite is a multiply; per-pixel normals need the
light direction per fragment, which means either a second target or moving
the lighting back into each object's material — the thing the full-screen
multiply was chosen to avoid. Worth it when a game asks, and worth
re-reading `balaur_render::light_map`'s trade-off first.

## What a frame costs the GPU

`--timings` names every pass the fork times, so a frame says where the GPU
spent itself rather than only how much. On an Apple M1, 200 frames offscreen,
each project at its own size:

| | render gpu | of which |
| --- | ---: | --- |
| `examples/hello`, 1600x1000 | 1.87 ms | tonemap 0.89, opaque 0.73, shadows 0.25 |
| `examples/rig3d`, 1600x1000 | 3.16 ms | tonemap 1.14, opaque 0.96, shadows 1.06 |
| `examples/concave`, 1920x1080 | 1.15 ms | tonemap 0.83, 2d 0.17, opaque 0.15 |
| the editor, 1920x1080 | 6.92 ms | tonemap 2.71, opaque 2.03, 2d 1.92 |

The floor is bandwidth. The film is RGBA16F, eight bytes a pixel and 16.6 MB
at 1920x1080, and a frame writes it, loads and stores it again for the 2D
pass, then reads it to tonemap. That is about 65 MB a frame, which is a
millisecond of traffic at the M1's peak and two to four in practice.

**Built 2026-09-20.** The 2D pass is skipped where the 2D scene has no
children. It is a full-screen load and store of a film the 3D scene has
already written, so on a 3D-only project it was pure traffic: `rig3d` went
from 5.35 ms to 3.02. The 3D pass over an empty scene is not the same cost,
because the film it loads was just cleared: `concave` pays 0.15 ms for it.

What is left, in the order the measurements rank it:

- **The film's format.** R11G11B10F is half the bytes of RGBA16F and is what
  a colour film with no alpha wants. Every pass above is bandwidth, so this
  halves the floor.
- **Post as one pass.** Tonemap, grading and the composite are separate
  passes, each loading and storing the film.
- **The editor's viewport.** Its tonemap is 2.71 ms where a game's at the same
  size is 0.83. Several passes share that timer scope, so the first job is
  finding which of them the editor adds.
- **A render scale.** Drawing the 3D film at a fraction of the window and
  upscaling is the setting every engine with a mobile target carries.
- **No subpasses in wgpu.** A tiled GPU could keep the film in tile memory
  across these passes; WebGPU has no way to say so, which is why the round
  trips above are paid at all. Nothing to do here but know it.

Turning things off works and is measurable: `shadows = false` on
`environment3d` takes `examples/hello` from 2.68 ms to 2.13, and bloom and
auto-exposure, both off by default, are no longer even compiled.
