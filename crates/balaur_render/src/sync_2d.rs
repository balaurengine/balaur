//! The 2D half of the backend's per-frame sync: which nodes draw, in what
//! order, and the kiss3d nodes they are built into.
//!
//! Split from `kiss3d_backend` because the ordering is 2D's alone: a 3D node
//! is drawn by its depth and a 2D one by its `z_index`, and the pass that
//! resolves that is most of what is here.

use std::collections::{HashMap, HashSet};

use balaur_core::hecs::Entity;
use balaur_core::{App, GlobalAppearance, GlobalTransform};
use glamx::Vec2;
use kiss3d::color::Color;
use kiss3d::scene::SceneNode2d;

use crate::kiss3d_backend::{Slot2d, build_2d_node, build_polyline_node, polygon_palette};
use crate::{Renderable2d, Shape2d, SpriteTexture};

/// Mirror `Renderable2d` + `GlobalTransform` into the kiss3d 2D scene graph
/// (x/y translation, z rotation, x/y scale).
///
/// The 2D nodes in the order they draw, detaching the slots that no longer
/// sit in it so the caller rebuilds those, and only those, in the new one.
///
/// kiss3d appends children and its `detach` is a `swap_remove`, so only a
/// suffix can be re-ordered: what the old and new orders share up front keeps
/// its places, and the rest is dropped last-first and rebuilt by appending.
pub(crate) fn draw_order_2d(world: &balaur_core::hecs::World, root: Entity) -> Vec<Entity> {
    let mut desired: Vec<(i32, f32, Entity)> = Vec::new();
    for entity in balaur_core::scene::collect_subtree(world, root) {
        if world.get::<&Renderable2d>(entity).is_ok() {
            desired.push(layer_of(world, entity));
        }
    }
    sorted(desired)
}

/// One 2D node's place in the order: its `z_index` first, then how far along
/// z it sits, then where the tree put it.
fn layer_of(world: &balaur_core::hecs::World, entity: Entity) -> (i32, f32, Entity) {
    let z = world
        .get::<&GlobalTransform>(entity)
        .map_or(0.0, |g| g.position.z);
    let layer = world
        .get::<&GlobalAppearance>(entity)
        .map_or(0, |a| a.z_index);
    (layer, z, entity)
}

fn sorted(mut desired: Vec<(i32, f32, Entity)>) -> Vec<Entity> {
    desired.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    desired.into_iter().map(|(_, _, entity)| entity).collect()
}

/// The drawing nodes under `root`, in the order they draw: `z_index` first,
/// then z, then where the tree put them. `has_node` says which entities have
/// something in the scene, whichever pass built it.
fn ordered_nodes(
    world: &balaur_core::hecs::World,
    root: Entity,
    has_node: impl Fn(Entity) -> bool,
) -> Vec<Entity> {
    let mut desired: Vec<(i32, f32, Entity)> = Vec::new();
    for entity in balaur_core::scene::collect_subtree(world, root) {
        if has_node(entity) {
            desired.push(layer_of(world, entity));
        }
    }
    sorted(desired)
}

/// Sprites, polygons and tilemaps in one draw order.
///
/// kiss3d draws 2D nodes in the order they were added, and a tilemap builds
/// its node in a pass of its own, so without this every map drew over every
/// sprite whatever the two said in `z_index`. The nodes from the first
/// divergence on are detached and added again in the order, which costs
/// nothing on a frame whose order held.
pub(crate) fn order_layer_2d(
    world: &balaur_core::hecs::World,
    root: Entity,
    scene: &mut SceneNode2d,
    slots: &mut HashMap<Entity, Slot2d>,
    batches: &mut Batches,
    maps: &mut HashMap<Entity, crate::tilemap::TilemapSlot>,
    order_cache: &mut Vec<Entity>,
) {
    // A run draws where its first member would have: every other member is
    // inside it, so only the head stands in the order.
    let order = ordered_nodes(world, root, |entity| {
        slots.contains_key(&entity) || maps.contains_key(&entity) || batches.head(entity).is_some()
    });
    let kept = order
        .iter()
        .zip(order_cache.iter())
        .take_while(|(now, before)| now == before)
        .count();
    let recut = std::mem::take(&mut batches.recut);
    if !recut && kept == order.len() && order.len() == order_cache.len() {
        return;
    }
    // A run built again this frame is a new object at the end of the scene's
    // children, so the whole order is laid out rather than its tail.
    let kept = if recut { 0 } else { kept };
    // A node holds its place while the prefix does; the rest is taken off
    // last-first and put back in order, which is the only move kiss3d has.
    let mut node_of = |entity: &Entity| -> Option<SceneNode2d> {
        slots
            .get(entity)
            .map(|slot| slot.node.clone())
            .or_else(|| maps.get(entity).map(|slot| slot.node.clone()))
            .or_else(|| batches.head(*entity).map(|run| run.node.clone()))
    };
    let moved: Vec<SceneNode2d> = order[kept..].iter().filter_map(&mut node_of).collect();
    for mut node in moved.iter().rev().cloned() {
        node.detach();
    }
    for node in moved {
        scene.add_child(node);
    }
    order_cache.clone_from(&order);
}

