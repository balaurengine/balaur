//! What a tile is, where a cell sits, and which cells are solid.
//!
//! The `tileset` asset's parse and the grid a `tilemap` carries live here, in
//! core, because two backends read them: the renderer builds the mesh and
//! physics builds the collider, and neither may depend on the other. Nothing
//! here touches a GPU or rapier, so a headless test asserts a cell's corner.

use std::collections::BTreeMap;

use anyhow::{Result, anyhow, bail};
use glamx::Vec2;

use crate::components::as_f64;

pub const TILESET_ASSET_TYPE: &str = "tileset";

/// What a tile collides as.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Collision {
    /// The tile is passed through; the default, and what every tile was
    /// before a tileset could say otherwise.
    #[default]
    None,
    /// The whole cell is solid. These cells become one voxel collider, so a
    /// body sliding along a wall of them cannot catch on a seam.
    Full,
    /// Polygons in tile pixels, y down from the tile's top-left corner: a
    /// slope, a half-height block, a ledge.
    Shape(Vec<Vec<[f32; 2]>>),
}

/// What a `[tiles.<id>]` table says about one tile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tile {
    pub collision: Collision,
    /// A platform a body passes through from below. Its own collider, since
    /// the shape carries no data per cell.
    pub one_way: bool,
}

impl Tile {
    /// Which collider a tile belongs in, or `None` for one that collides
    /// with nothing.
    #[must_use]
    pub fn group(&self) -> Option<Group> {
        match (&self.collision, self.one_way) {
            (Collision::None, _) => None,
            (_, true) => Some(Group::OneWay),
            (_, false) => Some(Group::Solid),
        }
    }
}

/// The colliders a map builds. The voxel shape carries no data per cell, so
/// each behaviour is a collider of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Solid,
    OneWay,
}

/// A parsed `tileset`: how the image is cut, and what each tile is.
#[derive(Clone, Debug, PartialEq)]
pub struct TileSet {
    /// Project-relative path to the atlas image.
    pub texture: String,
    /// One tile in texture pixels.
    pub tile_size: [f32; 2],
    /// Gutter between tiles, in pixels.
    pub spacing: f32,
    /// Border around the whole sheet, in pixels.
    pub margin: f32,
    /// Tiles per texture row.
    pub columns: u32,
    /// Keyed by tile id, ordered so two runs build a collider the same way.
    pub tiles: BTreeMap<u32, Tile>,
}

impl TileSet {
    #[must_use]
    pub fn tile(&self, id: u32) -> Option<&Tile> {
        self.tiles.get(&id)
    }

    /// Which collider group a tile id belongs in, if any.
    #[must_use]
    pub fn group(&self, id: u32) -> Option<Group> {
        self.tile(id).and_then(Tile::group)
    }

    /// The pixel rect of a tile on the sheet: `[x, y, w, h]`.
    #[must_use]
    pub fn tile_rect(&self, id: u32) -> [f32; 4] {
        let columns = self.columns.max(1);
        let (column, row) = (id % columns, id / columns);
        let [w, h] = self.tile_size;
        [
            self.margin + column as f32 * (w + self.spacing),
            self.margin + row as f32 * (h + self.spacing),
            w,
            h,
        ]
    }
}

/// A tileset definition table.
pub fn parse_tileset(value: &toml::Value) -> Result<TileSet> {
    let texture = value
        .get("texture")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| anyhow!("a tileset needs a `texture` string naming its image"))?
        .to_string();
    if texture.is_empty() {
        bail!("a tileset's `texture` names no image");
    }
    let tile_size = tile_size(value)?;
    let columns = value
        .get("columns")
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| anyhow!("a tileset needs an integer `columns` count of tiles per row"))?;
    if columns < 1 {
        bail!("a tileset's `columns` must be at least 1, not {columns}");
    }
    let gap = |key: &str| -> Result<f32> {
        let value = value.get(key).and_then(as_f64).unwrap_or(0.0) as f32;
        if value < 0.0 {
            bail!("a tileset's `{key}` must not be negative, and is {value}");
        }
        Ok(value)
    };
    Ok(TileSet {
        texture,
        tile_size,
        spacing: gap("spacing")?,
        margin: gap("margin")?,
        columns: columns as u32,
        tiles: parse_tiles(value)?,
    })
}

