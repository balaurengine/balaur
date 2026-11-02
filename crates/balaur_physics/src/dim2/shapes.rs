//! The `collider2d` shapes built from an asset — a mesh, a voxel grid, a
//! heightfield — and the build-time choices each one takes, read off the
//! shape's own record and the components beside the collider.

use anyhow::{Result, anyhow};
use balaur_core::Engine;

use crate::dim2::collider::{ConvexDecomposition2d, Cut2d, MeshCollider2d, VhacdCut};
use crate::dim2::collider_shape::{ConvexPolygonCollider, TriangleMeshCollider2d};
use crate::dim2::decompose;
use crate::properties::{Edges2d, Fill, Fit2d};
use crate::rapier2d::math::Vector;
use crate::rapier2d::parry::transformation::vhacd::VHACDParameters;
use crate::rapier2d::parry::transformation::voxelization::FillMode;
use crate::rapier2d::prelude::{
    ColliderBuilder as ColliderBuilder2, MassProperties, MeshConverter, SharedShape, TriMeshFlags,
};
use crate::scalar::{self, Pose2, Real};

/// The `mesh` asset's points as 2D, with the triangles over them.
pub(crate) fn mesh_of(eng: &Engine, reference: &str) -> Result<(Vec<Vector>, Vec<[u32; 3]>)> {
    if reference.is_empty() {
        return Err(anyhow!("a mesh-backed collider2d needs a `mesh` asset"));
    }
    let definition =
        balaur_core::assets::load_typed::<balaur_geometry::mesh::MeshData>(eng, reference)?;
    let mesh = balaur_geometry::mesh::load_from(eng, &definition)?;
    let points = mesh
        .positions
        .iter()
        .map(|p| scalar::v2(p[0], p[1]))
        .collect();
    Ok((points, mesh.indices.clone()))
}

/// The mesh's triangles as they are, with the flags 3D passes, for the same
/// reason: without them a body catches on the seam between two triangles of
/// flat ground.
pub(crate) fn triangle_mesh(
    eng: &Engine,
    source: &TriangleMeshCollider2d,
    tidy: &MeshCollider2d,
) -> Result<ColliderBuilder2> {
    let (points, indices) = mesh_of(eng, &source.mesh)?;
    let flags = TriMeshFlags::from_bits_truncate(crate::shared::collider::trimesh_bits(
        source.fix_internal_edges,
        source.two_sided_edges,
        &crate::shared::collider::Tidy {
            merge_vertices: tidy.merge_vertices,
            drop_degenerate_triangles: tidy.drop_degenerate_triangles,
            drop_duplicate_triangles: tidy.drop_duplicate_triangles,
            drop_bad_topology: tidy.drop_bad_topology,
            topology: tidy.topology,
            connected_components: tidy.connected_components,
            oriented: tidy.oriented,
        },
    ));
    ColliderBuilder2::trimesh_with_flags(points, indices, flags)
        .map_err(|e| anyhow!("that mesh cannot be a triangle_mesh collider: {e}"))
}

/// The smallest convex outline around the mesh's points, rounded by `border`.
pub(crate) fn convex_hull(eng: &Engine, reference: &str, border: Real) -> Result<ColliderBuilder2> {
    let (points, _) = mesh_of(eng, reference)?;
    let hull = if border > 0.0 {
        ColliderBuilder2::round_convex_hull(&points, border)
    } else {
        ColliderBuilder2::convex_hull(&points)
    };
    hull.ok_or_else(|| anyhow!("those {} points have no hull", points.len()))
}

/// The mesh's points in order, taken as a convex counter-clockwise outline:
/// no hull is computed, and `keep_collinear` keeps the points on a straight
/// edge.
pub(crate) fn convex_polygon(
    eng: &Engine,
    polygon: &ConvexPolygonCollider,
) -> Result<ColliderBuilder2> {
    use crate::rapier2d::parry::shape::{ConvexPolygon, RoundShape};
    let (points, _) = mesh_of(eng, &polygon.mesh)?;
    let count = points.len();
    let shape = if polygon.keep_collinear {
        ConvexPolygon::from_convex_polyline_unmodified(points)
    } else {
        ConvexPolygon::from_convex_polyline(points)
    }
    .ok_or_else(|| anyhow!("those {count} points do not outline a convex polygon"))?;
    let border = polygon.edge_radius.max(0.0);
    let shape = if border > 0.0 {
        SharedShape::new(RoundShape {
            inner_shape: shape,
            border_radius: border,
        })
    } else {
        SharedShape::new(shape)
    };
    Ok(ColliderBuilder2::new(shape))
}

