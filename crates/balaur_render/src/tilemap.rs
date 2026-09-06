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

/// Mirror the map into the [`TileGrid`] core carries, which is what physics
/// reads: it may not see a render component, and both need the same cells.
///
/// A map whose tileset will not load carries no grid, so nothing collides
/// with cells nobody can size.
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
    for row in 0..painted.len() as i32 {
        for column in 0..painted.first().map_or(0, Vec::len) as i32 {
            let (x, y) = (origin[0] + column, origin[1] + row);
            match balaur_core::tiles::resolve(&set.rules, &value_at, &inside, x, y, map.seed) {
                Some((tile, flags)) => {
                    write_cell(map, x, y, Some(tile));
                    write_flags(map, x, y, flags);
                }
                None => {
                    write_cell(map, x, y, None);
                    write_flags(map, x, y, 0);
                }
            }
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
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (x, y) = (column + dx, row + dy);
            let resolved =
                balaur_core::tiles::resolve(&set.rules, &value_at, &inside, x, y, map.seed);
            match resolved {
                Some((tile, flags)) => {
                    write_cell(map, x, y, Some(tile));
                    write_flags(map, x, y, flags);
                }
                None if value_at(x, y).is_none() => {
                    write_cell(map, x, y, None);
                    write_flags(map, x, y, 0);
                }
                None => {}
            }
        }
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
    let columns = map.grid.iter().map(Vec::len).max().unwrap_or(0) as i32;
    let rows = map.grid.len() as i32;
    let left = (map.origin[0] - column).max(0);
    let up = (map.origin[1] - row).max(0);
    let right = (column - (map.origin[0] + columns - 1)).max(0);
    let down = (row - (map.origin[1] + rows - 1)).max(0);
    if left > 0 || right > 0 {
        for line in &mut map.grid {
            let mut grown = vec![None; left as usize];
            grown.append(line);
            grown.resize((columns + left + right) as usize, None);
            *line = grown;
        }
        map.origin[0] -= left;
    }
    let width = map.grid.iter().map(Vec::len).max().unwrap_or(0);
    if up > 0 {
        let mut grown = vec![vec![None; width]; up as usize];
        grown.append(&mut map.grid);
        map.grid = grown;
        map.origin[1] -= up;
    }
    for _ in 0..down {
        map.grid.push(vec![None; width]);
    }
}

