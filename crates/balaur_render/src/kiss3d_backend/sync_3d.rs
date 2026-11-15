//! The 3D half of the backend's per-frame mirror: each `Renderable3d` onto
//! the kiss3d node or the instanced group that draws it.

use std::collections::{HashMap, HashSet};

use balaur_core::hecs::Entity;
use balaur_core::{App, GlobalAppearance, GlobalTransform};
use glamx::Pose3;
use kiss3d::prelude::*;

use super::{
    Slot, deform_node, dress, drop_unseen, geometry, instances, lit_and_layered, take_over,
};
use crate::Renderable3d;

/// Mirror `Renderable3d` + `GlobalTransform` into the kiss3d scene graph:
/// every renderable when `dirty` says everything or the groups are cut
/// again, and otherwise only the ones it names and the `live` ones, whose
/// drawing moves every frame.
pub(super) fn sync(
    app: &App,
    scene: &mut SceneNode3d,
    slots: &mut HashMap<Entity, Slot>,
    batches: &mut crate::batch_3d::Batches3d,
    materials: &mut crate::shader_material_3d::MaterialCache3d,
    (reloaded, dirty, live): (bool, &crate::mirror::Dirty, &mut HashSet<Entity>),
) {
    let world = app.engine.world();
    // A relink rebuilds the nodes holding the old pipeline; a channel view
    // rebuilds every node, whether or not it names a material.
    let channel = crate::debug_view::channel_view(&app.engine);
    let relinked = materials.refresh(app);
    let channel_changed = materials.channel_changed(&channel);
    materials.answer_probe(app);

    let eye = crate::lods::eye(&app.engine);
    if relinked || channel_changed || reloaded {
        batches.reconsider();
    }
    let frame = Frame3d {
        app,
        channel: &channel,
        rebuild: (channel_changed, relinked, reloaded),
        eye,
    };
    let everything = dirty.all
        || relinked
        || channel_changed
        || reloaded
        || batches.keys_moved(&world, &dirty.nodes);
    if !everything {
        let mut visit: Vec<Entity> = dirty.nodes.iter().chain(live.iter()).copied().collect();
        visit.sort_unstable();
        visit.dedup();
        for entity in visit {
            if crate::batch_3d::rewrite_instance(&world, entity, batches) {
                continue;
            }
            let mut found = world.query_one::<(&Renderable3d, &GlobalTransform)>(entity);
            let Ok((renderable, global)) = found.get() else {
                continue;
            };
            sync_one(
                &frame,
                scene,
                slots,
                materials,
                &world,
                (entity, renderable, global),
                live,
            );
        }
        crate::batch_3d::flush(batches, false);
        return;
    }
    let member_of = crate::batch_3d::cut_groups(app, scene, materials, &channel, batches);
    live.clear();
    let mut seen: HashSet<Entity> = HashSet::new();
    for (entity, renderable, global) in
        &mut world.query::<(Entity, &Renderable3d, &GlobalTransform)>()
    {
        if let Some(group) = member_of.get(&entity) {
            take_over(slots, entity, *group, batches, &world);
            continue;
        }
        seen.insert(entity);
        sync_one(
            &frame,
            scene,
            slots,
            materials,
            &world,
            (entity, renderable, global),
            live,
        );
    }
    crate::batch_3d::flush(batches, true);
    drop_unseen(slots, &seen);
}

/// What one frame's 3D sync reads the same for every node.
struct Frame3d<'a> {
    app: &'a App,
    channel: &'a str,
    /// The channel view changed, a material relinked, an asset reloaded.
    rebuild: (bool, bool, bool),
    eye: Vec3,
}

