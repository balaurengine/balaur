//! What a tile is, where a cell sits, and which cells are solid.
//!
//! The `tileset` asset's parse and the grid a `tilemap` carries live here, in
//! core, because two backends read them: the renderer builds the mesh and
//! physics builds the collider, and neither may depend on the other. Nothing
//! here touches a GPU or rapier, so a headless test asserts a cell's corner.

use std::collections::BTreeMap;

use anyhow::{Result, anyhow, bail};
use glamx::Vec2;

pub mod rules;

pub use rules::{Mode, Outside, Quarter, Rule, Terrain, resolve, template};

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

/// Frames a tile cycles through, and how fast.
#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    /// Tile ids, the first drawn first.
    pub frames: Vec<u32>,
    pub fps: f32,
}

impl Animation {
    /// The frame showing at a time, which is a clock reading and not part of
    /// the simulation: an animated tile is a picture, not a rule.
    #[must_use]
    pub fn frame_at(&self, seconds: f32) -> u32 {
        if self.frames.is_empty() {
            return 0;
        }
        let step = (seconds * self.fps.max(0.0)) as i64;
        let count = i64::try_from(self.frames.len()).unwrap_or(1).max(1);
        let at = usize::try_from(step.rem_euclid(count)).unwrap_or(0);
        self.frames[at]
    }
}

/// What a `[tiles.<id>]` table says about one tile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tile {
    pub collision: Collision,
    /// A platform a body passes through from below. Its own collider, since
    /// the shape carries no data per cell.
    pub one_way: bool,
    /// Whether the cell blocks 2D light.
    pub occluder: bool,
    /// The frames this tile cycles through, if it moves at all.
    pub animation: Option<Animation>,
    /// Anything else the game wants to know about the tile.
    pub data: Option<toml::Value>,
}

impl Tile {
    /// Which collider a tile belongs in, or `None` for one that collides with
    /// nothing.
    ///
    /// Only a `full` tile joins a voxel group: a tile drawing its own polygons
    /// gets a collider of its own, so answering a group for it would name a
    /// shape its cells never reach.
    #[must_use]
    pub fn group(&self) -> Option<Group> {
        match (&self.collision, self.one_way) {
            (Collision::Full, false) => Some(Group::Solid),
            (Collision::Full, true) => Some(Group::OneWay),
            (Collision::None | Collision::Shape(_), _) => None,
        }
    }

    /// Whether a body passes through this tile from below, whichever shape it
    /// collides as.
    #[must_use]
    pub fn is_one_way(&self) -> bool {
        self.one_way && self.collision != Collision::None
    }
}

/// The colliders a map builds. The voxel shape carries no data per cell, so
/// each behaviour is a collider of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Solid,
    OneWay,
}

