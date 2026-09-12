> **Status:** every step shipped; steps 1-6, 8 and 9 on 2026-09-06 and step
> 7 on 2026-09-07. What a tile is,
> collision from the solid cells as one parry voxel shape, a map anchored on
> its node with an origin and per-cell flags, the rule table with its
> templates, animated and light-blocking tiles, per-tile data, the Tiles
> tool's fill, line, pick, stamp and terrain brushes, a Set panel that writes
> the tile set, isometric and hexagonal layouts, cells a level may keep in its
> own file, `balaur import` for Tiled and LDtk, and quarter-tile sheets, which
> draw four sub-quads per cell from the five tiles an RPG-Maker-A2 sheet
> ships. What is left is the open questions in §8. The
> tileset editor is a panel in the Tiles dock rather than a document tab, and
> D24 is fixed: the mirror inlines an asset file's definition, so a tileset
> kept in a file draws in the editor.

# Plan: tile maps — collision, rules and terrains, animated and occluding tiles, and the two tile editors

## 0. Where the tree is today

- A `tileset` asset is an image, a square `tile_size` and `columns`
  (`crates/balaur_render/src/tilemap.rs:17`). Rows come off the image height.
  Nothing per tile.
- A `tilemap` is a `cells` string or a list of rows of ids, a `tileset`, a
  `material` and `pixels_per_unit`; `set_cell` and `cell` on the handle
  rebuild the mesh next frame. A layer is a sibling node with a `z_index`.
- Nothing collides with a tile: a level draws its walls with `tilemap` and
  draws them again with `collider2d` rectangles by hand
  (`examples/angrynerds`).
- 2D lights and `occluder2d` are built; a wall of fifty tiles casts no shadow.
- The Tiles tool (`editor/scripts/tiles.rn`): a palette dock cut from the
  tileset, left-drag paints, right-drag erases, a Rectangle mode, a layer as a
  sibling map, every stroke through `history`. No autotile, no bucket, line,
  eyedropper or stamp.
- `balaur import file.aseprite` writes an atlas, a `sprite_sheet` and a clip
  per tag; `.glb` writes a scene. A `.tmx` or `.ldtk` writes a tileset per
  sheet and a scene per level, rooted at that level.

Three constraints the code imposes on everything below:

1. **`balaur_physics` and `balaur_render` do not see each other**; both
   depend on core. Tile collision cannot be "the render crate builds a
   rapier compound".