/// One renderable onto its slot, built when it has none or must be built
/// again; noted `live` when what it draws moves without a write: a rig, a
/// solver, levels of detail, copies, particles, morphs, or a build that failed
/// and is tried again.
fn sync_one(
    frame: &Frame3d<'_>,
    scene: &mut SceneNode3d,
    slots: &mut HashMap<Entity, Slot>,
    materials: &mut crate::shader_material_3d::MaterialCache3d,
    world: &balaur_core::hecs::World,
    (entity, renderable, global): (Entity, &Renderable3d, &GlobalTransform),
    live: &mut HashSet<Entity>,
) {
    let (channel_changed, relinked, reloaded) = frame.rebuild;
    let app = frame.app;
    // Read once: the ancestors' tint, visibility and material come off
    // the same propagated component.
    let appearance = world
        .get::<&GlobalAppearance>(entity)
        .map_or_else(|_| GlobalAppearance::identity(), |a| *a);
    let owns_material = !renderable.material.is_empty();
    // A reload rebuilds what was built from a file: the mesh is read
    // again and the texture uploaded under the new generation's name.
    let from_file = renderable.mesh.is_some() || !renderable.texture.is_empty();
    let rebuild = slots.get(&entity).is_none_or(|slot| {
        slot.version != renderable.version
            || channel_changed
            || (relinked && (owns_material || !appearance.material.is_none()))
            || (reloaded && from_file)
            || (!owns_material && slot.inherited != appearance.material)
    });
    // The material a node effectively draws with: its own, or the one
    // an ancestor handed down.
    let inherited = appearance.material.reference();
    let reference = if owns_material {
        renderable.material.as_str()
    } else {
        &inherited
    };
    if rebuild {
        if let Some(mut old) = slots.remove(&entity) {
            old.node.remove();
        }
        // Nothing to draw yet, and whatever failed said why.
        let Some((mut node, skin, geometry, lods)) =
            geometry::build_node(app, scene, renderable, entity)
        else {
            live.insert(entity);
            return;
        };
        // After the texture: a material reads it, and kiss3d's own
        // material stays on a node whose shader would not link.
        let custom = materials.for_node(app, reference, frame.channel);
        let mut palette = None;
        if let Some(material) = custom {
            node.set_material(material);
        } else if let Some(mesh) = geometry {
            palette = Some(crate::skinned_3d::attach(&mut node, &mesh));
        }
        slots.insert(
            entity,
            Slot {
                node,
                version: renderable.version,
                inherited: appearance.material,
                skin,
                palette,
                surface: None,
                overlay: None,
                builtin: None,
                lods,
                solved: None,
                companion: None,
                particles: None,
            },
        );
    }
    // The block above inserts the slot when it is missing.
    let slot = slots.get_mut(&entity).unwrap();
    deform_node(world, entity, slot);
    let color = crate::film::color(crate::sync_2d::modulate(
        renderable.color,
        appearance.tint.to_array(),
    ));
    // Every shape is real geometry at its authored size now, so the node
    // carries the scene's scale and nothing of the shape's.
    let scale = global.scale;
    let visible = appearance.visible && !crate::multimesh::draws_nothing(world, entity);
    if let Some(lods) = &mut slot.lods {
        lods.show(&mut slot.node, global.position.distance(frame.eye));
    }
    slot.node
        .set_pose(Pose3::from_parts(global.position, global.rotation))
        .set_local_scale(scale.x, scale.y, scale.z)
        .set_color(color)
        .set_visible(visible);
    lit_and_layered(&mut slot.node, renderable);
    let debug = crate::debug_draw::DebugDraw::of(world, entity);
    dress(app, slot, renderable, &debug, reference);
    crate::overlay::follow_3d(slot.companion.as_mut(), color, scale);
    instances::draw(app, world, entity, slot, (global, frame.eye, visible));
    let moving = slot.skin.is_some()
        || slot.solved.is_some()
        || slot.lods.is_some()
        || slot.particles.is_some()
        || world.get::<&crate::MorphWeights>(entity).is_ok()
        || world.get::<&crate::MultiMesh>(entity).is_ok()
        || world.get::<&crate::particles_3d::Emitter3d>(entity).is_ok()
        || world
            .get::<&balaur_geometry::mesh::SolvedMesh>(entity)
            .is_ok();
    if moving {
        live.insert(entity);
    } else {
        live.remove(&entity);
    }
}
