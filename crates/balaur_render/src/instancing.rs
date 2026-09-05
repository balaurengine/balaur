//! The instanced draw path: one node, many copies, one call.
//!
//! The seam a cloner draws through, and the one automatic instancing and a
//! scripted mass of copies will draw through when they arrive. Splitting a
//! copy's world matrix into the two halves the shader reads is the whole of
//! it, and it lives here rather than in the backend so that arithmetic can
//! be read, and tested, on its own.

#![cfg_attr(
    not(feature = "kiss3d"),
    allow(dead_code, reason = "the arithmetic is the backend's, and the tests'")
)]

use balaur_core::scene::GlobalTransform;
use glamx::{Mat3, Mat4, Vec3};

/// The node's own model matrix, as the shader's object uniform carries it.
#[must_use]
pub(crate) fn model_of(global: &GlobalTransform) -> Mat4 {
    Mat4::from_scale_rotation_translation(global.scale, global.rotation, global.position)
}

/// One copy, split into what the shader multiplies and what it adds.
///
/// The shader computes `offset + turn * (deform * (scale * vertex)) +
/// position`, so a copy's world matrix `placed` has to be divided by the
/// node's own before it can be handed over. `None` when the node is squashed
/// flat and has no inverse to divide by.
#[must_use]
pub(crate) fn split(here: Mat4, placed: Mat4, rotation: glamx::Quat) -> Option<(Mat3, Vec3)> {
    let model = Mat3::from_mat4(here);
    if model.determinant().abs() < 1e-12 {
        return None;
    }
    let turn = Mat3::from_quat(rotation);
    // What the copy does to the node's whole frame, with the node's own
    // rotation and scale divided out.
    let world_linear = Mat3::from_mat4(placed) * model.inverse();
    Some((
        turn.transpose() * world_linear * turn,
        placed.w_axis.truncate() - here.w_axis.truncate(),
    ))
}

/// Put a node's copies on it, as the instance data the shader reads.
#[cfg(feature = "kiss3d")]
pub(crate) fn set_instances_3d(
    node: &mut kiss3d::scene::SceneNode3d,
    clones: Option<&crate::Clones>,
    global: &GlobalTransform,
) {
    use kiss3d::prelude::Color;
    use kiss3d::scene::InstanceData3d;

    let Some(clones) = clones.filter(|clones| !clones.0.is_empty()) else {
        return;
    };
    let here = model_of(global);
    let instances: Vec<InstanceData3d> = clones
        .0
        .iter()
        .filter_map(|placed| split(here, *placed, global.rotation))
        .map(|(deformation, position)| InstanceData3d {
            position,
            deformation,
            // White: a copy's colour multiplies the node's, and every copy
            // draws in the node's own colour until something asks otherwise.
            color: Color::new(1.0, 1.0, 1.0, 1.0),
            lines_color: None,
            lines_width: None,
            points_color: None,
            points_size: None,
        })
        .collect();
    node.set_instances(&instances);
}

#[cfg(test)]
mod tests {
    use super::*;
    use glamx::Quat;

    fn pose(position: Vec3, rotation: Quat, scale: Vec3) -> GlobalTransform {
        GlobalTransform {
            position,
            rotation,
            scale,
        }
    }

    /// What the shader works out for one vertex, spelled the way
    /// `shaders/mesh.wesl` spells it.
    fn shaded(here: &GlobalTransform, deform: Mat3, offset: Vec3, vertex: Vec3) -> Vec3 {
        let scaled = here.scale * vertex;
        let deformed = deform * scaled;
        let model = Mat4::from_rotation_translation(here.rotation, here.position);
        offset + model.transform_point3(deformed)
    }

    /// The split is only right if putting it back together lands the vertex
    /// where the copy's world matrix says it should.
    ///
    /// `copy` is what the copy does to the node's whole frame, so the matrix
    /// handed over is that applied to the node's own -- which is what a
    /// cloner writes.
    fn round_trips(here: GlobalTransform, copy: Mat4) {
        let model = model_of(&here);
        let placed = copy * model;
        let (deform, offset) = split(model, placed, here.rotation).expect("a node with an inverse");
        for vertex in [Vec3::ZERO, Vec3::X, Vec3::new(0.3, -0.7, 1.9)] {
            let want = placed.transform_point3(vertex);
            let got = shaded(&here, deform, offset, vertex);
            assert!(
                (want - got).length() < 1e-4,
                "vertex {vertex:?} landed at {got:?}, not {want:?}"
            );
        }
    }

    #[test]
    fn a_copy_moved_sideways_lands_where_it_was_moved() {
        round_trips(
            pose(Vec3::new(0.0, 1.0, 0.0), Quat::IDENTITY, Vec3::ONE),
            Mat4::from_translation(Vec3::new(2.0, 0.0, 0.0)),
        );
    }

    #[test]
    fn a_copy_turned_about_the_cloner_lands_turned() {
        round_trips(
            pose(Vec3::new(1.0, 0.0, 0.0), Quat::IDENTITY, Vec3::ONE),
            Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2),
        );
    }

    /// The hard case: the node itself is turned and unevenly scaled, so the
    /// division by its own frame has to be exact.
    #[test]
    fn a_turned_and_scaled_node_still_lands_its_copies() {
        round_trips(
            pose(
                Vec3::new(-2.0, 0.5, 3.0),
                Quat::from_rotation_z(0.7) * Quat::from_rotation_y(1.3),
                Vec3::new(2.0, 0.5, 1.5),
            ),
            Mat4::from_scale_rotation_translation(
                Vec3::splat(1.4),
                Quat::from_rotation_x(-0.4),
                Vec3::new(4.0, -1.0, 0.25),
            ),
        );
    }

    #[test]
    fn a_node_squashed_flat_has_no_copies_to_place() {
        let here = pose(Vec3::ZERO, Quat::IDENTITY, Vec3::new(1.0, 0.0, 1.0));
        assert!(split(model_of(&here), Mat4::IDENTITY, here.rotation).is_none());
    }
}
