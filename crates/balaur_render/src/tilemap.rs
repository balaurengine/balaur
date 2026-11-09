//! The `tileset` asset and the `tilemap` component: a grid of tiles drawn
//! from one atlas texture. The component and parser are backend-free; the
//! feature-gated kiss3d mirror is in `tilemap_mesh`.

use crate::vocabulary::words;
use anyhow::{Context, Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::derived::{Component, Record, Variant, Written};
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;
use balaur_script::Module;

pub use balaur_geometry::tiles::{TILESET_ASSET_TYPE, TileSet};

#[cfg(feature = "window")]
pub(crate) use crate::tilemap_mesh::{TilemapSlot, sync_tilemaps};
use crate::vocabulary::keys;

const CELL: &str = "cell";
const SET_CELL: &str = "set_cell";
const SET_TERRAIN: &str = "set_terrain";
const TERRAIN: &str = "terrain";
const TILE_DATA: &str = "tile_data";

/// What a definition table holds, for the generated reference.
const TILESET_ASSET_DOC: &str = r#"An image cut into equal tiles for `tilemap`: `texture`, `tile_size` in pixels and `columns` per row. `[tiles.<id>]` gives a tile `collision`; `[[terrains]]` auto-tiles by `kind`.

```toml
type = "tileset"
texture = "art/dungeon.png"
tile_size = 16                   # or [w, h]
columns = 8
spacing = 0                      # gutter between tiles
margin = 0                       # border around the sheet

[tiles.3]                        # tile ids count row by row from the top left
collision = "full"

[tiles.7]
collision = [[[0, 16], [16, 16], [16, 8]]]   # polygons in tile pixels, y down
one_way = true                   # a platform a body passes through from below

[[terrains]]                     # paints by value and picks the tiles
name = "grass"
value = 1
kind = "quarters"                # rules, sides, corners, corners_and_sides or quarters
first_tile = 16
# quarters = [fill, horizontal edge, vertical edge, outer corner, inner corner] tile ids, when they do not follow first_tile
```"#;

/// The `tileset` asset type: files live in `tilesets/`.
pub(crate) fn register_tileset_asset(reg: &mut Registry<'_>) {
    reg.register_checked_asset(
        TILESET_ASSET_TYPE,
        "tilesets",
        TILESET_ASSET_DOC,
        balaur_geometry::tiles::check_tileset,
        |value| {
            Ok(
                std::rc::Rc::new(balaur_geometry::tiles::parse_tileset(value)?)
                    as std::rc::Rc<dyn std::any::Any>,
            )
        },
    );
}

/// A grid of tiles from one `tileset` asset, centred on the node. `cells` holds rows of tile ids, or names the `.cells` file holding them; `pixels_per_unit` is tile pixels per world unit.
// The node draws the [`TileLayout`] each write lays out beside it; a script's
// painting writes what it painted back here, so a save keeps it.
#[derive(Clone, Component)]
#[component(
    name = "tilemap",
    on_remove = tilemap_removed,
    tags = [words::ORTHOGRAPHIC, "render"],
    after_write = tilemap_written,
)]
pub struct Tilemap {
    /// The tileset naming the texture and tile grid
    #[property(asset = "tileset")]
    pub tileset: String,
    /// Tint over every tile, as channel floats or #rrggbb / #rrggbbaa
    #[property(color, default = [1.0, 1.0, 1.0, 1.0])]
    pub color: [f32; 4],
    /// The material the whole map draws with; empty draws with the built-in one
    #[property(asset = "material")]
    pub material: String,
    /// The tile ids: rows of them in the scene, or a `.cells` file holding them, for a level too big to read in a scene
    #[property(group = "cells")]
    pub cells: Cells,
    /// What was painted, as rows of terrain values, -1 where nothing was, when the map autotiles: the cells are resolved from this through the tileset's rules
    #[property(group = "cells")]
    pub terrain: Vec<Vec<i64>>,
    /// How each cell is turned, as rows of numbers beside the cells: 1 mirrors it left to right, 2 top to bottom, 4 across its diagonal
    #[property(group = "cells")]
    pub flags: Vec<Vec<u8>>,
    /// Which way the variation falls where a rule offers alternates; the same seed lays a map out the same way every time
    #[property(min = 0, group = "cells")]
    pub seed: i64,
    /// The column and row of the first cell: a map grows in any direction by moving this, and cell 0,0 always has its top-left corner on the node
    #[property(group = "placement")]
    pub origin: [f32; 2],
    /// Tile-texture pixels per world unit
    #[property(default = 100.0, min = 0.01, group = "placement")]
    pub pixels_per_unit: f32,
}