/// `tile_size = 16` or `tile_size = [16, 24]`; square is what most sheets are.
fn tile_size(value: &toml::Value) -> Result<[f32; 2]> {
    let size = value
        .get("tile_size")
        .ok_or_else(|| anyhow!("a tileset needs a `tile_size` in pixels per tile"))?;
    let pair = match size {
        toml::Value::Array(pair) if pair.len() == 2 => {
            let at = |i: usize| pair.get(i).and_then(as_f64).unwrap_or(0.0) as f32;
            [at(0), at(1)]
        }
        other => {
            let edge = as_f64(other).unwrap_or(0.0) as f32;
            [edge, edge]
        }
    };
    if pair[0] <= 0.0 || pair[1] <= 0.0 {
        bail!("a tileset's `tile_size` must be positive, not {pair:?}");
    }
    Ok(pair)
}

/// The `[tiles.<id>]` tables, if the sheet carries any.
fn parse_tiles(value: &toml::Value) -> Result<BTreeMap<u32, Tile>> {
    let Some(tiles) = value.get("tiles") else {
        return Ok(BTreeMap::new());
    };
    let tiles = tiles
        .as_table()
        .ok_or_else(|| anyhow!("a tileset's `tiles` is a table of tile ids"))?;
    let mut out = BTreeMap::new();
    for (key, table) in tiles {
        let id: u32 = key
            .parse()
            .map_err(|_| anyhow!("'{key}' is not a tile id: a tile is named by its number"))?;
        out.insert(id, parse_tile(id, table)?);
    }
    Ok(out)
}

fn parse_tile(id: u32, value: &toml::Value) -> Result<Tile> {
    let collision = match value.get("collision") {
        None => Collision::None,
        Some(toml::Value::String(word)) => match word.as_str() {
            "full" => Collision::Full,
            "none" => Collision::None,
            other => {
                bail!("tile {id}: `collision` is \"full\", \"none\" or polygons, not \"{other}\"")
            }
        },
        Some(toml::Value::Boolean(solid)) => {
            if *solid {
                Collision::Full
            } else {
                Collision::None
            }
        }
        Some(toml::Value::Array(polygons)) => Collision::Shape(parse_polygons(id, polygons)?),
        Some(other) => bail!("tile {id}: `collision` cannot be {}", other.type_str()),
    };
    Ok(Tile {
        collision,
        one_way: value
            .get("one_way")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
    })
}

fn parse_polygons(id: u32, polygons: &[toml::Value]) -> Result<Vec<Vec<[f32; 2]>>> {
    polygons
        .iter()
        .map(|polygon| {
            let points = polygon
                .as_array()
                .ok_or_else(|| anyhow!("tile {id}: a collision polygon is a list of points"))?;
            if points.len() < 3 {
                bail!("tile {id}: a collision polygon needs at least three points");
            }
            points
                .iter()
                .map(|point| {
                    let pair =
                        point
                            .as_array()
                            .filter(|pair| pair.len() == 2)
                            .ok_or_else(|| {
                                anyhow!(
                                    "tile {id}: a collision point is an [x, y] pair in tile pixels"
                                )
                            })?;
                    let at = |i: usize| pair.get(i).and_then(as_f64).unwrap_or(0.0) as f32;
                    Ok([at(0), at(1)])
                })
                .collect()
        })
        .collect()
}

/// The grid a `tilemap` carries, in the form both backends read.
///
/// Row 0 of `rows` is the row at `origin[1]`, and column 0 the column at
/// `origin[0]`; row numbers count downward, as the text form reads in a scene
/// file. A cell's place is its coordinate alone — cell (0, 0) has its
/// top-left corner on the node — so a map may grow in any direction without
/// moving a tile that is already down.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileGrid {
    /// The `tileset` asset the cells index.
    pub tileset: String,
    /// Tile ids, `None` for an empty cell. Rows are padded to the widest.
    pub rows: Vec<Vec<Option<u32>>>,
    /// The grid coordinate of `rows[0][0]`.
    pub origin: [i32; 2],
    /// How each cell is turned, when any of them is. Empty means none are.
    pub flags: Vec<Vec<u8>>,
    /// One cell in world units.
    pub tile_world: [f32; 2],
    /// Bumped when the content changes, so a backend knows to rebuild.
    pub version: u64,
}

/// A cell drawn mirrored left to right.
pub const FLIP_X: u8 = 1;
/// A cell drawn mirrored top to bottom.
pub const FLIP_Y: u8 = 2;
/// A cell drawn across its diagonal, which with the two flips gives every
/// quarter turn.
pub const TRANSPOSE: u8 = 4;

