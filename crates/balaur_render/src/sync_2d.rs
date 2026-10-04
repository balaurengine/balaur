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
    for entity in in_tree_order(world, root) {
        if world.get::<&Renderable2d>(entity).is_ok() {
            desired.push(layer_of(world, entity));
        }
    }
    sorted(desired)
}

/// `root` and everything under it in tree order: a node before its children,
/// and a child's subtree before the next child's, so at one `z_index` and z
/// a later sibling draws over an earlier one, as in Godot.
fn in_tree_order(world: &balaur_core::hecs::World, root: Entity) -> Vec<Entity> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        out.push(entity);
        if let Ok(children) = world.get::<&balaur_core::scene::Children>(entity) {
            stack.extend(children.0.iter().rev().copied());
        }
    }
    out
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

fn sorted(desired: Vec<(i32, f32, Entity)>) -> Vec<Entity> {
    sorted_layers(desired)
        .into_iter()
        .map(|(_, entity)| entity)
        .collect()
}

fn sorted_layers(mut desired: Vec<(i32, f32, Entity)>) -> Vec<(i32, Entity)> {
    desired.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    desired
        .into_iter()
        .map(|(layer, _, entity)| (layer, entity))
        .collect()
}

/// One place in the 2D draw order: a node's own, the holder of what scripts
/// drew at one `z_index`, or the light map's composite.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Placed {
    Node(Entity),
    Layer(i32),
    Composite,
}

/// The drawing nodes under `root`, in the order they draw: `z_index` first,
/// then z, then where the tree put them, each with its index. `has_node`
/// says which entities have something in the scene, whichever pass built it.
fn ordered_nodes(
    world: &balaur_core::hecs::World,
    root: Entity,
    has_node: impl Fn(Entity) -> bool,
) -> Vec<(i32, Entity)> {
    let mut desired: Vec<(i32, f32, Entity)> = Vec::new();
    for entity in in_tree_order(world, root) {
        if has_node(entity) {
            desired.push(layer_of(world, entity));
        }
    }
    sorted_layers(desired)
}

/// The nodes in their order with each drawn layer after the last node of
/// its index, so it draws over that index and under the next.
pub(crate) fn with_layers(
    nodes: Vec<(i32, Entity)>,
    layers: impl Iterator<Item = i32>,
) -> Vec<Placed> {
    let mut layers = layers.peekable();
    let mut out = Vec::with_capacity(nodes.len());
    for (layer, entity) in nodes {
        while let Some(z) = layers.next_if(|z| *z < layer) {
            out.push(Placed::Layer(z));
        }
        out.push(Placed::Node(entity));
    }
    out.extend(layers.map(Placed::Layer));
    out
}

/// The order with the light map's composite in it: every lit node and drawn
/// layer before it, and the unlit nodes after it, in their own order. With no
/// composite one order holds everything.
pub(crate) fn arrange(
    nodes: Vec<(i32, Entity)>,
    layers: impl Iterator<Item = i32>,
    unlit: impl Fn(Entity) -> bool,
    composite: bool,
) -> Vec<Placed> {
    if !composite {
        return with_layers(nodes, layers);
    }
    let (after, before): (Vec<_>, Vec<_>) =
        nodes.into_iter().partition(|(_, entity)| unlit(*entity));
    let mut out = with_layers(before, layers);
    out.push(Placed::Composite);
    out.extend(after.into_iter().map(|(_, entity)| Placed::Node(entity)));
    out
}

/// What the 2D order places beside the sprite slots and runs.
pub(crate) struct Placeable<'a> {
    pub(crate) maps: &'a HashMap<Entity, crate::tilemap::TilemapSlot>,
    pub(crate) text: &'a crate::world_text::Frame,
    /// Particles, which stay out of the light map.
    pub(crate) emitters: &'a HashMap<Entity, crate::particles::EmitterSlot>,
    pub(crate) layers: &'a crate::draw_2d::Layers2d,
    /// The light map's composite, while the scene has a light.
    pub(crate) composite: Option<SceneNode2d>,
}

