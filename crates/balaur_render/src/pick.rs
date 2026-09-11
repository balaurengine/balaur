//! What a ray hits, tested against `Renderable` and `GlobalTransform`.
//!
//! The renderer is not asked. Picking reads the same components a backend
//! draws from, so it answers identically in a windowed run and a headless
//! one — which is the only way the editor's own tests can cover it.
//!
//! A node's box narrows the field; the triangles decide, through parry, so a
//! click between the spokes of a wheel misses it as the eye expects.

use balaur_core::Engine;
use balaur_core::hecs;
use balaur_core::mesh::MeshData;
use balaur_core::scene::GlobalTransform;
use glamx::Vec3;

use crate::{Renderable, Shape, Solid};

/// The box a renderable fills in its own space, as centre and half-extents.
///
/// Every shape carries its size, and a mesh carries the box measured from its
/// vertices when the asset resolved. `None` is a mesh whose asset would not
/// load, which is also a mesh that draws nothing.
fn local_box(renderable: &Renderable) -> Option<(Vec3, Vec3)> {
    let Some(solid) = renderable.shape.solid() else {
        // A mesh and a built shape are the two not centred on their own
        // origin: both carry the box measured from their vertices.
        let bounds = renderable.bounds?;
        return Some((bounds.centre, bounds.half));
    };
    Some((Vec3::ZERO, Vec3::from_array(solid.half_extents())))
}

/// A scale no axis of which is zero, so dividing by it cannot explode.
fn safe_scale(scale: Vec3) -> Vec3 {
    Vec3::new(
        if scale.x.abs() < 1e-6 { 1e-6 } else { scale.x },
        if scale.y.abs() < 1e-6 { 1e-6 } else { scale.y },
        if scale.z.abs() < 1e-6 { 1e-6 } else { scale.z },
    )
}

/// Distance to the nearest triangle of `mesh`, or `None` for a miss.
///
/// The ray comes into the node's own space first, both ends scaled, so `t`
/// still measures world distance and hits from different nodes compare.
fn hit_mesh(at: &GlobalTransform, mesh: &MeshData, origin: Vec3, dir: Vec3) -> Option<f32> {
    use parry3d::query::RayCast;

    let inverse = at.rotation.inverse();
    let scale = safe_scale(at.scale);
    let ray = parry3d::query::Ray::new(
        (inverse * (origin - at.position)) / scale,
        (inverse * dir) / scale,
    );
    let mut best: Option<f32> = None;
    for corners in &mesh.indices {
        let point = |i: u32| mesh.positions.get(i as usize).map(|p| Vec3::from_array(*p));
        let (Some(a), Some(b), Some(c)) = (point(corners[0]), point(corners[1]), point(corners[2]))
        else {
            continue;
        };
        let Some(distance) =
            parry3d::shape::Triangle::new(a, b, c).cast_local_ray(&ray, f32::MAX, false)
        else {
            continue;
        };
        if best.is_none_or(|so_far| distance < so_far) {
            best = Some(distance);
        }
    }
    best
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

/// A candidate the box pass kept, and what its triangles can be found in.
struct Candidate {
    entity: hecs::Entity,
    distance: f32,
    at: GlobalTransform,
    mesh: Option<String>,
    built: Option<std::sync::Arc<MeshData>>,
    /// A ball is smooth; its faceted mesh would pick *less* like what is
    /// drawn, so its sphere distance is the answer.
    refine: bool,
}

/// The `mesh` asset's triangles, resolved the way a collider resolves them.
fn loaded(eng: &Engine, reference: &str) -> Option<std::rc::Rc<MeshData>> {
    // Through the cache: the pointer is picked every frame, and following a
    // mesh's `source` reads and parses the whole file.
    balaur_core::mesh::resolved(eng, reference).ok()
}

/// The nearest `Renderable` the ray meets, and how far along it that is.
///
/// `dir` need not be a unit vector; the distance is in multiples of it, so
/// only the ordering matters to a caller choosing what was clicked.
///
/// Boxes first, triangles second: a candidate whose box is further away than
/// an exact hit already found is never loaded at all.
fn candidates(world: &hecs::World, origin: Vec3, dir: Vec3) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    for (entity, renderable, at) in
        &mut world.query::<(hecs::Entity, &Renderable, &GlobalTransform)>()
    {
        let ball = matches!(renderable.shape, Shape::Solid(Solid::Ball { .. }));
        let hit = match renderable.shape {
            Shape::Solid(Solid::Ball { radius, .. }) => hit_sphere(at, radius, origin, dir),
            _ => local_box(renderable)
                .and_then(|(centre, half)| hit_box(at, centre, half, origin, dir)),
        };
        let Some(distance) = hit else { continue };
        out.push(Candidate {
            entity,
            distance,
            at: *at,
            mesh: renderable.mesh.clone(),
            built: renderable.built.clone(),
            refine: !ball,
        });
    }
    out.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    out
}

pub(crate) fn along_ray(eng: &Engine, origin: Vec3, dir: Vec3) -> Option<(hecs::Entity, f32)> {
    let candidates = {
        let world = eng.world();
        candidates(&world, origin, dir)
    };
    let mut best: Option<(hecs::Entity, f32)> = None;
    for candidate in candidates {
        if best.is_some_and(|(_, so_far)| candidate.distance >= so_far) {
            break;
        }
        let triangles = if candidate.refine {
            match candidate.built {
                Some(built) => Some(hit_mesh(&candidate.at, &built, origin, dir)),
                None => candidate
                    .mesh
                    .as_deref()
                    .filter(|name| !name.is_empty())
                    .and_then(|name| loaded(eng, name))
                    .map(|data| hit_mesh(&candidate.at, &data, origin, dir)),
            }
        } else {
            None
        };
        let distance = match triangles {
            // The triangles answered: a miss here is a miss, box or no box.
            Some(Some(exact)) => exact,
            Some(None) => continue,
            None => candidate.distance,
        };
        if best.is_none_or(|(_, so_far)| distance < so_far) {
            best = Some((candidate.entity, distance));
        }
    }
    best
}

