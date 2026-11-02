//! What a `collider3d`'s `shape` holds: a kind, and the dimensions or the
//! source asset that kind reads and no other row.

use balaur_core::components::derived::{Record, Variant};

use crate::properties::{Axis3d, Edges, Fit3d};

/// A 3D collision shape and what it is built from.
#[derive(Clone, Debug, PartialEq, Variant)]
pub enum ColliderShape3d {
    /// A ball
    Sphere(BallCollider),
    /// A box
    #[option(default)]
    Box(BoxCollider),
    /// A cylinder with round ends
    Capsule(CapsuleCollider),
    /// A cylinder standing along y
    Cylinder(RoundCollider),
    /// A cone standing along y
    Cone(RoundCollider),
    /// One triangle
    Triangle(TriangleCollider),
    /// A line between two points
    Segment(SegmentCollider),
    /// An infinite plane
    WorldBoundary(BoundaryCollider),
    /// The mesh's triangles, as they are
    TriangleMesh(TriangleMeshCollider),
    /// The convex hull of the mesh's points
    ConvexHull(RoundMeshCollider),
    /// Convex pieces that together cover the mesh, which a `convex_decomposition3d` beside it tunes
    ConvexDecomposition(RoundMeshCollider),
    /// The mesh's points in order, or its triangles' edges, as a line
    Polyline(PolylineCollider),
    /// A terrain grid
    Heightfield(HeightfieldCollider),
    /// A grid of filled cells
    Voxels(VoxelsCollider),
    /// The mesh filled into cells
    VoxelizedMesh(VoxelizedCollider),
    /// A simple shape fitted to the mesh
    Fit(FitCollider),
    /// The mesh, taken as already convex
    ConvexMesh(RoundMeshCollider),
    /// A cell for each of the mesh's points
    VoxelizedPoints(VoxelizedCollider),
}

/// A ball's size.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct BallCollider {
    /// Radius
    #[property(default = 0.5, min = 0.01)]
    pub radius: f32,
}

/// A box's size.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct BoxCollider {
    /// Whole size along each axis
    #[property(default = [1.0, 1.0, 1.0])]
    pub size: [f32; 3],
    /// Rounds the box by this radius; a rounded shape slides over seams instead of catching on them
    #[property(min = 0.0)]
    pub edge_radius: f32,
}

/// A capsule's size.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct CapsuleCollider {
    /// Radius
    #[property(default = 0.5, min = 0.01)]
    pub radius: f32,
    /// Length tip to tip; 0 runs the capsule from a to b instead
    #[property(default = 2.0, min = 0.0)]
    pub height: f32,
    /// The axis the capsule lies along
    #[property(default = Axis3d::Y)]
    pub up_axis: Axis3d,
    /// The first end, when height is 0
    #[property(name = "a")]
    pub first: [f32; 3],
    /// The other end, when height is 0
    #[property(name = "b", default = [1.0, 0.0, 0.0])]
    pub second: [f32; 3],
}

/// A cylinder's or a cone's size.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct RoundCollider {
    /// Radius of the base
    #[property(default = 0.5, min = 0.01)]
    pub radius: f32,
    /// Length tip to tip
    #[property(default = 2.0, min = 0.0)]
    pub height: f32,
    /// Rounds the shape by this radius; a rounded shape slides over seams instead of catching on them
    #[property(min = 0.0)]
    pub edge_radius: f32,
}

/// A triangle's corners.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct TriangleCollider {
    /// First corner
    #[property(name = "a")]
    pub first: [f32; 3],
    /// Second corner
    #[property(name = "b", default = [1.0, 0.0, 0.0])]
    pub second: [f32; 3],
    /// Third corner
    #[property(name = "c", default = [0.0, 1.0, 0.0])]
    pub third: [f32; 3],
    /// Rounds the triangle by this radius
    #[property(min = 0.0)]
    pub edge_radius: f32,
}

/// A segment's ends.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct SegmentCollider {
    /// One end
    #[property(name = "a")]
    pub first: [f32; 3],
    /// The other end
    #[property(name = "b", default = [1.0, 0.0, 0.0])]
    pub second: [f32; 3],
}

/// Which way a world boundary faces.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct BoundaryCollider {
    /// Which way the infinite plane faces
    #[property(default = [0.0, 1.0, 0.0])]
    pub normal: [f32; 3],
}

/// A triangle mesh's source and how its seams behave.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct TriangleMeshCollider {
    /// The geometry
    #[property(asset = "mesh")]
    pub mesh: String,
    /// Smooth the seams between the mesh's triangles, so a character does not catch on flat ground
    #[property(default = true)]
    pub fix_internal_edges: bool,
    /// fix_internal_edges for a mesh hit from both sides: a contact from behind a triangle is kept and smoothed, not dropped
    pub two_sided_edges: bool,
}

/// A convex shape's source mesh, rounded or not.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct RoundMeshCollider {
    /// The geometry
    #[property(asset = "mesh")]
    pub mesh: String,
    /// Rounds the shape by this radius; a rounded shape slides over seams instead of catching on them
    #[property(min = 0.0)]
    pub edge_radius: f32,
}

/// A polyline's source mesh and which of its edges it takes.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct PolylineCollider {
    /// The geometry
    #[property(asset = "mesh")]
    pub mesh: String,
    /// Which edges the line takes: the points in order, or every edge of the triangles
    #[property(default = Edges::Chain)]
    pub edges: Edges,
}

/// The mesh a shape is fitted to, and the shape.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct FitCollider {
    /// The geometry
    #[property(asset = "mesh")]
    pub mesh: String,
    /// The shape fitted to the mesh; a fitted box keeps the pose it was fitted at
    #[property(default = Fit3d::ConvexHull)]
    pub fit: Fit3d,
}

/// A heightfield's grid and scale.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct HeightfieldCollider {
    /// Terrain grid; the asset's holes are cut out of it
    #[property(asset = "heightfield")]
    pub heightfield: String,
    /// Cell size and height scale
    #[property(default = [1.0, 1.0, 1.0])]
    pub scale: [f32; 3],
    /// Smooth the seams between the grid's triangles, so a character does not catch on flat ground
    #[property(default = true)]
    pub fix_internal_edges: bool,
}

/// A voxel grid and its cell size.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct VoxelsCollider {
    /// Filled cells; a script may dig into them while the game runs
    #[property(asset = "voxels")]
    pub voxels: String,
    /// How big one cell is; 0 keeps the asset's own cell size
    #[property(min = 0.0)]
    pub voxel_size: f32,
}

/// A mesh filled into cells, and their size.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct VoxelizedCollider {
    /// The geometry
    #[property(asset = "mesh")]
    pub mesh: String,
    /// How big one cell is; 0 is 0.25
    #[property(min = 0.0)]
    pub voxel_size: f32,
}