/// Where a `tilemap`'s tile ids are kept.
#[derive(Clone, Debug, PartialEq, Variant)]
pub enum Cells {
    /// In the scene, as rows
    #[option(default)]
    Inline(InlineCells),
    /// In a `.cells` file of their own
    File(CellsFile),
}

/// A map's tile ids, held in the scene.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct InlineCells {
    /// Rows of tile ids, the top row first; -1 is an empty cell
    pub rows: Vec<Vec<i64>>,
}

/// A map's tile ids, kept in a file of their own.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct CellsFile {
    /// The `.cells` file: one row a line, ids apart by spaces, -1 for an empty cell
    pub path: String,
}

/// A [`Tilemap`] laid out: the cells resolved from what was painted, as the
/// backend draws them and a script paints them. The node's transform places
/// the map (its center, like every other 2D renderable).
pub struct TileLayout {
    /// Reference to the `tileset` asset.
    pub tileset: String,
    /// The cells as the component holds them, written back so `get` returns
    /// them: a file's name until a paint or a terrain writes them as rows.
    pub cells: Cells,
    /// The `material` asset the map draws with; empty is the built-in one.
    pub material: String,
    /// Tile indices, row 0 at the top so the text reads like the scene;
    /// `None` is an empty cell.
    pub grid: Vec<Vec<Option<u32>>>,
    /// The grid coordinate of the first stored cell. A map grows in any
    /// direction by moving this rather than moving the node.
    pub origin: [i32; 2],
    /// How each cell is turned, when any of them is; empty means none are.
    pub flags: Vec<Vec<u8>>,
    /// What was painted, when the map is painted by terrain rather than by
    /// tile: the cells above are then resolved from this.
    pub terrain: Vec<Vec<Option<u32>>>,
    /// Which way the variation falls, for a map that wants its own.
    pub seed: u64,
    /// Tile-texture pixels per world unit.
    pub pixels_per_unit: f32,
    /// Tint over every tile, under the node's inherited one.
    pub color: [f32; 4],
    /// Bumped when the content changes so backends rebuild their mesh.
    pub version: u64,
}

/// The grid `cells` holds, read out of its file where it keeps one.
fn cells_of(eng: &Engine, cells: &Cells) -> Result<Vec<Vec<Option<u32>>>> {
    match cells {
        Cells::Inline(inline) => Ok(grid_of_ids(&inline.rows)),
        // A file not chosen yet: no cells to read.
        Cells::File(file) if file.path.is_empty() => Ok(Vec::new()),
        Cells::File(file) => Ok(grid_of_ids(&read_cells_file(eng, &file.path)?)),
    }
}

/// A `.cells` file's rows: one row per line, ids separated by spaces, `-1`
/// for an empty cell.
fn read_cells_file(eng: &Engine, path: &str) -> Result<Vec<Vec<i64>>> {
    let text = eng
        .resource::<balaur_core::project::ProjectFiles>()
        .borrow()
        .read(path)
        .map(String::from_utf8)
        .with_context(|| format!("a tilemap's cells file '{path}'"))?
        .with_context(|| format!("a tilemap's cells file '{path}' is not text"))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(row, line)| {
            line.split_whitespace()
                .map(|cell| {
                    cell.parse().map_err(|_| {
                        anyhow!(
                            "a tilemap's cells file '{path}', row {row}: '{cell}' is not a tile id"
                        )
                    })
                })
                .collect()
        })
        .collect()
}

/// Rows of ids as a grid, where anything below zero is empty.
fn grid_of_ids(rows: &[Vec<i64>]) -> Vec<Vec<Option<u32>>> {
    rows.iter()
        .map(|row| row.iter().map(|id| u32::try_from(*id).ok()).collect())
        .collect()
}

/// A grid back as rows of ids, -1 for an empty cell.
fn ids_of(grid: &[Vec<Option<u32>>]) -> Vec<Vec<i64>> {
    grid.iter()
        .map(|row| row.iter().map(|cell| cell.map_or(-1, i64::from)).collect())
        .collect()
}

/// The grid back as the cells a scene holds, the form `set_cell` writes.
fn cells_value(grid: &[Vec<Option<u32>>]) -> Cells {
    Cells::Inline(InlineCells { rows: ids_of(grid) })
}

