//! What a ray hits, tested against `Renderable` and `GlobalTransform`.
//!
//! The renderer is not asked. Picking reads the same components a backend
//! draws from, so it answers identically in a windowed run and a headless
//! one — which is the only way the editor's own tests can cover it.

use balaur_core::hecs;
use balaur_core::scene::GlobalTransform;
use glamx::Vec3;

use crate::{Renderable, Shape};

/// The box a renderable fills in its own space, as centre and half-extents.
///
/// Every shape carries its size, and a mesh carries the box measured from its
/// vertices when the asset resolved. `None` is a mesh whose asset would not
/// load, which is also a mesh that draws nothing.
fn local_box(renderable: &Renderable) -> Option<(Vec3, Vec3)> {
    let half = match renderable.shape {
        Shape::Ball { radius } => Vec3::splat(radius),
        Shape::Cuboid { hx, hy, hz } => Vec3::new(hx, hy, hz),
        // The caps add a radius at each end of the straight part.
        Shape::Capsule { radius, height } => Vec3::new(radius, height / 2.0 + radius, radius),
        Shape::Cylinder { radius, height } | Shape::Cone { radius, height } => {
            Vec3::new(radius, height / 2.0, radius)
        }
        // A quad has no thickness; picking one needs some, or the slab test
        // divides by zero and every ray misses it.
        Shape::Plane { hx, hz } => Vec3::new(hx, 1e-4, hz),
        // A mesh is the one shape not centred on its own origin.
        Shape::Mesh => {
            let bounds = renderable.bounds?;
            return Some((bounds.centre, bounds.half));
        }
    };
    Some((Vec3::ZERO, half))
}

/// Distance along `dir` to the near face of the box, or `None` for a miss.
///
/// The slab test, with the ray put into the box's own space first so a
/// rotated or scaled node is tested as the axis-aligned box it started as.
fn hit_box(at: &GlobalTransform, centre: Vec3, half: Vec3, origin: Vec3, dir: Vec3) -> Option<f32> {
    let inverse = at.rotation.inverse();
    let scale = Vec3::new(
        if at.scale.x.abs() < 1e-6 {
            1e-6
        } else {
            at.scale.x
        },
        if at.scale.y.abs() < 1e-6 {
            1e-6
        } else {
            at.scale.y
        },
        if at.scale.z.abs() < 1e-6 {
            1e-6
        } else {
            at.scale.z
        },
    );
    // Both ends scale, so `t` still measures world distance and hits from
    // different nodes stay comparable.
    let o = (inverse * (origin - at.position)) / scale - centre;
    let d = (inverse * dir) / scale;

    let mut near = f32::NEG_INFINITY;
    let mut far = f32::INFINITY;
    for axis in 0..3 {
        let (o, d, h) = (o[axis], d[axis], half[axis]);
        if d.abs() < 1e-9 {
            // Parallel to this pair of slabs: outside them is a miss.
            if o < -h || o > h {
                return None;
            }
            continue;
        }
        let (mut lo, mut hi) = ((-h - o) / d, (h - o) / d);
        if lo > hi {
            std::mem::swap(&mut lo, &mut hi);
        }
        near = near.max(lo);
        far = far.min(hi);
        if near > far {
            return None;
        }
    }
    // Behind the eye is not a hit; inside the box is, at the eye.
    if far < 0.0 {
        return None;
    }
    Some(near.max(0.0))
}

/// Distance to the front of the sphere, or `None`. A ball tested as a box
/// picks its corners, which is 41% too generous at the diagonal.
fn hit_sphere(at: &GlobalTransform, radius: f32, origin: Vec3, dir: Vec3) -> Option<f32> {
    // The largest axis, so a squashed ball still contains what it draws.
    let radius = radius * at.scale.abs().max_element();
    let to_centre = at.position - origin;
    let length = dir.length();
    if length < 1e-9 {
        return None;
    }
    let unit = dir / length;
    let along = to_centre.dot(unit);
    let closest = to_centre - unit * along;
    let gap = radius * radius - closest.length_squared();
    if gap < 0.0 {
        return None;
    }
    let half_chord = gap.sqrt();
    let near = along - half_chord;
    let far = along + half_chord;
    if far < 0.0 {
        return None;
    }
    Some(near.max(0.0) / length)
}

