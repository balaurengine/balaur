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

mod look;
use crate::{Renderable2d, Shape2d};
use look::{draw_multimesh, draws, frame_sprite, lean_and_shift, set_nine_mesh};
pub(crate) use look::{modulate, nine_of, sprite_uv_rect, tint_pieces};

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
    let drawn: Vec<Entity> = world
        .query::<(Entity, &Renderable2d)>()
        .iter()
        .map(|(entity, _)| entity)
        .collect();
    let desired = balaur_core::scene::tree_rank(world, root)
        .sorted(drawn)
        .into_iter()
        .map(|entity| layer_of(world, entity))
        .collect();
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
    // In tree order, so at one `z_index` and z a later sibling draws over an
    // earlier one, as in Godot; the order is kept until the tree changes.
    let desired = balaur_core::scene::tree_rank(world, root)
        .order()
        .iter()
        .copied()
        .filter(|&entity| has_node(entity))
        .map(|entity| layer_of(world, entity))
        .collect();
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
    /// What the mirror was told changed this frame.
    pub(crate) dirty: &'a HashSet<Entity>,
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
    let around = Around::of(placeable);
    // A tilemap, text or particles node that moved may have moved in the order.
    let moved = around
        .drawn
        .iter()
        .any(|entity| placeable.dirty.contains(entity));
    if !std::mem::take(&mut batches.order_again) && around == batches.around && !moved {
        return;
    }
    batches.around = around;
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
                    || crate::lit_2d::lights_itself(&app.engine, &world, entity)
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
    /// The instances, one per member that is visible.
    pending: Vec<kiss3d::scene::InstanceData2d>,
    /// Every member in the order its instance was written, and where each
    /// visible one's sits in `pending`: what a member written alone rewrites.
    written: Vec<Entity>,
    at: HashMap<Entity, usize>,
    /// Whether `pending` moved since it was last handed to the node.
    touched: bool,
}

/// What a node's place in the 2D order is read from: its `z_index`, its z,
/// and whether it lights itself, which puts it before the light map.
type Placing = (i32, u32, bool);

/// The 2D order's other inputs: the nodes the tilemaps, text and particles
/// draw through, the layers scripts draw at, and the light map's composite.
#[derive(Default, PartialEq)]
pub(crate) struct Around {
    drawn: Vec<Entity>,
    layers: Vec<i32>,
    composite: bool,
}

impl Around {
    fn of(placeable: &Placeable<'_>) -> Self {
        let mut drawn: Vec<Entity> = placeable
            .maps
            .keys()
            .chain(placeable.emitters.keys())
            .copied()
            .chain(placeable.text.drawn_2d())
            .collect();
        drawn.sort_unstable();
        Self {
            drawn,
            layers: placeable.layers.indices().collect(),
            composite: placeable.composite.is_some(),
        }
    }
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
    /// The tree's shape the order was last worked out at, and each drawing
    /// node's place in it.
    shape: u64,
    placing: HashMap<Entity, Placing>,
    /// Each drawing node's run key at the last cut, and the run it is in.
    keys: HashMap<Entity, Option<crate::batch_2d::BatchKey>>,
    member_of: HashMap<Entity, usize>,
    /// The nodes drawn again every frame: a rig, a deform, copies, or a build
    /// that failed and is tried again.
    live: std::collections::HashSet<Entity>,
    /// Whether the scene's 2D order has to be laid out again, and what was
    /// around it when it last was.
    order_again: bool,
    around: Around,
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
    } else if let Some(lit) =
        crate::lit_2d::of(&app.engine, reference).filter(|_| renderable.shape != Shape2d::Polygon)
    {
        crate::lit_2d::dress(app, &mut node, &lit);
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
    batches.keys.clear();
    let cut = crate::batch_2d::runs(order, |entity| {
        let key = run_key(app, world, entity);
        batches.keys.insert(entity, key.clone());
        key
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
            let surface = crate::material::flat_surface_of(&app.engine, &run.key.material);
            crate::overlay::apply_2d(&mut node, &surface, &run.key.debug);
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
                written: Vec::new(),
                at: HashMap::new(),
                touched: false,
            });
        }
    }
    let mut member_of = HashMap::new();
    batches.heads.clear();
    for (index, run) in batches.runs.iter_mut().enumerate() {
        run.pending.clear();
        run.written.clear();
        run.at.clear();
        if let Some(head) = run.members.first() {
            batches.heads.insert(*head, index);
        }
        for &entity in &run.members {
            member_of.insert(entity, index);
        }
    }
    batches.member_of.clone_from(&member_of);
    member_of
}

