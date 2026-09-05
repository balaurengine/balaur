//! The `tileset` asset and the `tilemap` component: a grid of tiles drawn
//! from one atlas texture. The component and parser are backend-free; the
//! kiss3d mirror at the bottom of the file is feature-gated.

use anyhow::{anyhow, Result};
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::Engine;
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt};

/// The asset type name a `tileset` definition declares.
pub const TILESET_ASSET_TYPE: &str = "tileset";

/// A parsed `tileset` asset: which image to cut tiles from and how.
pub struct Tileset {
    /// Project-relative path to the atlas image.
    pub texture: String,
    /// Pixels per tile edge; tiles are square.
    pub tile_size: f32,
    /// Tiles per texture row.
    pub columns: u32,
}

fn parse_tileset(value: &toml::Value) -> Result<Tileset> {
    let texture = value
        .get("texture")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| anyhow!("a tileset needs a `texture` string naming its image"))?
        .to_string();
    if texture.is_empty() {
        return Err(anyhow!("a tileset's `texture` names no image"));
    }
    let tile_size = value
        .get("tile_size")
        .and_then(balaur_core::components::as_f64)
        .ok_or_else(|| anyhow!("a tileset needs a `tile_size` in pixels per tile edge"))?
        as f32;
    if tile_size <= 0.0 {
        return Err(anyhow!(
            "a tileset's `tile_size` must be positive, not {tile_size}"
        ));
    }
    let columns = value
        .get("columns")
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| anyhow!("a tileset needs an integer `columns` count of tiles per row"))?;
    if columns < 1 {
        return Err(anyhow!(
            "a tileset's `columns` must be at least 1, not {columns}"
        ));
    }
    Ok(Tileset {
        texture,
        tile_size,
        columns: columns as u32,
    })
}

/// What a definition table holds, for the generated reference.
const TILESET_ASSET_DOC: &str = r#"An image cut into equal tiles for the `tilemap` component: `texture` names
the image, `tile_size` is the pixel length of one tile edge and `columns` is
how many tiles one row of the image holds. Tile indices count row by row
from the top left.

```toml
[[assets]]
id = "dungeon"
type = "tileset"
texture = "art/dungeon.png"
tile_size = 16
columns = 8
```"#;

/// The `tileset` asset type: files live in `tilesets/`.
pub(crate) fn register_tileset_asset(reg: &mut Registry<'_>) {
    reg.register_asset_type(TILESET_ASSET_TYPE, "tilesets", TILESET_ASSET_DOC, |value| {
        Ok(std::rc::Rc::new(parse_tileset(value)?) as std::rc::Rc<dyn std::any::Any>)
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
    /// Tile-texture pixels per world unit.
    pub pixels_per_unit: f32,
    /// Bumped when the content changes so backends rebuild their mesh.
    pub version: u64,
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

fn set_tilemap(eng: &Engine, entity: Entity, next: Tilemap) -> Result<()> {
    let mut world = eng.world_mut();
    if let Ok(mut map) = world.get::<&mut Tilemap>(entity) {
        let changed = map.tileset != next.tileset
            || map.grid != next.grid
            || map.material != next.material
            || map.pixels_per_unit.to_bits() != next.pixels_per_unit.to_bits();
        let version = map.version + u64::from(changed);
        *map = next;
        map.version = version;
        return Ok(());
    }
    world
        .insert_one(entity, next)
        .map_err(|_| anyhow!("node is dead"))
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
                r#"tileset = { type = "asset", asset = "tileset", default = "", description = "The tileset naming the texture and tile grid" }
cells = { type = "string", default = "", description = "Rows of tile characters, one row per line: . is empty, 0-9 then a-z index into the tileset. Also accepted: a list of rows of tile ids, -1 for empty, for a tileset past 36 tiles" }
pixels_per_unit = { type = "float", default = 100.0, min = 0.01, description = "Tile-texture pixels per world unit" }
material = { type = "asset", asset = "material", default = "", description = "The material the whole map draws with; empty draws with the built-in one" }"#,
            ),
            tags: &["2d", "render"],
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
                    .get("cells")
                    .cloned()
                    .unwrap_or_else(|| toml::Value::String(String::new()));
                let grid = parse_cells_value(&cells)?;
                let material = text("material");
                let ppu = params
                    .get("pixels_per_unit")
                    .and_then(balaur_core::components::as_f64)
                    .unwrap_or(f64::from(crate::DEFAULT_PIXELS_PER_UNIT))
                    as f32;
                // Checked here so a bad definition is reported where it was
                // written, but only warned: one bad asset must not kill the scene.
                if !tileset.is_empty() {
                    if let Err(why) = balaur_core::assets::load_typed::<Tileset>(eng, &tileset) {
                        tracing::warn!("tilemap tileset '{tileset}': {why:#}");
                    }
                }
                set_tilemap(
                    eng,
                    entity,
                    Tilemap {
                        tileset,
                        cells,
                        material,
                        grid,
                        pixels_per_unit: ppu.max(0.01),
                        version: 0,
                    },
                )
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Tilemap>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let map = world.get::<&Tilemap>(entity).ok()?;
                let mut out = toml::map::Map::new();
                out.insert("tileset".into(), toml::Value::String(map.tileset.clone()));
                out.insert("cells".into(), map.cells.clone());
                out.insert("material".into(), toml::Value::String(map.material.clone()));
                out.insert(
                    "pixels_per_unit".into(),
                    toml::Value::Float(f64::from(map.pixels_per_unit)),
                );
                Some(toml::Value::Table(out))
            }),
        },
    );
}