2. **kiss3d's `Tilemap` node takes a uniform `SpriteSheet::new(columns,
   rows)`** and one `u32` per cell. Spacing, margin, non-square tiles,
   per-cell flip, animation and iso/hex placement are all outside what it
   can say.
3. **The editor deep-copies the whole document per undo step**
   (`editor/scripts/history.rn:12`), and a stroke rewrites the whole `cells`
   value and rebuilds the whole mesh. A 200 × 200 map is 40 000 ids. Chunking
   is what makes the tool usable past a toy, not a later optimisation.

## 1. Design

**Core owns the data.** `balaur_core::tiles` holds the tileset parse, the
grid, the layout functions (cell to world and back, for every layout), the
occluder outline union — core already has `geometry2d` — and the rule
resolver. Backend-free, so a headless test asserts a merged run count or a
resolved corner with no window and no rapier. `balaur_render` registers the
`tileset` asset over core's parser and builds the mesh; `balaur_physics`
reads the same data through a component of its own.

**The tileset carries what a tile is.** `[tiles.<id>]` tables hold a tile's
collision, occlusion, animation, terrain and custom data; a tile without one
is the plain quad it is today. The grid gains `tile_size = [w, h]`, `spacing`
and `margin`, so a sheet exported from any tool cuts right.

**The map stores what you painted.** A map with terrains keeps an optional
`terrain` grid of small integers beside `cells`; the painter writes the
terrain grid and the tiles are resolved from it and cached into `cells`. A
map with no terrains is exactly what it is today, and the runtime pays
nothing.

**Rules are one ordered table, first match wins.** Bitmask terrains are sugar
that compiles into the same table, so there is one resolver (§2).

**Collision is a component physics owns, over parry's voxel shape.** A full
tile is not a cuboid to merge with its neighbours. parry 0.30 has a `Voxels`
shape in *both* dimensions, and `ColliderBuilder::voxels(size, &cells)` takes
the filled cells directly. It classifies every cell as interior, face, edge
or vertex from its neighbours and generates contacts only against exposed
features — the internal-edge problem, *ghost collisions*, solved at the
source, so a body sliding along a floor of tiles cannot catch on the seam
between two of them. The shape is sparse and signed, so negative cells cost
nothing; it chunks itself at 16 × 16; `set_voxel` edits one cell of a live
collider through `SharedShape::make_mut`; and contact manifolds, rays, point
queries, shape casts and the character controller all reach it. This is not
speculative: `collider3d` already ships it as `kind = "voxels"` over the core
`voxels` asset (`crates/balaur_physics/src/collider.rs:109`), and 2D gets the
same shape from the same parry version.

So `tile_collision` on the map's node builds one voxel collider per material
group — solid, one-way, icy — from the cells whose tile says
`collision = "full"`, and one ordinary collider per tile that authors a
polygon instead, through the existing `add_collider_at`. It reuses the whole
`collider2d` material schema: friction, restitution, layers, mask, one_way,
sensor. Physics never learns what a render component looks like, the
inspector gets a place to put the knobs, and collision stays opt-in.
`geometry2d.union` stays for the light map's occluder outlines, where the
merge is about shadow edges, not contacts.

**Layouts are one function.** Cell to world and world to cell live in one
place the mesh, the collider, picking and the painter all call, so an
isometric or hexagonal map is a `layout` on the tileset and nothing else.

**Animated tiles are render time.** Frames cycle on the frame clock like the
`wave` markup does, outside the digest; a tile that changes what the game
does is a cell a script writes.

## 2. Rules and terrains

### What the other tools can say

| Tool | What it expresses | Authored how |
| --- | --- | --- |
| Tiled | Wang sets: corner, edge or mixed terrain bits per tile | terrain colours clicked onto tile corners and edges |
| Godot 4 | terrain sets with peering bits, three match modes | peering bits painted per tile |
| LDtk | odd-sized pattern rules over an IntGrid, per-cell must-be / must-not-be, flips, chance, ordered groups, a source layer | a matrix editor per rule |
| Unity | RuleTile: a 3 × 3 neighbourhood of This / Not This / any, rotate and mirror, random output | a rule list in the inspector |

Two are bitmask systems, two are pattern systems. A peering mask *is* a
3 × 3 pattern, so patterns subsume bitmasks; the reverse is not true.

### The model

A rule is a pattern over an odd-sized neighbourhood — 3 × 3 by default, 5 × 5
and 7 × 7 allowed — an output tile or a weighted list, the transforms it may
be matched under, and a chance. Cells match against a **terrain value**, not
against a tile, so the same table serves a painted terrain grid, a map whose
values are derived from the tiles already down, and an imported IntGrid.

```toml
[[terrains]]
name = "grass"
value = 1
mode  = "rules"          # or "sides" | "corners" | "corners_and_sides"

[[rules]]
terrain    = "grass"
pattern    = ["?#?",     # ? any   # this terrain   . anything else, empty included
              "?#.",     # 0-9 exactly that value   x empty   * any non-empty
              "?##"]