/// kiss3d draws 2D nodes in insertion order, so draw order is made explicit
/// and deterministic: scene-tree traversal order, stably sorted by the
/// global z coordinate (z acts as a 2D layer; equal z means later-declared
/// nodes draw on top). When the order changes, the kiss3d nodes are rebuilt
/// in the new order.
/// A run of nodes drawing through one object, and who is in it.
///
/// The object sits at the origin unscaled and holds no colour of its own:
/// every member's pose and tint rides in its instance.
pub(crate) struct LiveRun {
    key: crate::batch_2d::BatchKey,
    node: SceneNode2d,
    members: Vec<Entity>,
    /// This frame's instances, one per member that is visible.
    pending: Vec<kiss3d::scene::InstanceData2d>,
}

/// Every run this frame, in draw order. A run is addressed by the entity at
/// its head, which is an entity no `Slot2d` is kept for.
#[derive(Default)]
pub(crate) struct Batches {
    runs: Vec<LiveRun>,
    heads: HashMap<Entity, usize>,
    /// Whether the objects were built again since the order was last laid
    /// out, which is what makes the cached order stale.
    recut: bool,
}

impl Batches {
    fn head(&self, entity: Entity) -> Option<&LiveRun> {
        self.heads.get(&entity).and_then(|at| self.runs.get(*at))
    }

    /// Detach every object, for a sync that is cutting the runs again.
    fn clear(&mut self) {
        for run in &mut self.runs {
            run.node.detach();
        }
        self.runs.clear();
        self.heads.clear();
        self.recut = true;
    }
}

/// The scene node one 2D renderable draws through, with the handles a frame
/// writes into it. `None` for a renderable with nothing to draw.
pub(crate) fn build_slot_2d(
    app: &App,
    scene: &mut SceneNode2d,
    materials: &mut crate::shader_material::MaterialCache,
    channel: &str,
    renderable: &Renderable2d,
    inherited: balaur_core::scene::MaterialId,
) -> Option<Slot2d> {
    let mut pieces = Vec::new();
    let built = match renderable.shape {
        Shape2d::Polygon => crate::skinned_2d::build_polygon_node(app, scene, renderable),
        Shape2d::Polyline(stroke) => {
            build_polyline_node(app, scene, renderable, &stroke).map(|(node, built)| {
                pieces = built;
                (node, None, None)
            })
        }
        _ => build_2d_node(scene, renderable).map(|node| (node, None, None)),
    };
    let (mut node, skin, deform) = built?;
    if let Some(sprite) = &renderable.sprite {
        crate::texture::attach_texture_2d(&app.engine, &mut node, &sprite.path);
    }
    // After the texture: a material reads it, and kiss3d's own material stays
    // on a node whose shader would not link.
    let from_parent = inherited.reference();
    let reference = if renderable.material.is_empty() {
        &from_parent
    } else {
        renderable.material.as_str()
    };
    if let Some(material) = materials.for_node(app, reference, channel) {
        node.set_material(material);
    }
    Some(Slot2d {
        node,
        version: renderable.version,
        inherited,
        flip: (false, false),
        skin,
        deform,
        deformed: false,
        pieces,
        shear: 0.0,
    })
}

