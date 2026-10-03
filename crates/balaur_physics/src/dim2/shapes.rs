//! The `collider2d` shapes built from an asset — a mesh, a voxel grid, a
//! heightfield — and the build-time choices each one takes.

use anyhow::{Result, anyhow};
use balaur_core::Engine;

use crate::dim2::decompose;
use crate::rapier2d::math::Vector;
use crate::rapier2d::parry::transformation::vhacd::VHACDParameters;
use crate::rapier2d::parry::transformation::voxelization::FillMode;
use crate::rapier2d::prelude::{
    ColliderBuilder as ColliderBuilder2, MassProperties, MeshConverter, SharedShape, TriMeshFlags,
};
use crate::scalar::{self, Pose2, Real};
use crate::vocabulary::{self as v, keys as k, words as w};

/// The `mesh` asset's points as 2D, with the triangles over them.
pub(crate) fn mesh_of(
    eng: &Engine,
    params: &toml::Value,
    kind: &str,
) -> Result<(Vec<Vector>, Vec<[u32; 3]>)> {
    let reference = params
        .get(k::MESH)
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a {kind} collider2d needs a `mesh` asset"))?;
    let definition =
        balaur_core::assets::load_typed::<balaur_core::mesh::MeshData>(eng, reference)?;
    let mesh = balaur_core::mesh::load_from(eng, &definition)?;
    let points = mesh
        .positions
        .iter()
        .map(|p| scalar::v2(p[0], p[1]))
        .collect();
    Ok((points, mesh.indices.clone()))
}

/// A 2D shape from a `mesh` asset, reading the x and y of its points: the
/// same asset a `polygon` draws, so the outline a player sees and the one they
/// collide with are one authored thing.
pub(crate) fn mesh_collider(
    eng: &Engine,
    params: &toml::Value,
    kind: &str,
    border: Real,
) -> Result<ColliderBuilder2> {
    let (points, indices) = mesh_of(eng, params, kind)?;
    match kind {
        // The flags 3D passes, for the same reason: without them a body
        // catches on the seam between two triangles of flat ground.
        w::TRIANGLE_MESH => {
            let flags =
                TriMeshFlags::from_bits_truncate(crate::shared::collider::trimesh_bits(params));
            ColliderBuilder2::trimesh_with_flags(points, indices, flags)
                .map_err(|e| anyhow!("that mesh cannot be a triangle_mesh collider: {e}"))
        }
        w::CONVEX_HULL => {
            let hull = if border > 0.0 {
                ColliderBuilder2::round_convex_hull(&points, border)
            } else {
                ColliderBuilder2::convex_hull(&points)
            };
            hull.ok_or_else(|| anyhow!("those {} points have no hull", points.len()))
        }
        w::CONVEX_POLYGON => convex_polygon(params, points, border),
        // A box, an oriented box or a hull fitted to the mesh, which keeps
        // the pose it was fitted at: `add_collider_at` composes it.
        w::FIT => {
            let converter = match v::text(params, k::FIT, w::CONVEX_HULL) {
                w::AABB => MeshConverter::Aabb,
                w::OBB => MeshConverter::Obb,
                _ => MeshConverter::ConvexHull,
            };
            ColliderBuilder2::converted_trimesh(points, indices, converter)
                .map_err(|e| anyhow!("that mesh cannot be fitted: {e}"))
        }
        w::VOXELIZED_POINTS => {
            let size = crate::shapes::voxel_size(params).unwrap_or(DEFAULT_VOXEL_SIZE);
            Ok(ColliderBuilder2::voxels_from_points(
                Vector::splat(size),
                &points,
            ))
        }
        w::VOXELIZED_MESH => {
            let size = crate::shapes::voxel_size(params).unwrap_or(DEFAULT_VOXEL_SIZE);
            Ok(ColliderBuilder2::voxelized_mesh(
                &points,
                &boundary(&indices),
                size,
                fill_mode(params),
            ))
        }
        _ => {
            if points.len() < 2 {
                return Err(anyhow!(
                    "a polyline collider needs at least two points, not {}",
                    points.len()
                ));
            }
            let edges = match v::text(params, k::EDGES, w::CHAIN) {
                w::OUTLINE => Some(boundary(&indices)),
                w::MESH => Some(crate::shared::collider::mesh_edges(&indices)),
                _ => None,
            };
            // `oriented` is opt-in: it reads the winding to decide which side
            // is solid, so it is wrong on a chain wound the other way.
            if v::boolean(params, k::ORIENTED, false) {
                return Ok(ColliderBuilder2::oriented_polyline(points, edges));
            }
            Ok(ColliderBuilder2::polyline(points, edges))
        }
    }
}