/// The node the pointer is over, or `None`.
///
/// 3D casts the viewport's own picking ray, which the backend publishes each
/// frame and a replay restores; 2D takes the smallest shape whose box holds
/// the point. Headless the snapshots are zero, so nothing is ever under the
/// pointer and no hook fires, which is what a test with no window wants.
pub fn under_pointer(eng: &Engine) -> Option<hecs::Entity> {
    let (origin, dir) = {
        let vp = eng.resource::<crate::ViewportSnapshot>();
        let vp = vp.borrow();
        (
            Vec3::from_array(vp.ray_origin),
            Vec3::from_array(vp.ray_dir),
        )
    };
    if dir.length_squared() > 1e-8
        && let Some((entity, _)) = along_ray(eng, origin, dir)
    {
        return Some(entity);
    }
    under_pointer_2d(eng)
}

/// The smallest 2D shape whose box holds the pointer. The same rule the
/// editor's own picker uses, so what a click selects and what a hook fires on
/// are the same node.
fn under_pointer_2d(eng: &Engine) -> Option<hecs::Entity> {
    let point = {
        let vp = eng.resource::<crate::ViewportSnapshot2d>();
        let vp = vp.borrow();
        vp.mouse_world
    };
    let world = eng.world();
    let mut best: Option<(hecs::Entity, f32)> = None;
    for (entity, renderable, at) in &mut world.query::<(
        hecs::Entity,
        &crate::Renderable2d,
        &balaur_core::GlobalTransform,
    )>() {
        let visible = world
            .get::<&balaur_core::GlobalAppearance>(entity)
            .is_ok_and(|a| a.visible);
        if !visible {
            continue;
        }
        let Some((hx, hy)) = half_extents_2d(renderable) else {
            continue;
        };
        let hx = hx * at.scale.x;
        let hy = hy * at.scale.y;
        let (angle, _, _) = at.rotation.to_euler(glamx::EulerRot::ZYX);
        let (sin, cos) = angle.sin_cos();
        let dx = point[0] - at.position.x;
        let dy = point[1] - at.position.y;
        let lx = cos * dx + sin * dy;
        let ly = -sin * dx + cos * dy;
        if lx.abs() <= hx && ly.abs() <= hy {
            let area = hx * hy;
            if best.is_none_or(|(_, so_far)| area < so_far) {
                best = Some((entity, area));
            }
        }
    }
    best.map(|(entity, _)| entity)
}

/// A 2D renderable's half extents, or `None` for a shape with no box —
/// a polyline and a polygon carry their points in a mesh asset.
fn half_extents_2d(renderable: &crate::Renderable2d) -> Option<(f32, f32)> {
    match renderable.shape {
        crate::Shape2d::Sprite { hx, hy } => Some((hx, hy)),
        crate::Shape2d::Flat(flat) => {
            let points = flat.outline();
            if points.is_empty() {
                return None;
            }
            let hx = points.iter().map(|p| p.x.abs()).fold(0.0, f32::max);
            let hy = points.iter().map(|p| p.y.abs()).fold(0.0, f32::max);
            Some((hx, hy))
        }
        crate::Shape2d::Polyline { .. } | crate::Shape2d::Polygon => None,
    }
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
            skew: 0.0,
        }
    }

    /// A cuboid, spelled once: the tests care about the box a shape covers,
    /// not about how finely it is cut.
    fn cuboid(hx: f32, hy: f32, hz: f32) -> Shape {
        Shape::Solid(Solid::cuboid(hx, hy, hz))
    }

    fn renderable(shape: Shape) -> Renderable {
        Renderable {
            shape,
            bounds: None,
            color: [1.0; 4],
            mesh: None,
            built: None,
            skeleton: String::new(),
            texture: String::new(),
            material: String::new(),
            shadows: true,
            layers: u32::MAX,
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
        assert!(
            hit_box(
                &place,
                Vec3::ZERO,
                half,
                Vec3::new(-20.0, 0.0, -10.0),
                across
            )
            .is_some()
        );
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
        let flat = Shape::Solid(Solid::Plane {
            hx: 5.0,
            hz: 5.0,
            segments: 1,
        });
        let (_, half) = local_box(&renderable(flat)).unwrap();
        let above = Vec3::new(1.0, 4.0, 1.0);
        let down = Vec3::new(0.0, -1.0, 0.0);
        assert!(hit_box(&place, Vec3::ZERO, half, above, down).is_some());
    }

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
            renderable(cuboid(1.0, 1.0, 1.0)),
            at(Vec3::new(0.0, 0.0, -5.0)),
        ));
        let _far = world.spawn((
            renderable(cuboid(1.0, 1.0, 1.0)),
            at(Vec3::new(0.0, 0.0, -20.0)),
        ));
        let found = candidates(&world, Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0));
        let (entity, distance) = (found[0].entity, found[0].distance);
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
            renderable(cuboid(1.0, 1.0, 1.0)),
            at(Vec3::new(0.0, 0.0, -5.0)),
        ));
        assert!(
            candidates(&world, Vec3::new(50.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0)).is_empty()
        );
    }
}