/// Cut the draw order into runs and answer which run each member is in.
///
/// The objects are rebuilt only when the cut itself moved: a frame that draws
/// the same runs over again writes instances and nothing else.
fn cut_runs(
    app: &App,
    scene: &mut SceneNode2d,
    materials: &mut crate::shader_material::MaterialCache,
    channel: &str,
    world: &balaur_core::hecs::World,
    order: &[Entity],
    batches: &mut Batches,
) -> HashMap<Entity, usize> {
    let cut = crate::batch_2d::runs(order, |entity| {
        let renderable = world.get::<&Renderable2d>(entity).ok()?;
        if !crate::batch_2d::batchable(&renderable) {
            return None;
        }
        let material = world
            .get::<&GlobalAppearance>(entity)
            .map_or_else(|_| GlobalAppearance::identity().material, |a| a.material);
        Some(crate::batch_2d::key_of(&renderable, material))
    });
    let same = cut.len() == batches.runs.len()
        && cut
            .iter()
            .zip(batches.runs.iter())
            .all(|(now, live)| now.key == live.key && now.members == live.members);
    if !same {
        batches.clear();
        for run in cut {
            let Some(head) = run.members.first().copied() else {
                continue;
            };
            let Ok(renderable) = world.get::<&Renderable2d>(head) else {
                continue;
            };
            let material = world
                .get::<&GlobalAppearance>(head)
                .map_or_else(|_| GlobalAppearance::identity().material, |a| a.material);
            let Some(slot) = build_slot_2d(app, scene, materials, channel, &renderable, material)
            else {
                continue;
            };
            let mut node = slot.node;
            // The shader multiplies the object's colour by the instance's, so
            // the object stays white and every member's tint rides in its own.
            node.set_position(Vec2::ZERO)
                .set_rotation(0.0)
                .set_local_scale(1.0, 1.0)
                .set_color(Color::new(1.0, 1.0, 1.0, 1.0))
                .set_visible(true);
            batches.runs.push(LiveRun {
                key: run.key,
                node,
                members: run.members,
                pending: Vec::new(),
            });
        }
    }
    let mut member_of = HashMap::new();
    batches.heads.clear();
    for (index, run) in batches.runs.iter_mut().enumerate() {
        run.pending.clear();
        if let Some(head) = run.members.first() {
            batches.heads.insert(*head, index);
        }
        for &entity in &run.members {
            member_of.insert(entity, index);
        }
    }
    member_of
}

/// One member's pose and tint, as the instance its run draws it through.
fn write_instance(
    world: &balaur_core::hecs::World,
    entity: Entity,
    run: usize,
    batches: &mut Batches,
) {
    let (Ok(renderable), Ok(global)) = (
        world.get::<&Renderable2d>(entity),
        world.get::<&GlobalTransform>(entity),
    ) else {
        return;
    };
    let appearance = world
        .get::<&GlobalAppearance>(entity)
        .map_or_else(|_| GlobalAppearance::identity(), |a| *a);
    if !appearance.visible {
        return;
    }
    let (at, deformation) = crate::batch_2d::pose(&renderable, &global);
    let color = modulate(renderable.color, appearance.tint.to_array());
    if let Some(run) = batches.runs.get_mut(run) {
        run.pending.push(kiss3d::scene::InstanceData2d {
            position: at,
            deformation,
            color,
            ..Default::default()
        });
    }
}

