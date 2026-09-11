//! A `TileMapLayer` as `tilemap` nodes over `tileset` assets.
//!
//! Godot's layer paints from a TileSet of any number of atlas sources; a
//! tilemap here paints from one atlas. So a layer becomes one tilemap per
//! source it uses: the busiest on the layer's own node, the rest on nodes
//! under it. Cells keep their grid coordinates, since both engines count rows
//! downward from a top-left origin at the node.

use std::collections::BTreeMap;

use balaur_plugin::toml;
use toml::Value as Toml;

use crate::godot::{Section, Value};
use crate::godot::nodes::{Asset, Mapped, Resources, load, points_of};

/// Godot's alternative-tile bits for a flipped or transposed cell.
const FLIP_H: u16 = 1 << 12;
const FLIP_V: u16 = 1 << 13;
const TRANSPOSE: u16 = 1 << 14;

/// One cell of `tile_map_data`.
struct Cell {
    x: i32,
    y: i32,
    source: u16,
    atlas: (u16, u16),
    alternative: u16,
}

/// One atlas source of a TileSet, as far as a tilemap needs it.
struct Atlas {
    texture: String,
    region: (u32, u32),
    columns: u32,
    margin: u32,
    spacing: u32,
    /// Collision polygons by atlas coordinate, in tile pixels from the
    /// tile's top-left corner, y down.
    collision: BTreeMap<(u16, u16), Vec<Vec<[f64; 2]>>>,
}

/// Map one layer onto `out`, adding a child per extra atlas it paints from.
pub(crate) fn layer(section: &Section, res: &Resources<'_>, out: &mut Mapped) {
    let cells = match section.field("tile_map_data").map(cells) {
        Some(Ok(cells)) => cells,
        Some(Err(why)) => {
            out.notes.push(format!("TileMapLayer: {why}"));
            return;
        }
        None => return,
    };
    if cells.is_empty() {
        return;
    }
    let Some(tile_set) = section.field("tile_set") else {
        out.notes
            .push("TileMapLayer with no tile_set; its cells were dropped".into());
        return;
    };
    // A TileSet saved inline in the scene, or in its own `.tres`.
    let loaded = load(res, tile_set);
    let (set, set_res): (&Section, &Resources<'_>) = match (&loaded, res.sub(tile_set)) {
        (_, Some(inline)) => (inline, res),
        (Some((document, nested)), None) => match document.first("resource") {
            Some(resource) => (resource, nested),
            None => return,
        },
        _ => {
            out.notes
                .push("TileMapLayer: its TileSet would not load".into());
            return;
        }
    };
    let grid = set
        .field("tile_size")
        .and_then(Value::numbers)
        .and_then(|n| Some((*n.first()? as u32, *n.get(1)? as u32)))
        .unwrap_or((16, 16));

    let mut by_source: BTreeMap<u16, Vec<&Cell>> = BTreeMap::new();
    for cell in &cells {
        by_source.entry(cell.source).or_default().push(cell);
    }
    let mut sources: Vec<(u16, Vec<&Cell>)> = by_source.into_iter().collect();
    sources.sort_by_key(|(id, cells)| (std::cmp::Reverse(cells.len()), *id));
    for (index, (source, cells)) in sources.into_iter().enumerate() {
        let Some(atlas) = set
            .field(&format!("sources/{source}"))
            .and_then(|s| set_res.sub(s))
            .and_then(|s| atlas(s, set_res, &mut out.notes))
        else {
            out.notes.push(format!(
                "TileMapLayer: atlas source {source} is not an atlas this reads; its cells were dropped"
            ));
            continue;
        };
        if atlas.region != grid {
            out.notes.push(format!(
                "TileMapLayer: tiles cut at {}x{} on a {}x{} grid draw at the grid's size here",
                atlas.region.0, atlas.region.1, grid.0, grid.1
            ));
        }
        let (tileset, tilemap) = map(&atlas, &cells);
        if index == 0 {
            if let Some(Toml::Table(have)) = out.components.get_mut("tilemap") {
                have.extend(tilemap);
            } else {
                out.components
                    .insert("tilemap".into(), Toml::Table(tilemap));
            }
            out.assets.push(Asset {
                component: "tilemap",
                key: "tileset",
                table: tileset,
            });
        } else {
            let mut child = Mapped::default();
            child
                .components
                .insert("tilemap".into(), Toml::Table(tilemap));
            child.assets.push(Asset {
                component: "tilemap",
                key: "tileset",
                table: tileset,
            });
            out.children.push((format!("Atlas{source}"), child));
        }
    }
}

