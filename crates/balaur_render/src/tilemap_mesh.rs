//! The kiss3d mirror of a `tilemap`: one mesh node per chunk of cells,
//! rebuilt only where the cells moved.

use anyhow::Result;
use balaur_core::hecs::Entity;

use crate::tile_quad::{corners_of, rect_uvs, tile_uvs};
use crate::tilemap::{TileSet, Tilemap, grid_of};

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
    /// The node's inherited material at the last build, for a map naming none.
    inherited: balaur_core::scene::MaterialId,
    /// The material the chunks were built with. A chunk outlives a rebuild
    /// while its cells hold, so a change here starts every chunk over.
    drawn: String,
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
        let appearance = world
            .get::<&GlobalAppearance>(entity)
            .map_or_else(|_| GlobalAppearance::identity(), |a| *a);
        let rebuild = reloaded
            || slots.get(&entity).is_none_or(|slot| {
                slot.version != map.version
                    || slot.frame != frame
                    || (map.material.is_empty() && slot.inherited != appearance.material)
            });
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
                inherited: appearance.material,
                drawn: String::new(),
            }
        });
        if rebuild {
            let inherited = appearance.material.reference();
            let reference = if map.material.is_empty() {
                &inherited
            } else {
                map.material.as_str()
            };
            if slot.drawn != reference {
                for (_, mut chunk) in slot.chunks.drain() {
                    chunk.node.detach();
                }
                slot.drawn = reference.to_string();
            }
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
            let material = materials.for_node(app, reference, &channel);
            for chunk in slot.chunks.values_mut() {
                if let Some(material) = material.clone() {
                    chunk.node.set_material(material);
                }
            }
            slot.version = map.version;
            slot.frame = frame;
            slot.inherited = appearance.material;
        }
        let (angle, _, _) = global.rotation.to_euler(glamx::EulerRot::ZYX);
        let visible = appearance.visible;
        // A map has no colour of its own, so the inherited tint is the whole
        // colour, and untinted is the white that leaves the atlas alone.
        let [r, g, b, a] = appearance.tint.to_array();
        for chunk in slot.chunks.values_mut() {
            chunk.node.set_color(kiss3d::color::Color::new(r, g, b, a));
        }
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
    let (width, height) = crate::texture::size_of(eng, &tileset.texture)?;
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
fn chunks_of(
    grid: &balaur_core::tiles::TileGrid,
    set: &TileSet,
    seconds: f32,
    frame: i64,
) -> std::collections::BTreeMap<[i32; 2], (Vec<Cell>, u64)> {
    let seed = [
        grid.tile_world[0].to_bits().into(),
        grid.tile_world[1].to_bits().into(),
        frame as u64,
    ]
    .into_iter()
    .fold(0xcbf2_9ce4_8422_2325, mix);
    let mut out: std::collections::BTreeMap<[i32; 2], (Vec<Cell>, u64)> =
        std::collections::BTreeMap::new();
    for (column, row, id) in grid.filled() {
        let id = animated(set, id, seconds);
        let flags = grid.cell_flags(column, row);
        let quarters = grid.quarters(set, column, row);
        let key = [column.div_euclid(CHUNK), row.div_euclid(CHUNK)];
        let entry = out.entry(key).or_insert_with(|| (Vec::new(), seed));
        entry.0.push(Cell {
            column,
            row,
            id,
            flags,
            quarters,
        });
        // The quarters ride in the digest because a quartered cell is drawn
        // from its neighbours, which a chunk of its own may not hold.
        for part in [
            i64::from(column) as u64,
            i64::from(row) as u64,
            id.into(),
            flags.into(),
        ]
        .into_iter()
        .chain(quarters.into_iter().flatten().map(u64::from))
        {
            entry.1 = mix(entry.1, part);
        }
    }
    out
}

/// One cell as the mesh builder wants it: where it is, what it draws with,
/// and the tile each corner takes its picture from when the terrain is
/// drawn in quarters.
#[cfg(feature = "kiss3d")]
struct Cell {
    column: i32,
    row: i32,
    id: u32,
    flags: u8,
    quarters: Option<[u32; 4]>,
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
    cells: &[Cell],
) -> kiss3d::scene::SceneNode2d {
    use kiss3d::resource::GpuMesh2d;

    // A hair off each edge of a tile's rect, or a neighbouring tile bleeds in
    // at some zoom levels.
    let inset = glamx::Vec2::new(0.05 / sheet.x, 0.05 / sheet.y);
    let mut coords: Vec<glamx::Vec2> = Vec::new();
    let mut uvs: Vec<glamx::Vec2> = Vec::new();
    let mut faces: Vec<[u32; 3]> = Vec::new();
    let mut quad = |corners: [glamx::Vec2; 4], uv: [glamx::Vec2; 4]| {
        let base = coords.len() as u32;
        coords.extend(corners);
        uvs.extend(uv);
        faces.push([base, base + 1, base + 2]);
        faces.push([base, base + 2, base + 3]);
    };
    for cell in cells {
        let centre = grid.cell_centre(cell.column, cell.row);
        let half = glamx::Vec2::new(grid.tile_world[0], grid.tile_world[1]) / 2.0;
        let Some(quarters) = cell.quarters else {
            quad(
                corners_of(centre, half),
                tile_uvs(tileset, cell.id, sheet, inset, cell.flags),
            );
            continue;
        };
        // A quarter sits in the corner it is cut from, and a half-sized quad's
        // corners are the four quarter centres. The cell's turn stays off: its
        // neighbours already decided which way each quarter faces.
        let quarter = half / 2.0;
        for (corner, tile) in quarters.into_iter().enumerate() {
            quad(
                corners_of(corners_of(centre, quarter)[corner], quarter),
                rect_uvs(tileset.quarter_rect(tile, corner), sheet, inset),
            );
        }
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