/// A cell's size where neither the author nor an asset gave one.
const DEFAULT_VOXEL_SIZE: Real = 0.25;

/// The mesh's points in order, taken as a convex counter-clockwise outline:
/// no hull is computed, and `keep_collinear` keeps the points on a straight
/// edge.
fn convex_polygon(
    params: &toml::Value,
    points: Vec<Vector>,
    border: Real,
) -> Result<ColliderBuilder2> {
    use crate::rapier2d::parry::shape::{ConvexPolygon, RoundShape};
    let count = points.len();
    let polygon = if v::boolean(params, k::KEEP_COLLINEAR, false) {
        ConvexPolygon::from_convex_polyline_unmodified(points)
    } else {
        ConvexPolygon::from_convex_polyline(points)
    }
    .ok_or_else(|| anyhow!("those {count} points do not outline a convex polygon"))?;
    let shape = if border > 0.0 {
        SharedShape::new(RoundShape {
            inner_shape: polygon,
            border_radius: border,
        })
    } else {
        SharedShape::new(polygon)
    };
    Ok(ColliderBuilder2::new(shape))
}

/// How a voxelized outline is filled: its edge, or its inside, with or
/// without walled-off cavities and its own crossings untangled.
fn fill_mode(params: &toml::Value) -> FillMode {
    if v::text(params, k::FILL, w::SOLID) == w::SURFACE {
        FillMode::SurfaceOnly
    } else {
        FillMode::FloodFill {
            detect_cavities: v::boolean(params, k::FILL_CAVITIES, false),
            detect_self_intersections: v::boolean(params, k::FIX_SELF_INTERSECTIONS, false),
        }
    }
}

/// The VHACD keys, over parry's 2D defaults.
fn vhacd(params: &toml::Value) -> VHACDParameters {
    let mut tuning = VHACDParameters::default();
    let whole = |key: &str, default: u32| v::f(params, key, default as f32).max(1.0) as u32;
    let real = |key: &str, default: Real| scalar::real(v::f(params, key, scalar::f32_of(default)));
    tuning.resolution = whole(k::RESOLUTION, tuning.resolution);
    tuning.concavity = real(k::MAX_CONCAVITY, tuning.concavity);
    tuning.max_convex_hulls = whole(k::MAX_CONVEX_HULLS, tuning.max_convex_hulls);
    tuning.alpha = real(k::SYMMETRY_BIAS, tuning.alpha);
    tuning.beta = real(k::REVOLUTION_BIAS, tuning.beta);
    tuning.plane_downsampling = whole(k::PLANE_DOWNSAMPLING, tuning.plane_downsampling);
    tuning.convex_hull_downsampling = whole(k::HULL_DOWNSAMPLING, tuning.convex_hull_downsampling);
    tuning.convex_hull_approximation = v::boolean(
        params,
        k::APPROXIMATE_HULLS,
        tuning.convex_hull_approximation,
    );
    tuning.fill_mode = fill_mode(params);
    tuning
}

/// A concave polygon as convex pieces that overlap across their seams: the
/// only shape a concave *dynamic* 2D body can have and not wedge a thin one.
///
/// The mass is the ungrown pieces', since the grown ones share the ground
/// they overlap on and would weigh it twice.
pub(crate) fn decomposition_collider(
    eng: &Engine,
    params: &toml::Value,
) -> Result<(ColliderBuilder2, Option<MassProperties>)> {
    let (points, indices) = mesh_of(eng, params, w::CONVEX_DECOMPOSITION)?;
    let border = scalar::real(v::f(params, k::EDGE_RADIUS, 0.0)).max(0.0);
    match v::text(params, k::METHOD, w::EXACT) {
        w::VHACD => return Ok((vhacd_collider(params, &points, &indices, border), None)),
        w::VOXELS => return Ok((voxel_parts(params, &points, &indices)?, None)),
        _ => {}
    }
    let pieces = decompose::pieces(&points, &indices);
    let overlap = v::f(params, k::OVERLAP, 0.9);
    let density = scalar::real(v::f(params, k::DENSITY, 1.0).max(0.0));
    let mut weights = pieces.iter().map(|piece| {
        let ring: Vec<Vector> = piece.iter().map(|&at| points[at as usize]).collect();
        MassProperties::from_convex_polygon(density, &ring)
    });
    let mass = weights
        .next()
        .map(|first| weights.fold(first, |sum, next| sum + next));
    let shapes: Vec<_> = decompose::grown(&points, &pieces, overlap)
        .into_iter()
        .filter_map(|piece| Some((Pose2::IDENTITY, convex_piece(piece, border)?)))
        .collect();
    if shapes.is_empty() {
        return Err(anyhow!(
            "that mesh has no area, so it cannot be cut into convex pieces"
        ));
    }
    Ok((ColliderBuilder2::compound(shapes), mass))
}