/// A box, an oriented box or a hull fitted to the mesh, which keeps the pose
/// it was fitted at: `add_collider_at` composes it.
pub(crate) fn fit(eng: &Engine, reference: &str, fit: Fit2d) -> Result<ColliderBuilder2> {
    let (points, indices) = mesh_of(eng, reference)?;
    let converter = match fit {
        Fit2d::Aabb => MeshConverter::Aabb,
        Fit2d::Obb => MeshConverter::Obb,
        Fit2d::ConvexHull => MeshConverter::ConvexHull,
    };
    ColliderBuilder2::converted_trimesh(points, indices, converter)
        .map_err(|e| anyhow!("that mesh cannot be fitted: {e}"))
}

/// The mesh's points in order, its outline, or every edge of its triangles,
/// as a line. `oriented` is opt-in: it reads the winding to decide which
/// side is solid, so it is wrong on a chain wound the other way.
pub(crate) fn polyline(
    eng: &Engine,
    reference: &str,
    edges: Edges2d,
    oriented: bool,
) -> Result<ColliderBuilder2> {
    let (points, indices) = mesh_of(eng, reference)?;
    if points.len() < 2 {
        return Err(anyhow!(
            "a polyline collider needs at least two points, not {}",
            points.len()
        ));
    }
    let edges = match edges {
        Edges2d::Outline => Some(boundary(&indices)),
        Edges2d::Mesh => Some(crate::shared::collider::mesh_edges(&indices)),
        Edges2d::Chain => None,
    };
    if oriented {
        return Ok(ColliderBuilder2::oriented_polyline(points, edges));
    }
    Ok(ColliderBuilder2::polyline(points, edges))
}

/// A cell for each of the mesh's points.
pub(crate) fn voxelized_points(
    eng: &Engine,
    reference: &str,
    size: f32,
) -> Result<ColliderBuilder2> {
    let (points, _) = mesh_of(eng, reference)?;
    let size = crate::shapes::voxel_size(size).unwrap_or(DEFAULT_VOXEL_SIZE);
    Ok(ColliderBuilder2::voxels_from_points(
        Vector::splat(size),
        &points,
    ))
}

/// The mesh's outline filled into cells.
pub(crate) fn voxelized_mesh(
    eng: &Engine,
    reference: &str,
    size: f32,
    tidy: &MeshCollider2d,
) -> Result<ColliderBuilder2> {
    let (points, indices) = mesh_of(eng, reference)?;
    let size = crate::shapes::voxel_size(size).unwrap_or(DEFAULT_VOXEL_SIZE);
    Ok(ColliderBuilder2::voxelized_mesh(
        &points,
        &boundary(&indices),
        size,
        fill_mode(tidy),
    ))
}

/// A cell's size where neither the author nor an asset gave one.
const DEFAULT_VOXEL_SIZE: Real = 0.25;

/// How a voxelized outline is filled: its edge, or its inside, with or
/// without walled-off cavities and its own crossings untangled.
fn fill_mode(tidy: &MeshCollider2d) -> FillMode {
    match tidy.fill {
        Fill::Surface => FillMode::SurfaceOnly,
        Fill::Solid => FillMode::FloodFill {
            detect_cavities: tidy.fill_cavities,
            detect_self_intersections: tidy.fix_self_intersections,
        },
    }
}

