//! What a tool drawing a modifier asks: where its target sits, and which
//! bones it drives. The same walk the solver makes, so a gizmo cannot
//! disagree with what the rig will do.

use crate::modifier::{Modifier2d, Modifier3d, chain_of, origin_2d, origin_3d, pose_2d, pose_3d};
use balaur_core::Engine;
use balaur_core::hecs::Entity;
use balaur_core::scene;
use glamx::Vec2;

/// Where the modifier's target is, for a tool that draws the reach.
#[must_use]
pub fn target_of(eng: &Engine, entity: Entity) -> Option<Vec2> {
    let world = eng.world();
    let (target, dim3) = match world.get::<&Modifier2d>(entity) {
        Ok(m) => (m.0.target.clone(), false),
        Err(_) => (
            world.get::<&Modifier3d>(entity).ok()?.0.target.clone(),
            true,
        ),
    };
    let target = scene::find_node(&world, entity, &target)?;
    Some(if dim3 {
        origin_3d(&pose_3d(&world, target)).truncate()
    } else {
        origin_2d(&pose_2d(&world, target))
    })
}

/// The bones a modifier drives, for a tool that draws the chain it solves.
///
/// The editor's gizmo needs the same walk the solver makes — a `chain` of two
/// on a rig five deep draws two bones, not five — and this is that walk.
#[must_use]
pub fn chain_of_node(eng: &Engine, entity: Entity) -> Vec<Entity> {
    let world = eng.world();
    let (bone_path, chain) = match world.get::<&Modifier2d>(entity) {
        Ok(m) => (m.0.bone.clone(), m.0.chain),
        Err(_) => match world.get::<&Modifier3d>(entity) {
            Ok(m) => (m.0.bone.clone(), m.0.chain),
            Err(_) => return Vec::new(),
        },
    };
    let bone = if bone_path.is_empty() {
        Some(entity)
    } else {
        scene::find_node(&world, entity, &bone_path)
    };
    bone.map(|bone| chain_of(&world, bone, chain))
        .unwrap_or_default()
}