/// The run `entity` could be drawn through, or `None` where it draws alone.
fn run_key(
    app: &App,
    world: &balaur_core::hecs::World,
    entity: Entity,
) -> Option<crate::batch_2d::BatchKey> {
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
    let reference = if renderable.material.is_empty() {
        material.reference().to_string()
    } else {
        renderable.material.clone()
    };
    if crate::lit_2d::of(&app.engine, &reference).is_some() {
        return None;
    }
    let debug = crate::debug_draw::DebugDraw::of(world, entity);
    Some(crate::batch_2d::key_of(&renderable, mesh, debug, material))
}

/// Where `entity` stands in the 2D order, or `None` for a node with nothing
/// 2D to draw.
fn placing_of(app: &App, world: &balaur_core::hecs::World, entity: Entity) -> Option<Placing> {
    world.get::<&Renderable2d>(entity).ok()?;
    let (layer, z, _) = layer_of(world, entity);
    let lit = crate::lit_2d::lights_itself(&app.engine, world, entity);
    Some((layer, z.to_bits(), lit))
}

/// One member's pose, tint and frame, as the instance its run draws it through.
fn write_instance(
    app: &App,
    world: &balaur_core::hecs::World,
    entity: Entity,
    run: usize,
    batches: &mut Batches,
) {
    let Some(run) = batches.runs.get_mut(run) else {
        return;
    };
    let made = instance(app, world, entity, &run.node);
    run.written.push(entity);
    if let Some(made) = made {
        run.at.insert(entity, run.pending.len());
        run.pending.push(made);
    }
}

/// One member's instance, `None` while it is hidden.
fn instance(
    app: &App,
    world: &balaur_core::hecs::World,
    entity: Entity,
    node: &SceneNode2d,
) -> Option<kiss3d::scene::InstanceData2d> {
    let (Ok(renderable), Ok(global)) = (
        world.get::<&Renderable2d>(entity),
        world.get::<&GlobalTransform>(entity),
    ) else {
        return None;
    };
    let appearance = world
        .get::<&GlobalAppearance>(entity)
        .map_or_else(|_| GlobalAppearance::identity(), |a| *a);
    if !appearance.visible {
        return None;
    }
    let (at, deformation) = crate::batch_2d::pose(&renderable, &global);
    let color = crate::film::rgba(modulate(renderable.color, appearance.tint.to_array()));
    // A frame, a region or a flip is this instance's corner of the image the
    // whole run shares, so one sheet is still one call.
    let uv = match &renderable.sprite {
        Some(sprite) => {
            let drawn = sprite
                .region
                .and_then(|_| crate::texture::size_of(&app.engine, &sprite.path).ok());
            let texture = node.data().object().map(|o| o.data().texture().size);
            let (min, max) = sprite_uv_rect(sprite, drawn, texture);
            [min.x, min.y, max.x, max.y]
        }
        None => kiss3d::scene::UV_WHOLE_2D,
    };
    Some(kiss3d::scene::InstanceData2d {
        position: at,
        deformation,
        color,
        uv,
        ..Default::default()
    })
}

/// Write one member's instance again where it stands among its run's; one
/// shown or hidden lays the run's out again in the order written. False for
/// a node no run draws.
fn rewrite_instance(
    app: &App,
    world: &balaur_core::hecs::World,
    entity: Entity,
    batches: &mut Batches,
) -> bool {
    let Some(&index) = batches.member_of.get(&entity) else {
        return false;
    };
    let Some(run) = batches.runs.get_mut(index) else {
        return true;
    };
    match (
        run.at.get(&entity).copied(),
        instance(app, world, entity, &run.node),
    ) {
        (Some(at), Some(made)) => run.pending[at] = made,
        (None, None) => return true,
        _ => {
            run.pending.clear();
            run.at.clear();
            for &member in &run.written {
                if let Some(made) = instance(app, world, member, &run.node) {
                    run.at.insert(member, run.pending.len());
                    run.pending.push(made);
                }
            }
        }
    }
    run.touched = true;
    true
}