/// How a map's cells are laid out in the world.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// A square grid, which is what most maps are.
    #[default]
    Orthogonal,
    /// Diamonds: a column steps half a cell down, a row half a cell across.
    Isometric,
    /// Pointy-top hexagons in odd-r offset rows, which is how every hex sheet
    /// is drawn.
    Hex,
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
    /// How a map of these tiles is laid out.
    pub layout: Layout,
    /// Keyed by tile id, ordered so two runs build a collider the same way.
    pub tiles: BTreeMap<u32, Tile>,
    /// What a painted value is called, and how its tiles are chosen.
    pub terrains: Vec<Terrain>,
    /// Ordered: the first rule that matches a cell wins.
    pub rules: Vec<Rule>,
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

    /// The terrain a cell drawn in quarters belongs to, found by the one tile
    /// its rules place. `None` for every tile drawn as one quad.
    #[must_use]
    pub fn quarters_terrain(&self, id: u32) -> Option<&Terrain> {
        self.terrains
            .iter()
            .find(|terrain| terrain.mode == Mode::Quarters && terrain.first_tile == id)
    }

    /// The pixel rect of one quarter of a tile: `[x, y, w, h]`, the corner
    /// counted clockwise from the top left.
    #[must_use]
    pub fn quarter_rect(&self, id: u32, corner: usize) -> [f32; 4] {
        let [x, y, w, h] = self.tile_rect(id);
        let (half_w, half_h) = (w / 2.0, h / 2.0);
        let (dx, dy) = match corner {
            0 => (0.0, 0.0),
            1 => (half_w, 0.0),
            2 => (half_w, half_h),
            _ => (0.0, half_h),
        };
        [x + dx, y + dy, half_w, half_h]
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
    let terrains = rules::parse_terrains(value)?;
    let rules = rules::parse_rules(value, &terrains)?;
    Ok(TileSet {
        texture,
        tile_size,
        spacing: gap("spacing")?,
        margin: gap("margin")?,
        columns: columns as u32,
        layout: match value.get("layout").and_then(toml::Value::as_str) {
            None | Some("orthogonal") => Layout::Orthogonal,
            Some("isometric") => Layout::Isometric,
            Some("hex") => Layout::Hex,
            Some(other) => {
                bail!("a tileset's `layout` is orthogonal, isometric or hex, not '{other}'")
            }
        },
        tiles: parse_tiles(value)?,
        terrains,
        rules,
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
    let animation = match value.get("animation") {
        None => None,
        Some(table) => {
            let frames = table
                .get("frames")
                .and_then(toml::Value::as_array)
                .ok_or_else(|| anyhow!("tile {id}: an animation needs a list of `frames`"))?
                .iter()
                .map(|frame| {
                    frame
                        .as_integer()
                        .and_then(|frame| u32::try_from(frame).ok())
                        .ok_or_else(|| anyhow!("tile {id}: a frame is a tile id"))
                })
                .collect::<Result<Vec<_>>>()?;
            if frames.is_empty() {
                bail!("tile {id}: an animation with no frames shows nothing");
            }
            Some(Animation {
                frames,
                fps: table.get("fps").and_then(as_f64).unwrap_or(8.0) as f32,
            })
        }
    };
    Ok(Tile {
        collision,
        one_way: value
            .get("one_way")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        occluder: value
            .get("occluder")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        animation,
        data: value.get("data").cloned(),
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
    /// How the cells are laid out, from the tileset.
    pub layout: Layout,
    /// Bumped when the content changes, so a backend knows to rebuild.
    pub version: u64,
}

/// How many cells a count of stored rows or columns spans, as a coordinate.
/// A map past two billion cells on an edge is not one this counts wrong.
fn span(count: usize) -> i32 {
    i32::try_from(count).unwrap_or(i32::MAX)
}

/// Whether a hex row is one of the ones pushed half a cell along. The number
/// is whole, so this is which of the two it is, not a comparison of floats.
fn odd_row(row: f32) -> bool {
    (row.rem_euclid(2.0) - 1.0).abs() < 0.5
}

/// The four corners of a cell, clockwise from the top left: the order a
/// quarter's tile, its rect and its quad are all counted in.
const CORNERS: [(i32, i32); 4] = [(-1, -1), (1, -1), (1, 1), (-1, 1)];

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
        (self.origin[0], self.origin[0] + span(self.columns()))
    }

    /// The half-open range of rows the grid stores.
    #[must_use]
    pub fn row_range(&self) -> (i32, i32) {
        (self.origin[1], self.origin[1] + span(self.row_count()))
    }

    /// Where a coordinate sits in `rows`, or `None` for one outside it.
    ///
    /// Bounded by the row's own length rather than by the widest of them: the
    /// answer is the same, and a cell lookup does not walk the whole map to
    /// find out how wide it is.
    #[must_use]
    pub fn index_of(&self, column: i32, row: i32) -> Option<(usize, usize)> {
        let column = usize::try_from(column - self.origin[0]).ok()?;
        let row = usize::try_from(row - self.origin[1]).ok()?;
        (column < self.rows.get(row)?.len()).then_some((column, row))
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
                        self.origin[0] + span(column),
                        self.origin[1] + span(row),
                        id,
                    )
                })
            })
        })
    }

    /// The centre of a cell in the node's own space.
    #[must_use]
    pub fn cell_centre(&self, column: i32, row: i32) -> Vec2 {
        let [w, h] = self.tile_world;
        let (column, row) = (column as f32, row as f32);
        match self.layout {
            Layout::Orthogonal => Vec2::new((column + 0.5) * w, -(row + 0.5) * h),
            // A diamond: one step east is half a cell right and half down.
            Layout::Isometric => Vec2::new(
                (column - row) * w / 2.0,
                -(column + row) * h / 2.0 - h / 2.0,
            ),
            // Pointy-top, odd rows pushed half a cell right.
            Layout::Hex => Vec2::new(
                (column + if odd_row(row) { 1.0 } else { 0.5 }) * w,
                -(row * 0.75 + 0.5) * h,
            ),
        }
    }

    /// The cell a point in the node's own space falls in. Outside the grid is
    /// still a cell, which is what lets a painter grow the map by writing
    /// there.
    #[must_use]
    pub fn cell_at(&self, local: Vec2) -> (i32, i32) {
        let [w, h] = self.tile_world;
        match self.layout {
            Layout::Orthogonal => ((local.x / w).floor() as i32, (-local.y / h).floor() as i32),
            Layout::Isometric => {
                let x = local.x / w;
                let y = -local.y / h - 0.5;
                ((y + x).floor() as i32, (y - x).floor() as i32)
            }
            // Near enough for a painter: the nearest row, then the nearest
            // cell along it, which is what a hex hit test comes to.
            Layout::Hex => {
                let row = ((-local.y / h - 0.5) / 0.75).round();
                let shift = if odd_row(row) { 1.0 } else { 0.5 };
                let column = (local.x / w - shift).round();
                (column as i32, row as i32)
            }
        }
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
            .filter(|(_, _, id)| set.group(*id) == Some(group))
            .map(|(column, row, _)| self.voxel_key(column, row))
            .collect()
    }

    /// The edges of the cells that block light, each one an edge no other
    /// occluding cell is behind: the outline of the wall, not its insides.
    #[must_use]
    pub fn occluder_edges(&self, set: &TileSet) -> Vec<[Vec2; 2]> {
        let occludes = |column: i32, row: i32| {
            self.cell(column, row)
                .and_then(|id| set.tile(id))
                .is_some_and(|tile| tile.occluder)
        };
        let mut out = Vec::new();
        for (column, row, _) in self.filled() {
            if !occludes(column, row) {
                continue;
            }
            let centre = self.cell_centre(column, row);
            let half = Vec2::new(self.tile_world[0], self.tile_world[1]) / 2.0;
            let corner = |x: f32, y: f32| centre + Vec2::new(x * half.x, y * half.y);
            let sides = [
                ((0, -1), [corner(-1.0, 1.0), corner(1.0, 1.0)]),
                ((1, 0), [corner(1.0, 1.0), corner(1.0, -1.0)]),
                ((0, 1), [corner(1.0, -1.0), corner(-1.0, -1.0)]),
                ((-1, 0), [corner(-1.0, -1.0), corner(-1.0, 1.0)]),
            ];
            for ((dx, dy), edge) in sides {
                if !occludes(column + dx, row + dy) {
                    out.push(edge);
                }
            }
        }
        out
    }

    /// The tile each corner of a cell takes its picture from, clockwise from
    /// the top left, for a cell whose terrain is drawn in quarters.
    ///
    /// `None` is a cell drawn as one quad, which is every cell of every other
    /// terrain. The answer is the neighbourhood's alone, so a hand-placed
    /// tile autotiles the same way a painted one does.
    #[must_use]
    pub fn quarters(&self, set: &TileSet, column: i32, row: i32) -> Option<[u32; 4]> {
        let terrain = set.quarters_terrain(self.cell(column, row)?)?;
        let same = |dx: i32, dy: i32| self.cell(column + dx, row + dy) == Some(terrain.first_tile);
        Some(CORNERS.map(|(dx, dy)| {
            let quarter = Quarter::of(same(dx, 0), same(0, dy), same(dx, dy));
            terrain.quarters[quarter.index()]
        }))
    }

    /// What the tile at a cell carries, for a game that reads it.
    #[must_use]
    pub fn data_at<'a>(&self, set: &'a TileSet, column: i32, row: i32) -> Option<&'a toml::Value> {
        set.tile(self.cell(column, row)?)?.data.as_ref()
    }

    /// Every cell whose tile draws its own collision polygons, with the centre
    /// of the cell in the node's own space and the turn the cell is drawn
    /// with — a mirrored slope has to collide mirrored.
    pub fn shaped_cells<'a>(
        &'a self,
        set: &'a TileSet,
    ) -> impl Iterator<Item = (Vec2, &'a Tile, u8)> {
        self.filled().filter_map(move |(column, row, id)| {
            let tile = set.tile(id)?;
            matches!(tile.collision, Collision::Shape(_)).then(|| {
                (
                    self.cell_centre(column, row),
                    tile,
                    self.cell_flags(column, row),
                )
            })
        })
    }
}

