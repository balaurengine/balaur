//! What a 3D node draws per frame beyond its pose: morph weights, a
//! `multimesh3d`'s copies and a `particles3d`'s live particles.
//!
//! Split from the backend's sync loop for its length.

use balaur_core::App;
use balaur_core::hecs::{Entity, World};
use balaur_core::scene::GlobalTransform;

use super::Slot;

pub(super) fn draw(
    app: &App,
    world: &World,
    entity: Entity,
    slot: &mut Slot,
    (global, eye, visible): (&GlobalTransform, glamx::Vec3, bool),
) {
    // How far the mesh is blended towards each of its shapes, this tick.
    if let Ok(morphs) = world.get::<&crate::MorphWeights>(entity) {
        slot.node.set_morph_weights(&morphs.weights);
    }
    if let Ok(multimesh) = world.get::<&crate::MultiMesh>(entity) {
        crate::instancing::draw_multimesh_3d(&mut slot.node, &multimesh, global);
        // The companion sits at the node's origin, so the same copies in its
        // own space land where the node's do.
        if let Some(companion) = slot.companion.as_mut() {
            crate::instancing::draw_multimesh_3d(companion, &multimesh, global);
        }
    }
    if let Ok(emitter) = world.get::<&crate::particles_3d::Particles3d>(entity) {
        let live = slot
            .particles
            .get_or_insert_with(|| crate::particles_3d::Live3d::new(entity));
        let delta = app.engine.delta();
        let any = crate::particles_3d::draw(&mut slot.node, live, &emitter, global, (eye, delta));
        slot.node.set_visible(visible && any);
    }
}
