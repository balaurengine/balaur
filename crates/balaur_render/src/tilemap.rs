//! The `tileset` asset and the `tilemap` component: a grid of tiles drawn
//! from one atlas texture. The component and parser are backend-free; the
//! kiss3d mirror at the bottom of the file is feature-gated.

use crate::shape::{keys as k, words};
use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt};

pub use balaur_core::tiles::{TILESET_ASSET_TYPE, TileSet};

/// What a definition table holds, for the generated reference.
const TILESET_ASSET_DOC: &str = r#"An image cut into equal tiles for the `tilemap` component: `texture` names
the image, `tile_size` is one tile in pixels — a number, or `[w, h]` for a
sheet whose tiles are not square — and `columns` is how many tiles one row of
the image holds. `spacing` is the gutter between tiles and `margin` the border
around the sheet, both zero by default. Tile indices count row by row from the
top left.

A `[tiles.<id>]` table says what one tile is. `collision` is `"full"` for a
solid cell, or a list of polygons in tile pixels with y down from the tile's
top-left corner; `one_way` makes a platform a body passes through from below.
A tile with no table of its own is the plain quad it always was.

```toml
[[assets]]
id = "dungeon"
type = "tileset"
texture = "art/dungeon.png"
tile_size = 16
columns = 8

[tiles.3]
collision = "full"

[tiles.7]
collision = [[[0, 16], [16, 16], [16, 8]]]
```"#;

/// The `tileset` asset type: files live in `tilesets/`.
pub(crate) fn register_tileset_asset(reg: &mut Registry<'_>) {
    reg.register_asset_type(TILESET_ASSET_TYPE, "tilesets", TILESET_ASSET_DOC, |value| {
        Ok(std::rc::Rc::new(balaur_core::tiles::parse_tileset(value)?)
            as std::rc::Rc<dyn std::any::Any>)
    });
}

/// What the `tilemap` component wrote on the node. The node's transform
/// places the map (its center, like every other 2D renderable).
pub struct Tilemap {
    /// Reference to the `tileset` asset.
    pub tileset: String,
    /// The authored rows, kept verbatim so `get` returns what was written:
    /// a string of characters or a list of rows of ids.
    pub cells: toml::Value,
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
    /// Bumped when the content changes so backends rebuild their mesh.
    pub version: u64,
}

/// `flags` as rows of numbers, or nothing when no cell is turned.
fn parse_flags(value: Option<&toml::Value>) -> Result<Vec<Vec<u8>>> {
    let Some(toml::Value::Array(rows)) = value else {
        return Ok(Vec::new());
    };
    rows.iter()
        .enumerate()
        .map(|(row, line)| {
            let line = line
                .as_array()
                .ok_or_else(|| anyhow!("flags row {row} should be a list of numbers"))?;
            line.iter()
                .map(|value| {
                    let bits = value
                        .as_integer()
                        .ok_or_else(|| anyhow!("flags row {row}: a cell's turn is a number"))?;
                    u8::try_from(bits).map_err(|_| anyhow!("flags row {row}: {bits} is not a turn"))
                })
                .collect()
        })
        .collect()
}

/// `cells` as a grid: the one-character-per-cell text, or a list of rows of
/// tile ids where anything below zero is empty — the form that lifts the
/// 36-tile cap.
fn parse_cells_value(cells: &toml::Value) -> Result<Vec<Vec<Option<u32>>>> {
    match cells {
        toml::Value::String(text) => parse_cells(text),
        toml::Value::Array(rows) => rows
            .iter()
            .enumerate()
            .map(|(row, line)| {
                let ids = line
                    .as_array()
                    .ok_or_else(|| anyhow!("cells row {row} should be a list of tile ids"))?;
                ids.iter()
                    .enumerate()
                    .map(|(column, id)| {
                        let id = id.as_integer().ok_or_else(|| {
                            anyhow!("cells row {row}, column {column}: a tile id is a whole number")
                        })?;
                        Ok(u32::try_from(id).ok())
                    })
                    .collect()
            })
            .collect(),
        other => Err(anyhow!(
            "cells should be a string of tile characters or a list of rows, got {other}"
        )),
    }
}

