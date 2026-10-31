//! The `collider3d` shapes built from an asset — a mesh, a voxel grid, a
//! heightfield — and the build-time choices each one takes, read off the
//! shape's own record and the components beside the collider.

use anyhow::{Result, anyhow, bail};
use balaur_core::Engine;

use crate::collider::{ConvexDecomposition3d, MeshCollider3d};
use crate::collider_shape::TriangleMeshCollider;
use crate::properties::{Decomposition3d, Edges, Fill, Fit3d};
use crate::rapier3d::math::Vector;
use crate::rapier3d::parry::transformation::vhacd::VHACDParameters;
use crate::rapier3d::parry::transformation::voxelization::FillMode;
use crate::rapier3d::prelude::{ColliderBuilder, MeshConverter, SharedShape, TriMeshFlags};
use crate::scalar::{self, Real};

/// The geometry a mesh-backed collider names, through the same asset the
/// renderer uses.
pub(crate) fn collider_mesh(
    eng: &Engine,
    reference: &str,
) -> Result<balaur_geometry::mesh::MeshData> {
    if reference.is_empty() {
        bail!("a mesh-backed collider needs a `mesh` asset");
    }
    let definition =
        balaur_core::assets::load_typed::<balaur_geometry::mesh::MeshData>(eng, reference)?;
    balaur_geometry::mesh::load_from(eng, &definition)
}

/// A mesh's points, in rapier's precision.
fn points_of(mesh: &balaur_geometry::mesh::MeshData) -> Vec<Vector> {
    mesh.positions.iter().map(|p| scalar::v3a(*p)).collect()
}

/// The mesh's triangles as they are. The flags are what stop a character
/// controller catching on the seam between two triangles of a flat floor.
pub(crate) fn triangle_mesh(
    eng: &Engine,
    source: &TriangleMeshCollider,
    tidy: &MeshCollider3d,
) -> Result<ColliderBuilder> {
    let mesh = collider_mesh(eng, &source.mesh)?;
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
    ColliderBuilder::trimesh_with_flags(points_of(&mesh), mesh.indices.clone(), flags)
        .map_err(|e| anyhow!("that mesh cannot be a triangle_mesh collider: {e}"))
}

/// The smallest convex shape around the mesh's points, rounded by `border`.
pub(crate) fn convex_hull(eng: &Engine, reference: &str, border: Real) -> Result<ColliderBuilder> {
    let mesh = collider_mesh(eng, reference)?;
    let points = points_of(&mesh);
    let hull = if border > 0.0 {
        ColliderBuilder::round_convex_hull(&points, border)
    } else {
        ColliderBuilder::convex_hull(&points)
    };
    Ok(hull.unwrap_or_else(|| {
        // Degenerate input (every point on one line or plane) has no hull.
        // The node keeps a collider rather than losing one silently.
        let (min, max) = mesh.bounds().unwrap_or(([-0.5; 3], [0.5; 3]));
        tracing::warn!(
            "convex_hull: those {} points are degenerate; using their bounding box",
            points.len()
        );
        let half = |i: usize| scalar::real(((max[i] - min[i]) / 2.0).max(0.01));
        ColliderBuilder::cuboid(half(0), half(1), half(2))
    }))
}

/// The mesh taken as already convex, rounded by `border`.
pub(crate) fn convex_mesh(eng: &Engine, reference: &str, border: Real) -> Result<ColliderBuilder> {
    let mesh = collider_mesh(eng, reference)?;
    let points = points_of(&mesh);
    let built = if border > 0.0 {
        ColliderBuilder::round_convex_mesh(points, &mesh.indices, border)
    } else {
        ColliderBuilder::convex_mesh(points, &mesh.indices)
    };
    built.ok_or_else(|| anyhow!("that mesh's triangles do not bound a convex shape"))
}