/// The corners of a cell's tile, as which of the tile's own corners is drawn
/// at each corner of the cell: top left, top right, bottom right, bottom left.
///
/// The renderer permutes a quad's texture coordinates exactly this way, so
/// deriving the collider's turn from it is what keeps a mirrored slope from
/// colliding the way it is not drawn.
#[must_use]
fn turned_corners(flags: u8) -> [usize; 4] {
    let mut corners = [0, 1, 2, 3];
    if flags & TRANSPOSE != 0 {
        corners.swap(1, 3);
    }
    if flags & FLIP_X != 0 {
        corners.swap(0, 1);
        corners.swap(2, 3);
    }
    if flags & FLIP_Y != 0 {
        corners.swap(0, 3);
        corners.swap(1, 2);
    }
    corners
}

/// A point of the tile's own centred square, turned the way the cell is drawn.
///
/// The square's corners are `(±1, ±1)`, and a turn is the map carrying each
/// corner of the tile to the corner of the cell it is drawn at.
#[must_use]
fn turn_unit(point: Vec2, flags: u8) -> Vec2 {
    const CORNERS: [Vec2; 4] = [
        Vec2::new(-1.0, 1.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(1.0, -1.0),
        Vec2::new(-1.0, -1.0),
    ];
    if flags == 0 {
        return point;
    }
    let turned = turned_corners(flags);
    let at = |corner: usize| {
        turned
            .iter()
            .position(|drawn| *drawn == corner)
            .unwrap_or(corner)
    };
    // The square's two top corners are a basis, so where they land is the
    // whole map: `point` is written in that basis and read back in the turned
    // one.
    let (a, b) = ((point.y - point.x) / 2.0, f32::midpoint(point.y, point.x));
    CORNERS[at(0)] * a + CORNERS[at(1)] * b
}

/// A tile's polygon in world units, around the centre of the cell it sits in,
/// turned the way that cell's `flags` draw it.
///
/// Authored in tile pixels with y down from the tile's top-left corner, which
/// is what every tile editor exports.
#[must_use]
pub fn polygon_in_world(
    points: &[[f32; 2]],
    tile_pixels: [f32; 2],
    tile_world: [f32; 2],
    flags: u8,
) -> Vec<Vec2> {
    let half = Vec2::new(tile_world[0], tile_world[1]) / 2.0;
    points
        .iter()
        .map(|[x, y]| {
            let unit = Vec2::new(
                x / tile_pixels[0].max(f32::MIN_POSITIVE) * 2.0 - 1.0,
                1.0 - y / tile_pixels[1].max(f32::MIN_POSITIVE) * 2.0,
            );
            turn_unit(unit, flags) * half
        })
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

    /// A sheet whose one terrain is drawn in quarters: fill 0, horizontal 1,
    /// vertical 2, outer 3, inner 4.
    fn quartered() -> TileSet {
        set("texture = \"a.png\"\ntile_size = 8\ncolumns = 8\n\n[[terrains]]\nname = \"grass\"\nmode = \"quarters\"\nfirst_tile = 0")
    }

    #[test]
    fn a_cell_with_nothing_beside_it_is_four_outer_corners() {
        let (set, grid) = (quartered(), grid(&[&[0]]));
        assert_eq!(grid.quarters(&set, 0, 0), Some([3, 3, 3, 3]));
    }

    #[test]
    fn a_cell_walled_in_by_its_own_terrain_is_four_fills() {
        let (set, grid) = (quartered(), grid(&[&[0, 0, 0], &[0, 0, 0], &[0, 0, 0]]));
        assert_eq!(grid.quarters(&set, 1, 1), Some([0, 0, 0, 0]));
    }

    #[test]
    fn an_edge_runs_the_way_the_terrain_carries_on() {
        let set = quartered();
        let across = grid(&[&[0, 0, 0]]);
        assert_eq!(
            across.quarters(&set, 1, 0),
            Some([1, 1, 1, 1]),
            "a strip with nothing above or below draws four horizontal edges"
        );
        let down = grid(&[&[0], &[0], &[0]]);
        assert_eq!(
            down.quarters(&set, 0, 1),
            Some([2, 2, 2, 2]),
            "and a strip with nothing either side draws four vertical ones"
        );
    }

    #[test]
    fn a_corner_the_terrain_wraps_around_is_an_inner_one() {
        let (set, grid) = (
            quartered(),
            grid(&[&[-1, 0, 0], &[0, 0, 0], &[0, 0, 0]]),
        );
        let quarters = grid.quarters(&set, 1, 1).expect("the cell is quartered");
        assert_eq!(
            quarters[0], 4,
            "its west and north are the terrain and its north-west is not"
        );
        assert_eq!(quarters[2], 0, "while the corner away from the hole is filled");
    }

    #[test]
    fn a_tile_no_quartered_terrain_claims_is_drawn_as_one_quad() {
        let (set, grid) = (quartered(), grid(&[&[7]]));
        assert_eq!(grid.quarters(&set, 0, 0), None);
        assert_eq!(grid.quarters(&set, 9, 9), None, "and so is an empty cell");
    }

    #[test]
    #[allow(clippy::float_cmp, reason = "whole pixels, halved exactly")]
    fn a_quarter_is_the_corner_of_its_own_tile() {
        let set = quartered();
        assert_eq!(set.quarter_rect(0, 0), [0.0, 0.0, 4.0, 4.0]);
        assert_eq!(set.quarter_rect(0, 2), [4.0, 4.0, 4.0, 4.0]);
        assert_eq!(set.quarter_rect(1, 3), [8.0, 4.0, 4.0, 4.0], "tile 1 is the next along");
    }

    #[test]
    fn a_terrain_whose_block_is_not_five_in_a_row_names_its_tiles() {
        let named = set("texture = \"a.png\"\ntile_size = 8\ncolumns = 2\n\n[[terrains]]\nname = \"grass\"\nmode = \"quarters\"\nquarters = [0, 1, 2, 3, 6]");
        let grid = grid(&[&[-1, 0], &[0, 0]]);
        let quarters = grid.quarters(&named, 1, 1).expect("the cell is quartered");
        assert_eq!(quarters[0], 6, "the inner corner is where the sheet keeps it");
        let wrong = toml::from_str::<toml::Value>(
            "texture = \"a.png\"\ntile_size = 8\ncolumns = 2\n\n[[terrains]]\nname = \"grass\"\nmode = \"quarters\"\nquarters = [0, 1]",
        )
        .unwrap();
        assert!(parse_tileset(&wrong).is_err(), "a short list is a typo, not a default");
    }

    #[test]
    #[allow(clippy::float_cmp, reason = "a parsed size, not an arithmetic one")]
    fn a_square_tile_size_is_the_same_as_a_pair() {
        let square = set("texture = \"a.png\"\ntile_size = 16\ncolumns = 4");
        let pair = set("texture = \"a.png\"\ntile_size = [16, 16]\ncolumns = 4");
        assert_eq!(square.tile_size, pair.tile_size);
    }

    #[test]
    #[allow(clippy::float_cmp, reason = "whole pixels, laid out exactly")]
    fn spacing_and_margin_move_a_tile_along_the_sheet() {
        let set =
            set("texture = \"a.png\"\ntile_size = [16, 16]\ncolumns = 4\nspacing = 2\nmargin = 1");
        assert_eq!(set.tile_rect(0), [1.0, 1.0, 16.0, 16.0]);
        assert_eq!(set.tile_rect(1), [19.0, 1.0, 16.0, 16.0]);
        assert_eq!(set.tile_rect(4), [1.0, 19.0, 16.0, 16.0], "the second row");
    }

    #[test]
    fn a_tile_says_what_it_collides_as() {
        let set = set("texture = \"a.png\"\ntile_size = 16\ncolumns = 4\n\
             [tiles.1]\ncollision = \"full\"\n\
             [tiles.2]\ncollision = \"full\"\none_way = true\n");
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
    fn an_isometric_map_steps_half_a_cell_at_a_time() {
        let mut map = grid(&[&[0, 0], &[0, 0]]);
        map.layout = Layout::Isometric;
        let first = map.cell_centre(0, 0);
        let east = map.cell_centre(1, 0);
        assert!(
            (east.x - first.x - 0.5).abs() < 1e-5 && (east.y - first.y + 0.5).abs() < 1e-5,
            "one step east is half a cell right and half a cell down ({east:?})"
        );
        for row in 0..2 {
            for column in 0..2 {
                assert_eq!(
                    map.cell_at(map.cell_centre(column, row)),
                    (column, row),
                    "a diamond's centre is in it"
                );
            }
        }
    }

    #[test]
    fn a_hex_row_is_pushed_half_a_cell_across() {
        let mut map = grid(&[&[0, 0], &[0, 0]]);
        map.layout = Layout::Hex;
        let top = map.cell_centre(0, 0);
        let below = map.cell_centre(0, 1);
        assert!(below.x > top.x, "odd rows sit half a cell to the right");
        assert!(
            (top.y - below.y - 0.75).abs() < 1e-5,
            "and three quarters of a cell down ({below:?})"
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
        let set = set("texture = \"a.png\"\ntile_size = 16\ncolumns = 4\n\
             [tiles.1]\ncollision = \"full\"\n");
        let map = grid(&[&[1, -1], &[1, 1]]);
        let solid = map.group_cells(&set, Group::Solid);
        assert_eq!(solid.len(), 3);
        assert!(solid.contains(&[0, -1]), "the top-left cell");
        assert!(!solid.contains(&[1, -1]), "the empty cell is not solid");
    }

    #[test]
    fn a_tile_polygon_lands_around_the_cell_it_is_in() {
        let square = [[0.0, 0.0], [16.0, 0.0], [16.0, 16.0], [0.0, 16.0]];
        let world = polygon_in_world(&square, [16.0, 16.0], [1.0, 1.0], 0);
        assert_eq!(world[0], Vec2::new(-0.5, 0.5), "pixels count y down");
        assert_eq!(world[2], Vec2::new(0.5, -0.5));
    }

    /// A slope filling the bottom-right half of its tile. Mirrored, it has to
    /// fill the bottom-left half — a body walking up it from the other side.
    #[test]
    fn a_turned_tile_collides_the_way_it_is_drawn() {
        let slope = [[16.0, 0.0], [16.0, 16.0], [0.0, 16.0]];
        let upright = polygon_in_world(&slope, [16.0, 16.0], [2.0, 2.0], 0);
        assert_eq!(upright[0], Vec2::new(1.0, 1.0), "the high corner is right");
        let mirrored = polygon_in_world(&slope, [16.0, 16.0], [2.0, 2.0], FLIP_X);
        assert_eq!(mirrored[0], Vec2::new(-1.0, 1.0), "and now it is left");
        let flipped = polygon_in_world(&slope, [16.0, 16.0], [2.0, 2.0], FLIP_Y);
        assert_eq!(flipped[0], Vec2::new(1.0, -1.0));
    }

    /// Every turn is a symmetry of the cell: it moves the corners around and
    /// never off the tile, whatever combination of flags names it.
    #[test]
    fn a_turn_keeps_the_tile_inside_its_own_cell() {
        let corners = [[0.0, 0.0], [16.0, 0.0], [16.0, 16.0], [0.0, 16.0]];
        for flags in 0..8u8 {
            let world = polygon_in_world(&corners, [16.0, 16.0], [1.0, 1.0], flags);
            let mut seen: Vec<[i32; 2]> = world
                .iter()
                .map(|p| [(p.x * 2.0) as i32, (p.y * 2.0) as i32])
                .collect();
            seen.sort_unstable();
            assert_eq!(
                seen,
                vec![[-1, -1], [-1, 1], [1, -1], [1, 1]],
                "flags {flags} moved a corner off the cell"
            );
        }
    }
}