tile       = 12
transforms = ["rotate", "mirror"]
chance     = 1.0
outside    = "same"      # what lies past the edge: empty | same | a value
```

A cell the string form cannot spell takes the long form:

```toml
[[rules.cell]]
at  = [0, 1]
not = [2, 3]
```

Four decisions that matter more than they look:

- **`transforms`.** With rotate and mirror, a 47-tile blob is about twelve
  authored rules, not forty-seven. This is what makes a strange sheet
  tolerable to author by hand.
- **`outside`.** Whether the map's edge counts as solid decides whether a
  level has a border of broken corners.
- **Variation is hashed, not rolled.** A weighted alternate is picked from
  `hash(seed, x, y, rule)`, so re-resolving a region gives the same answer,
  two machines give the same answer, and the digest does not move. This is
  the Godot complaint answered directly.
- **Unmatched cells are loud.** A cell no rule matches gets a marker tile and
  a count in the dock, with a jump to it. A rule set is debugged by finding
  the hole.

### Templates

Most sheets are one of six layouts. A template fills a whole rule set from a
first tile id, and the editor proposes one from the tile count:

| Template | Tiles | Where it comes from |
| --- | --- | --- |
| `single` | 1 | anything |
| `minimal9` | 9 | the 3 × 3 nine-slice block |
| `wang16_sides` | 16 | 4-bit edge sheets |
| `wang16_corners` | 16 | 4-bit corner sheets |
| `blob47` | 47 | the standard blob, with order variants |
| `rpgmaker_a2` | 5 | quarter-tile sheets (§ quarters, below) |

Sheets disagree about the order *within* a layout, so the template guesses
and the rule list is where the three tiles it got wrong are fixed. That
repair loop is the feature, not the guess.

### Quarters

`mode = "quarters"`: a cell is four quarter quads, each chosen by the two
cells beside that corner and the one across it. Five cases cover every
neighbourhood — fill, horizontal edge, vertical edge, outer corner, inner
corner — so the terrain draws from five tiles and takes the quarter that sits
where the corner does. Twenty quarter rects from five tiles, not sixteen
named ones: the corner a quarter is cut from is the corner it lands in, which
is what makes one tile serve all four sides.

This is the RPG-Maker-A2 family — a large share of free sheets ship as five
tiles and produce all 47 combinations this way, and without it those sheets
cannot be used at all. A sheet whose five are not consecutive names them:
`quarters = [fill, horizontal, vertical, outer, inner]`.

A cell is quartered when its tile is the terrain's `first_tile`, so a
hand-placed tile autotiles the same way a painted one does, and the four
tiles a cell resolves to ride in the chunk digest — a cell drawn from its
neighbours has to rebuild when one of them is in another chunk.

### Reading another layer

A rule set may read a sibling layer's terrain grid (`terrain_from = "height"`)
rather than its own. That is how cliffs and elevation are drawn, and it is
what LDtk's auto-layer source is, so those rules import rather than bake.

### Hex is not 3 × 3

A hexagonal neighbourhood is six cells, not eight. Hex rule patterns are
their own shape, authored on a hex widget; the resolver takes the layout's
neighbourhood from the same place the layout functions live. Isometric maps
stay square — the grid is square, the view is rotated — so 3 × 3 holds.

## 3. The surface

| Need | Decision |
| --- | --- |
| Collision from tiles | Step 1: `[tiles.<id>] collision = "full"` or polygons in tile pixels; a `tile_collision` component building one rapier `Voxels` collider per material group and one ordinary collider per shaped tile, with the `collider2d` material keys and `one_way` per tile. `collider2d`'s `kind = "voxels"`, built 2026-09-06, is what it stands on |
| Non-square tiles, spacing, margin | Step 1: `tile_size = [w, h]`, `spacing`, `margin` on the tileset |
| Tiles that are not a grid | Step 1: a tileset may name a `sprite_sheet` instead of a grid — the Tiled image-collection case, and what `balaur import x.aseprite` already writes |
| Painting left and up | Step 2: `origin = [col, row]` on the map, which replaces the node-shifting the tool does today |
| Flipped and rotated cells | Step 2: an optional `flags` grid beside `cells`, written only when something is flipped |
| Large maps | Step 2: cells move to their own file in 32 × 32 chunks; `set_cell` rebuilds one chunk. Culling of chunks is `docs/PLAN-views-and-culling.md` |
| Terrains and autotile | Step 3: §2 in full — values, rules, templates, transforms, hashed variation |
| Custom data on a tile | Step 4: `[tiles.<id>.data]`, any table; `tilemap.tile_data(id)` and `tilemap.data_at(x, y)` |
| Animated tiles | Step 4: `[tiles.<id>] animation = { frames = [ids], fps = 8 }` |
| Tiles casting 2D shadows | Step 4: `[tiles.<id>] occluder = true` or a polygon; the map contributes one merged outline per run to the light map, the item `docs/PLAN-rendering.md` deferred |
| Painting in the editor | Step 5: bucket, line, eyedropper, a multi-tile stamp with flip and rotate, and the terrain brush |
| Editing a tileset | Step 6: the tileset document tab (§4) |
| Quarter-tile sheets | Step 7 |
| Isometric and hexagonal maps | Step 8: `layout`, `hex_side`, and the hex rule widget |
| Tiled and LDtk projects | Step 9 |
| Y-sorted tiles | **Not planned.** Within a map the mesh is painter-ordered row by row, which is what an isometric floor wants; an *entity* moving among tiles orders itself with `z_index`, as `docs/PLAN-2d-games.md` said |
| Navigation over tiles | `docs/PLAN-navigation.md` step 7 bakes a grid from the collision |

## 4. The editors

Two surfaces, because they are two jobs: painting a level is a dock beside
the viewport, and describing a tileset is a document.

### The Tiles dock — painting

The bottom dock, `tall`, as today. Brushes as an icon row: paint, rectangle,
bucket, line, stamp, eyedropper, terrain. A stamp is a box dragged in the
palette, painted as a block, with `[` `]` to rotate and `x` `y` to flip. The
layer list moves out of the palette's foot into its own column with a
visibility eye per layer.

The viewport draws the grid it already draws, the hover cell, the swept
rectangle, and two overlays behind a chip each: **collision**, the merged
runs in the physics debug colour, and **terrain**, a tint per value so the
grid being edited is visible.

### The tileset tab — describing

A tileset opens as a document tab in the centre, beside `main.toml` and a
script, from the Assets dock or an Edit set button in the Tiles dock. The
centre is where the editor already puts a full-height editing surface, and a
212 px dock cannot hold a rule matrix — least of all in the compact layout.

```
┌ ◇ main.toml   ▦ dungeon.toml ✕                                             ┐
├──────────────────────────────────┬─────────────────────────────────────────┤
│  the atlas, a tile selectable    │  Tile 14      [ Tile · Rules · Sheet ]  │
│  ▣▣▣▣▣▣▣▣  overlays:             │  ┌──────────┐  collision  [ full    ▾ ] │
│  ▣▣■▣▣▣▣▣  ⛨ collision           │  │ the tile │  occluder   [ collision▾ ]│
│  ▣▣▣▣▣▣▣▣  ◫ terrain             │  │ at 8×    │  terrain    [ grass   ▾ ] │
│  ▣▣▣▣▣▣▣▣  ⏵ animation           │  └──────────┘  weight     [ 1.00      ] │
└──────────────────────────────────┴─────────────────────────────────────────┘
```

- **Tile** — the tile at 8×, its collision polygon drawn over it with the
  polygon tool's point mode and a Full tile button for the common case, its
  terrain, its weight, its animation frames picked in order with a preview at
  the chosen fps, and its data table.
- **Rules** — the ordered list. Each row is its matrix as a widget, the
  output tile's thumbnail, and badges for transforms and chance. Left-click a
  matrix cell cycles any → this → other; right-click picks a value. Drag the
  handle to reorder, because order is the semantics. A Template button fills
  the set; a preview strip resolves a scratch grid live; a warning line
  counts the cells that matched nothing.
- **Sheet** — texture, tile size, spacing, margin, layout, and the
  `sprite_sheet` a non-grid set names.

Edits go through `assets::load` / `assets::save`, which is how the inspector
already edits an asset (`editor/scripts/inspector.rn:957`).

**The resolver is called, never reimplemented.** The tool asks the engine
through a `tiles::` script module — pure functions over a tileset definition
and rows of values — so the editor and the game cannot disagree about a
level.

**D24 blocks this.** A tileset that is a *file* does not draw in the editor's
mirror, because the mirror engine's root is the editor's and the texture path
inside the asset resolves under `editor/` (`docs/EDITOR-SCREENS.md` D24). The
Tiles tool papers over it by reading the asset itself; a tileset document
cannot. Either the mirror gets the game as a second root or an asset file is
mirrored inline with its references resolved — which fixes meshes and
materials too.

## 5. Import

Both importers live in `balaur_cli`, where `.aseprite` and `.glb` already
land, so the web bundle does not grow.

- **Tiled**: the `tiled` crate (0.16) reads `.tmx` and `.tsx`, decodes csv,
  base64, gzip, zlib and zstd layers, and takes a custom `ResourceReader`, so
  reads go through `ProjectFiles` rather than `std::fs`.
- **LDtk**: `ldtk_rust` (0.6) over the documented JSON; external `.ldtkl`
  levels are the same types.

| Source | Becomes |
| --- | --- |
| tileset image and grid | `tilesets/<name>.toml`, image copied to `art/` |
| per-tile object group (Tiled) | `[tiles.N] collision` |
| tile animation (Tiled) | `[tiles.N] animation` |
| properties, LDtk entity fields | `[tiles.N.data]`, a `data` table on the node |
| Wang sets (Tiled) | `[[terrains]]` in the bitmask form |
| LDtk auto-layer rules | `[[rules]]`: matrices, must-be and must-not-be, flips, chance, group order, source layer |
| tile layer | a `tilemap` node, `z_index` by order, opacity as colour alpha, offset as position |
| object layer, LDtk entities | a node each, with a `data` table |
| LDtk level | a scene; a world is a scene per level |
| infinite map chunks | chunked cells, verbatim |

Lossy, and said in the output: LDtk's perlin modifiers and its tile stacking
do not import, and an image-collection tileset needs the `sprite_sheet` form.
The source file stays the tool's; nothing exports back.

### The Tiled dependency

`tiled` 0.16 pins `quick-xml` 0.31, which carries two denial-of-service
advisories (RUSTSEC-2026-0194 and -0195: a crafted start tag pins a CPU core,
a crafted namespace list exhausts memory). 0.16 is its newest release, so
there is no version to move to, and `deny.toml` ignores both.

That is a judgement about where the XML comes from, not about the bugs. It is
parsed by `balaur import level.tmx`, which a developer runs by hand on a file
they chose, at author time. `tiled` is a `cfg(not(target_family = "wasm"))`
dependency of `balaur_cli` alone: no shipped game links it, and nothing in the
engine parses XML at run time. A pipeline that imported maps a stranger
uploaded would be a different question, and would want a wall-clock bound
around the parse.

Drop the ignore the day `tiled` moves to `quick-xml` 0.41, or the day the
`.tmx` reader is the engine's own.

## 6. Steps

1. Core `tiles`, tile metadata, the collision runs and outline, the grid
   keys, `tile_collision`. `examples/angrynerds` loses its hand-placed wall
   colliders.
2. `origin`, the `flags` grid, chunked cells in their own file, and the mesh
   builder moving out of kiss3d into `balaur_render`.
3. Terrains, rules, templates, transforms, hashed variation, `set_terrain`.
4. Custom data, animated tiles, occluding tiles.
5. The Tiles dock's remaining brushes and the terrain brush, each asserted by
   `tilesdemo`.
6. The tileset document tab, behind D24.
7. Quarter-tile sheets. *Built 2026-09-07.*
8. Isometric and hexagonal layouts, and the hex rule widget.
9. Tiled and LDtk import. *Built 2026-09-11.*

## 7. What CI can prove, and what it cannot

Headless proves that a ray hits a wall cell, that a body slid along a floor
of tiles keeps its speed across every seam and never stops on one — the
ghost-collision test, and the reason the collider is parry's voxel shape and
not a row of cuboids — that a rule set resolves a corner, that the
same seed and the same cells resolve identically twice and on every OS, and
that a map's digest does not move. `tilesdemo` paints and asserts `cells`;
each new brush adds a stroke to it, and the rule editor gets a `--state`
self-test that fills a template and asserts the resolved grid. CI cannot
prove a brush feels right; the showcase clip is where a person checks.

## 8. Open questions

1. **A tile map on a dynamic body** has no mass model — nothing says what a
   tile weighs. Static and kinematic only, and the error says so.
2. **A tile's friction and its one-way flag are per group.** parry's voxel
   shape carries no data per cell, so each material — solid, icy, one-way —
   is a collider of its own. Nothing says how many groups is too many.
3. **Chunk size.** Thirty-two is a guess; the benchmark project gets a
   tile-map case before step 2 picks a number.
4. **Stacked rule output** — one rule writing into a second layer, which is
   how LDtk paints flowers over grass — is not in step 3. It wants a target
   layer per rule group and a rule for what a stacked cell collides with.