/// One piece as a shape, rounded when the collider asked for a border.
fn convex_piece(piece: Vec<Vector>, border: Real) -> Option<SharedShape> {
    if border > 0.0 {
        SharedShape::round_convex_polyline(piece, border)
    } else {
        SharedShape::convex_polyline(piece)
    }
}

/// The approximate cut, for an outline dense enough that the exact one's
/// quadratic merge shows. rapier voxelises the outline, so it takes segments.
fn vhacd_collider(
    params: &toml::Value,
    points: &[Vector],
    indices: &[[u32; 3]],
    border: Real,
) -> ColliderBuilder2 {
    let tuning = vhacd(params);
    let outline = boundary(indices);
    if border > 0.0 {
        return ColliderBuilder2::round_convex_decomposition_with_params(
            points, &outline, &tuning, border,
        );
    }
    ColliderBuilder2::convex_decomposition_with_params(points, &outline, &tuning)
}

/// The outline cut into voxel parts, kept as one compound.
fn voxel_parts(
    params: &toml::Value,
    points: &[Vector],
    indices: &[[u32; 3]],
) -> Result<ColliderBuilder2> {
    let parts: Vec<_> = SharedShape::voxelized_convex_decomposition_with_params(
        points,
        &boundary(indices),
        &vhacd(params),
    )
    .into_iter()
    .map(|part| (Pose2::IDENTITY, part))
    .collect();
    if parts.is_empty() {
        return Err(anyhow!(
            "that outline voxelized to nothing; raise `resolution` or check the mesh"
        ));
    }
    Ok(ColliderBuilder2::compound(parts))
}

/// The outline of a triangulated mesh: every edge only one triangle uses,
/// which is what a 2D decomposition over a polyline needs.
pub(crate) fn boundary(indices: &[[u32; 3]]) -> Vec<[u32; 2]> {
    let mut edges: std::collections::BTreeMap<(u32, u32), [u32; 2]> =
        std::collections::BTreeMap::new();
    for &[a, b, c] in indices {
        for edge in [[a, b], [b, c], [c, a]] {
            let key = (edge[0].min(edge[1]), edge[0].max(edge[1]));
            if edges.remove(&key).is_none() {
                edges.insert(key, edge);
            }
        }
    }
    edges.into_values().collect()
}

/// A voxel grid from a `voxels` asset, the 2D twin of the 3D kind. parry
/// classifies each cell from its neighbours, so a body sliding along a wall
/// of them cannot catch on the seam between two.
pub(crate) fn voxel_collider(eng: &Engine, params: &toml::Value) -> Result<ColliderBuilder2> {
    let reference = params
        .get(k::VOXELS)
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a voxels collider2d needs a `voxels` asset"))?;
    let grid = balaur_core::assets::load_typed::<balaur_core::voxels::VoxelsData>(eng, reference)?;
    let cells: Vec<crate::rapier2d::math::IVector> = grid
        .cells
        .iter()
        .map(|c| scalar::cell2(c[0], c[1]))
        .collect();
    let size = crate::shapes::voxel_size(params)
        .map_or_else(|| scalar::v2(grid.size[0], grid.size[1]), Vector::splat);
    Ok(ColliderBuilder2::voxels(size, &cells))
}

/// A 2D heightfield is one row of heights: a side-scroller's ground. A hole
/// in the asset removes the segment starting at that height.
pub(crate) fn heightfield_collider(eng: &Engine, params: &toml::Value) -> Result<ColliderBuilder2> {
    let reference = params
        .get(k::HEIGHTFIELD)
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a heightfield collider2d needs a `heightfield` asset"))?;
    let field = balaur_core::assets::load_typed::<balaur_core::heightfield::HeightfieldData>(
        eng, reference,
    )?;
    let mut ground = crate::rapier2d::parry::shape::HeightField::new(
        field.heights.iter().map(|h| scalar::real(*h)).collect(),
        scalar::v2a(v::vec2(params, k::SCALE, [1.0, 1.0])),
    );
    for &[row, column] in &field.holes {
        ground.set_segment_removed(row * field.columns + column, true);
    }
    Ok(ColliderBuilder2::new(SharedShape::new(ground)))
}
