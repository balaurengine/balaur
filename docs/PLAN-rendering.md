> **Status:** the plan this file opened with is built and its text is gone.
> 2D lights and shadows (`light2d`, `occluder2d`, `camera.ambient`, the light
> map), GPU skinning in 3D, `camera.post` and the editor's light gizmos all
> shipped on 2026-09-04; the manual's Rendering page documents them. What is
> left are the two the plan deferred, kept here because neither is started.

# Plan: what 2D lighting still does not do

## Tile-map occluders

A tile set could mark tiles as occluders, and the occluder polygon would be
built from the map rather than authored per node. Today a tile map is lit
like everything else but casts no shadow unless a node over it carries an
`occluder2d` — which is a poor fit for a wall drawn as fifty tiles.

The shape of it: a per-tile `occludes` flag in the `tileset` asset, and a
pass that walks the `tilemap`'s grid and emits one outline per connected
run of occluding tiles. Merging the runs is what makes it worth doing —
fifty tile-sized squares would be fifty times the shadow polygons of one
merged outline, and the light map already costs a pass per shadow-casting
light.

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
