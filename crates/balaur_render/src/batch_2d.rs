//! Sprites and shapes that draw alike, drawn in one call.
//!
//! The 2D pass submitted one draw per node, and five thousand of them spent
//! 40% of the frame in the command encoder with the GPU idle. A run of nodes
//! that share a mesh, a texture and a material becomes one object with an
//! instance each, which is what the fork's `InstancesBuffer2d` was built for.
//!
//! A run is contiguous in the draw order, so what covers what does not move.

#![cfg_attr(
    not(feature = "kiss3d"),
    allow(dead_code, reason = "the run cutting is the backend's, and the tests'")
)]

use balaur_core::hecs::Entity;
use balaur_core::GlobalTransform;
use glamx::{Mat2, Vec2};

use crate::{Renderable2d, Shape2d};

/// Below this a run draws node by node: one object holding two instances
/// costs a buffer upload to save a draw, and the pair is not worth it.
pub(crate) const MIN_RUN: usize = 4;

/// What two nodes must share to draw in one call: the mesh their shape
/// builds, the image on it, and the material it draws through.
#[derive(Clone, PartialEq)]
pub(crate) struct BatchKey {
    pub(crate) shape: Shape2d,
    /// The image a sprite draws; empty for a shape that carries none.
    pub(crate) texture: String,
    /// The material asset, the node's own or the one it inherited.
    pub(crate) material: String,
}

/// Whether a node can hand its pose to an instance instead of a node.
///
/// A polygon and a polyline carry their own geometry, and a sprite whose UVs
/// were rewritten for a frame, a region or a flip no longer matches the mesh
/// its run shares.
pub(crate) fn batchable(renderable: &Renderable2d) -> bool {
    match renderable.shape {
        Shape2d::Flat(_) => true,
        Shape2d::Sprite { .. } => renderable.sprite.as_ref().is_some_and(|sprite| {
            sprite.sheet.is_none() && sprite.region.is_none() && !sprite.flip_x && !sprite.flip_y
        }),
        Shape2d::Polyline(_) | Shape2d::Polygon => false,
    }
}

/// The key a batchable node joins a run by.
pub(crate) fn key_of(
    renderable: &Renderable2d,
    inherited: balaur_core::scene::MaterialId,
) -> BatchKey {
    let material = if renderable.material.is_empty() {
        inherited.reference().to_string()
    } else {
        renderable.material.clone()
    };
    BatchKey {
        shape: renderable.shape,
        texture: renderable
            .sprite
            .as_ref()
            .map_or_else(String::new, |sprite| sprite.path.clone()),
        material,
    }
}

/// The pose a node hands to its instance, as the fork composes one.
///
/// The object the run draws through sits at the origin unscaled, so the
/// instance carries the whole of `translate . rotate . shear . scale` that a
/// node of its own would have held in its transform.
pub(crate) fn pose(renderable: &Renderable2d, global: &GlobalTransform) -> (Vec2, Mat2) {
    let size = match renderable.shape {
        Shape2d::Sprite { hx, hy } => Vec2::new(2.0 * hx, 2.0 * hy),
        _ => Vec2::ONE,
    };
    let (angle, _, _) = global.rotation.to_euler(glamx::EulerRot::ZYX);
    let (sin, cos) = (
        balaur_core::libm::sinf(angle),
        balaur_core::libm::cosf(angle),
    );
    let turn = Mat2::from_cols(Vec2::new(cos, sin), Vec2::new(-sin, cos));
    let (skew_sin, skew_cos) = (
        balaur_core::libm::sinf(global.skew),
        balaur_core::libm::cosf(global.skew),
    );
    let lean = Mat2::from_cols(Vec2::new(1.0, 0.0), Vec2::new(-skew_sin, skew_cos));
    let scale = Mat2::from_cols(
        Vec2::new(size.x * global.scale.x, 0.0),
        Vec2::new(0.0, size.y * global.scale.y),
    );
    let at = Vec2::new(global.position.x, global.position.y) + shift(renderable, global);
    (at, turn * lean * scale)
}

/// How far a sprite's quad sits off its node, taken through the node's own
/// turn and scale the way the per-node path takes it.
fn shift(renderable: &Renderable2d, global: &GlobalTransform) -> Vec2 {
    match (&renderable.sprite, renderable.shape) {
        (Some(sprite), Shape2d::Sprite { hx, hy }) => {
            let [x, y] = sprite.centre(hx, hy);
            (global.affine_2d() * glamx::Vec3::new(x, y, 0.0)).truncate()
        }
        _ => Vec2::ZERO,
    }
}

