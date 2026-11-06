//! `geometry3d`: parry's mesh toolkit, as pure functions on points and
//! triangles.
//!
//! Nothing here touches the world. They are the operations a game needs
//! *around* physics rather than in it: cut a mesh in two, work out its convex
//! pieces, turn it into voxels, and together with `collider_mesh` they are
//! the destruction toolkit: slice a crate, spawn each piece as a body with a
//! `convex_decomposition` collider, dig the voxel terrain underneath.
//!
//! A mesh crosses the seam as a `geometry3d::Mesh`: a list of positions and a
//! flat list of triangle corners. `load_mesh` reads one from a `mesh` asset,
//! so a script rarely builds one by hand.

use crate::rapier3d::parry::shape::{TriMesh, TriMeshFlags};
use crate::scalar::{self, Pose, Vector};
use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_script::{Module, Value};

use crate::vocabulary::{Opts, keys as k, map};

/// The most cells a side of a voxelisation may have.
///
/// `voxelize` walks the whole grid, so a resolution a script wrote is a cube
/// of work: 512 is a third of a second, and above it a typo hangs the game.
const MAX_VOXEL_RESOLUTION: u32 = 512;

/// A `geometry3d::Mesh`'s points and triangles.
fn mesh_of(mesh: &Value) -> Result<(Vec<Vector>, Vec<[u32; 3]>)> {
    let opts = Opts(Some(mesh));
    let points = opts
        .list(k::POINTS)
        .ok_or_else(|| anyhow!("a mesh needs a `points` list"))?
        .iter()
        .map(|p| match p {
            Value::Vec3(v) => Ok(scalar::v3a(*v)),
            other => Err(anyhow!(
                "a mesh's point is a Vec3, not a {}",
                other.type_name()
            )),
        })
        .collect::<Result<Vec<_>>>()?;
    let flat = opts
        .list(k::INDICES)
        .ok_or_else(|| anyhow!("a mesh needs an `indices` list"))?
        .iter()
        .map(|i| match i {
            Value::Int(i) => Ok(*i as u32),
            other => Err(anyhow!(
                "a mesh's index is an int, not a {}",
                other.type_name()
            )),
        })
        .collect::<Result<Vec<u32>>>()?;
    Ok((points, flat.as_chunks::<3>().0.to_vec()))
}

/// The shape every function here returns.
fn mesh_value(points: &[Vector], indices: &[[u32; 3]]) -> Value {
    map([
        (
            k::POINTS,
            Value::List(points.iter().map(|p| Value::Vec3(scalar::a3(*p))).collect()),
        ),
        (
            k::INDICES,
            Value::List(
                indices
                    .iter()
                    .flat_map(|t| t.iter().map(|i| Value::Int(i64::from(*i))))
                    .collect(),
            ),
        ),
    ])
}

/// The tables this module's mesh operations take and give.
const RECORDS: &[balaur_script::RecordDoc] = &[
    (
        "Mesh",
        "{ points: [Vec3], indices: [int] }",
        "A triangle mesh: its positions, and a flat list of three corners a triangle.",
    ),
    (
        "DecompositionOptions",
        "{ resolution: int?, max_concavity: float?, max_convex_hulls: int? }",
        "How V-HACD cuts a mesh: the voxel resolution it works at, the concavity a piece may keep, and the most pieces it makes.",
    ),
    (
        "VoxelizeOptions",
        "{ resolution: int?, fill: string? }",
        "How finely `voxelize` cuts a mesh, and `fill`: `surface` for its shell alone.",
    ),
    (
        "Voxels",
        "{ size: Vec3, cells: [[int]] }",
        "A voxel grid: one cell's size, and every filled cell as its column, row and layer.",
    ),
    (
        "Plane",
        "{ point: Vec3?, normal: Vec3? }",
        "A plane through a point, facing along a normal.",
    ),
];