/// Resolve every painted cell through the tileset's rules.
///
/// A map that carries a terrain grid has its cells derived from it, so an
/// editor paints values and the engine picks the tiles — one resolver, and
/// the same one a script reaches through `set_terrain`.
/// A painted terrain grid read in the rules' own coordinates: cell `origin`
/// is the grid's first, and the two questions a rule asks are answered here.
struct Painted<'a> {
    terrain: &'a [Vec<Option<u32>>],
    origin: [i32; 2],
}

impl Painted<'_> {
    fn value_at(&self, x: i32, y: i32) -> Option<u32> {
        let x = usize::try_from(x - self.origin[0]).ok()?;
        let y = usize::try_from(y - self.origin[1]).ok()?;
        self.terrain.get(y)?.get(x).copied().flatten()
    }

    fn inside(&self, x: i32, y: i32) -> bool {
        let (Ok(x), Ok(y)) = (
            usize::try_from(x - self.origin[0]),
            usize::try_from(y - self.origin[1]),
        ) else {
            return false;
        };
        self.terrain.get(y).is_some_and(|line| x < line.len())
    }
}

fn resolve_all(eng: &Engine, map: &mut TileLayout) {
    if map.terrain.is_empty() {
        return;
    }
    let Ok(set) = balaur_core::assets::load_typed::<TileSet>(eng, &map.tileset) else {
        return;
    };
    if set.rules.is_empty() {
        return;
    }
    let terrain = map.terrain.clone();
    let origin = map.origin;
    let painted = Painted {
        terrain: &terrain,
        origin,
    };
    let value_at = |x, y| painted.value_at(x, y);
    let inside = |x, y| painted.inside(x, y);
    for row in 0..span(terrain.len()) {
        for column in 0..span(terrain.first().map_or(0, Vec::len)) {
            let (x, y) = (origin[0] + column, origin[1] + row);
            let (tile, flags) =
                balaur_geometry::tiles::resolve(&set.rules, &value_at, &inside, x, y, map.seed)
                    .map_or((None, 0), |(tile, flags)| (Some(tile), flags));
            write_cell(map, x, y, tile);
            write_flags(map, x, y, flags);
        }
    }
    map.cells = cells_value(&map.grid);
}

/// Resolve one cell and the ring around it, which is every cell the write
/// could have changed the neighbourhood of.
fn resolve_around(eng: &Engine, map: &mut TileLayout, column: i32, row: i32) {
    let Ok(set) = balaur_core::assets::load_typed::<TileSet>(eng, &map.tileset) else {
        return;
    };
    if set.rules.is_empty() {
        return;
    }
    // How far a rule can see is how far the write reached: a 7-wide pattern
    // three cells away was matched on the cell that just changed.
    let radius = set
        .rules
        .iter()
        .map(|rule| span(rule.size / 2))
        .max()
        .unwrap_or(1);
    // Read first and write after, so the resolver reads the painted grid in
    // place rather than a copy of it taken for every cell of a stroke.
    let mut resolved = Vec::new();
    {
        let painted = Painted {
            terrain: &map.terrain,
            origin: map.origin,
        };
        let value_at = |x, y| painted.value_at(x, y);
        let inside = |x, y| painted.inside(x, y);
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let (x, y) = (column + dx, row + dy);
                match balaur_geometry::tiles::resolve(
                    &set.rules, &value_at, &inside, x, y, map.seed,
                ) {
                    Some((tile, flags)) => resolved.push((x, y, Some(tile), flags)),
                    None if value_at(x, y).is_none() => resolved.push((x, y, None, 0)),
                    None => {}
                }
            }
        }
    }
    for (x, y, tile, flags) in resolved {
        write_cell(map, x, y, tile);
        write_flags(map, x, y, flags);
    }
}

/// Paint a terrain value, growing the painted grid the way the cells grow.
fn write_terrain(map: &mut TileLayout, column: i32, row: i32, value: Option<u32>) -> bool {
    grow_to(map, column, row);
    let width = map.grid.iter().map(Vec::len).max().unwrap_or(0);
    map.terrain.resize(map.grid.len(), Vec::new());
    for line in &mut map.terrain {
        line.resize(width, None);
    }
    let Some((x, y)) = grid_index(map, column, row) else {
        return false;
    };
    if map.terrain[y][x] == value {
        return false;
    }
    map.terrain[y][x] = value;
    true
}