impl TileGrid {
    #[must_use]
    pub fn columns(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// The half-open range of columns the grid stores.
    #[must_use]
    pub fn column_range(&self) -> (i32, i32) {
        (self.origin[0], self.origin[0] + self.columns() as i32)
    }

    /// The half-open range of rows the grid stores.
    #[must_use]
    pub fn row_range(&self) -> (i32, i32) {
        (self.origin[1], self.origin[1] + self.row_count() as i32)
    }

    /// Where a coordinate sits in `rows`, or `None` for one outside it.
    #[must_use]
    pub fn index_of(&self, column: i32, row: i32) -> Option<(usize, usize)> {
        let column = usize::try_from(column - self.origin[0]).ok()?;
        let row = usize::try_from(row - self.origin[1]).ok()?;
        (column < self.columns() && row < self.row_count()).then_some((column, row))
    }

    /// The tile at a coordinate, or `None` for an empty cell or one outside
    /// the grid.
    #[must_use]
    pub fn cell(&self, column: i32, row: i32) -> Option<u32> {
        let (column, row) = self.index_of(column, row)?;
        self.rows.get(row)?.get(column).copied().flatten()
    }

    /// How the cell at a coordinate is turned.
    #[must_use]
    pub fn cell_flags(&self, column: i32, row: i32) -> u8 {
        let Some((column, row)) = self.index_of(column, row) else {
            return 0;
        };
        self.flags
            .get(row)
            .and_then(|line| line.get(column))
            .copied()
            .unwrap_or(0)
    }

    /// Every cell that holds a tile, in reading order, as coordinates.
    pub fn filled(&self) -> impl Iterator<Item = (i32, i32, u32)> + '_ {
        self.rows.iter().enumerate().flat_map(move |(row, line)| {
            line.iter().enumerate().filter_map(move |(column, cell)| {
                cell.map(|id| {
                    (
                        self.origin[0] + column as i32,
                        self.origin[1] + row as i32,
                        id,
                    )
                })
            })
        })
    }

    /// The centre of a cell in the node's own space.
    #[must_use]
    pub fn cell_centre(&self, column: i32, row: i32) -> Vec2 {
        Vec2::new(
            (column as f32 + 0.5) * self.tile_world[0],
            -(row as f32 + 0.5) * self.tile_world[1],
        )
    }

    /// The cell a point in the node's own space falls in. Outside the grid is
    /// still a cell, which is what lets a painter grow the map by writing
    /// there.
    #[must_use]
    pub fn cell_at(&self, local: Vec2) -> (i32, i32) {
        let column = (local.x / self.tile_world[0]).floor();
        let row = (-local.y / self.tile_world[1]).floor();
        (column as i32, row as i32)
    }

    /// A cell as a voxel-grid key: the same lattice, counted up rather than
    /// down, so a collider needs no offset of its own.
    #[must_use]
    pub fn voxel_key(&self, column: i32, row: i32) -> [i32; 2] {
        [column, -row - 1]
    }

    /// The cells of one collider group, as voxel keys, in reading order so
    /// two runs build the same shape.
    #[must_use]
    pub fn group_cells(&self, set: &TileSet, group: Group) -> Vec<[i32; 2]> {
        self.filled()
            .filter(|(_, _, id)| {
                set.group(*id) == Some(group)
                    && set.tile(*id).map(|tile| &tile.collision) == Some(&Collision::Full)
            })
            .map(|(column, row, _)| self.voxel_key(column, row))
            .collect()
    }

    /// Every cell whose tile draws its own collision polygons, with the
    /// centre of the cell in the node's own space.
    #[must_use]
    pub fn shaped_cells<'a>(&'a self, set: &'a TileSet) -> impl Iterator<Item = (Vec2, &'a Tile)> {
        self.filled().filter_map(move |(column, row, id)| {
            let tile = set.tile(id)?;
            matches!(tile.collision, Collision::Shape(_))
                .then(|| (self.cell_centre(column, row), tile))
        })
    }
}