/// The nearest `Renderable` the ray meets, and how far along it that is.
///
/// `dir` need not be a unit vector; the distance is in multiples of it, so
/// only the ordering matters to a caller choosing what was clicked.
pub(crate) fn along_ray(
    world: &hecs::World,
    origin: Vec3,
    dir: Vec3,
) -> Option<(hecs::Entity, f32)> {
    let mut best: Option<(hecs::Entity, f32)> = None;
    for (entity, renderable, at) in
        &mut world.query::<(hecs::Entity, &Renderable, &GlobalTransform)>()
    {
        let hit = match renderable.shape {
            Shape::Ball { radius } => hit_sphere(at, radius, origin, dir),
            _ => local_box(renderable)
                .and_then(|(centre, half)| hit_box(at, centre, half, origin, dir)),
        };
        let Some(distance) = hit else { continue };
        if best.is_none_or(|(_, best)| distance < best) {
            best = Some((entity, distance));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use glamx::Quat;

    fn at(position: Vec3) -> GlobalTransform {
        GlobalTransform {
            position,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    fn renderable(shape: Shape) -> Renderable {
        Renderable {
            shape,
            bounds: None,
            color: [1.0; 4],
            mesh: None,
            skeleton: String::new(),
            texture: String::new(),
            material: String::new(),
            version: 0,
        }
    }

    #[test]
    fn a_ray_down_the_z_axis_hits_the_near_face_of_a_cube() {
        let place = at(Vec3::new(0.0, 0.0, -10.0));
        let half = Vec3::splat(1.0);
        let hit = hit_box(
            &place,
            Vec3::ZERO,
            half,
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -1.0),
        );
        assert_eq!(
            hit,
            Some(9.0),
            "the near face, not the centre or the far one"
        );
    }

    #[test]
    fn a_ray_pointing_away_misses() {
        let place = at(Vec3::new(0.0, 0.0, -10.0));
        let half = Vec3::splat(1.0);
        assert_eq!(
            hit_box(
                &place,
                Vec3::ZERO,
                half,
                Vec3::ZERO,
                Vec3::new(0.0, 0.0, 1.0)
            ),
            None
        );
    }

    #[test]
    fn a_ray_beside_the_box_misses() {
        let place = at(Vec3::new(0.0, 0.0, -10.0));
        let half = Vec3::splat(1.0);
        assert_eq!(
            hit_box(
                &place,
                Vec3::ZERO,
                half,
                Vec3::new(5.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, -1.0)
            ),
            None
        );
    }

    /// Scale is why the ray is put into the node's space rather than the box
    /// into the world's: a stretched node is picked over the space it covers.
    #[test]
    fn scale_widens_what_a_node_covers() {
        let mut place = at(Vec3::new(0.0, 0.0, -10.0));
        let half = Vec3::splat(1.0);
        let beside = Vec3::new(3.0, 0.0, 0.0);
        let down = Vec3::new(0.0, 0.0, -1.0);
        assert_eq!(hit_box(&place, Vec3::ZERO, half, beside, down), None);
        place.scale = Vec3::new(5.0, 1.0, 1.0);
        assert_eq!(hit_box(&place, Vec3::ZERO, half, beside, down), Some(9.0));
    }

    /// A quarter turn about y puts a long node's length across the ray.
    #[test]
    fn rotation_turns_what_a_node_covers_with_it() {
        let mut place = at(Vec3::new(0.0, 0.0, -10.0));
        let half = Vec3::new(4.0, 1.0, 1.0);
        let beside = Vec3::new(0.0, 0.0, -3.0);
        let across = Vec3::new(1.0, 0.0, 0.0);
        // Long on x, so a ray along x down its length meets its end.
        assert!(hit_box(
            &place,
            Vec3::ZERO,
            half,
            Vec3::new(-20.0, 0.0, -10.0),
            across
        )
        .is_some());
        // Turned, the same node no longer reaches a ray 3 units off its side.
        place.rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        assert!(hit_box(&place, Vec3::ZERO, half, beside, Vec3::new(0.0, 1.0, 0.0)).is_none());
    }

    /// A ball is round: its corners are not part of it.
    #[test]
    fn a_ball_is_not_picked_at_the_corner_a_box_would_have() {
        let place = at(Vec3::new(0.0, 0.0, -10.0));
        let down = Vec3::new(0.0, 0.0, -1.0);
        // Straight at the centre it hits the front, one radius nearer.
        assert_eq!(hit_sphere(&place, 1.0, Vec3::ZERO, down), Some(9.0));
        // At the corner of the box that contains it, it does not.
        let corner = Vec3::new(0.9, 0.9, 0.0);
        assert!(hit_sphere(&place, 1.0, corner, down).is_none());
        assert!(hit_box(&place, Vec3::ZERO, Vec3::splat(1.0), corner, down).is_some());
    }

    #[test]
    fn a_ray_starting_inside_hits_at_the_eye() {
        let place = at(Vec3::ZERO);
        let inside = Vec3::new(0.1, 0.0, 0.0);
        let down = Vec3::new(0.0, 0.0, -1.0);
        assert_eq!(
            hit_box(&place, Vec3::ZERO, Vec3::splat(1.0), inside, down),
            Some(0.0)
        );
        assert_eq!(hit_sphere(&place, 1.0, inside, down), Some(0.0));
    }

    /// A flat plane has no thickness, and a slab test on zero misses
    /// everything; it is given just enough to be pickable.
    #[test]
    fn a_plane_is_pickable_from_above() {
        let place = at(Vec3::ZERO);
        let (_, half) = local_box(&renderable(Shape::Plane { hx: 5.0, hz: 5.0 })).unwrap();
        let above = Vec3::new(1.0, 4.0, 1.0);
        let down = Vec3::new(0.0, -1.0, 0.0);
        assert!(hit_box(&place, Vec3::ZERO, half, above, down).is_some());
    }

    /// A mesh is picked over the box its vertices filled, wherever that box
    /// sits relative to the node's origin.
    #[test]
    fn a_mesh_is_picked_over_the_box_its_vertices_filled() {
        let mut r = renderable(Shape::Mesh);
        // A metre cube sitting two to the right of the node's origin.
        r.bounds = Some(crate::Bounds {
            centre: Vec3::new(2.0, 0.0, 0.0),
            half: Vec3::splat(0.5),
        });
        let place = at(Vec3::new(0.0, 0.0, -10.0));
        let (centre, half) = local_box(&r).unwrap();
        let down = Vec3::new(0.0, 0.0, -1.0);
        // Over the vertices, not over the origin.
        assert!(hit_box(&place, centre, half, Vec3::new(2.0, 0.0, 0.0), down).is_some());
        assert!(hit_box(&place, centre, half, Vec3::ZERO, down).is_none());
    }

    /// A mesh whose asset would not load draws nothing, and picks nothing.
    #[test]
    fn a_mesh_with_no_bounds_is_not_pickable() {
        assert!(local_box(&renderable(Shape::Mesh)).is_none());
    }

    #[test]
    fn the_nearest_of_several_nodes_is_the_one_picked() {
        let mut world = hecs::World::new();
        let near = world.spawn((
            renderable(Shape::Cuboid {
                hx: 1.0,
                hy: 1.0,
                hz: 1.0,
            }),
            at(Vec3::new(0.0, 0.0, -5.0)),
        ));
        let _far = world.spawn((
            renderable(Shape::Cuboid {
                hx: 1.0,
                hy: 1.0,
                hz: 1.0,
            }),
            at(Vec3::new(0.0, 0.0, -20.0)),
        ));
        let (entity, distance) = along_ray(&world, Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0)).unwrap();
        assert_eq!(entity, near);
        assert!(
            (distance - 4.0).abs() < 1e-5,
            "the near face at 4, got {distance}"
        );
    }

    #[test]
    fn a_ray_that_meets_nothing_picks_nothing() {
        let mut world = hecs::World::new();
        world.spawn((
            renderable(Shape::Cuboid {
                hx: 1.0,
                hy: 1.0,
                hz: 1.0,
            }),
            at(Vec3::new(0.0, 0.0, -5.0)),
        ));
        assert!(along_ray(&world, Vec3::new(50.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0)).is_none());
    }
}