/// Put a turn on a cell, growing the flags grid to match the cells.
fn write_flags(map: &mut TileLayout, column: i32, row: i32, flags: u8) {
    if flags == 0 && map.flags.is_empty() {
        return;
    }
    let width = map.grid.iter().map(Vec::len).max().unwrap_or(0);
    map.flags.resize(map.grid.len(), Vec::new());
    for line in &mut map.flags {
        line.resize(width, 0);
    }
    if let Some((x, y)) = grid_index(map, column, row) {
        map.flags[y][x] = flags;
    }
}

/// The core grid a map describes: what physics collides with, and what the
/// mesh is built from, so the two cannot disagree about a cell.
pub(crate) fn grid_of(map: &TileLayout, set: &TileSet) -> balaur_geometry::tiles::TileGrid {
    balaur_geometry::tiles::TileGrid {
        tileset: map.tileset.clone(),
        rows: map.grid.clone(),
        origin: map.origin,
        flags: map.flags.clone(),
        layout: set.layout,
        tile_world: [
            set.tile_size[0] / map.pixels_per_unit,
            set.tile_size[1] / map.pixels_per_unit,
        ],
        version: map.version,
    }
}

/// Put a tile at a coordinate, growing the grid in whatever direction it has
/// to. Answers whether anything changed.
fn write_cell(map: &mut TileLayout, column: i32, row: i32, tile: Option<u32>) -> bool {
    grow_to(map, column, row);
    let Some((x, y)) = grid_index(map, column, row) else {
        return false;
    };
    if map.grid[y][x] == tile {
        return false;
    }
    map.grid[y][x] = tile;
    true
}

/// Grow the stored rows until they hold a coordinate, moving the origin
/// rather than the node.
fn grow_to(map: &mut TileLayout, column: i32, row: i32) {
    if grid_index(map, column, row).is_some() {
        return;
    }
    let columns = span(map.grid.iter().map(Vec::len).max().unwrap_or(0));
    let rows = span(map.grid.len());
    let left = (map.origin[0] - column).max(0);
    let up = (map.origin[1] - row).max(0);
    let right = (column - (map.origin[0] + columns - 1)).max(0);
    let down = (row - (map.origin[1] + rows - 1)).max(0);
    let width = (columns + left + right).max(0) as usize;
    let height = (rows + up + down).max(0) as usize;
    let (left, up) = (left as usize, up as usize);
    // Growing left or up moves every stored cell along, so the flags and the
    // painted values move with it: a grid the map keeps per cell that grew
    // only at its far edge would answer for the wrong cell from then on.
    grow_rows(&mut map.grid, None, left, up, width, height);
    if !map.terrain.is_empty() {
        grow_rows(&mut map.terrain, None, left, up, width, height);
    }
    if !map.flags.is_empty() {
        grow_rows(&mut map.flags, 0, left, up, width, height);
    }
    map.origin[0] -= span(left);
    map.origin[1] -= span(up);
}

/// Grow one of a map's per-cell grids to `width` by `height`, putting `left`
/// new columns before the stored ones and `up` new rows above them.
fn grow_rows<T: Clone>(
    rows: &mut Vec<Vec<T>>,
    empty: T,
    left: usize,
    up: usize,
    width: usize,
    height: usize,
) {
    for line in rows.iter_mut() {
        let mut grown = vec![empty.clone(); left];
        grown.append(line);
        grown.resize(width, empty.clone());
        *line = grown;
    }
    let mut grown = vec![vec![empty.clone(); width]; up];
    grown.append(rows);
    *rows = grown;
    rows.resize(height, vec![empty; width]);
}

/// How many cells a count of stored rows or columns spans, as a coordinate.
fn span(count: usize) -> i32 {
    i32::try_from(count).unwrap_or(i32::MAX)
}

/// Where a coordinate sits in the stored rows.
fn grid_index(map: &TileLayout, column: i32, row: i32) -> Option<(usize, usize)> {
    let x = usize::try_from(column - map.origin[0]).ok()?;
    let y = usize::try_from(row - map.origin[1]).ok()?;
    (y < map.grid.len() && x < map.grid[y].len()).then_some((x, y))
}