/// The grid back as rows of ids, the form `set_cell` writes.
fn cells_value(grid: &[Vec<Option<u32>>]) -> toml::Value {
    toml::Value::Array(
        grid.iter()
            .map(|row| {
                toml::Value::Array(
                    row.iter()
                        .map(|cell| toml::Value::Integer(cell.map_or(-1, i64::from)))
                        .collect(),
                )
            })
            .collect(),
    )
}

/// `cells` text as a grid: one row per line, `.` empty, `0`-`9` then
/// `a`-`z` indexing the tileset left-to-right, top-to-bottom.
fn parse_cells(cells: &str) -> Result<Vec<Vec<Option<u32>>>> {
    cells
        .lines()
        .enumerate()
        .map(|(row, line)| {
            line.chars()
                .enumerate()
                .map(|(column, c)| match c {
                    '.' => Ok(None),
                    '0'..='9' => Ok(Some(u32::from(c) - u32::from('0'))),
                    'a'..='z' => Ok(Some(10 + u32::from(c) - u32::from('a'))),
                    other => Err(anyhow!(
                        "cells row {row}, column {column}: '{other}' is not '.', 0-9 or a-z"
                    )),
                })
                .collect()
        })
        .collect()
}

/// Resolve every painted cell through the tileset's rules.
///
/// A map that carries a terrain grid has its cells derived from it, so an
/// editor paints values and the engine picks the tiles — one resolver, and
/// the same one a script reaches through `set_terrain`.
fn resolve_all(eng: &Engine, map: &mut Tilemap) {
    if map.terrain.is_empty() {
        return;
    }
    let Ok(set) = balaur_core::assets::load_typed::<TileSet>(eng, &map.tileset) else {
        return;
    };
    if set.rules.is_empty() {
        return;
    }
    let painted = map.terrain.clone();
    let origin = map.origin;
    let value_at = |x: i32, y: i32| -> Option<u32> {
        let x = usize::try_from(x - origin[0]).ok()?;
        let y = usize::try_from(y - origin[1]).ok()?;
        painted.get(y)?.get(x).copied().flatten()
    };
    let inside = |x: i32, y: i32| {
        let (Ok(x), Ok(y)) = (
            usize::try_from(x - origin[0]),
            usize::try_from(y - origin[1]),
        ) else {
            return false;
        };
        painted.get(y).is_some_and(|line| x < line.len())
    };
    for row in 0..span(painted.len()) {
        for column in 0..span(painted.first().map_or(0, Vec::len)) {
            let (x, y) = (origin[0] + column, origin[1] + row);
            let (tile, flags) =
                balaur_core::tiles::resolve(&set.rules, &value_at, &inside, x, y, map.seed)
                    .map_or((None, 0), |(tile, flags)| (Some(tile), flags));
            write_cell(map, x, y, tile);
            write_flags(map, x, y, flags);
        }
    }
    map.cells = cells_value(&map.grid);
}