pub(crate) fn sync_2d(
    app: &App,
    scene: &mut SceneNode2d,
    slots: &mut HashMap<Entity, Slot2d>,
    batches: &mut Batches,
    materials: &mut crate::shader_material::MaterialCache,
    reloaded: bool,
) {
    let world = app.engine.world();
    let order = draw_order_2d(&world, app.engine.root());

    // A relink rebuilds the nodes holding the old pipeline; a channel view
    // rebuilds every node, whether or not it names a material.
    let channel = crate::debug_view::channel_view(&app.engine);
    let relinked = materials.refresh(app);
    let channel_changed = materials.channel_changed(&channel);
    materials.answer_probe(app);

    let member_of = cut_runs(app, scene, materials, &channel, &world, &order, batches);

    let mut seen: HashSet<Entity> = HashSet::new();
    for &entity in &order {
        if let Some(run) = member_of.get(&entity) {
            // The run draws it. A slot from before it joined one would draw
            // it twice.
            if let Some(mut old) = slots.remove(&entity) {
                old.node.detach();
            }
            write_instance(&world, entity, *run, batches);
            continue;
        }
        let Ok(renderable) = world.get::<&Renderable2d>(entity) else {
            continue;
        };
        let Ok(global) = world.get::<&GlobalTransform>(entity) else {
            continue;
        };
        seen.insert(entity);
        // Read once: the ancestors' tint, visibility and material come off
        // the same propagated component.
        let appearance = world
            .get::<&GlobalAppearance>(entity)
            .map_or_else(|_| GlobalAppearance::identity(), |a| *a);
        let owns_material = !renderable.material.is_empty();
        // A sprite's image and a polyline's mesh are both files; a reload
        // re-reads them, as it does in three dimensions.
        let from_file = renderable.sprite.is_some() || renderable.polyline.is_some();
        let rebuild = match slots.get(&entity) {
            Some(slot) => {
                slot.version != renderable.version
                    || channel_changed
                    || (relinked && (owns_material || !appearance.material.is_none()))
                    || (reloaded && from_file)
                    || (!owns_material && slot.inherited != appearance.material)
            }
            None => true,
        };
        if rebuild {
            if let Some(mut old) = slots.remove(&entity) {
                old.node.detach();
            }
            let Some(slot) = build_slot_2d(
                app,
                scene,
                materials,
                &channel,
                &renderable,
                appearance.material,
            ) else {
                continue;
            };
            slots.insert(entity, slot);
        }
        // The block above inserts the slot when it is missing.
        let slot = slots.get_mut(&entity).unwrap();
        let inherited = appearance.tint.to_array();
        let [r, g, b, a] = modulate(renderable.color, inherited);
        // A sprite is the one 2D shape still built at unit size: its extents
        // come from the image and change without rebuilding the node.
        let size = match renderable.shape {
            Shape2d::Sprite { hx, hy } => Vec2::new(2.0 * hx, 2.0 * hy),
            _ => Vec2::ONE,
        };
        // Every frame: the rig moved even when nothing about the polygon did.
        if let (Some(handle), Some(polygon)) = (&slot.skin, renderable.polygon.as_deref()) {
            handle.set(polygon_palette(&world, entity, polygon));
        }
        if let (Some(handle), Some(polygon)) = (&slot.deform, renderable.polygon.as_deref()) {
            slot.deformed =
                crate::skinned_2d::write_deform(&world, entity, polygon, handle, slot.deformed);
        }
        tint_pieces(slot, &renderable, inherited);
        // Every sync, not just on rebuild: frames and flips are UV changes, so
        // an animation that flips frames must not rebuild the node.
        if let Some(sprite) = &renderable.sprite {
            let flip = (sprite.flip_x, sprite.flip_y);
            if sprite.sheet.is_some() || sprite.region.is_some() || flip != slot.flip {
                // The rectangle is in the pixels the image was drawn at, which
                // a shrunk copy no longer has: `size_of` answers those.
                let drawn = sprite
                    .region
                    .and_then(|_| crate::texture::size_of(&app.engine, &sprite.path).ok());
                sync_sprite_uvs(&mut slot.node, sprite, drawn);
            }
            slot.flip = flip;
        }
        let (angle, _, _) = global.rotation.to_euler(glamx::EulerRot::ZYX);
        let visible = appearance.visible;
        let shift = lean_and_shift(slot, &renderable, &global);
        slot.node
            .set_position(Vec2::new(global.position.x, global.position.y) + shift)
            .set_rotation(angle)
            .set_local_scale(size.x * global.scale.x, size.y * global.scale.y)
            .set_color(Color::new(r, g, b, a))
            .set_visible(visible);
    }
    flush(batches, slots, &seen);
}