/// Mirror the map into the [`TileGrid`] core carries, which is what physics
/// reads: it may not see a render component, and both need the same cells.
///
/// A map whose tileset will not load carries no grid, so nothing collides
/// with cells nobody can size.
fn sync_grid(eng: &Engine, entity: Entity) {
    let (tileset, rows, origin, flags, ppu, version) = {
        let world = eng.world();
        let Ok(map) = world.get::<&TileLayout>(entity) else {
            return;
        };
        (
            map.tileset.clone(),
            map.grid.clone(),
            map.origin,
            map.flags.clone(),
            map.pixels_per_unit,
            map.version,
        )
    };
    let set = balaur_core::assets::load_typed::<TileSet>(eng, &tileset).ok();
    let Some(set) = set else {
        let _ = eng.remove_one::<balaur_geometry::tiles::TileGrid>(entity);
        return;
    };
    let grid = grid_of(
        &TileLayout {
            tileset,
            cells: Cells::default(),
            material: String::new(),
            grid: rows,
            origin,
            flags,
            terrain: Vec::new(),
            seed: 0,
            pixels_per_unit: ppu,
            color: [1.0; 4],
            version,
        },
        &set,
    );
    if let Ok(mut current) = eng
        .world()
        .get::<&mut balaur_geometry::tiles::TileGrid>(entity)
    {
        *current = grid;
        return;
    }
    let _ = eng.insert_one(entity, grid);
}

fn set_tilemap(eng: &Engine, entity: Entity, next: TileLayout) -> Result<()> {
    // The world's borrow ends before the grid is mirrored: `sync_grid` takes
    // it again, and it loads an asset in between.
    let fresh = {
        let world = eng.world();
        if let Ok(mut map) = world.get::<&mut TileLayout>(entity) {
            // Everything the mesh or the collider is built from: an origin
            // moves every cell, and a flag turns one.
            let changed = map.tileset != next.tileset
                || map.grid != next.grid
                || map.material != next.material
                || map.origin != next.origin
                || map.flags != next.flags
                || map.terrain != next.terrain
                || map.seed != next.seed
                || map.pixels_per_unit.to_bits() != next.pixels_per_unit.to_bits();
            let version = map.version + u64::from(changed);
            *map = next;
            map.version = version;
            None
        } else {
            Some(next)
        }
    };
    if let Some(next) = fresh {
        eng.insert_one(entity, next)
            .map_err(|_| anyhow!("node is dead"))?;
    }
    sync_grid(eng, entity);
    Ok(())
}

impl TileLayout {
    /// `asked` laid out, its cells resolved from what it painted.
    fn of(eng: &Engine, asked: Tilemap) -> Result<Self> {
        let grid = cells_of(eng, &asked.cells)?;
        // Checked here so a bad definition is reported where it was
        // written, but only warned: one bad asset must not kill the scene.
        if !asked.tileset.is_empty()
            && let Err(why) = balaur_core::assets::load_typed::<TileSet>(eng, &asked.tileset)
        {
            tracing::warn!("tilemap tileset '{}': {why:#}", asked.tileset);
        }
        let mut next = Self {
            tileset: asked.tileset,
            cells: asked.cells,
            material: asked.material,
            grid,
            origin: asked.origin.map(|at| at as i32),
            flags: asked.flags,
            terrain: grid_of_ids(&asked.terrain),
            seed: asked.seed.cast_unsigned(),
            pixels_per_unit: asked.pixels_per_unit.max(0.01),
            color: asked.color,
            version: 0,
        };
        resolve_all(eng, &mut next);
        Ok(next)
    }
}

/// What a laid-out map holds that its component did not say: the cells its
/// terrain resolved to, and what a script painted. Written back so `get`
/// reports it and a save keeps it.
fn write_back(eng: &Engine, entity: Entity) {
    let world = eng.world();
    let (Ok(map), Ok(mut asked)) = (
        world.get::<&TileLayout>(entity),
        world.get::<&mut Tilemap>(entity),
    ) else {
        return;
    };
    asked.cells = map.cells.clone();
    asked.origin = map.origin.map(|at| at as f32);
    if !map.terrain.is_empty() {
        asked.terrain = ids_of(&map.terrain);
    }
    if !map.flags.is_empty() {
        asked.flags.clone_from(&map.flags);
    }
}

/// What a write of `tilemap` leaves to be worked out again.
fn tilemap_written(eng: &Engine, entity: Entity, _written: Written) -> Result<()> {
    tilemap_laid_out(eng, entity)
}

