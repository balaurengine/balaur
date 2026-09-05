> **Status:** not started, but for the Tiles tool, which shipped the same
> day (`docs/PLAN-editor.md` §6). Written 2026-09-05 from the Godot parity
> investigation. It reverses a decision: `docs/PLAN-2d-games.md` marked
> autotile, terrains and tile collision *not planned*; all three are planned
> here, and the tile occluders `docs/PLAN-rendering.md` deferred move here
> too.

# Plan: tile maps — collision, terrains, animated and occluding tiles, and the Tiles tool

## 0. Where the tree is today

- A `tileset` asset is an image, a square `tile_size` and `columns`
  (`crates/balaur_render/src/tilemap.rs:17`). Nothing per tile.
- A `tilemap` is a `cells` string or a list of rows of ids, a `tileset`, a
  `material` and `pixels_per_unit`; `set_cell` and `cell` on the handle
  rebuild the mesh next frame. A layer is a sibling node with a `z_index`.
- Nothing collides with a tile: a level draws its walls with `tilemap` and
  draws them again with `collider2d` rectangles by hand
  (`examples/angrynerds`).
- 2D lights and `occluder2d` are built; a wall of fifty tiles casts no shadow.
- `collider2d` takes `compound`-free shapes: cuboid, ball, capsule, polyline,
  trimesh, convex hull from a `mesh` asset; `geometry2d.union` and the
  ear-clipper are in core.
- The Tiles tool (`editor/scripts/tiles.rn`, built 2026-09-05): a palette
  dock cut from the tileset through `ui.image_button` and `region` on
  `ui.image`, left-drag paints, right-drag erases, a Rectangle mode, a layer
  as a sibling map from an Add layer button, every stroke through `history`.
  No autotile, no bucket, line, eyedropper or stamp, and a map grows right
  and down only because it is centred on its node.
- `balaur import file.aseprite` writes an atlas, a `sprite_sheet` with
  frames, tags and slices, and a clip per tag (`docs/PLAN-scenes-and-assets.md`).
- The polygon tool (`editor/scripts/polygon.rn`) draws points and loops over
  a texture; the Assets dock lists files.

## 1. Design

**The tileset carries what a tile is.** `[tiles.<id>]` tables hold a tile's
collision, occlusion, animation, terrain and custom data; a tile without
one is the plain quad it is today. The grid gains `tile_size = [w, h]`,
`spacing` and `margin`, so a sheet exported from any tool cuts right.

**Collision is one static compound the map owns.** At sync the map builds a
rapier `compound` of the solid cells — runs of full tiles merged into one
cuboid per run, row-major, so the order is fixed — and attaches it as a
collider on the node's own `body2d` when it has one, else as static
geometry. `set_cell` marks the compound dirty and it is rebuilt once on the
next fixed step, whatever the number of writes that frame. A map that wants
an outline instead (`collision = "outline"`) unions its solid cells through
`geometry2d.union` into polylines, which is what stops a character catching
on the seam between two cuboids.

**Autotile is a pure function.** A terrain is a name; a tile in it says which
of its eight neighbours it expects to be the same terrain, as a bitmask; the
painter picks the tile whose mask matches the cell's neighbourhood, and
`tilemap.set_terrain(x, y, name)` in a script does the same, so a
procedural level autotiles without the editor. The resolver lives beside the
parser with no backend behind it, which is what lets a headless test assert
a corner.

**Animated tiles are render time.** Frames cycle on the frame clock like the
`wave` markup does, outside the digest; a tile that changes what the game
does is a cell a script writes.

**Layouts are one function.** Cell to world and world to cell are computed in
one place the mesh, the collider, picking and the painter all call, so an
isometric or hexagonal map is a `layout` on the tileset and nothing else.

## 2. The surface