/// Resolve one cell and the ring around it, which is every cell the write
/// could have changed the neighbourhood of.
fn resolve_around(eng: &Engine, map: &mut Tilemap, column: i32, row: i32) {
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
        let painted = &map.terrain;
        let origin = map.origin;
        let value_at = |x: i32, y: i32| -> Option<u32> {
            let x = usize::try_from(x - origin[0]).ok()?;
            let y = usize::try_from(y - origin[1]).ok()?;
            painted.get(y)?.get(x).copied().flatten()
        };
        let inside = |x: i32, y: i32| {
            let (Ok(x), Ok(y)) = (
                usize::try_from(x - origin[0]),
                usize::try_from(y - origin[1]),
            ) else {
                return false;
            };
            painted.get(y).is_some_and(|line| x < line.len())
        };
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let (x, y) = (column + dx, row + dy);
                match balaur_core::tiles::resolve(&set.rules, &value_at, &inside, x, y, map.seed) {
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
fn write_terrain(map: &mut Tilemap, column: i32, row: i32, value: Option<u32>) -> bool {
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
fn write_flags(map: &mut Tilemap, column: i32, row: i32, flags: u8) {
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

/// What a script painted, as rows of values, for the document to keep.
fn terrain_value(rows: &[Vec<Option<u32>>]) -> toml::Value {
    toml::Value::Array(
        rows.iter()
            .map(|row| {
                toml::Value::Array(
                    row.iter()
                        .map(|cell| toml::Value::Integer(cell.map_or(-1, i64::from)))
                        .collect(),
                )
            })
            .collect(),
    )
}

/// The core grid a map describes: what physics collides with, and what the
/// mesh is built from, so the two cannot disagree about a cell.
fn grid_of(map: &Tilemap, set: &TileSet) -> balaur_core::tiles::TileGrid {
    balaur_core::tiles::TileGrid {
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
fn write_cell(map: &mut Tilemap, column: i32, row: i32, tile: Option<u32>) -> bool {
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
fn grow_to(map: &mut Tilemap, column: i32, row: i32) {
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
fn grid_index(map: &Tilemap, column: i32, row: i32) -> Option<(usize, usize)> {
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
        let Ok(map) = world.get::<&Tilemap>(entity) else {
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
    let mut world = eng.world_mut();
    let Some(set) = set else {
        let _ = world.remove_one::<balaur_core::tiles::TileGrid>(entity);
        return;
    };
    let grid = grid_of(
        &Tilemap {
            tileset,
            cells: toml::Value::Boolean(false),
            material: String::new(),
            grid: rows,
            origin,
            flags,
            terrain: Vec::new(),
            seed: 0,
            pixels_per_unit: ppu,
            version,
        },
        &set,
    );
    if let Ok(mut current) = world.get::<&mut balaur_core::tiles::TileGrid>(entity) {
        *current = grid;
        return;
    }
    let _ = world.insert_one(entity, grid);
}

fn set_tilemap(eng: &Engine, entity: Entity, next: Tilemap) -> Result<()> {
    // The world's borrow ends before the grid is mirrored: `sync_grid` takes
    // it again, and it loads an asset in between.
    let fresh = {
        let world = eng.world_mut();
        if let Ok(mut map) = world.get::<&mut Tilemap>(entity) {
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
        eng.world_mut()
            .insert_one(entity, next)
            .map_err(|_| anyhow!("node is dead"))?;
    }
    sync_grid(eng, entity);
    Ok(())
}

/// What the `tilemap` key writes on a node, and what it resolves first.
fn apply_tilemap(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let text = |key: &str| {
        params
            .get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let tileset = text("tileset");
    let cells = params
        .get(k::CELLS)
        .cloned()
        .unwrap_or_else(|| toml::Value::String(String::new()));
    let grid = parse_cells_value(&cells)?;
    let material = text("material");
    let ppu = params
        .get(k::PIXELS_PER_UNIT)
        .and_then(balaur_core::components::as_f64)
        .unwrap_or(f64::from(crate::DEFAULT_PIXELS_PER_UNIT)) as f32;
    // Checked here so a bad definition is reported where it was
    // written, but only warned: one bad asset must not kill the scene.
    if !tileset.is_empty()
        && let Err(why) = balaur_core::assets::load_typed::<TileSet>(eng, &tileset)
    {
        tracing::warn!("tilemap tileset '{tileset}': {why:#}");
    }
    let origin = params.get(k::ORIGIN).map_or([0, 0], |value| {
        let at = |i: usize| {
            value
                .as_array()
                .and_then(|pair| pair.get(i))
                .and_then(balaur_core::components::as_f64)
                .unwrap_or(0.0) as i32
        };
        [at(0), at(1)]
    });
    let flags = parse_flags(params.get(k::FLAGS))?;
    let terrain = match params.get(k::TERRAIN) {
        Some(value) => parse_cells_value(value)?,
        None => Vec::new(),
    };
    let seed = params
        .get(k::SEED)
        .and_then(toml::Value::as_integer)
        .unwrap_or(0) as u64;
    let mut next = Tilemap {
        tileset,
        cells,
        material,
        grid,
        origin,
        flags,
        terrain,
        seed,
        pixels_per_unit: ppu.max(0.01),
        version: 0,
    };
    resolve_all(eng, &mut next);
    set_tilemap(eng, entity, next)
}

/// The `tilemap` component. Writes a [`Tilemap`] on the node; the kiss3d
/// backend mirrors it as one atlas-textured mesh, rebuilt only when
/// [`Tilemap::version`] moves — a tilemap is static between edits.
pub(crate) fn register_tilemap_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "tilemap",
        ComponentDef {
            doc: "A grid of tiles cut from one `tileset` atlas and centred on the node, one character per cell, drawn at `pixels_per_unit` tile-texture pixels per world unit.",
            schema: ComponentDef::parse_schema(
                "tilemap",
                &balaur_core::components::ComponentDef::schema(&[
                    (k::TILESET, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "The tileset naming the texture and tile grid" }}"#, crate::tilemap::TILESET_ASSET_TYPE)),
                    (k::CELLS, r#"{ type = "string", default = "", description = "Rows of tile characters, one row per line: . is empty, 0-9 then a-z index into the tileset. Also accepted: a list of rows of tile ids, -1 for empty, for a tileset past 36 tiles" }"#),
                    (k::PIXELS_PER_UNIT, r#"{ type = "float", default = 100.0, min = 0.01, description = "Tile-texture pixels per world unit" }"#),
                    (k::ORIGIN, r#"{ type = "vec2", default = [0.0, 0.0], description = "The column and row of the first cell: a map grows in any direction by moving this, and cell 0,0 always has its top-left corner on the node" }"#),
                    (k::FLAGS, r#"{ type = "string", default = "", description = "How each cell is turned, as rows of numbers beside `cells`: 1 mirrors it left to right, 2 top to bottom, 4 across its diagonal" }"#),
                    (k::TERRAIN, r#"{ type = "string", default = "", description = "What was painted, as rows of terrain values, when the map autotiles: the cells are resolved from this through the tileset's rules" }"#),
                    (k::SEED, r#"{ type = "int", default = 0, min = 0, description = "Which way the variation falls where a rule offers alternates; the same seed lays a map out the same way every time" }"#),
                    (k::MATERIAL, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "The material the whole map draws with; empty draws with the built-in one" }}"#, crate::material::MATERIAL_ASSET_TYPE)),
                ]),
            ),
            tags: &[words::ORTHOGRAPHIC, "render"],
            expects: &[],
            apply: Box::new(apply_tilemap),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Tilemap>(entity);
                let _ = world.remove_one::<balaur_core::tiles::TileGrid>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let map = world.get::<&Tilemap>(entity).ok()?;
                let mut out = toml::map::Map::new();
                out.insert("tileset".into(), toml::Value::String(map.tileset.clone()));
                out.insert(k::CELLS.into(), map.cells.clone());
                out.insert("material".into(), toml::Value::String(map.material.clone()));
                out.insert(
                    k::PIXELS_PER_UNIT.into(),
                    toml::Value::Float(f64::from(map.pixels_per_unit)),
                );
                out.insert(
                    k::ORIGIN.into(),
                    toml::Value::Array(
                        map.origin
                            .iter()
                            .map(|at| toml::Value::Integer(i64::from(*at)))
                            .collect(),
                    ),
                );
                if !map.terrain.is_empty() {
                    out.insert(k::TERRAIN.into(), terrain_value(&map.terrain));
                    out.insert(
                        k::SEED.into(),
                        toml::Value::Integer(i64::try_from(map.seed).unwrap_or(i64::MAX)),
                    );
                }
                if !map.flags.is_empty() {
                    out.insert(
                        k::FLAGS.into(),
                        toml::Value::Array(
                            map.flags
                                .iter()
                                .map(|line| {
                                    toml::Value::Array(
                                        line.iter()
                                            .map(|bits| toml::Value::Integer(i64::from(*bits)))
                                            .collect(),
                                    )
                                })
                                .collect(),
                        ),
                    );
                }
                Some(toml::Value::Table(out))
            }),
        },
    );
}

/// What a map painted by terrain answers: the values, the rules' output, and
/// what a tile carries. Split from `install_tilemap_api` under `MAX_FN_LINES`.
pub(crate) fn install_tilemap_terrain_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_terrain", &["tilemap"], "(x: int, y: int, terrain: int)", "Paint a terrain value at a column and row and let the tileset's rules pick the tiles, for that cell and the ring around it; below zero clears it."),
        ("terrain", &["tilemap"], "(x: int, y: int) -> int", "The terrain value painted at a column and row, or -1 where nothing was painted."),
        ("tile_data", &["tilemap"], "(x: int, y: int)", "What the tileset says about the tile at a column and row -- its `[tiles.<id>.data]` table -- or nil where the cell is empty or the tile carries none."),
    ]);
    m.function(
        "tile_data",
        |eng: &Engine, (node, x, y): (balaur_script::NodeId, i64, i64)| {
            let entity = balaur_core::entity_of(node)?;
            let world = eng.world();
            let map = world
                .get::<&Tilemap>(entity)
                .map_err(|_| anyhow!("the node carries no tilemap"))?;
            let grid = balaur_core::tiles::TileGrid {
                tileset: map.tileset.clone(),
                rows: map.grid.clone(),
                origin: map.origin,
                ..Default::default()
            };
            let tileset = map.tileset.clone();
            drop(map);
            drop(world);
            let set = balaur_core::assets::load_typed::<TileSet>(eng, &tileset)?;
            let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) else {
                return Ok(balaur_script::Value::Nil);
            };
            Ok(match grid.data_at(&set, x, y) {
                Some(data) => balaur_core::node_api::from_toml(data)?,
                None => balaur_script::Value::Nil,
            })
        },
    );
    m.function(
        "terrain",
        |eng: &Engine, (node, x, y): (balaur_script::NodeId, i64, i64)| {
            let entity = balaur_core::entity_of(node)?;
            let world = eng.world();
            let map = world
                .get::<&Tilemap>(entity)
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
pub(crate) fn install_tilemap_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_cell", &["tilemap"], "(x: int, y: int, tile: int)", "Put one tile at a column and row; a tile below zero clears the cell, and a cell outside the map grows it in that direction. The mesh rebuilds on the next frame."),
        ("cell", &["tilemap"], "(x: int, y: int) -> int", "The tile at a column and row, or -1 for an empty cell or one past the edge."),
    ]);
    m.function(
        "set_cell",
        |eng: &Engine, (node, x, y, tile): (balaur_script::NodeId, i64, i64, i64)| {
            let entity = balaur_core::entity_of(node)?;
            let (x, y) = (
                i32::try_from(x).map_err(|_| anyhow!("that column is too far out"))?,
                i32::try_from(y).map_err(|_| anyhow!("that row is too far out"))?,
            );
            let changed = {
                let world = eng.world();
                let mut map = world
                    .get::<&mut Tilemap>(entity)
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
            }
            Ok(())
        },
    );
    m.function(
        "set_terrain",
        |eng: &Engine, (node, x, y, terrain): (balaur_script::NodeId, i64, i64, i64)| {
            let entity = balaur_core::entity_of(node)?;
            let (x, y) = (
                i32::try_from(x).map_err(|_| anyhow!("that column is too far out"))?,
                i32::try_from(y).map_err(|_| anyhow!("that row is too far out"))?,
            );
            let changed = {
                let world = eng.world();
                let mut map = world
                    .get::<&mut Tilemap>(entity)
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
            }
            Ok(())
        },
    );
    m.function(
        "cell",
        |eng: &Engine, (node, x, y): (balaur_script::NodeId, i64, i64)| {
            let entity = balaur_core::entity_of(node)?;
            let world = eng.world();
            let map = world
                .get::<&Tilemap>(entity)
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

/// How many cells one chunk of the mesh covers along each edge.
///
/// A map is one mesh node per chunk, so writing a cell rebuilds the buffer
/// around it rather than the whole level: a paint stroke costs its stroke.
#[cfg(feature = "kiss3d")]
const CHUNK: i32 = 32;

#[cfg(feature = "kiss3d")]
pub(crate) struct TilemapSlot {
    /// The map's own node. Every chunk is a child of it, so the map is posed
    /// once however many chunks it happens to be made of.
    node: kiss3d::scene::SceneNode2d,
    chunks: std::collections::HashMap<[i32; 2], Chunk>,
    version: u64,
    /// Which frame the map's animated tiles were built at. A picture, not a
    /// rule: it moves on the frame clock and stays out of the digest.
    frame: i64,
}

/// One block of the map's mesh, and what its cells were when it was built.
#[cfg(feature = "kiss3d")]
struct Chunk {
    node: kiss3d::scene::SceneNode2d,
    digest: u64,
}

/// Mirror [`Tilemap`] + `GlobalTransform` into the kiss3d 2D scene graph.
///
/// Each map is one mesh node (kiss3d's own `Tilemap`, a quad per non-empty
/// cell with the same anti-bleed UV inset `sync_sprite_uvs` uses), built once
/// per component application and re-posed each frame.
#[cfg(feature = "kiss3d")]
pub(crate) fn sync_tilemaps(
    app: &balaur_core::App,
    scene: &mut kiss3d::scene::SceneNode2d,
    slots: &mut std::collections::HashMap<Entity, TilemapSlot>,
    materials: &mut crate::shader_material::MaterialCache,
    reloaded: bool,
) {
    use balaur_core::{GlobalAppearance, GlobalTransform};

    let channel = crate::debug_view::channel_view(&app.engine);
    let world = app.engine.world();
    let mut seen: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    for (entity, map, global) in &mut world.query::<(Entity, &Tilemap, &GlobalTransform)>() {
        seen.insert(entity);
        // Every map is built from a file — the tileset document and the
        // atlas it names — so a reload rebuilds all of them.
        let frame = balaur_core::assets::load_typed::<TileSet>(&app.engine, &map.tileset)
            .map_or(0, |set| animation_frame(&set, app.engine.time() as f32));
        let rebuild = reloaded
            || slots
                .get(&entity)
                .is_none_or(|slot| slot.version != map.version || slot.frame != frame);
        if reloaded && let Some(mut old) = slots.remove(&entity) {
            // Every map is built from files, so a reload starts them over.
            old.node.detach();
        }
        let slot = slots.entry(entity).or_insert_with(|| {
            let node = kiss3d::scene::SceneNode2d::empty();
            scene.add_child(node.clone());
            TilemapSlot {
                node,
                chunks: std::collections::HashMap::new(),
                version: map.version,
                frame,
            }
        });
        if rebuild {
            // A failed build leaves the chunks it had, so a missing texture is
            // reported once rather than sixty times a second.
            if let Err(err) = rebuild_chunks(app, slot, map, frame) {
                tracing::error!("tilemap: {err:#}");
                // Nothing to draw rather than the last good mesh: a map whose
                // tileset went missing is missing.
                for (_, mut chunk) in slot.chunks.drain() {
                    chunk.node.detach();
                }
            }
            let material = materials.for_node(app, &map.material, &channel);
            for chunk in slot.chunks.values_mut() {
                if let Some(material) = material.clone() {
                    chunk.node.set_material(material);
                }
            }
            slot.version = map.version;
            slot.frame = frame;
        }
        let (angle, _, _) = global.rotation.to_euler(glamx::EulerRot::ZYX);
        let visible = world
            .get::<&GlobalAppearance>(entity)
            .is_ok_and(|a| a.visible);
        slot.node
            .set_position(glamx::Vec2::new(global.position.x, global.position.y))
            .set_rotation(angle)
            .set_local_scale(global.scale.x, global.scale.y)
            .set_visible(visible);
    }
    slots.retain(|entity, slot| {
        if seen.contains(entity) {
            true
        } else {
            slot.node.detach();
            false
        }
    });
}

/// Rebuild the chunks whose cells moved, and drop the ones that emptied.
///
/// The map's own node is not touched: chunks come and go under it, so a map
/// keeps its place in the scene however it is edited.
#[cfg(feature = "kiss3d")]
fn rebuild_chunks(
    app: &balaur_core::App,
    slot: &mut TilemapSlot,
    map: &Tilemap,
    frame: i64,
) -> Result<()> {
    let eng = &app.engine;
    let tileset = balaur_core::assets::load_typed::<TileSet>(eng, &map.tileset)?;
    let bytes = eng
        .resource::<balaur_core::project::ProjectFiles>()
        .borrow()
        .read(&tileset.texture)?;
    let (width, height) = crate::texture::image_size(&bytes, &tileset.texture)?;
    let sheet = glamx::Vec2::new(width as f32, height as f32);
    let grid = grid_of(map, &tileset);
    let wanted = chunks_of(&grid, &tileset, eng.time() as f32, frame);
    for (key, (cells, digest)) in &wanted {
        if slot
            .chunks
            .get(key)
            .is_some_and(|held| held.digest == *digest)
        {
            continue;
        }
        if let Some(mut old) = slot.chunks.remove(key) {
            old.node.detach();
        }
        let mut node = build_chunk_node(&tileset, &grid, sheet, cells);
        slot.node.add_child(node.clone());
        crate::texture::attach_texture_2d(eng, &mut node, &tileset.texture);
        slot.chunks.insert(
            *key,
            Chunk {
                node,
                digest: *digest,
            },
        );
    }
    slot.chunks.retain(|key, chunk| {
        if wanted.contains_key(key) {
            return true;
        }
        chunk.node.detach();
        false
    });
    Ok(())
}

/// The cells of every chunk the map covers, each with a digest of what its
/// cells are: a chunk whose digest has not moved keeps the mesh it has.
///
/// The digest carries the cell size and the animation frame too, so a map
/// that was rescaled or a tile that turned over rebuilds like an edit.
#[cfg(feature = "kiss3d")]
#[allow(clippy::type_complexity, reason = "one map of one thing, named below")]
fn chunks_of(
    grid: &balaur_core::tiles::TileGrid,
    set: &TileSet,
    seconds: f32,
    frame: i64,
) -> std::collections::BTreeMap<[i32; 2], (Vec<(i32, i32, u32, u8)>, u64)> {
    let seed = [
        grid.tile_world[0].to_bits().into(),
        grid.tile_world[1].to_bits().into(),
        frame as u64,
    ]
    .into_iter()
    .fold(0xcbf2_9ce4_8422_2325, mix);
    let mut out: std::collections::BTreeMap<[i32; 2], (Vec<(i32, i32, u32, u8)>, u64)> =
        std::collections::BTreeMap::new();
    for (column, row, id) in grid.filled() {
        let id = animated(set, id, seconds);
        let flags = grid.cell_flags(column, row);
        let key = [column.div_euclid(CHUNK), row.div_euclid(CHUNK)];
        let entry = out.entry(key).or_insert_with(|| (Vec::new(), seed));
        entry.0.push((column, row, id, flags));
        for part in [
            column as i64 as u64,
            row as i64 as u64,
            id.into(),
            flags.into(),
        ] {
            entry.1 = mix(entry.1, part);
        }
    }
    out
}

/// One more number folded into a digest.
#[cfg(feature = "kiss3d")]
fn mix(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(0x0100_0000_01b3)
}

/// One mesh node for one chunk of the map, cells indexing the tileset atlas.
///
/// Built here rather than by the fork's uniform sheet, which has no gutter to
/// skip and no way to turn a cell: a quad per filled cell, placed by the
/// grid's own maths so the map is anchored on its node.
#[cfg(feature = "kiss3d")]
fn build_chunk_node(
    tileset: &TileSet,
    grid: &balaur_core::tiles::TileGrid,
    sheet: glamx::Vec2,
    cells: &[(i32, i32, u32, u8)],
) -> kiss3d::scene::SceneNode2d {
    use kiss3d::resource::GpuMesh2d;

    // A hair off each edge of a tile's rect, or a neighbouring tile bleeds in
    // at some zoom levels.
    let inset = glamx::Vec2::new(0.05 / sheet.x, 0.05 / sheet.y);
    let mut coords: Vec<glamx::Vec2> = Vec::new();
    let mut uvs: Vec<glamx::Vec2> = Vec::new();
    let mut faces: Vec<[u32; 3]> = Vec::new();
    for (column, row, id, flags) in cells.iter().copied() {
        let centre = grid.cell_centre(column, row);
        let half = glamx::Vec2::new(grid.tile_world[0], grid.tile_world[1]) / 2.0;
        let base = coords.len() as u32;
        coords.extend([
            centre + glamx::Vec2::new(-half.x, half.y),
            centre + glamx::Vec2::new(half.x, half.y),
            centre + glamx::Vec2::new(half.x, -half.y),
            centre + glamx::Vec2::new(-half.x, -half.y),
        ]);
        uvs.extend(tile_uvs(tileset, id, sheet, inset, flags));
        faces.push([base, base + 1, base + 2]);
        faces.push([base, base + 2, base + 3]);
    }
    let mesh = GpuMesh2d::new(coords, faces, Some(uvs), true);
    kiss3d::scene::SceneNode2d::mesh(
        std::rc::Rc::new(std::cell::RefCell::new(mesh)),
        glamx::Vec2::ONE,
    )
}

/// The frame an animated tile is showing; a still tile is itself.
#[cfg(feature = "kiss3d")]
fn animated(set: &TileSet, id: u32, seconds: f32) -> u32 {
    set.tile(id)
        .and_then(|tile| tile.animation.as_ref())
        .map_or(id, |animation| animation.frame_at(seconds))
}

/// Which frame every animated tile in a set is on, as one number: the mesh is
/// rebuilt when it moves, and not otherwise.
#[cfg(feature = "kiss3d")]
fn animation_frame(set: &TileSet, seconds: f32) -> i64 {
    set.tiles
        .values()
        .filter_map(|tile| tile.animation.as_ref())
        .map(|animation| (seconds * animation.fps.max(0.0)) as i64)
        .fold(0, |sum, step| sum.wrapping_mul(31).wrapping_add(step))
}

/// The four corners of a tile on the sheet, in the order the quad above
/// wants them, turned by the cell's flags.
#[cfg(feature = "kiss3d")]
fn tile_uvs(
    set: &TileSet,
    id: u32,
    sheet: glamx::Vec2,
    inset: glamx::Vec2,
    flags: u8,
) -> [glamx::Vec2; 4] {
    let [x, y, w, h] = set.tile_rect(id);
    let min = glamx::Vec2::new(x / sheet.x, y / sheet.y) + inset;
    let max = glamx::Vec2::new((x + w) / sheet.x, (y + h) / sheet.y) - inset;
    let mut corners = [
        glamx::Vec2::new(min.x, min.y),
        glamx::Vec2::new(max.x, min.y),
        glamx::Vec2::new(max.x, max.y),
        glamx::Vec2::new(min.x, max.y),
    ];
    if flags & balaur_core::tiles::TRANSPOSE != 0 {
        corners.swap(1, 3);
    }
    if flags & balaur_core::tiles::FLIP_X != 0 {
        corners.swap(0, 1);
        corners.swap(2, 3);
    }
    if flags & balaur_core::tiles::FLIP_Y != 0 {
        corners.swap(0, 3);
        corners.swap(1, 2);
    }
    corners
}

#[cfg(test)]
mod tests {
    use super::{Tilemap, grid_index, write_flags, write_terrain};

    fn map() -> Tilemap {
        Tilemap {
            tileset: "set".into(),
            cells: toml::Value::String(String::new()),
            material: String::new(),
            grid: vec![vec![Some(0)]],
            origin: [0, 0],
            flags: Vec::new(),
            terrain: Vec::new(),
            seed: 0,
            pixels_per_unit: 100.0,
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

        let at = |map: &Tilemap, column, row| {
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