/// The mesh's points in order, or every edge of its triangles, as a line.
pub(crate) fn polyline(eng: &Engine, reference: &str, edges: Edges) -> Result<ColliderBuilder> {
    let mesh = collider_mesh(eng, reference)?;
    let points = points_of(&mesh);
    if points.len() < 2 {
        bail!(
            "a polyline collider needs at least two points, not {}",
            points.len()
        );
    }
    let edges = match edges {
        Edges::Mesh => Some(crate::shared::collider::mesh_edges(&mesh.indices)),
        // The points in order, which is what a vertex list means; a closed
        // loop repeats the first point at the end.
        Edges::Chain => None,
    };
    Ok(ColliderBuilder::polyline(points, edges))
}

/// A shape fitted to the mesh rather than made of it: a box, an oriented
/// box, a hull, or convex pieces cut as `cut` says.
pub(crate) fn fit(
    eng: &Engine,
    reference: &str,
    fit: Fit3d,
    cut: &ConvexDecomposition3d,
    tidy: &MeshCollider3d,
) -> Result<ColliderBuilder> {
    let mesh = collider_mesh(eng, reference)?;
    let converter = match fit {
        Fit3d::Aabb => MeshConverter::Aabb,
        Fit3d::Obb => MeshConverter::Obb,
        Fit3d::ConvexDecomposition => {
            MeshConverter::ConvexDecompositionWithParams(vhacd(cut, tidy))
        }
        Fit3d::ConvexHull => MeshConverter::ConvexHull,
    };
    ColliderBuilder::converted_trimesh(points_of(&mesh), mesh.indices.clone(), converter)
        .map_err(|e| anyhow!("that mesh cannot be fitted: {e}"))
}

/// A cell for each of the mesh's points.
pub(crate) fn voxelized_points(
    eng: &Engine,
    reference: &str,
    size: f32,
) -> Result<ColliderBuilder> {
    let mesh = collider_mesh(eng, reference)?;
    let size = voxel_size(size).unwrap_or(DEFAULT_VOXEL_SIZE);
    Ok(ColliderBuilder::voxels_from_points(
        Vector::splat(size),
        &points_of(&mesh),
    ))
}

/// A cell's size where neither the author nor an asset gave one.
const DEFAULT_VOXEL_SIZE: Real = 0.25;

/// A `voxel_size`, or `None` for the 0 that leaves the size to the asset.
pub(crate) fn voxel_size(size: f32) -> Option<Real> {
    let size = scalar::real(size);
    (size > 0.0).then(|| size.max(0.001))
}

/// A decomposition's rows over parry's 3D defaults.
fn vhacd(cut: &ConvexDecomposition3d, tidy: &MeshCollider3d) -> VHACDParameters {
    let mut tuning = VHACDParameters::default();
    let whole = |value: i64| u32::try_from(value.max(1)).unwrap_or(u32::MAX);
    tuning.resolution = whole(cut.resolution);
    tuning.concavity = scalar::real(cut.max_concavity);
    tuning.max_convex_hulls = whole(cut.max_convex_hulls);
    tuning.alpha = scalar::real(cut.symmetry_bias);
    tuning.beta = scalar::real(cut.revolution_bias);
    tuning.plane_downsampling = whole(cut.plane_downsampling);
    tuning.convex_hull_downsampling = whole(cut.hull_downsampling);
    tuning.convex_hull_approximation = cut.approximate_hulls;
    tuning.fill_mode = fill_mode(tidy);
    tuning
}

/// How a voxelized mesh is filled: its shell, or its inside, with or without
/// the cavities a closed surface walls off.
pub(crate) fn fill_mode(tidy: &MeshCollider3d) -> FillMode {
    match tidy.fill {
        Fill::Surface => FillMode::SurfaceOnly,
        Fill::Solid => FillMode::FloodFill {
            detect_cavities: tidy.fill_cavities,
        },
    }
}