/// The tileset asset and the tilemap component for one atlas's cells.
fn map(atlas: &Atlas, cells: &[&Cell]) -> (toml::Table, toml::Table) {
    let mut tileset = toml::Table::new();
    tileset.insert("type".into(), Toml::String("tileset".into()));
    tileset.insert("texture".into(), Toml::String(atlas.texture.clone()));
    tileset.insert(
        "tile_size".into(),
        Toml::Array(vec![
            Toml::Integer(i64::from(atlas.region.0)),
            Toml::Integer(i64::from(atlas.region.1)),
        ]),
    );
    tileset.insert("columns".into(), Toml::Integer(i64::from(atlas.columns)));
    if atlas.margin > 0 {
        tileset.insert("margin".into(), Toml::Integer(i64::from(atlas.margin)));
    }
    if atlas.spacing > 0 {
        tileset.insert("spacing".into(), Toml::Integer(i64::from(atlas.spacing)));
    }
    let index = |(ax, ay): (u16, u16)| i64::from(ay) * i64::from(atlas.columns) + i64::from(ax);
    let mut tiles = toml::Table::new();
    for (coords, polygons) in &atlas.collision {
        let polygons: Vec<Toml> = polygons
            .iter()
            .map(|polygon| {
                Toml::Array(
                    polygon
                        .iter()
                        .map(|[x, y]| Toml::Array(vec![Toml::Float(*x), Toml::Float(*y)]))
                        .collect(),
                )
            })
            .collect();
        let mut tile = toml::Table::new();
        tile.insert("collision".into(), Toml::Array(polygons));
        tiles.insert(index(*coords).to_string(), Toml::Table(tile));
    }
    if !tiles.is_empty() {
        tileset.insert("tiles".into(), Toml::Table(tiles));
    }

    let min_x = cells.iter().map(|c| c.x).min().unwrap_or(0);
    let max_x = cells.iter().map(|c| c.x).max().unwrap_or(0);
    let min_y = cells.iter().map(|c| c.y).min().unwrap_or(0);
    let max_y = cells.iter().map(|c| c.y).max().unwrap_or(0);
    let width = (max_x - min_x + 1) as usize;
    let height = (max_y - min_y + 1) as usize;
    let mut ids = vec![vec![-1i64; width]; height];
    let mut flags = vec![vec![0i64; width]; height];
    let mut flipped = false;
    for cell in cells {
        let (row, column) = ((cell.y - min_y) as usize, (cell.x - min_x) as usize);
        ids[row][column] = index(cell.atlas);
        // Godot's flip and transpose bits are the three this engine has,
        // in the same order: 1 mirrors left to right, 2 top to bottom, 4
        // across the diagonal.
        let mut bits = 0;
        if cell.alternative & FLIP_H != 0 {
            bits |= 1;
        }
        if cell.alternative & FLIP_V != 0 {
            bits |= 2;
        }
        if cell.alternative & TRANSPOSE != 0 {
            bits |= 4;
        }
        flags[row][column] = bits;
        flipped |= bits != 0;
    }
    let rows = |grid: Vec<Vec<i64>>| {
        Toml::Array(
            grid.into_iter()
                .map(|row| Toml::Array(row.into_iter().map(Toml::Integer).collect()))
                .collect(),
        )
    };
    let mut tilemap = toml::Table::new();
    tilemap.insert("cells".into(), rows(ids));
    tilemap.insert(
        "origin".into(),
        Toml::Array(vec![
            Toml::Float(f64::from(min_x)),
            Toml::Float(f64::from(min_y)),
        ]),
    );
    if flipped {
        let text: Vec<String> = flags
            .iter()
            .map(|row| row.iter().map(i64::to_string).collect::<Vec<_>>().join(" "))
            .collect();
        tilemap.insert("flags".into(), Toml::String(text.join("\n")));
    }
    (tileset, tilemap)
}