/// A stretch of the draw order that draws as one call, and what is in it.
pub(crate) struct Run {
    pub(crate) key: BatchKey,
    pub(crate) members: Vec<Entity>,
}

/// Cut the draw order into runs.
///
/// `entry` answers what a node would join a run by, and `None` for one that
/// cannot join any. A stretch shorter than [`MIN_RUN`] is left out, so its
/// nodes keep the objects they already have.
pub(crate) fn runs(
    order: &[Entity],
    mut entry: impl FnMut(Entity) -> Option<BatchKey>,
) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    let mut open: Option<Run> = None;
    for &entity in order {
        match entry(entity) {
            Some(key) => match &mut open {
                Some(run) if run.key == key => run.members.push(entity),
                _ => {
                    close(&mut out, open.take());
                    open = Some(Run {
                        key,
                        members: vec![entity],
                    });
                }
            },
            None => close(&mut out, open.take()),
        }
    }
    close(&mut out, open);
    out
}

fn close(out: &mut Vec<Run>, run: Option<Run>) {
    if let Some(run) = run
        && run.members.len() >= MIN_RUN
    {
        out.push(run);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(id: u32) -> Entity {
        Entity::from_bits(u64::from(id) | (1 << 32)).expect("a live id")
    }

    fn key(name: &str) -> BatchKey {
        BatchKey {
            shape: Shape2d::Flat(balaur_core::primitive::Flat::Circle {
                radius: 1.0,
                segments: 16,
            }),
            texture: name.to_string(),
            material: String::new(),
        }
    }

    /// The instance carries what the node's own object would have held, in
    /// the order the fork applies it: turn, then lean, then scale.
    #[test]
    fn an_instance_carries_the_nodes_whole_pose() {
        let renderable = Renderable2d::fresh(Shape2d::Flat(balaur_core::primitive::Flat::Rect {
            hx: 1.0,
            hy: 1.0,
            corner_radius: 0.0,
            segments: 0,
        }));
        let global = GlobalTransform {
            position: glamx::Vec3::new(2.0, 3.0, 0.0),
            rotation: glamx::Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            scale: glamx::Vec3::new(2.0, 1.0, 1.0),
            skew: 0.0,
        };
        let (at, matrix) = pose(&renderable, &global);
        assert!((at - Vec2::new(2.0, 3.0)).length() < 1e-5, "where the node is");
        let corner = matrix * Vec2::new(1.0, 0.0);
        assert!(
            (corner - Vec2::new(0.0, 2.0)).length() < 1e-5,
            "x scaled by two, then turned a quarter: {corner:?}"
        );
        let up = matrix * Vec2::new(0.0, 1.0);
        assert!((up - Vec2::new(-1.0, 0.0)).length() < 1e-5, "y turned: {up:?}");
    }

    #[test]
    fn a_stretch_shorter_than_the_minimum_is_not_a_run() {
        let order: Vec<Entity> = (0..3).map(entity).collect();
        assert!(runs(&order, |_| Some(key("a"))).is_empty());
    }

    #[test]
    fn nodes_sharing_a_key_become_one_run() {
        let order: Vec<Entity> = (0..6).map(entity).collect();
        let cut = runs(&order, |_| Some(key("a")));
        assert_eq!(cut.len(), 1);
        assert_eq!(cut[0].members.len(), 6);
    }

    #[test]
    fn a_node_that_cannot_batch_cuts_the_run() {
        let order: Vec<Entity> = (0..9).map(entity).collect();
        let cut = runs(&order, |e| (e != entity(4)).then(|| key("a")));
        assert_eq!(cut.len(), 2, "four each side of the node that cannot");
        assert_eq!(cut[0].members.len(), 4);
        assert_eq!(cut[1].members.len(), 4);
    }

    #[test]
    fn a_different_key_starts_another_run() {
        let order: Vec<Entity> = (0..8).map(entity).collect();
        let cut = runs(&order, |e| {
            Some(if e.id() < 4 { key("a") } else { key("b") })
        });
        assert_eq!(cut.len(), 2);
        assert_eq!(cut[0].members.len(), 4);
        assert_eq!(cut[1].members.len(), 4);
    }
}
