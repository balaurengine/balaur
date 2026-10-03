//! The `collider3d` shapes built from an asset — a mesh, a voxel grid, a
//! heightfield — and the build-time choices each one takes.

use anyhow::{Result, anyhow, bail};
use balaur_core::Engine;

use crate::rapier3d::math::Vector;
use crate::rapier3d::parry::transformation::vhacd::VHACDParameters;
use crate::rapier3d::parry::transformation::voxelization::FillMode;
use crate::rapier3d::prelude::{ColliderBuilder, MeshConverter, SharedShape, TriMeshFlags};
use crate::scalar::{self, Real};
use crate::vocabulary::{self as v, keys as k, words as w};

/// The geometry a mesh-backed collider names, through the same asset the
/// renderer uses.
pub(crate) fn collider_mesh(
    eng: &Engine,
    params: &toml::Value,
) -> Result<balaur_core::mesh::MeshData> {
    let reference = params
        .get(k::MESH)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a mesh-backed collider needs a `mesh` asset"))?;
    let definition =
        balaur_core::assets::load_typed::<balaur_core::mesh::MeshData>(eng, reference)?;
    balaur_core::mesh::load_from(eng, &definition)
}

/// The collider kinds built from a `mesh` asset, so the model the renderer
/// draws and the shape it collides with stay one authored thing. `border`
/// rounds the convex kinds.
pub(crate) fn mesh_collider(
    eng: &Engine,
    params: &toml::Value,
    kind: &str,
    border: Real,
) -> Result<ColliderBuilder> {
    let mesh = collider_mesh(eng, params)?;
    let points: Vec<Vector> = mesh.positions.iter().map(|p| scalar::v3a(*p)).collect();
    let indices = mesh.indices.clone();
    match kind {
        // The flags are what stop a character controller catching on the seam
        // between two triangles of a flat floor.
        w::TRIANGLE_MESH => {
            let flags =
                TriMeshFlags::from_bits_truncate(crate::shared::collider::trimesh_bits(params));
            ColliderBuilder::trimesh_with_flags(points, indices, flags)
                .map_err(|e| anyhow!("that mesh cannot be a triangle_mesh collider: {e}"))
        }
        // The only way to get a *dynamic* concave shape: the mesh cut into
        // convex pieces, kept as one compound.
        w::CONVEX_DECOMPOSITION => decomposition(params, &points, &indices, border),
        // A shape fitted to the mesh rather than made of it: a box, an
        // oriented box, or a hull, whichever `fit` asks for.
        w::FIT => {
            let converter = match v::text(params, k::FIT, w::CONVEX_HULL) {
                w::AABB => MeshConverter::Aabb,
                w::OBB => MeshConverter::Obb,
                w::CONVEX_DECOMPOSITION => {
                    MeshConverter::ConvexDecompositionWithParams(vhacd(params))
                }
                _ => MeshConverter::ConvexHull,
            };
            ColliderBuilder::converted_trimesh(points, indices, converter)
                .map_err(|e| anyhow!("that mesh cannot be fitted: {e}"))
        }
        w::CONVEX_HULL => Ok(hull(&points, border).unwrap_or_else(|| {
            // Degenerate input (every point on one line or plane) has no hull.
            // The node keeps a collider rather than losing one silently.
            let (min, max) = mesh.bounds().unwrap_or(([-0.5; 3], [0.5; 3]));
            tracing::warn!(
                "convex_hull: those {} points are degenerate; using their bounding box",
                points.len()
            );
            let half = |i: usize| scalar::real(((max[i] - min[i]) / 2.0).max(0.01));
            ColliderBuilder::cuboid(half(0), half(1), half(2))
        })),
        w::CONVEX_MESH => {
            let built = if border > 0.0 {
                ColliderBuilder::round_convex_mesh(points, &indices, border)
            } else {
                ColliderBuilder::convex_mesh(points, &indices)
            };
            built.ok_or_else(|| anyhow!("that mesh's triangles do not bound a convex shape"))
        }
        w::VOXELIZED_POINTS => {
            let size = voxel_size(params).unwrap_or(DEFAULT_VOXEL_SIZE);
            Ok(ColliderBuilder::voxels_from_points(
                Vector::splat(size),
                &points,
            ))
        }
        _ => {
            if points.len() < 2 {
                bail!(
                    "a polyline collider needs at least two points, not {}",
                    points.len()
                );
            }
            let edges = match v::text(params, k::EDGES, w::CHAIN) {
                w::MESH => Some(crate::shared::collider::mesh_edges(&indices)),
                // The points in order, which is what a vertex list means; a
                // closed loop repeats the first point at the end.
                _ => None,
            };
            Ok(ColliderBuilder::polyline(points, edges))
        }
    }
}

fn hull(points: &[Vector], border: Real) -> Option<ColliderBuilder> {
    if border > 0.0 {
        ColliderBuilder::round_convex_hull(points, border)
    } else {
        ColliderBuilder::convex_hull(points)
    }
}

/// A cell's size where neither the author nor an asset gave one.
const DEFAULT_VOXEL_SIZE: Real = 0.25;