/// A tile's polygon in world units, around the centre of the cell it sits in.
///
/// Authored in tile pixels with y down from the tile's top-left corner, which
/// is what every tile editor exports.
#[must_use]
pub fn polygon_in_world(
    points: &[[f32; 2]],
    tile_pixels: [f32; 2],
    tile_world: [f32; 2],
) -> Vec<Vec2> {
    let scale = Vec2::new(
        tile_world[0] / tile_pixels[0].max(f32::MIN_POSITIVE),
        tile_world[1] / tile_pixels[1].max(f32::MIN_POSITIVE),
    );
    let half = Vec2::new(tile_world[0], tile_world[1]) / 2.0;
    points
        .iter()
        .map(|[x, y]| Vec2::new(x * scale.x - half.x, half.y - y * scale.y))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(text: &str) -> TileSet {
        parse_tileset(&toml::from_str::<toml::Value>(text).unwrap()).unwrap()
    }

    fn grid(rows: &[&[i32]]) -> TileGrid {
        TileGrid {
            tileset: "set".into(),
            rows: rows
                .iter()
                .map(|row| row.iter().map(|id| u32::try_from(*id).ok()).collect())
                .collect(),
            tile_world: [1.0, 1.0],
            ..TileGrid::default()
        }
    }

    #[test]
    fn a_square_tile_size_is_the_same_as_a_pair() {
        let square = set("texture = \"a.png\"\ntile_size = 16\ncolumns = 4");
        let pair = set("texture = \"a.png\"\ntile_size = [16, 16]\ncolumns = 4");
        assert_eq!(square.tile_size, pair.tile_size);
    }

    #[test]
    fn spacing_and_margin_move_a_tile_along_the_sheet() {
        let set =
            set("texture = \"a.png\"\ntile_size = [16, 16]\ncolumns = 4\nspacing = 2\nmargin = 1");
        assert_eq!(set.tile_rect(0), [1.0, 1.0, 16.0, 16.0]);
        assert_eq!(set.tile_rect(1), [19.0, 1.0, 16.0, 16.0]);
        assert_eq!(set.tile_rect(4), [1.0, 19.0, 16.0, 16.0], "the second row");
    }

    #[test]
    fn a_tile_says_what_it_collides_as() {
        let set = set(
            "texture = \"a.png\"\ntile_size = 16\ncolumns = 4\n\
             [tiles.1]\ncollision = \"full\"\n\
             [tiles.2]\ncollision = \"full\"\none_way = true\n",
        );
        assert_eq!(set.group(1), Some(Group::Solid));
        assert_eq!(set.group(2), Some(Group::OneWay));
        assert_eq!(
            set.group(3),
            None,
            "a tile with no table collides with nothing"
        );
    }

    #[test]
    fn a_cell_and_the_point_in_it_answer_each_other() {
        let map = grid(&[&[0, 0], &[0, 0]]);
        for row in -2..3 {
            for column in -2..3 {
                let centre = map.cell_centre(column, row);
                assert_eq!(
                    map.cell_at(centre),
                    (column, row),
                    "the centre of ({column}, {row}) is in it"
                );
            }
        }
    }

    #[test]
    fn a_cell_stays_where_it_is_when_the_map_grows() {
        let small = grid(&[&[1]]);
        let mut wide = grid(&[&[1, 1], &[1, 1]]);
        wide.origin = [-1, -1];
        assert_eq!(
            small.cell_centre(0, 0),
            wide.cell_centre(0, 0),
            "a map anchored on its node does not slide when it grows"
        );
    }

    #[test]
    fn the_top_row_is_the_top_of_the_map() {
        let map = grid(&[&[0], &[0]]);
        assert!(
            map.cell_centre(0, 0).y > map.cell_centre(0, 1).y,
            "row 0 reads as the top in the file, so it draws at the top"
        );
        assert_eq!(map.voxel_key(0, 0), [0, -1], "and is the higher voxel row");
    }

    #[test]
    fn an_origin_moves_which_coordinates_the_rows_hold() {
        let mut map = grid(&[&[1, 2]]);
        map.origin = [-1, -3];
        assert_eq!(map.cell(-1, -3), Some(1));
        assert_eq!(map.cell(0, -3), Some(2));
        assert_eq!(map.cell(0, 0), None, "nothing is stored there");
    }

    #[test]
    fn only_the_solid_cells_reach_a_collider() {
        let set = set(
            "texture = \"a.png\"\ntile_size = 16\ncolumns = 4\n\
             [tiles.1]\ncollision = \"full\"\n",
        );
        let map = grid(&[&[1, -1], &[1, 1]]);
        let solid = map.group_cells(&set, Group::Solid);
        assert_eq!(solid.len(), 3);
        assert!(solid.contains(&[0, -1]), "the top-left cell");
        assert!(!solid.contains(&[1, -1]), "the empty cell is not solid");
    }

    #[test]
    fn a_tile_polygon_lands_around_the_cell_it_is_in() {
        let square = [[0.0, 0.0], [16.0, 0.0], [16.0, 16.0], [0.0, 16.0]];
        let world = polygon_in_world(&square, [16.0, 16.0], [1.0, 1.0]);
        assert_eq!(world[0], Vec2::new(-0.5, 0.5), "pixels count y down");
        assert_eq!(world[2], Vec2::new(0.5, -0.5));
    }
}