| Need | Decision |
| --- | --- |
| Collision from tiles | Step 1: `[tiles.<id>] collision = "full"` or a list of polygons in tile pixels; `collision = "compound" \| "outline" \| "none"`, `layers`, `mask`, `friction`, `restitution`, `one_way` on the map |
| Non-square tiles, spacing, margin | Step 1: `tile_size = [w, h]`, `spacing`, `margin` on the tileset |
| Tiles casting 2D shadows | Step 2: `[tiles.<id>] occluder = true` or a polygon; the map contributes one merged outline per run to the light map, the item `docs/PLAN-rendering.md` deferred |
| Animated tiles | Step 2: `[tiles.<id>] animation = { frames = [ids], fps = 8 }` |
| Custom data on a tile | Step 2: `[tiles.<id>.data]`, any table; `tilemap.tile_data(id)` and `tilemap.data_at(x, y)` |
| Terrains and autotile | Step 3: `[[terrains]]` on the tileset with `mode = "sides" \| "corners" \| "corners_and_sides"`, a `terrain` and `peering` per tile; `set_terrain` on the handle; `probability` for a random pick among matches |
| Flipped and rotated cells | Step 3: the list form of `cells` takes `[id, flags]` for a cell that flips or transposes; the string form stays for plain maps |
| Layers | Have: sibling `tilemap` nodes; step 1 adds `color` and `visible` like every renderable |
| Y-sorted tiles | **Not planned**, with `docs/PLAN-2d-games.md`'s reasoning: `z_index` from a script |
| Large maps | Step 4: the mesh and the compound are chunked in 32 × 32 cells so `set_cell` rebuilds one chunk; culling of chunks outside the camera is `docs/PLAN-views-and-culling.md` |
| Painting in the editor | Have: the Tiles tool with paint, erase, rectangle and layers. Step 5 adds bucket, line, eyedropper, a multi-tile stamp with flip and rotate, a terrain brush, and a map that grows left and up by shifting its cells rather than refusing |
| Editing a tileset | Step 6: per-tile collision polygons drawn over the tile with the polygon tool's point mode, terrains painted per tile, animation frames picked in order |
| Isometric and hexagonal maps | Step 7: `layout = "orthogonal" \| "isometric" \| "hex"`, `hex_side` |
| Tiled and LDtk projects | Step 8: `balaur import level.tmx` and `level.ldtk` through the `tiled` and `ldtk` crates: tilesets, layers as nodes, objects as nodes with a `data` table; the source stays the tool's file |
| Navigation over tiles | `docs/PLAN-navigation.md` step 7 bakes a grid from `collision` |

## 3. Steps

1. Tile metadata, the collision compound and the outline, the grid keys.
   `examples/angrynerds` loses its hand-placed wall colliders.
2. Occluding tiles, animated tiles, custom data.
3. Terrains, `set_terrain`, cell flags.
4. Chunking.
5. The Tiles tool's remaining brushes and growth in every direction, each
   asserted by `tilesdemo`.
6. The tileset editor.
7. Isometric and hexagonal layouts.
8. Tiled and LDtk import.

## 4. What CI can prove, and what it cannot

Headless proves a ray hits a wall cell, that the compound has the merged
run count a fixture predicts, that `set_terrain` picks the corner tile, and
that a map's digest is the same on every OS. `tilesdemo` already paints a
rectangle and asserts the `cells` string; each new brush adds a stroke to
it. CI cannot prove a brush feels right; the showcase clip is where a person
checks.

## 5. Open questions

1. **The compound on a moving map.** A map on a kinematic `body2d` is a
   moving platform of tiles; rebuilding its compound each `set_cell` while
   it moves is fine, but a tile map on a dynamic body wants a mass, and
   nothing says what a tile weighs. Static and kinematic first.
2. **Where a tile's collision shape is authored.** Tile pixels is what every
   tool exports; world units is what every other collider says. Pixels, with
   the map's `pixels_per_unit` converting, is the proposal.
3. **Chunk size.** Thirty-two is a guess; the benchmark project gets a
   tile-map case before step 4 picks a number.