/// `voxel_size`, or `None` for the 0 that leaves the size to the asset.
pub(crate) fn voxel_size(params: &toml::Value) -> Option<Real> {
    let size = scalar::real(v::f(params, k::VOXEL_SIZE, 0.0));
    (size > 0.0).then(|| size.max(0.001))
}

/// The VHACD keys, over parry's 3D defaults.
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

/// How a voxelized mesh is filled: its shell, or its inside, with or without
/// the cavities a closed surface walls off.
pub(crate) fn fill_mode(params: &toml::Value) -> FillMode {
    if v::text(params, k::FILL, w::SOLID) == w::SURFACE {
        FillMode::SurfaceOnly
    } else {
        FillMode::FloodFill {
            detect_cavities: v::boolean(params, k::FILL_CAVITIES, false),
        }
    }
}

/// The mesh cut into convex hulls, or into voxel parts, which rapier keeps as
/// one compound either way. Voxel parts have no border to round.
fn decomposition(
    params: &toml::Value,
    points: &[Vector],
    indices: &[[u32; 3]],
    border: Real,
) -> Result<ColliderBuilder> {
    let tuning = vhacd(params);
    if v::text(params, k::METHOD, w::VHACD) == w::VOXELS {
        let parts: Vec<_> =
            SharedShape::voxelized_convex_decomposition_with_params(points, indices, &tuning)
                .into_iter()
                .map(|part| (scalar::Pose::IDENTITY, part))
                .collect();
        if parts.is_empty() {
            bail!("that mesh voxelized to nothing; lower `resolution` or check its triangles");
        }
        return Ok(ColliderBuilder::compound(parts));
    }
    Ok(if border > 0.0 {
        ColliderBuilder::round_convex_decomposition_with_params(points, indices, &tuning, border)
    } else {
        ColliderBuilder::convex_decomposition_with_params(points, indices, &tuning)
    })
}

/// A voxel grid from a `voxels` asset: filled cells on a lattice, editable
/// from a script while the game runs. `voxel_size` resizes the asset's cells.
pub(crate) fn voxel_collider(eng: &Engine, params: &toml::Value) -> Result<ColliderBuilder> {
    let reference = params
        .get(k::VOXELS)
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a voxels collider needs a `voxels` asset"))?;
    let grid = balaur_core::assets::load_typed::<balaur_core::voxels::VoxelsData>(eng, reference)?;
    let cells: Vec<crate::rapier3d::math::IVector> = grid
        .cells
        .iter()
        .map(|c| scalar::cell(c[0], c[1], c[2]))
        .collect();
    let size = voxel_size(params).map_or_else(|| scalar::v3a(grid.size), Vector::splat);
    Ok(ColliderBuilder::voxels(size, &cells))
}

/// A voxel grid built from a mesh, so a model can become destructible terrain
/// without anyone authoring a cell list.
pub(crate) fn voxelized_mesh_collider(
    eng: &Engine,
    params: &toml::Value,
) -> Result<ColliderBuilder> {
    let mesh = collider_mesh(eng, params)?;
    let points: Vec<Vector> = mesh.positions.iter().map(|p| scalar::v3a(*p)).collect();
    let size = voxel_size(params).unwrap_or(DEFAULT_VOXEL_SIZE);
    Ok(ColliderBuilder::voxelized_mesh(
        &points,
        &mesh.indices,
        size,
        fill_mode(params),
    ))
}

/// Terrain from a `heightfield` asset, with the asset's holes cut out. The
/// extent belongs to the collider, so one grid can be placed at several sizes.
pub(crate) fn heightfield_collider(
    eng: &Engine,
    params: &toml::Value,
    extent: Vector,
) -> Result<ColliderBuilder> {
    use crate::rapier3d::parry::shape::{HeightField, HeightFieldCellStatus, HeightFieldFlags};
    let reference = params
        .get(k::HEIGHTFIELD)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a heightfield collider needs a `heightfield` asset"))?;
    let field = balaur_core::assets::load_typed::<balaur_core::heightfield::HeightfieldData>(
        eng, reference,
    )?;
    // The asset checked its own shape, so Array2's assert cannot fire. The
    // asset is f32; a f64 build widens each height here, once, on load.
    let heights: Vec<Real> = field.heights.iter().map(|h| scalar::real(*h)).collect();
    let grid = crate::rapier3d::parry::utils::Array2::new(field.rows, field.columns, heights);
    // The flag a triangle_mesh gets by default, for the same reason: without it a
    // character catches on the seam between two cells of flat ground.
    let mut flags = HeightFieldFlags::empty();
    if v::boolean(params, k::FIX_INTERNAL_EDGES, true) {
        flags |= HeightFieldFlags::FIX_INTERNAL_EDGES;
    }
    let mut terrain = HeightField::with_flags(grid, extent, flags);
    for &[row, column] in &field.holes {
        terrain.set_cell_status(row, column, HeightFieldCellStatus::CELL_REMOVED);
    }
    Ok(ColliderBuilder::new(SharedShape::new(terrain)))
}