pub(crate) fn install_geometry_api(m: &mut Module<Engine>) {
    m.module_doc(
        "Mesh operations: hulls, convex decomposition, voxelisation, cutting and boolean intersection, on a `Mesh` that `load_mesh` reads from an asset or a script builds.",
    );
    m.describe(&[
        ("load_mesh", &[], "(reference: string) -> Mesh", "A `mesh` asset's points and triangles."),
        ("convex_hull", &[], "(mesh: Mesh) -> Mesh", "The tightest convex shape containing every point: what a dynamic collider wants when the model is concave."),
        ("convex_decomposition", &[], "(mesh: Mesh, opts: DecompositionOptions?) -> [Mesh]", "The mesh cut into convex pieces, each one a mesh: the only way to collide a concave shape dynamically."),
        ("voxelize", &[], "(mesh: Mesh, opts: VoxelizeOptions?) -> Voxels", "The mesh as a voxel grid (`size` and `cells`, ready to be a `voxels` asset) so a model can become destructible terrain."),
    ]);
    m.records(RECORDS);
    m.function("load_mesh", |eng: &Engine, reference: String| {
        let definition =
            balaur_core::assets::load_typed::<balaur_geometry::mesh::MeshData>(eng, &reference)?;
        let mesh = balaur_geometry::mesh::load_from(eng, &definition)?;
        let points: Vec<Vector> = mesh.positions.iter().map(|p| scalar::v3a(*p)).collect();
        Ok(mesh_value(&points, &mesh.indices))
    });
    m.function("convex_hull", |_: &Engine, mesh: Value| {
        let (points, _) = mesh_of(&mesh)?;
        let (hull_points, hull_indices) =
            crate::rapier3d::parry::transformation::try_convex_hull(&points)
                .map_err(|e| anyhow!("those points have no hull: {e:?}"))?;
        Ok(mesh_value(&hull_points, &hull_indices))
    });
    m.function(
        "convex_decomposition",
        |_: &Engine, (mesh, opts): (Value, Option<Value>)| {
            let (points, indices) = mesh_of(&mesh)?;
            let opts = Opts(opts.as_ref());
            let mut params =
                crate::rapier3d::parry::transformation::vhacd::VHACDParameters::default();
            params.resolution = opts.f32(k::RESOLUTION, params.resolution as f32).max(1.0) as u32;
            params.concavity = opts.f32(k::MAX_CONCAVITY, params.concavity);
            params.max_convex_hulls = opts
                .f32(k::MAX_CONVEX_HULLS, params.max_convex_hulls as f32)
                .max(1.0) as u32;
            let vhacd = crate::rapier3d::parry::transformation::vhacd::VHACD::decompose(
                &params, &points, &indices, true,
            );
            let pieces = vhacd.compute_exact_convex_hulls(&points, &indices);
            Ok(Value::List(
                pieces
                    .iter()
                    .map(|(points, indices)| mesh_value(points, indices))
                    .collect(),
            ))
        },
    );
    m.function(
        "voxelize",
        |_: &Engine, (mesh, opts): (Value, Option<Value>)| {
            let (points, indices) = mesh_of(&mesh)?;
            let opts = Opts(opts.as_ref());
            let resolution = opts.f32(k::RESOLUTION, 32.0).max(1.0) as u32;
            if resolution > MAX_VOXEL_RESOLUTION {
                return Err(anyhow!(
                    "a voxel resolution of {resolution} would walk {}+ cells; the ceiling is {MAX_VOXEL_RESOLUTION}",
                    u64::from(MAX_VOXEL_RESOLUTION).pow(3)
                ));
            }
            let fill = if opts.text(k::FILL) == Some(crate::vocabulary::words::SURFACE) {
                crate::rapier3d::parry::transformation::voxelization::FillMode::SurfaceOnly
            } else {
                crate::rapier3d::parry::transformation::voxelization::FillMode::FloodFill {
                    detect_cavities: false,
                }
            };
            let volume = crate::rapier3d::parry::transformation::voxelization::VoxelizedVolume::voxelize(
                &points, &indices, resolution, fill, false,
            );
            let size = volume.scale();
            let [rx, ry, rz] = volume.resolution();
            // The volume is a dense grid of states; a `voxels` asset is the
            // filled cells, so this is where dense becomes sparse.
            let mut cells = Vec::new();
            for i in 0..rx {
                for j in 0..ry {
                    for k in 0..rz {
                        let filled = matches!(
                            volume.voxel(i, j, k),
                            crate::rapier3d::parry::transformation::voxelization::VoxelValue::PrimitiveInsideSurface
                                | crate::rapier3d::parry::transformation::voxelization::VoxelValue::PrimitiveOnSurface
                        );
                        if filled {
                            cells.push(Value::List(vec![
                                Value::Int(i64::from(i)),
                                Value::Int(i64::from(j)),
                                Value::Int(i64::from(k)),
                            ]));
                        }
                    }
                }
            }
            Ok(map([
                (k::SIZE, Value::Vec3([size; 3])),
                (k::CELLS, Value::List(cells)),
            ]))
        },
    );
}

