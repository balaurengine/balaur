> **Status:** the plan this file opened with shipped on 2026-09-04 and its
> text is gone; the manual's Rendering page documents what it built. Of the
> two it deferred, tile-map occluders moved to `docs/PLAN-tilemap.md` step 2
> on 2026-09-05; the one left is below, not started.

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