/// Hand each run the instances its members wrote, and drop the slots of the
/// nodes that no longer draw.
fn flush(batches: &mut Batches, slots: &mut HashMap<Entity, Slot2d>, seen: &HashSet<Entity>) {
    for run in &mut batches.runs {
        // Taken and put back: `set_instances` borrows the node, and the
        // buffer is kept so a frame of the same size allocates nothing.
        let instances = std::mem::take(&mut run.pending);
        run.node.set_instances(&instances);
        run.pending = instances;
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

/// Lean a node by its shear and answer how far its sprite's quad sits off
/// it: the shift turns and scales with the node, so it is taken through both.
fn lean_and_shift(slot: &mut Slot2d, renderable: &Renderable2d, global: &GlobalTransform) -> Vec2 {
    // The shear is the node's one instance, applied between rotation and
    // scale as Godot does; a polyline's group node has no object to lean.
    if slot.shear.to_bits() != global.skew.to_bits() && slot.node.data().object().is_some() {
        let (sin, cos) = (
            balaur_core::libm::sinf(global.skew),
            balaur_core::libm::cosf(global.skew),
        );
        slot.node.set_instances(&[kiss3d::scene::InstanceData2d {
            deformation: glamx::Mat2::from_cols(Vec2::new(1.0, 0.0), Vec2::new(-sin, cos)),
            ..Default::default()
        }]);
        slot.shear = global.skew;
    }
    match (&renderable.sprite, renderable.shape) {
        (Some(sprite), Shape2d::Sprite { hx, hy }) => {
            let [x, y] = sprite.centre(hx, hy);
            (global.affine_2d() * glamx::Vec3::new(x, y, 0.0)).truncate()
        }
        _ => Vec2::ZERO,
    }
}

/// A node's own colour with the tint every ancestor contributed multiplied
/// in, channel by channel, alpha included.
pub(crate) fn modulate(color: [f32; 4], tint: [f32; 4]) -> [f32; 4] {
    [
        color[0] * tint[0],
        color[1] * tint[1],
        color[2] * tint[2],
        color[3] * tint[3],
    ]
}

/// A polyline's pieces carry their own colour: the tint blended toward the
/// gradient by where each sits, since a group's colour stops at the group.
pub(crate) fn tint_pieces(slot: &mut Slot2d, renderable: &Renderable2d, inherited: [f32; 4]) {
    let [r, g, b, a] = modulate(renderable.color, inherited);
    let end = renderable
        .line
        .as_ref()
        .and_then(|style| style.gradient)
        .map_or([r, g, b, a], |gradient| modulate(gradient, inherited));
    for (piece, along) in &mut slot.pieces {
        let t = *along;
        piece.set_color(Color::new(
            r + (end[0] - r) * t,
            g + (end[1] - g) * t,
            b + (end[2] - b) * t,
            a + (end[3] - a) * t,
        ));
    }
}

/// Remap the node's UVs to the sprite's sheet frame, with the U or V extents
/// swapped for flips.
pub(crate) fn sync_sprite_uvs(
    node: &mut SceneNode2d,
    sprite: &SpriteTexture,
    drawn: Option<(u32, u32)>,
) {
    let sheet = sprite
        .sheet
        .map(|s| kiss3d::scene::SpriteSheet::new(s.columns, s.rows));
    let (mut min, mut max) = sheet.map_or((Vec2::ZERO, Vec2::ONE), |s| s.frame_uv(sprite.frame));
    // A region is a rectangle of the image in pixels; it wins over a sheet.
    if let Some([x, y, w, h]) = sprite.region {
        let size = drawn.or_else(|| node.data().object().map(|o| o.data().texture().size));
        if let Some((tw, th)) = size.filter(|(tw, th)| *tw > 0 && *th > 0) {
            let (tw, th) = (tw as f32, th as f32);
            min = Vec2::new(x as f32 / tw, y as f32 / th);
            max = Vec2::new((x + w) as f32 / tw, (y + h) as f32 / th);
        }
    }
    if sheet.is_some() && sprite.region.is_none() {
        // The sliver keeps a nearest-sampled edge fragment from rounding into
        // the neighbouring frame, matching kiss3d's own `set_sprite_frame`.
        let size = node.data().object().map(|o| o.data().texture().size);
        if let Some((w, h)) = size
            && w > 1
            && h > 1
        {
            let inset = Vec2::new(0.05 / w as f32, 0.05 / h as f32);
            min += inset;
            max -= inset;
        }
    }
    if sprite.flip_x {
        std::mem::swap(&mut min.x, &mut max.x);
    }
    if sprite.flip_y {
        std::mem::swap(&mut min.y, &mut max.y);
    }
    node.set_uv_rect(min, max);
}

#[cfg(test)]
mod tests {
    use balaur_core::{App, AppConfig};

    /// A tilemap draws by its `z_index` like everything else in 2D: the pass
    /// that builds it runs last, which used to put every map over every
    /// sprite whatever the scene said.
    #[test]
    fn a_tilemap_takes_its_place_in_the_draw_order() {
        let app = App::new(AppConfig::bare(std::path::PathBuf::from("tests/fixtures"))).unwrap();
        let root = app.engine.root();
        let sky = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Sky", root);
        let ship = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Ship", root);
        for (entity, layer) in [(sky, -100), (ship, -1)] {
            let world = app.engine.world_mut();
            let mut appearance = world
                .get::<&mut balaur_core::GlobalAppearance>(entity)
                .expect("a spawned node carries an appearance");
            appearance.z_index = layer;
        }
        let order = super::ordered_nodes(&app.engine.world(), root, |_| true);
        let places = |entity| order.iter().position(|held| *held == entity);
        assert!(
            places(sky) < places(ship),
            "the map at -100 draws under the ship at -1"
        );
    }
}