/// Mirror every 2D renderable when `dirty` says everything, or the tree's
/// shape, a node's place in the order or its run moved; otherwise only the
/// ones it names and the ones drawn again every frame.
pub(crate) fn sync_2d(
    app: &App,
    scene: &mut SceneNode2d,
    slots: &mut HashMap<Entity, Slot2d>,
    batches: &mut Batches,
    materials: &mut crate::shader_material::MaterialCache,
    (reloaded, dirty): (bool, &crate::mirror::Dirty),
) {
    let world = app.engine.world();
    // A relink rebuilds the nodes holding the old pipeline; a channel view
    // rebuilds every node, whether or not it names a material.
    let channel = crate::debug_view::channel_view(&app.engine);
    let relinked = materials.refresh(app);
    let channel_changed = materials.channel_changed(&channel);
    materials.answer_probe(app);
    let frame = Frame2d {
        app,
        channel: &channel,
        rebuild: (channel_changed, relinked, reloaded),
    };
    let shape = balaur_core::scene::shape_revision();
    let everything = dirty.all
        || relinked
        || channel_changed
        || reloaded
        || shape != batches.shape
        || dirty.nodes.iter().any(|&entity| {
            placing_of(app, &world, entity).as_ref() != batches.placing.get(&entity)
                || batches
                    .keys
                    .get(&entity)
                    .is_some_and(|key| *key != run_key(app, &world, entity))
        });
    if !everything {
        let mut visit: Vec<Entity> = dirty
            .nodes
            .iter()
            .chain(batches.live.iter())
            .copied()
            .collect();
        visit.sort_unstable();
        visit.dedup();
        for entity in visit {
            if rewrite_instance(app, &world, entity, batches) {
                continue;
            }
            let (Ok(renderable), Ok(global)) = (
                world.get::<&Renderable2d>(entity),
                world.get::<&GlobalTransform>(entity),
            ) else {
                continue;
            };
            let place = (entity, &*renderable, &*global);
            let built = sync_one_2d(
                &frame,
                scene,
                slots,
                materials,
                &world,
                place,
                &mut batches.live,
            );
            batches.order_again |= built;
        }
        flush(batches, slots, None);
        return;
    }
    let order = draw_order_2d(&world, app.engine.root());
    batches.shape = shape;
    batches.placing = order
        .iter()
        .filter_map(|&entity| placing_of(app, &world, entity).map(|placing| (entity, placing)))
        .collect();
    batches.order_again = true;
    let member_of = cut_runs(app, scene, materials, &channel, &world, &order, batches);
    batches.live.clear();
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
        let place = (entity, &*renderable, &*global);
        sync_one_2d(
            &frame,
            scene,
            slots,
            materials,
            &world,
            place,
            &mut batches.live,
        );
    }
    flush(batches, slots, Some(&seen));
}

/// What one frame's 2D sync reads the same for every node.
struct Frame2d<'a> {
    app: &'a App,
    channel: &'a str,
    /// The channel view changed, a material relinked, an asset reloaded.
    rebuild: (bool, bool, bool),
}