/// The mesh cut into convex hulls, or into voxel parts, which rapier keeps as
/// one compound either way. Voxel parts have no border to round.
pub(crate) fn decomposition(
    eng: &Engine,
    reference: &str,
    cut: &ConvexDecomposition3d,
    tidy: &MeshCollider3d,
    border: Real,
) -> Result<ColliderBuilder> {
    let mesh = collider_mesh(eng, reference)?;
    let points = points_of(&mesh);
    let tuning = vhacd(cut, tidy);
    if cut.method == Decomposition3d::Voxels {
        let parts: Vec<_> = SharedShape::voxelized_convex_decomposition_with_params(
            &points,
            &mesh.indices,
            &tuning,
        )
        .into_iter()
        .map(|part| (scalar::Pose::IDENTITY, part))
        .collect();
        if parts.is_empty() {
            bail!("that mesh voxelized to nothing; lower `resolution` or check its triangles");
        }
        return Ok(ColliderBuilder::compound(parts));
    }
    Ok(if border > 0.0 {
        ColliderBuilder::round_convex_decomposition_with_params(
            &points,
            &mesh.indices,
            &tuning,
            border,
        )
    } else {
        ColliderBuilder::convex_decomposition_with_params(&points, &mesh.indices, &tuning)
    })
}

/// A voxel grid from a `voxels` asset: filled cells on a lattice, editable
/// from a script while the game runs. `voxel_size` resizes the asset's cells.
pub(crate) fn voxel_collider(eng: &Engine, reference: &str, size: f32) -> Result<ColliderBuilder> {
    if reference.is_empty() {
        bail!("a voxels collider needs a `voxels` asset");
    }
    let grid =
        balaur_core::assets::load_typed::<balaur_geometry::voxels::VoxelsData>(eng, reference)?;
    let cells: Vec<crate::rapier3d::math::IVector> = grid
        .cells
        .iter()
        .map(|c| scalar::cell(c[0], c[1], c[2]))
        .collect();
    let size = voxel_size(size).map_or_else(|| scalar::v3a(grid.size), Vector::splat);
    Ok(ColliderBuilder::voxels(size, &cells))
}

/// A voxel grid built from a mesh, so a model can become destructible terrain
/// without anyone authoring a cell list.
pub(crate) fn voxelized_mesh_collider(
    eng: &Engine,
    reference: &str,
    size: f32,
    tidy: &MeshCollider3d,
) -> Result<ColliderBuilder> {
    let mesh = collider_mesh(eng, reference)?;
    let size = voxel_size(size).unwrap_or(DEFAULT_VOXEL_SIZE);
    Ok(ColliderBuilder::voxelized_mesh(
        &points_of(&mesh),
        &mesh.indices,
        size,
        fill_mode(tidy),
    ))
}

/// Terrain from a `heightfield` asset, with the asset's holes cut out. The
/// extent belongs to the collider, so one grid can be placed at several sizes.
pub(crate) fn heightfield_collider(
    eng: &Engine,
    reference: &str,
    extent: Vector,
    fix_internal_edges: bool,
) -> Result<ColliderBuilder> {
    use crate::rapier3d::parry::shape::{HeightField, HeightFieldCellStatus, HeightFieldFlags};
    if reference.is_empty() {
        bail!("a heightfield collider needs a `heightfield` asset");
    }
    let field = balaur_core::assets::load_typed::<balaur_geometry::heightfield::HeightfieldData>(
        eng, reference,
    )?;
    // The asset checked its own shape, so Array2's assert cannot fire. The
    // asset is f32; a f64 build widens each height here, once, on load.
    let heights: Vec<Real> = field.heights.iter().map(|h| scalar::real(*h)).collect();
    let grid = crate::rapier3d::parry::utils::Array2::new(field.rows, field.columns, heights);
    // The flag a triangle_mesh gets by default, for the same reason: without it a
    // character catches on the seam between two cells of flat ground.
    let mut flags = HeightFieldFlags::empty();
    if fix_internal_edges {
        flags |= HeightFieldFlags::FIX_INTERNAL_EDGES;
    }
    let mut terrain = HeightField::with_flags(grid, extent, flags);
    for &[row, column] in &field.holes {
        terrain.set_cell_status(row, column, HeightFieldCellStatus::CELL_REMOVED);
    }
    Ok(ColliderBuilder::new(SharedShape::new(terrain)))
}