/// The map laid out again from what was written, and what that resolved
/// written back.
fn tilemap_laid_out(eng: &Engine, entity: Entity) -> Result<()> {
    let Some(asked) = eng
        .world()
        .get::<&Tilemap>(entity)
        .ok()
        .map(|asked| Tilemap::clone(&asked))
    else {
        return Ok(());
    };
    set_tilemap(eng, entity, TileLayout::of(eng, asked)?)?;
    write_back(eng, entity);
    Ok(())
}

/// What a removed `Tilemap` leaves behind, taken with it.
fn tilemap_removed(eng: &Engine, entity: Entity) {
    let _ = eng.remove_one::<TileLayout>(entity);
    let _ = eng.remove_one::<balaur_geometry::tiles::TileGrid>(entity);
}

/// The `tilemap` component. Writes a [`TileLayout`] on the node; the kiss3d
/// backend mirrors it as one atlas-textured mesh, rebuilt only when
/// [`TileLayout::version`] moves — a tilemap is static between edits.
pub(crate) fn register_tilemap_component(reg: &mut Registry<'_>) {
    reg.add_component::<Tilemap>();
}

/// What a map painted by terrain answers: the values, the rules' output, and
/// what a tile carries. Split from `install_tilemap_api` under `MAX_FN_LINES`.
pub(crate) fn install_tilemap_terrain_api(m: &mut Module<Engine>) {
    m.describe(&[
        (SET_TERRAIN, &[keys::TILEMAP], "(x: int, y: int, terrain: int)", "Paint a terrain value at a column and row and let the tileset's rules pick the tiles, for that cell and the ring around it; below zero clears it."),
        (TERRAIN, &[keys::TILEMAP], "(x: int, y: int) -> int", "The terrain value painted at a column and row, or -1 where nothing was painted."),
        (TILE_DATA, &[keys::TILEMAP], "(x: int, y: int) -> any", "What the tileset says about the tile at a column and row -- its `[tiles.<id>.data]` table -- or nil where the cell is empty or the tile carries none."),
    ]);
    m.function(
        TILE_DATA,
        |eng: &Engine, (node, x, y): (balaur_script::NodeId, i64, i64)| {
            let entity = balaur_core::entity_of(node)?;
            let world = eng.world();
            let map = world
                .get::<&TileLayout>(entity)
                .map_err(|_| anyhow!("the node carries no tilemap"))?;
            let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) else {
                return Ok(balaur_script::Value::Nil);
            };
            // Read out of the map's own rows: a `TileGrid` to ask one cell
            // would copy every row of the map for every call.
            let cell = balaur_geometry::tiles::cell_in(&map.grid, map.origin, x, y);
            let tileset = map.tileset.clone();
            drop(map);
            drop(world);
            let set = balaur_core::assets::load_typed::<TileSet>(eng, &tileset)?;
            Ok(
                match cell
                    .and_then(|id| set.tile(id))
                    .and_then(|t| t.data.as_ref())
                {
                    Some(data) => balaur_core::node_api::from_toml(data)?,
                    None => balaur_script::Value::Nil,
                },
            )
        },
    );
    m.function(
        TERRAIN,
        |eng: &Engine, (node, x, y): (balaur_script::NodeId, i64, i64)| {
            let entity = balaur_core::entity_of(node)?;
            let world = eng.world();
            let map = world
                .get::<&TileLayout>(entity)
                .map_err(|_| anyhow!("the node carries no tilemap"))?;
            let found = i32::try_from(x)
                .ok()
                .zip(i32::try_from(y).ok())
                .and_then(|(x, y)| {
                    let x = usize::try_from(x - map.origin[0]).ok()?;
                    let y = usize::try_from(y - map.origin[1]).ok()?;
                    map.terrain.get(y)?.get(x).copied().flatten()
                });
            Ok(found.map_or(-1, i64::from))
        },
    );
}