/// One renderable onto its slot, built when it has none or must be built
/// again; answers whether it was. Noted `live` when what it draws moves
/// without a write: a rig, a deform, copies, or a build that failed and is
/// tried again.
fn sync_one_2d(
    frame: &Frame2d<'_>,
    scene: &mut SceneNode2d,
    slots: &mut HashMap<Entity, Slot2d>,
    materials: &mut crate::shader_material::MaterialCache,
    world: &balaur_core::hecs::World,
    (entity, renderable, global): (Entity, &Renderable2d, &GlobalTransform),
    live: &mut HashSet<Entity>,
) -> bool {
    let (channel_changed, relinked, reloaded) = frame.rebuild;
    let app = frame.app;
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
            frame.channel,
            renderable,
            appearance.material,
        ) else {
            live.insert(entity);
            return true;
        };
        slots.insert(entity, slot);
    }
    // The block above inserts the slot when it is missing.
    let slot = slots.get_mut(&entity).unwrap();
    let inherited = appearance.tint.to_array();
    let color = crate::film::color(modulate(renderable.color, inherited));
    let size = frame_sprite(app, slot, renderable);
    // Every frame: the rig moved even when nothing about the polygon did.
    if let (Some(handle), Some(polygon)) = (&slot.skin, renderable.polygon.as_deref()) {
        handle.set(polygon_palette(world, entity, polygon));
    }
    if let (Some(handle), Some(polygon)) = (&slot.deform, renderable.polygon.as_deref()) {
        slot.deformed =
            crate::skinned_2d::write_deform(world, entity, polygon, handle, slot.deformed);
    }
    tint_pieces(slot, renderable, inherited);
    let debug = crate::debug_draw::DebugDraw::of(world, entity);
    let reference = if renderable.material.is_empty() {
        appearance.material.reference().to_string()
    } else {
        renderable.material.clone()
    };
    let surface = crate::material::flat_surface_of(&app.engine, &reference);
    if slot.overlay != Some((surface, debug)) {
        crate::overlay::apply_2d(&mut slot.node, &surface, &debug);
        let mesh = overlay_mesh(app, &slot.node, renderable, appearance.material);
        crate::overlay::companion_2d(&mut slot.node, &mut slot.companion, &debug, mesh);
        slot.overlay = Some((surface, debug));
    }
    let (angle, _, _) = global.rotation.to_euler(glamx::EulerRot::ZYX);
    let shift = lean_and_shift(slot, renderable, global);
    let visible = appearance.visible
        && draws(renderable, appearance.material)
        && draw_multimesh(app, world, entity, slot, renderable, global);
    slot.node
        .set_position(Vec2::new(global.position.x, global.position.y) + shift)
        .set_rotation(angle)
        .set_local_scale(size.x * global.scale.x, size.y * global.scale.y)
        .set_color(color)
        .set_visible(visible);
    let moving = slot.skin.is_some()
        || slot.deform.is_some()
        || world.get::<&crate::MultiMesh>(entity).is_ok();
    if moving {
        live.insert(entity);
    } else {
        live.remove(&entity);
    }
    rebuild
}

/// Hand each run the instances its members wrote, and drop the slots of the
/// nodes that no longer draw; with nothing `seen`, only the runs a member was
/// written again in, and no slot is dropped.
fn flush(
    batches: &mut Batches,
    slots: &mut HashMap<Entity, Slot2d>,
    seen: Option<&HashSet<Entity>>,
) {
    for run in &mut batches.runs {
        if seen.is_none() && !run.touched {
            continue;
        }
        run.touched = false;
        // Taken and put back: `set_instances` borrows the node, and the
        // buffer is kept so a frame of the same size allocates nothing.
        let instances = std::mem::take(&mut run.pending);
        run.node.set_instances(&instances);
        run.pending = instances;
    }
    let Some(seen) = seen else {
        return;
    };
    slots.retain(|entity, slot| {
        if seen.contains(entity) {
            true
        } else {
            slot.node.detach();
            false
        }
    });
}

#[cfg(test)]
mod tests {
    use balaur_core::{App, AppConfig};

    fn sprite_draws(texture: &str) -> bool {
        let mut app = App::new(AppConfig::bare(std::path::PathBuf::from("."))).unwrap();
        balaur_plugin::load(&mut app, &mut crate::RenderPlugin).unwrap();
        let root = app.engine.root();
        let node = app.engine.spawn_node("Mast", root);
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

    // The pass that builds a tilemap runs last, so it must still sort by `z_index` with everything
    // else in 2D.
    #[test]
    fn a_tilemap_takes_its_place_in_the_draw_order() {
        let app = App::new(AppConfig::bare(std::path::PathBuf::from("tests/fixtures"))).unwrap();
        let root = app.engine.root();
        let sky = app.engine.spawn_node("Sky", root);
        let ship = app.engine.spawn_node("Ship", root);
        for (entity, layer) in [(sky, -100), (ship, -1)] {
            let world = app.engine.world();
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

    // Siblings at one index draw in tree order: the later one over the
    // earlier one, and a parent under its children.
    #[test]
    fn a_later_sibling_draws_over_an_earlier_one() {
        let app = App::new(AppConfig::bare(std::path::PathBuf::from("tests/fixtures"))).unwrap();
        let root = app.engine.root();
        let spawn = |name: &str, parent| app.engine.spawn_node(name, parent);
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

    // A shape a script draws at an index sits over that index's nodes and
    // under the next one's; an index no node has still takes its place.
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