/// `render.set_cell` and `render.cell`: one tile at a time, for a map a
/// script edits as it plays. A write past the grid's edge grows it.
pub(crate) fn install_tilemap_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_cell", &["tilemap"], "(x: int, y: int, tile: int)", "Put one tile at a column and row, counted from the top left; below zero clears the cell, and a cell past the edge grows the map. The mesh rebuilds on the next frame."),
        ("cell", &["tilemap"], "(x: int, y: int) -> int", "The tile at a column and row, or -1 for an empty cell or one past the edge."),
    ]);
    m.function(
        "set_cell",
        |eng: &Engine, (node, x, y, tile): (balaur_script::NodeId, i64, i64, i64)| {
            let entity = balaur_core::entity_of(node)?;
            let world = eng.world();
            let mut map = world
                .get::<&mut Tilemap>(entity)
                .map_err(|_| anyhow!("the node carries no tilemap"))?;
            let (x, y) = (
                usize::try_from(x).map_err(|_| anyhow!("a column is not negative"))?,
                usize::try_from(y).map_err(|_| anyhow!("a row is not negative"))?,
            );
            if map.grid.len() <= y {
                map.grid.resize(y + 1, Vec::new());
            }
            if map.grid[y].len() <= x {
                map.grid[y].resize(x + 1, None);
            }
            let next = u32::try_from(tile).ok();
            if map.grid[y][x] != next {
                map.grid[y][x] = next;
                map.cells = cells_value(&map.grid);
                map.version += 1;
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
            let found = usize::try_from(y)
                .ok()
                .and_then(|y| map.grid.get(y))
                .and_then(|row| usize::try_from(x).ok().and_then(|x| row.get(x)))
                .copied()
                .flatten();
            Ok(found.map_or(-1, i64::from))
        },
    );
}

#[cfg(feature = "kiss3d")]
pub(crate) struct TilemapSlot {
    node: kiss3d::scene::SceneNode2d,
    version: u64,
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
) {
    use balaur_core::{GlobalAppearance, GlobalTransform};

    let channel = crate::debug_view::channel_view(&app.engine);
    let world = app.engine.world();
    let mut seen: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    for (entity, map, global) in &mut world.query::<(Entity, &Tilemap, &GlobalTransform)>() {
        seen.insert(entity);
        let rebuild = slots
            .get(&entity)
            .is_none_or(|slot| slot.version != map.version);
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
#[cfg(feature = "kiss3d")]
fn build_map_node(eng: &Engine, map: &Tilemap) -> Result<kiss3d::scene::SceneNode2d> {
    use kiss3d::scene::{SpriteSheet, Tilemap as TilemapNode};

    let tileset = balaur_core::assets::load_typed::<Tileset>(eng, &map.tileset)?;
    let bytes = eng
        .resource::<balaur_core::project::ProjectFiles>()
        .borrow()
        .read(&tileset.texture)?;
    let (_, height) = crate::texture::image_size(&bytes, &tileset.texture)?;
    // The tileset declares columns; the atlas's row count comes off the image.
    let sheet_rows = ((height as f32 / tileset.tile_size) as u32).max(1);
    let sheet = SpriteSheet::new(tileset.columns.max(1), sheet_rows);
    let columns = map.grid.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let rows = map.grid.len().max(1);
    let mut tiles = vec![TilemapNode::EMPTY; columns * rows];
    for (row, line) in map.grid.iter().enumerate() {
        for (column, cell) in line.iter().enumerate() {
            if let Some(index) = cell {
                tiles[row * columns + column] = *index;
            }
        }
    }
    let tile_world = tileset.tile_size / map.pixels_per_unit;
    let mut mesh = TilemapNode::new(
        columns as u32,
        rows as u32,
        glamx::Vec2::splat(tile_world),
        sheet,
    );
    let mut node = mesh.node();
    // Texture before fill: the rebuild inside `fill` reads the texture size
    // for its anti-bleed UV inset.
    crate::texture::attach_texture_2d(eng, &mut node, &tileset.texture);
    mesh.fill(&tiles);
    Ok(node)
}