/// `render.set_cell` and `render.cell`: one tile at a time, for a map a
/// script edits as it plays. A write past the grid's edge grows it.
pub(crate) fn install_tilemap_api(m: &mut Module<Engine>) {
    m.describe(&[
        (SET_CELL, &[keys::TILEMAP], "(x: int, y: int, tile: int)", "Put one tile at a column and row; a tile below zero clears the cell, and a cell outside the map grows it in that direction. The mesh rebuilds on the next frame."),
        (CELL, &[keys::TILEMAP], "(x: int, y: int) -> int", "The tile at a column and row, or -1 for an empty cell or one past the edge."),
    ]);
    m.function(
        SET_CELL,
        |eng: &Engine, (node, x, y, tile): (balaur_script::NodeId, i64, i64, i64)| {
            let entity = balaur_core::entity_of(node)?;
            let (x, y) = (
                i32::try_from(x).map_err(|_| anyhow!("that column is too far out"))?,
                i32::try_from(y).map_err(|_| anyhow!("that row is too far out"))?,
            );
            let changed = {
                let world = eng.world();
                let mut map = world
                    .get::<&mut TileLayout>(entity)
                    .map_err(|_| anyhow!("the node carries no tilemap"))?;
                let next = u32::try_from(tile).ok();
                let changed = write_cell(&mut map, x, y, next);
                if changed {
                    map.cells = cells_value(&map.grid);
                    map.version += 1;
                }
                changed
            };
            if changed {
                sync_grid(eng, entity);
                write_back(eng, entity);
            }
            Ok(())
        },
    );
    m.function(
        SET_TERRAIN,
        |eng: &Engine, (node, x, y, terrain): (balaur_script::NodeId, i64, i64, i64)| {
            let entity = balaur_core::entity_of(node)?;
            let (x, y) = (
                i32::try_from(x).map_err(|_| anyhow!("that column is too far out"))?,
                i32::try_from(y).map_err(|_| anyhow!("that row is too far out"))?,
            );
            let changed = {
                let world = eng.world();
                let mut map = world
                    .get::<&mut TileLayout>(entity)
                    .map_err(|_| anyhow!("the node carries no tilemap"))?;
                let next = u32::try_from(terrain).ok();
                let changed = write_terrain(&mut map, x, y, next);
                if changed {
                    resolve_around(eng, &mut map, x, y);
                    map.cells = cells_value(&map.grid);
                    map.version += 1;
                }
                changed
            };
            if changed {
                sync_grid(eng, entity);
                write_back(eng, entity);
            }
            Ok(())
        },
    );
    m.function(
        CELL,
        |eng: &Engine, (node, x, y): (balaur_script::NodeId, i64, i64)| {
            let entity = balaur_core::entity_of(node)?;
            let world = eng.world();
            let map = world
                .get::<&TileLayout>(entity)
                .map_err(|_| anyhow!("the node carries no tilemap"))?;
            let found = i32::try_from(x)
                .ok()
                .zip(i32::try_from(y).ok())
                .and_then(|(x, y)| grid_index(&map, x, y))
                .and_then(|(x, y)| map.grid[y][x]);
            Ok(found.map_or(-1, i64::from))
        },
    );
}

#[cfg(test)]
mod tests {
    use super::{Cells, TileLayout, grid_index, write_flags, write_terrain};

    fn map() -> TileLayout {
        TileLayout {
            tileset: "set".into(),
            cells: Cells::default(),
            material: String::new(),
            grid: vec![vec![Some(0)]],
            origin: [0, 0],
            flags: Vec::new(),
            terrain: Vec::new(),
            seed: 0,
            pixels_per_unit: 100.0,
            color: [1.0; 4],
            version: 0,
        }
    }

    /// A map grows left and up by moving its origin, and everything it keeps
    /// per cell has to move with the cells or answer for the wrong one.
    #[test]
    fn what_a_cell_carries_stays_with_it_when_the_map_grows() {
        let mut map = map();
        write_terrain(&mut map, 0, 0, Some(7));
        write_flags(&mut map, 0, 0, 3);
        write_terrain(&mut map, -2, -1, Some(9));
        assert_eq!(map.origin, [-2, -1], "the map grew left and up");

        let at = |map: &TileLayout, column, row| {
            grid_index(map, column, row).map(|(x, y)| (map.terrain[y][x], map.flags[y][x]))
        };
        assert_eq!(at(&map, 0, 0), Some((Some(7), 3)), "the first cell painted");
        assert_eq!(
            at(&map, -2, -1),
            Some((Some(9), 0)),
            "and the one that grew"
        );
    }

    /// Painting the same value twice is not an edit, so it costs no undo step
    /// and no rebuild.
    #[test]
    fn painting_a_cell_the_value_it_holds_changes_nothing() {
        let mut map = map();
        assert!(write_terrain(&mut map, 1, 1, Some(2)));
        assert!(!write_terrain(&mut map, 1, 1, Some(2)));
    }
}
