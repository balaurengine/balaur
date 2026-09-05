//! The instanced draw path: one node, many copies, one call.
//!
//! The seam a cloner draws through, and the one automatic instancing and a
//! scripted mass of copies will draw through when they arrive. Splitting a
//! copy's world matrix into what the shader wants is the whole of it, and it
//! lives away from the backend so that arithmetic can be read on its own.

use glamx::{Mat3, Mat4, Vec3};
use kiss3d::prelude::*;
use kiss3d::scene::{InstanceData3d, SceneNode3d};

use balaur_core::scene::GlobalTransform;

/// Put a node's copies on it, as the instance data the shader reads.
///
/// The shader deforms in the node's own space and translates in the world's,
/// so a copy's world matrix has to be split into those two halves rather
/// than handed over whole.
pub(crate) fn set_instances_3d(
    node: &mut SceneNode3d,
    clones: Option<&crate::Clones>,
    global: &GlobalTransform,
    color: [f32; 4],
) {
    let Some(clones) = clones.filter(|clones| !clones.0.is_empty()) else {
        return;
    };
    let here =
        Mat4::from_scale_rotation_translation(global.scale, global.rotation, global.position);
    // The node's own rotation and scale, which the copy's world transform has
    // to be divided out of before the shader multiplies them back in.
    let model = Mat3::from_mat4(here);
    if model.determinant().abs() < 1e-12 {
        return;
    }
    let unpick = model.inverse();
    let turn = Mat3::from_quat(global.rotation);
    let untimed = turn.transpose();
    let origin = here.w_axis.truncate();
    let instances: Vec<InstanceData3d> = clones
        .0
        .iter()
        .map(|placed| {
            let world_linear = Mat3::from_mat4(*placed) * unpick;
            InstanceData3d {
                position: placed.w_axis.truncate() - origin,
                deformation: untimed * world_linear * turn,
                color: Color::new(color[0], color[1], color[2], color[3]),
                lines_color: None,
                lines_width: None,
                points_color: None,
                points_size: None,
            }
        })
        .collect();
    node.set_instances(&instances);
}
