//! `geometry3d`: parry's mesh toolkit, as pure functions on points and
//! triangles.
//!
//! Nothing here touches the world. They are the operations a game needs
//! *around* physics rather than in it — cut a mesh in two, work out its convex
//! pieces, turn it into voxels — and together with `collider_mesh` they are
//! the destruction toolkit: slice a crate, spawn each piece as a body with a
//! `convex_decomposition` collider, dig the voxel terrain underneath.
//!
//! A mesh crosses the seam as `#{ points, indices }`: a list of positions and
//! a flat list of triangle corners. A `mesh` asset's name works anywhere one
//! is taken, so a script rarely builds one by hand.

use crate::rapier3d::parry::shape::{TriMesh, TriMeshFlags};
use crate::scalar::{self, Pose, Real, Vector};
use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, Value};

use crate::vocabulary::{Opts, keys as k, map};

/// The most cells a side of a voxelisation may have.
///
/// `voxelize` walks the whole grid, so a resolution a script wrote is a cube
/// of work: 512 is a third of a second, and above it a typo hangs the game.
const MAX_VOXEL_RESOLUTION: u32 = 512;

/// Points and triangles, however the caller spelled them: a `mesh` asset's
/// name, or a table of the two lists.
fn mesh_of(eng: &Engine, value: Option<&Value>) -> Result<(Vec<Vector>, Vec<[u32; 3]>)> {
    match value {
        Some(Value::Str(reference)) => {
            let definition =
                balaur_core::assets::load_typed::<balaur_core::mesh::MeshData>(eng, reference)?;
            let mesh = balaur_core::mesh::load_from(eng, &definition)?;
            Ok((
                mesh.positions.iter().map(|p| scalar::v3a(*p)).collect(),
                mesh.indices.clone(),
            ))
        }
        Some(value @ Value::Map(_)) => {
            let opts = Opts(Some(value));
            let points = opts
                .list(k::POINTS)
                .ok_or_else(|| anyhow!("a mesh table needs a `points` list"))?
                .iter()
                .map(|p| match p {
                    Value::Vec3(v) => scalar::v3a(*v),
                    Value::List(items) => {
                        let at = |i: usize| match items.get(i) {
                            Some(Value::Num(n)) => *n as Real,
                            Some(Value::Int(n)) => *n as Real,
                            _ => 0.0,
                        };
                        Vector::new(at(0), at(1), at(2))
                    }
                    _ => Vector::ZERO,
                })
                .collect::<Vec<_>>();
            let flat: Vec<u32> = opts
                .list(k::INDICES)
                .ok_or_else(|| anyhow!("a mesh table needs an `indices` list"))?
                .iter()
                .map(|i| match i {
                    Value::Int(i) => *i as u32,
                    Value::Num(n) => *n as u32,
                    _ => 0,
                })
                .collect();
            Ok((points, flat.as_chunks::<3>().0.to_vec()))
        }
        _ => Err(anyhow!(
            "expected a mesh: an asset name, or a table of `points` and `indices`"
        )),
    }
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

pub(crate) fn install_geometry_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Mesh operations that stand outside the simulation: hulls, convex \
         decomposition, voxelisation, cutting and boolean intersection. A mesh \
         is an asset's name or a table of `points` and `indices`.",
    );
    m.describe(&[
        ("convex_hull", &[], "(mesh: any)", "The tightest convex shape containing every point: what a dynamic collider wants when the model is concave."),
        ("convex_decomposition", &[], "(mesh: any, opts: table?)", "The mesh cut into convex pieces, each one a mesh: the only way to collide a concave shape dynamically."),
        ("voxelize", &[], "(mesh: any, opts: table?)", "The mesh as a voxel grid — `#{ size, cells }`, ready to be a `voxels` asset — so a model can become destructible terrain."),
    ]);
    m.function("convex_hull", |eng: &Engine, mesh: Value| {
        let (points, _) = mesh_of(eng, Some(&mesh))?;
        let (hull_points, hull_indices) =
            crate::rapier3d::parry::transformation::try_convex_hull(&points)
                .map_err(|e| anyhow!("those points have no hull: {e:?}"))?;
        Ok(mesh_value(&hull_points, &hull_indices))
    });
    m.function(
        "convex_decomposition",
        |eng: &Engine, (mesh, opts): (Value, Option<Value>)| {
            let (points, indices) = mesh_of(eng, Some(&mesh))?;
            let opts = Opts(opts.as_ref());
            let mut params =
                crate::rapier3d::parry::transformation::vhacd::VHACDParameters::default();
            params.resolution = opts.f32(k::RESOLUTION, params.resolution as f32).max(1.0) as u32;
            params.concavity =
                scalar::real(opts.f32(k::CONCAVITY, scalar::f32_of(params.concavity)));
            params.max_convex_hulls = opts
                .f32(k::MAX_PIECES, params.max_convex_hulls as f32)
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
        |eng: &Engine, (mesh, opts): (Value, Option<Value>)| {
            let (points, indices) = mesh_of(eng, Some(&mesh))?;
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
                (k::SIZE, Value::Vec3([scalar::f32_of(size); 3])),
                (k::CELLS, Value::List(cells)),
            ]))
        },
    );
}

/// Cutting a mesh and combining two: the operations that make new geometry
/// rather than describing what is there.
///
/// Split from [`install_geometry_api`] under `MAX_FN_LINES`.
pub(crate) fn install_mesh_edit_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("split", &[], "(mesh: any, opts: table)", "Cut the mesh with a plane, returning the two halves: `#{ point = [..], normal = [..] }`."),
        ("intersect", &[], "(a: any, b: any)", "The mesh both meshes have in common, or nothing when they do not overlap."),
        ("pieces", &[], "(mesh: any)", "The mesh's separate parts, one mesh each: what a model that was already broken is made of."),
    ]);
    m.function("split", |eng: &Engine, (mesh, opts): (Value, Value)| {
        let (points, indices) = mesh_of(eng, Some(&mesh))?;
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
    m.function("intersect", |eng: &Engine, (a, b): (Value, Value)| {
        let (points_a, indices_a) = mesh_of(eng, Some(&a))?;
        let (points_b, indices_b) = mesh_of(eng, Some(&b))?;
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
    m.function("pieces", |eng: &Engine, mesh: Value| {
        let (points, indices) = mesh_of(eng, Some(&mesh))?;
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