/// A cut's tuning over parry's 2D defaults.
fn vhacd(cut: &VhacdCut, tidy: &MeshCollider2d) -> VHACDParameters {
    let mut tuning = VHACDParameters::default();
    let whole = |value: i64| u32::try_from(value.max(1)).unwrap_or(u32::MAX);
    tuning.resolution = whole(cut.resolution);
    tuning.concavity = cut.max_concavity;
    tuning.max_convex_hulls = whole(cut.max_convex_hulls);
    tuning.alpha = cut.symmetry_bias;
    tuning.beta = cut.revolution_bias;
    tuning.plane_downsampling = whole(cut.plane_downsampling);
    tuning.convex_hull_downsampling = whole(cut.hull_downsampling);
    tuning.convex_hull_approximation = cut.approximate_hulls;
    tuning.fill_mode = fill_mode(tidy);
    tuning
}

/// A concave polygon as convex pieces that overlap across their seams: the
/// only shape a concave *dynamic* 2D body can have and not wedge a thin one.
///
/// The mass is the ungrown pieces', since the grown ones share the ground
/// they overlap on and would weigh it twice.
pub(crate) fn decomposition_collider(
    eng: &Engine,
    reference: &str,
    border: Real,
    cut: &ConvexDecomposition2d,
    tidy: &MeshCollider2d,
    density: Real,
) -> Result<(ColliderBuilder2, Option<MassProperties>)> {
    let (points, indices) = mesh_of(eng, reference)?;
    let overlap = match &cut.method {
        Cut2d::Vhacd(tuning) => {
            return Ok((
                vhacd_collider(&vhacd(tuning, tidy), &points, &indices, border),
                None,
            ));
        }
        Cut2d::Voxels(tuning) => {
            return Ok((voxel_parts(&vhacd(tuning, tidy), &points, &indices)?, None));
        }
        Cut2d::Exact(exact) => exact.overlap,
    };
    let pieces = decompose::pieces(&points, &indices);
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
    tuning: &VHACDParameters,
    points: &[Vector],
    indices: &[[u32; 3]],
    border: Real,
) -> ColliderBuilder2 {
    let outline = boundary(indices);
    if border > 0.0 {
        return ColliderBuilder2::round_convex_decomposition_with_params(
            points, &outline, tuning, border,
        );
    }
    ColliderBuilder2::convex_decomposition_with_params(points, &outline, tuning)
}

/// The outline cut into voxel parts, kept as one compound.
fn voxel_parts(
    tuning: &VHACDParameters,
    points: &[Vector],
    indices: &[[u32; 3]],
) -> Result<ColliderBuilder2> {
    let parts: Vec<_> =
        SharedShape::voxelized_convex_decomposition_with_params(points, &boundary(indices), tuning)
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
pub(crate) fn voxel_collider(eng: &Engine, reference: &str, size: f32) -> Result<ColliderBuilder2> {
    if reference.is_empty() {
        return Err(anyhow!("a voxels collider2d needs a `voxels` asset"));
    }
    let grid =
        balaur_core::assets::load_typed::<balaur_geometry::voxels::VoxelsData>(eng, reference)?;
    let cells: Vec<crate::rapier2d::math::IVector> = grid
        .cells
        .iter()
        .map(|c| scalar::cell2(c[0], c[1]))
        .collect();
    let size = crate::shapes::voxel_size(size)
        .map_or_else(|| scalar::v2(grid.size[0], grid.size[1]), Vector::splat);
    Ok(ColliderBuilder2::voxels(size, &cells))
}

/// A 2D heightfield is one row of heights: a side-scroller's ground. A hole
/// in the asset removes the segment starting at that height.
pub(crate) fn heightfield_collider(
    eng: &Engine,
    reference: &str,
    scale: Vector,
) -> Result<ColliderBuilder2> {
    if reference.is_empty() {
        return Err(anyhow!(
            "a heightfield collider2d needs a `heightfield` asset"
        ));
    }
    let field = balaur_core::assets::load_typed::<balaur_geometry::heightfield::HeightfieldData>(
        eng, reference,
    )?;
    let mut ground = crate::rapier2d::parry::shape::HeightField::new(field.heights.clone(), scale);
    for &[row, column] in &field.holes {
        ground.set_segment_removed(row * field.columns + column, true);
    }
    Ok(ColliderBuilder2::new(SharedShape::new(ground)))
}