/// Cutting a mesh and combining two: the operations that make new geometry
/// rather than describing what is there.
///
/// Split from [`install_geometry_api`] under `MAX_FN_LINES`.
pub(crate) fn install_mesh_edit_api(m: &mut Module<Engine>) {
    m.describe(&[
        ("split", &[], "(mesh: Mesh, plane: Plane) -> [Mesh?]", "Cut the mesh with a plane, returning the two halves, nil for a half with nothing in it."),
        ("intersect", &[], "(a: Mesh, b: Mesh) -> Mesh?", "The mesh both meshes have in common, or nothing when they do not overlap."),
        ("pieces", &[], "(mesh: Mesh) -> [Mesh]", "The mesh's separate parts, one mesh each: what a model that was already broken is made of."),
    ]);
    m.function("split", |_: &Engine, (mesh, opts): (Value, Value)| {
        let (points, indices) = mesh_of(&mesh)?;
        let opts = Opts(Some(&opts));
        let normal = scalar::v3a(opts.vec3(k::NORMAL, [0.0, 1.0, 0.0]));
        let normal = if normal.length_squared() < 1.0e-12 {
            Vector::Y
        } else {
            normal.normalize()
        };
        let point = scalar::v3a(opts.vec3(k::POINT, [0.0; 3]));
        let trimesh =
            TriMesh::new(points, indices).map_err(|e| anyhow!("that mesh cannot be cut: {e}"))?;
        // Rapier's split takes the plane as an axis and a distance along it,
        // which is the same plane a point and a normal describe.
        let result = trimesh.split(&Pose::IDENTITY, normal, normal.dot(point), 1.0e-5);
        let half = |mesh: Option<&TriMesh>| {
            mesh.map_or(Value::Nil, |mesh| {
                mesh_value(mesh.vertices(), mesh.indices())
            })
        };
        Ok(match result {
            crate::rapier3d::parry::query::SplitResult::Pair(a, b) => {
                Value::List(vec![half(Some(&a)), half(Some(&b))])
            }
            // Entirely on one side: one half is the whole mesh and the other
            // is nothing, which is what a cut that missed means.
            crate::rapier3d::parry::query::SplitResult::Negative => {
                Value::List(vec![half(Some(&trimesh)), Value::Nil])
            }
            crate::rapier3d::parry::query::SplitResult::Positive => {
                Value::List(vec![Value::Nil, half(Some(&trimesh))])
            }
        })
    });
    m.function("intersect", |_: &Engine, (a, b): (Value, Value)| {
        let (points_a, indices_a) = mesh_of(&a)?;
        let (points_b, indices_b) = mesh_of(&b)?;
        let first = TriMesh::new(points_a, indices_a)
            .map_err(|e| anyhow!("the first mesh is not one: {e}"))?;
        let second = TriMesh::new(points_b, indices_b)
            .map_err(|e| anyhow!("the second mesh is not one: {e}"))?;
        let found = crate::rapier3d::parry::transformation::intersect_meshes(
            &Pose::IDENTITY,
            &first,
            false,
            &Pose::IDENTITY,
            &second,
            false,
        )
        .map_err(|e| anyhow!("those two meshes cannot be intersected: {e:?}"))?;
        Ok(found.map_or(Value::Nil, |mesh| {
            mesh_value(mesh.vertices(), mesh.indices())
        }))
    });
    m.function("pieces", |_: &Engine, mesh: Value| {
        let (points, indices) = mesh_of(&mesh)?;
        let trimesh = TriMesh::with_flags(
            points,
            indices,
            TriMeshFlags::CONNECTED_COMPONENTS | TriMeshFlags::MERGE_DUPLICATE_VERTICES,
        )
        .map_err(|e| anyhow!("that mesh cannot be taken apart: {e}"))?;
        let Some(parts) = trimesh.connected_component_meshes(TriMeshFlags::empty()) else {
            return Ok(Value::List(vec![mesh_value(
                trimesh.vertices(),
                trimesh.indices(),
            )]));
        };
        Ok(Value::List(
            parts
                .into_iter()
                .filter_map(Result::ok)
                .map(|part| mesh_value(part.vertices(), part.indices()))
                .collect(),
        ))
    });
}