/// Sprites, polygons, tilemaps, text, particles, the layers scripts draw at
/// and the light map's composite in one order.
///
/// kiss3d draws 2D nodes in the order they were added, and a tilemap builds
/// its node in a pass of its own, so without this every map drew over every
/// sprite whatever the two said in `z_index`. The nodes from the first
/// divergence on are detached and added again in the order, which costs
/// nothing on a frame whose order held.
pub(crate) fn order_layer_2d(
    app: &App,
    scene: &mut SceneNode2d,
    slots: &mut HashMap<Entity, Slot2d>,
    batches: &mut Batches,
    placeable: &Placeable<'_>,
    order_cache: &mut Vec<Placed>,
) {
    let maps = placeable.maps;
    let layers = placeable.layers;
    // A run draws where its first member would have: every other member is
    // inside it, so only the head stands in the order.
    let nodes = ordered_nodes(&app.engine.world(), app.engine.root(), |entity| {
        slots.contains_key(&entity)
            || maps.contains_key(&entity)
            || batches.head(entity).is_some()
            || placeable.text.draws_2d(entity)
            || placeable.emitters.contains_key(&entity)
    });
    let order = {
        let world = app.engine.world();
        arrange(
            nodes,
            layers.indices(),
            |entity| {
                placeable.emitters.contains_key(&entity)
                    || crate::lit_2d::lights_itself(&world, entity)
            },
            placeable.composite.is_some(),
        )
    };
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
    let mut node_of = |placed: &Placed| -> Option<SceneNode2d> {
        let entity = match *placed {
            Placed::Layer(z) => return layers.holder(z),
            Placed::Composite => return placeable.composite.clone(),
            Placed::Node(entity) => entity,
        };
        slots
            .get(&entity)
            .map(|slot| slot.node.clone())
            .or_else(|| maps.get(&entity).map(|slot| slot.node.clone()))
            .or_else(|| batches.head(entity).map(|run| run.node.clone()))
            .or_else(|| placeable.text.group_2d(entity))
            .or_else(|| crate::particles::group_of(placeable.emitters, entity))
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

/// The geometry a companion draws a node's overlay over, when one of
/// Balaur's own pipelines draws its surface: a polygon's authored triangles,
/// or the mesh a shader material is drawing. `None` where kiss3d draws it.
fn overlay_mesh(
    app: &App,
    node: &SceneNode2d,
    renderable: &Renderable2d,
    inherited: balaur_core::scene::MaterialId,
) -> Option<std::rc::Rc<std::cell::RefCell<kiss3d::resource::GpuMesh2d>>> {
    if let Some(polygon) = renderable.polygon.as_deref() {
        // A rig poses the vertices in the vertex stage; the rest pose would lie.
        if polygon.skin.is_some() || polygon.positions.is_empty() {
            return None;
        }
        let mesh = kiss3d::resource::GpuMesh2d::new(
            polygon.positions.clone(),
            polygon.indices.clone(),
            Some(polygon.uvs.clone()),
            false,
        );
        return Some(std::rc::Rc::new(std::cell::RefCell::new(mesh)));
    }
    let from_parent = inherited.reference();
    let reference = if renderable.material.is_empty() {
        from_parent.as_ref()
    } else {
        renderable.material.as_str()
    };
    let drawn_by_shader =
        !reference.is_empty() && crate::material::builtin_of(&app.engine, reference).is_none();
    drawn_by_shader
        .then(|| node.data().object().map(|object| object.mesh().clone()))
        .flatten()
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
    } else if matches!(renderable.shape, Shape2d::Flat(_)) {
        crate::texture::attach_texture_2d(&app.engine, &mut node, &renderable.texture);
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
    } else if let Some(lit) = renderable
        .lit
        .as_ref()
        .filter(|_| renderable.shape != Shape2d::Polygon)
    {
        crate::lit_2d::dress(app, &mut node, lit);
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
        overlay: None,
        nine: None,
        companion: None,
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
        let mut mesh = crate::batch_2d::batchable(&renderable)?;
        if renderable.sprite.as_ref().is_some_and(|s| s.nine.is_some()) {
            mesh = crate::batch_2d::Mesh::NineSlice(nine_of(
                app,
                &renderable,
                &SceneNode2d::empty(),
                true,
            )?);
        }
        let material = world
            .get::<&GlobalAppearance>(entity)
            .map_or_else(|_| GlobalAppearance::identity().material, |a| a.material);
        Some(crate::batch_2d::key_of(&renderable, mesh, material))
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
            if let crate::batch_2d::Mesh::NineSlice(key) = &run.key.mesh {
                set_nine_mesh(&mut node, Some(key));
            }
            crate::overlay::apply_2d(&mut node, &run.key.overlay);
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

/// One member's pose, tint and frame, as the instance its run draws it through.
fn write_instance(
    app: &App,
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
    let Some(run) = batches.runs.get_mut(run) else {
        return;
    };
    // A frame, a region or a flip is this instance's corner of the image the
    // whole run shares, so one sheet is still one call.
    let uv = match &renderable.sprite {
        Some(sprite) => {
            let drawn = sprite
                .region
                .and_then(|_| crate::texture::size_of(&app.engine, &sprite.path).ok());
            let texture = run.node.data().object().map(|o| o.data().texture().size);
            let (min, max) = sprite_uv_rect(sprite, drawn, texture);
            [min.x, min.y, max.x, max.y]
        }
        None => kiss3d::scene::UV_WHOLE_2D,
    };
    run.pending.push(kiss3d::scene::InstanceData2d {
        position: at,
        deformation,
        color,
        uv,
        ..Default::default()
    });
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
            write_instance(app, &world, entity, *run, batches);
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
        let from_file = renderable.sprite.is_some()
            || renderable.polyline.is_some()
            || !renderable.texture.is_empty();
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
        let size = frame_sprite(app, slot, &renderable);
        // Every frame: the rig moved even when nothing about the polygon did.
        if let (Some(handle), Some(polygon)) = (&slot.skin, renderable.polygon.as_deref()) {
            handle.set(polygon_palette(&world, entity, polygon));
        }
        if let (Some(handle), Some(polygon)) = (&slot.deform, renderable.polygon.as_deref()) {
            slot.deformed =
                crate::skinned_2d::write_deform(&world, entity, polygon, handle, slot.deformed);
        }
        tint_pieces(slot, &renderable, inherited);
        if slot.overlay != Some(renderable.overlay) {
            crate::overlay::apply_2d(&mut slot.node, &renderable.overlay);
            let mesh = overlay_mesh(app, &slot.node, &renderable, appearance.material);
            crate::overlay::companion_2d(
                &mut slot.node,
                &mut slot.companion,
                &renderable.overlay,
                mesh,
            );
            slot.overlay = Some(renderable.overlay);
        }
        let (angle, _, _) = global.rotation.to_euler(glamx::EulerRot::ZYX);
        let shift = lean_and_shift(slot, &renderable, &global);
        let visible = appearance.visible
            && draws(&renderable, appearance.material)
            && draw_multimesh(app, &world, entity, slot, &renderable, &global);
        slot.node
            .set_position(Vec2::new(global.position.x, global.position.y) + shift)
            .set_rotation(angle)
            .set_local_scale(size.x * global.scale.x, size.y * global.scale.y)
            .set_color(Color::new(r, g, b, a))
            .set_visible(visible);
    }
    flush(batches, slots, &seen);
}

/// A `multimesh2d`'s copies onto the node and its overlay companion; answers
/// whether any is drawn, and `true` for a node that carries none.
fn draw_multimesh(
    app: &App,
    world: &balaur_core::hecs::World,
    entity: Entity,
    slot: &mut Slot2d,
    renderable: &Renderable2d,
    global: &GlobalTransform,
) -> bool {
    let Ok(multimesh) = world.get::<&crate::MultiMesh>(entity) else {
        return true;
    };
    let drawn = renderable
        .polygon
        .as_ref()
        .and_then(|polygon| crate::texture::size_of(&app.engine, &polygon.texture).ok());
    let any = {
        let mut data = slot.node.data_mut();
        let Some(object) = data.object_mut() else {
            return true;
        };
        crate::instancing::draw_multimesh_2d(object, &multimesh, global, drawn)
    };
    // The shear cached with the old instance is gone with it.
    slot.shear = f32::NAN;
    // The companion sits at the node's origin, so the same copies in its own
    // space land where the node's do.
    if let Some(companion) = slot.companion.as_mut()
        && let Some(object) = companion.data_mut().object_mut()
    {
        crate::instancing::draw_multimesh_2d(object, &multimesh, global, drawn);
    }
    any
}

/// A sprite's size and frame onto its node: the nine-slice mesh it bakes,
/// or the unit quad's UVs. Answers the scale the node takes for its size.
fn frame_sprite(app: &App, slot: &mut Slot2d, renderable: &Renderable2d) -> Vec2 {
    // A sprite is the one 2D shape still built at unit size: its extents
    // come from the image and change without rebuilding the node. A
    // nine-slice one bakes its size, since its corners must not stretch.
    let nine = nine_of(app, renderable, &slot.node, false);
    if slot.nine != nine {
        set_nine_mesh(&mut slot.node, nine.as_ref());
        slot.nine = nine;
    }
    let size = match renderable.shape {
        Shape2d::Sprite { .. } if nine.is_some() => Vec2::ONE,
        Shape2d::Sprite { hx, hy } => Vec2::new(2.0 * hx, 2.0 * hy),
        _ => Vec2::ONE,
    };
    // Every sync, not just on rebuild: frames and flips are UV changes, so
    // an animation that flips frames must not rebuild the node.
    if let Some(sprite) = renderable.sprite.as_ref().filter(|_| nine.is_none()) {
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
    size
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

/// Whether a renderable puts anything on screen. A sprite with no image and
/// no material draws nothing, as Godot's does: a script clears the texture to
/// hide what it showed. It keeps its placeholder size for picking.
fn draws(renderable: &Renderable2d, inherited: balaur_core::scene::MaterialId) -> bool {
    renderable.sprite.as_ref().is_none_or(|sprite| {
        !sprite.path.is_empty() || !renderable.material.is_empty() || !inherited.is_none()
    })
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
    let texture = node.data().object().map(|o| o.data().texture().size);
    let (min, max) = sprite_uv_rect(sprite, drawn, texture);
    node.set_uv_rect(min, max);
}

/// The corner of the image a sprite draws, in the mesh's own UVs.
///
/// `drawn` is the size a region is measured against and `texture` the size of
/// the image as it was uploaded, which is what the sheet's sliver is in.
pub(crate) fn sprite_uv_rect(
    sprite: &SpriteTexture,
    drawn: Option<(u32, u32)>,
    texture: Option<(u32, u32)>,
) -> (Vec2, Vec2) {
    let sheet = sprite
        .sheet
        .map(|s| kiss3d::scene::SpriteSheet::new(s.columns, s.rows));
    let (mut min, mut max) = sheet.map_or((Vec2::ZERO, Vec2::ONE), |s| s.frame_uv(sprite.frame));
    // A region is a rectangle of the image in pixels; it wins over a sheet.
    if let Some([x, y, w, h]) = sprite.region {
        let size = drawn.or(texture);
        if let Some((tw, th)) = size.filter(|(tw, th)| *tw > 0 && *th > 0) {
            let (tw, th) = (tw as f32, th as f32);
            min = Vec2::new(x as f32 / tw, y as f32 / th);
            max = Vec2::new((x + w) as f32 / tw, (y + h) as f32 / th);
        }
    }
    if sheet.is_some() && sprite.region.is_none() {
        // The sliver keeps a nearest-sampled edge fragment from rounding into
        // the neighbouring frame, matching kiss3d's own `set_sprite_frame`.
        if let Some((w, h)) = texture
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
    (min, max)
}

/// The nine-slice mesh a sprite draws, or `None` for one drawn as a plain
/// quad. `whole` builds it over the whole image, for a run whose members
/// each pick their own rectangle in their instance.
pub(crate) fn nine_of(
    app: &App,
    renderable: &Renderable2d,
    node: &SceneNode2d,
    whole: bool,
) -> Option<crate::sprite::NineKey> {
    let sprite = renderable.sprite.as_ref()?;
    let margins = sprite.nine?;
    let Shape2d::Sprite { hx, hy } = renderable.shape else {
        return None;
    };
    let drawn = crate::texture::size_of(&app.engine, &sprite.path).ok();
    let cell = match (sprite.region, sprite.sheet, drawn) {
        (Some([_, _, w, h]), _, _) | (None, None, Some((w, h))) => (w as f32, h as f32),
        (None, Some(sheet), Some((w, h))) => (
            w as f32 / sheet.columns.max(1) as f32,
            h as f32 / sheet.rows.max(1) as f32,
        ),
        _ => (1.0, 1.0),
    };
    let rect = if whole {
        let mut rect = kiss3d::scene::UV_WHOLE_2D;
        if sprite.flip_x {
            rect.swap(0, 2);
        }
        if sprite.flip_y {
            rect.swap(1, 3);
        }
        rect
    } else {
        let uploaded = node.data().object().map(|o| o.data().texture().size);
        let (min, max) = sprite_uv_rect(sprite, sprite.region.and(drawn), uploaded);
        [min.x, min.y, max.x, max.y]
    };
    Some(crate::sprite::nine_key(
        margins,
        (hx, hy),
        renderable.pixels_per_unit,
        cell,
        rect,
    ))
}

/// The mesh a nine-slice key describes: kiss3d's own nine-slice, its UVs
/// carried into the rectangle the sprite draws.
pub(crate) fn nine_mesh(key: &crate::sprite::NineKey) -> kiss3d::resource::GpuMesh2d {
    use kiss3d::scene::Border;
    let border = |[left, right, top, bottom]: [f32; 4]| Border {
        left,
        right,
        top,
        bottom,
    };
    let mesh = kiss3d::scene::nine_slice_mesh(
        Vec2::from_array(key.size),
        border(key.world),
        border(key.uv),
    );
    let [x0, y0, x1, y1] = key.rect;
    if let Some(uvs) = mesh
        .uvs()
        .write()
        .ok()
        .as_mut()
        .and_then(|u| u.data_mut().as_mut())
    {
        for uv in uvs.iter_mut() {
            *uv = Vec2::new(x0 + uv.x * (x1 - x0), y0 + uv.y * (y1 - y0));
        }
    }
    mesh
}

/// Give a sprite's node the nine-slice mesh it now draws, or a plain unit
/// quad back. The quad kiss3d hands sprites is shared, so it is copied first.
fn set_nine_mesh(node: &mut SceneNode2d, key: Option<&crate::sprite::NineKey>) {
    let mesh = match key {
        Some(key) => nine_mesh(key),
        None => kiss3d::scene::nine_slice_mesh(
            Vec2::ONE,
            kiss3d::scene::Border::uniform(0.0),
            kiss3d::scene::Border::uniform(0.0),
        ),
    };
    let mut mesh = Some(mesh);
    node.apply_to_object_mut(&mut |object| {
        object.make_mesh_unique();
        if let Some(mesh) = mesh.take() {
            *object.mesh().borrow_mut() = mesh;
        }
    });
}

#[cfg(test)]
mod tests {
    use balaur_core::{App, AppConfig};

    fn sprite_draws(texture: &str) -> bool {
        let mut app = App::new(AppConfig::bare(std::path::PathBuf::from("."))).unwrap();
        balaur_plugin::load(&mut app, &mut crate::RenderPlugin::default()).unwrap();
        let root = app.engine.root();
        let node = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Mast", root);
        let table: toml::Value = toml::from_str(&format!("texture = \"{texture}\"")).unwrap();
        balaur_core::components::add(&app.engine, node, "sprite", Some(&table)).unwrap();
        let world = app.engine.world();
        let renderable = world.get::<&crate::Renderable2d>(node).unwrap();
        super::draws(&renderable, balaur_core::scene::MaterialId::NONE)
    }

    #[test]
    fn a_sprite_with_no_image_draws_nothing() {
        assert!(
            sprite_draws("tests/fixtures/sprite_200x100.png"),
            "control: an image draws"
        );
        assert!(!sprite_draws(""));
    }

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
        let places = |entity| order.iter().position(|(_, held)| *held == entity);
        assert!(
            places(sky) < places(ship),
            "the map at -100 draws under the ship at -1"
        );
    }

    /// Siblings at one index draw in tree order: the later one over the
    /// earlier one, and a parent under its children.
    #[test]
    fn a_later_sibling_draws_over_an_earlier_one() {
        let app = App::new(AppConfig::bare(std::path::PathBuf::from("tests/fixtures"))).unwrap();
        let root = app.engine.root();
        let spawn = |name: &str, parent| {
            balaur_core::scene::spawn_node(&mut app.engine.world_mut(), name, parent)
        };
        let table = spawn("Table", root);
        let map = spawn("Map", root);
        let coast = spawn("Coast", map);
        let order = super::ordered_nodes(&app.engine.world(), root, |_| true);
        let place = |entity| order.iter().position(|(_, held)| *held == entity);
        assert!(
            place(table) < place(map),
            "the map, added later, draws over the table"
        );
        assert!(place(map) < place(coast), "a child draws over its parent");
    }

    #[test]
    fn particles_draw_after_the_light_map_in_their_own_order() {
        use super::Placed::{Composite, Layer, Node};
        let mut world = balaur_core::hecs::World::new();
        let (ground, sparks, smoke, sign) = (
            world.spawn(()),
            world.spawn(()),
            world.spawn(()),
            world.spawn(()),
        );
        let nodes = vec![(-5, sparks), (0, ground), (2, smoke), (3, sign)];
        let unlit = |entity| entity == sparks || entity == smoke;
        let lit = super::arrange(nodes.clone(), [1].into_iter(), unlit, true);
        assert_eq!(
            lit,
            [
                Node(ground),
                Layer(1),
                Node(sign),
                Composite,
                Node(sparks),
                Node(smoke)
            ],
            "everything the light map multiplies comes before it"
        );
        let dark = super::arrange(nodes, [1].into_iter(), unlit, false);
        assert_eq!(
            dark,
            [
                Node(sparks),
                Node(ground),
                Layer(1),
                Node(smoke),
                Node(sign)
            ],
            "with no light, particles take their z_index place"
        );
    }

    /// A shape a script draws at an index sits over that index's nodes and
    /// under the next one's; an index no node has still takes its place.
    #[test]
    fn a_drawn_layer_sits_after_the_nodes_of_its_index() {
        use super::Placed::{Layer, Node};
        let mut world = balaur_core::hecs::World::new();
        let (sea, ship, flag) = (world.spawn(()), world.spawn(()), world.spawn(()));
        let nodes = vec![(-1, sea), (0, ship), (5, flag)];
        let order = super::with_layers(nodes, [-3, 0, 2, 9].into_iter());
        assert_eq!(
            order,
            [
                Layer(-3),
                Node(sea),
                Node(ship),
                Layer(0),
                Layer(2),
                Node(flag),
                Layer(9)
            ]
        );
    }
}