/// A `TileSetAtlasSource`: its texture, its grid, and the collision polygons
/// its tiles carry on physics layer 0.
fn atlas(source: &Section, res: &Resources<'_>, notes: &mut Vec<String>) -> Option<Atlas> {
    if source.attr_str("type") != Some("TileSetAtlasSource") {
        return None;
    }
    let texture = source
        .field("texture")
        .and_then(|t| res.path(t))?
        .to_string();
    let pair = |key: &str, default: (u32, u32)| {
        source
            .field(key)
            .and_then(Value::numbers)
            .and_then(|n| Some((*n.first()? as u32, *n.get(1)? as u32)))
            .unwrap_or(default)
    };
    let region = pair("texture_region_size", (16, 16));
    let margins = pair("margins", (0, 0));
    let separation = pair("separation", (0, 0));
    if margins.0 != margins.1 || separation.0 != separation.1 {
        notes.push(
            "TileSet: margins or separation differ across the axes; the x one is used".into(),
        );
    }
    let columns = if let Some((width, _)) = res.image_size(&texture) {
        let usable = width.saturating_sub(2 * margins.0) + separation.0;
        (usable / (region.0 + separation.0).max(1)).max(1)
    } else {
        notes.push(format!(
            "TileSet over {texture}: its width could not be read, so the atlas is taken as one tile wide"
        ));
        1
    };
    let mut collision: BTreeMap<(u16, u16), Vec<Vec<[f64; 2]>>> = BTreeMap::new();
    let (half_w, half_h) = (f64::from(region.0) / 2.0, f64::from(region.1) / 2.0);
    for (key, value) in &source.fields {
        // `x:y/0/physics_layer_0/polygon_N/points`, centred on the tile.
        let Some((coords, rest)) = key.split_once('/') else {
            continue;
        };
        if !rest.starts_with("0/physics_layer_0/polygon_") || !rest.ends_with("/points") {
            continue;
        }
        let Some((ax, ay)) = coords
            .split_once(':')
            .and_then(|(x, y)| Some((x.parse::<u16>().ok()?, y.parse::<u16>().ok()?)))
        else {
            continue;
        };
        let polygon: Vec<[f64; 2]> = points_of(value)
            .into_iter()
            .map(|[x, y]| [x + half_w, y + half_h])
            .collect();
        if polygon.len() >= 3 {
            collision.entry((ax, ay)).or_default().push(polygon);
        }
    }
    Some(Atlas {
        texture,
        region,
        columns,
        margin: margins.0,
        spacing: separation.0,
        collision,
    })
}

/// `tile_map_data`: a format version, then twelve bytes a cell.
fn cells(value: &Value) -> Result<Vec<Cell>, String> {
    let bytes = match value.call("PackedByteArray") {
        Some([Value::Str(encoded)]) => base64(encoded).ok_or("tile data is not base64")?,
        Some(args) => args
            .iter()
            .map(|a| a.as_i64().map(|n| n as u8))
            .collect::<Option<Vec<u8>>>()
            .ok_or("tile data holds a non-byte")?,
        None => return Err("tile data is not a PackedByteArray".into()),
    };
    let Some((version, body)) = bytes.split_first_chunk::<2>() else {
        return Ok(Vec::new());
    };
    if u16::from_le_bytes(*version) != 0 {
        return Err(format!(
            "tile data format {} is newer than this reads",
            u16::from_le_bytes(*version)
        ));
    }
    let u16_at = |chunk: &[u8], at: usize| u16::from_le_bytes([chunk[at], chunk[at + 1]]);
    Ok(body
        .as_chunks::<12>()
        .0
        .iter()
        .map(|chunk| Cell {
            x: i32::from(i16::from_le_bytes([chunk[0], chunk[1]])),
            y: i32::from(i16::from_le_bytes([chunk[2], chunk[3]])),
            source: u16_at(chunk, 4),
            atlas: (u16_at(chunk, 6), u16_at(chunk, 8)),
            alternative: u16_at(chunk, 10),
        })
        .collect())
}

/// Standard base64, padded or not.
fn base64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let digits: Vec<u8> = text
        .bytes()
        .filter(|c| !c.is_ascii_whitespace() && *c != b'=')
        .map(value)
        .collect::<Option<_>>()?;
    let mut out = Vec::with_capacity(digits.len() * 3 / 4);
    for chunk in digits.chunks(4) {
        let mut word = 0u32;
        for (i, d) in chunk.iter().enumerate() {
            word |= u32::from(*d) << (18 - 6 * i);
        }
        let bytes = [(word >> 16) as u8, (word >> 8) as u8, word as u8];
        out.extend_from_slice(&bytes[..chunk.len().saturating_sub(1)]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{Value, base64, cells};

    #[test]
    fn base64_decodes_padded_and_bare() {
        assert_eq!(base64("aGk=").unwrap(), b"hi");
        assert_eq!(base64("aGk").unwrap(), b"hi");
        assert_eq!(base64("AAEC").unwrap(), vec![0, 1, 2]);
    }

    /// The first cell of a layer in this game's `level.tscn`, decoded by hand:
    /// version 0, then cell (2, -1) from source 0, atlas (1, 1).
    #[test]
    fn tile_data_reads_twelve_bytes_a_cell_after_its_version() {
        let data = Value::Call {
            name: "PackedByteArray".into(),
            args: vec![Value::Str("AAACAP//AAABAAEAAAA=".into())],
        };
        let cells = cells(&data).unwrap();
        assert_eq!(cells.len(), 1);
        assert_eq!((cells[0].x, cells[0].y), (2, -1));
        assert_eq!(cells[0].source, 0);
        assert_eq!(cells[0].atlas, (1, 1));
        assert_eq!(cells[0].alternative, 0);
    }
}