/// Where a coordinate sits in the stored rows.
fn grid_index(map: &Tilemap, column: i32, row: i32) -> Option<(usize, usize)> {
    let x = usize::try_from(column - map.origin[0]).ok()?;
    let y = usize::try_from(row - map.origin[1]).ok()?;
    (y < map.grid.len() && x < map.grid[y].len()).then_some((x, y))
}

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
    let grid = balaur_core::tiles::TileGrid {
        tileset,
        rows,
        origin,
        flags,
        tile_world: [set.tile_size[0] / ppu, set.tile_size[1] / ppu],
        version,
    };
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
        let mut world = eng.world_mut();
        if let Ok(mut map) = world.get::<&mut Tilemap>(entity) {
            let changed = map.tileset != next.tileset
                || map.grid != next.grid
                || map.material != next.material
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
            apply: Box::new(|eng, entity, params| {
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
                    .unwrap_or(f64::from(crate::DEFAULT_PIXELS_PER_UNIT))
                    as f32;
                // Checked here so a bad definition is reported where it was
                // written, but only warned: one bad asset must not kill the scene.
                if !tileset.is_empty()
                    && let Err(why) = balaur_core::assets::load_typed::<TileSet>(eng, &tileset) {
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
            }),
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
                    out.insert(k::SEED.into(), toml::Value::Integer(map.seed as i64));
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

/// `render.set_cell` and `render.cell`: one tile at a time, for a map a
/// script edits as it plays. A write past the grid's edge grows it.
pub(crate) fn install_tilemap_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_cell", &["tilemap"], "(x: int, y: int, tile: int)", "Put one tile at a column and row; a tile below zero clears the cell, and a cell outside the map grows it in that direction. The mesh rebuilds on the next frame."),
        ("cell", &["tilemap"], "(x: int, y: int) -> int", "The tile at a column and row, or -1 for an empty cell or one past the edge."),
        ("set_terrain", &["tilemap"], "(x: int, y: int, terrain: int)", "Paint a terrain value at a column and row and let the tileset's rules pick the tiles, for that cell and the ring around it; below zero clears it."),
        ("terrain", &["tilemap"], "(x: int, y: int) -> int", "The terrain value painted at a column and row, or -1 where nothing was painted."),
        ("tile_data", &["tilemap"], "(x: int, y: int)", "What the tileset says about the tile at a column and row -- its `[tiles.<id>.data]` table -- or nil where the cell is empty or the tile carries none."),
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

#[cfg(feature = "kiss3d")]
pub(crate) struct TilemapSlot {
    node: kiss3d::scene::SceneNode2d,
    version: u64,
    /// Which frame the map's animated tiles were built at. A picture, not a
    /// rule: it moves on the frame clock and stays out of the digest.
    frame: i64,
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
        if rebuild {
            if let Some(mut old) = slots.remove(&entity) {
                old.node.detach();
            }
            // A failed build still fills the slot (with a bare group), so a
            // missing texture is reported once, not sixty times a second.
            let mut node = build_map_node(&app.engine, map).unwrap_or_else(|err| {
                tracing::error!("tilemap: {err:#}");
                kiss3d::scene::SceneNode2d::empty()
            });
            if let Some(material) = materials.for_node(app, &map.material, &channel) {
                node.set_material(material);
            }
            scene.add_child(node.clone());
            slots.insert(
                entity,
                TilemapSlot {
                    node,
                    frame,
                    version: map.version,
                },
            );
        }
        // The block above inserts the slot when it is missing.
        let slot = slots.get_mut(&entity).unwrap();
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

/// One mesh node for the whole map, cells indexing the tileset atlas.
///
/// Built here rather than by the fork's uniform sheet, which has no gutter to
/// skip and no way to turn a cell: a quad per filled cell, placed by the
/// grid's own maths so the map is anchored on its node.
#[cfg(feature = "kiss3d")]
fn build_map_node(eng: &Engine, map: &Tilemap) -> Result<kiss3d::scene::SceneNode2d> {
    use kiss3d::resource::GpuMesh2d;

    let tileset = balaur_core::assets::load_typed::<TileSet>(eng, &map.tileset)?;
    let bytes = eng
        .resource::<balaur_core::project::ProjectFiles>()
        .borrow()
        .read(&tileset.texture)?;
    let (width, height) = crate::texture::image_size(&bytes, &tileset.texture)?;
    let grid = grid_of(map, &tileset);
    let sheet = glamx::Vec2::new(width as f32, height as f32);
    // A hair off each edge of a tile's rect, or a neighbouring tile bleeds in
    // at some zoom levels.
    let inset = glamx::Vec2::new(0.05 / sheet.x, 0.05 / sheet.y);
    let mut coords: Vec<glamx::Vec2> = Vec::new();
    let mut uvs: Vec<glamx::Vec2> = Vec::new();
    let mut faces: Vec<[u32; 3]> = Vec::new();
    let seconds = eng.time() as f32;
    for (column, row, id) in grid.filled() {
        let id = animated(&tileset, id, seconds);
        let centre = grid.cell_centre(column, row);
        let half = glamx::Vec2::new(grid.tile_world[0], grid.tile_world[1]) / 2.0;
        let base = coords.len() as u32;
        coords.extend([
            centre + glamx::Vec2::new(-half.x, half.y),
            centre + glamx::Vec2::new(half.x, half.y),
            centre + glamx::Vec2::new(half.x, -half.y),
            centre + glamx::Vec2::new(-half.x, -half.y),
        ]);
        uvs.extend(tile_uvs(
            &tileset,
            id,
            sheet,
            inset,
            grid.cell_flags(column, row),
        ));
        faces.push([base, base + 1, base + 2]);
        faces.push([base, base + 2, base + 3]);
    }
    let mesh = GpuMesh2d::new(coords, faces, Some(uvs), true);
    let mut node = kiss3d::scene::SceneNode2d::mesh(
        std::rc::Rc::new(std::cell::RefCell::new(mesh)),
        glamx::Vec2::ONE,
    );
    crate::texture::attach_texture_2d(eng, &mut node, &tileset.texture);
    Ok(node)
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
