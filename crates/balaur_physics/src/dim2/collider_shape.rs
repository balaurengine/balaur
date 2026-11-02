//! What a `collider2d`'s `shape` holds: a kind, and the dimensions or the
//! source asset that kind reads and no other row.

use balaur_core::components::derived::{Record, Variant};

use crate::properties::{Axis2d, Edges2d, Fit2d};

/// A 2D collision shape and what it is built from.
#[derive(Clone, Debug, PartialEq, Variant)]
pub enum ColliderShape2d {
    /// A disc
    Circle(CircleCollider),
    /// A rectangle
    #[option(default)]
    Rectangle(RectangleCollider),
    /// A rectangle with round ends
    Capsule(CapsuleCollider2d),
    /// One triangle
    Triangle(TriangleCollider2d),
    /// A line between two points
    Segment(SegmentCollider2d),
    /// An infinite line
    WorldBoundary(BoundaryCollider2d),
    /// The mesh's triangles, as they are
    TriangleMesh(TriangleMeshCollider2d),
    /// The convex hull of the mesh's points
    ConvexHull(RoundMeshCollider2d),
    /// Convex pieces that together cover the mesh, which a `convex_decomposition2d` beside it tunes
    ConvexDecomposition(RoundMeshCollider2d),
    /// The mesh's points in order, its outline, or every edge of its triangles, as a line
    Polyline(PolylineCollider2d),
    /// A row of heights
    Heightfield(HeightfieldCollider2d),
    /// A grid of filled cells
    Voxels(VoxelsCollider2d),
    /// The mesh filled into cells
    VoxelizedMesh(VoxelizedCollider2d),
    /// A simple shape fitted to the mesh
    Fit(FitCollider2d),
    /// The mesh's outline, taken as already convex
    ConvexPolygon(ConvexPolygonCollider),
    /// A cell for each of the mesh's points
    VoxelizedPoints(VoxelizedCollider2d),
}

/// A circle's size.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct CircleCollider {
    /// Radius
    #[property(default = 0.5, min = 0.01)]
    pub radius: f32,
}

/// A rectangle's size.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct RectangleCollider {
    /// Whole size along each axis
    #[property(default = [1.0, 1.0])]
    pub size: [f32; 2],
    /// Rounds the rectangle by this radius, so it slides over seams instead of catching on them
    #[property(min = 0.0)]
    pub edge_radius: f32,
}

/// A 2D capsule's size.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct CapsuleCollider2d {
    /// Radius of the round ends
    #[property(default = 0.5, min = 0.01)]
    pub radius: f32,
    /// Length tip to tip; 0 runs the capsule from a to b instead
    #[property(default = 2.0, min = 0.0)]
    pub height: f32,
    /// The axis the capsule lies along
    #[property(default = Axis2d::Y)]
    pub up_axis: Axis2d,
    /// The first end, when height is 0
    #[property(name = "a")]
    pub first: [f32; 2],
    /// The other end, when height is 0
    #[property(name = "b", default = [1.0, 0.0])]
    pub second: [f32; 2],
}

/// A 2D triangle's corners.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct TriangleCollider2d {
    /// First corner
    #[property(name = "a")]
    pub first: [f32; 2],
    /// Second corner
    #[property(name = "b", default = [1.0, 0.0])]
    pub second: [f32; 2],
    /// Third corner
    #[property(name = "c", default = [0.0, 1.0])]
    pub third: [f32; 2],
    /// Rounds the triangle by this radius
    #[property(min = 0.0)]
    pub edge_radius: f32,
}

/// A 2D segment's ends.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct SegmentCollider2d {
    /// One end
    #[property(name = "a")]
    pub first: [f32; 2],
    /// The other end
    #[property(name = "b", default = [1.0, 0.0])]
    pub second: [f32; 2],
}

/// Which way a 2D world boundary faces.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct BoundaryCollider2d {
    /// Which way the infinite line faces
    #[property(default = [0.0, 1.0])]
    pub normal: [f32; 2],
}

/// A 2D triangle mesh's source and how its seams behave.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct TriangleMeshCollider2d {
    /// Points and triangles: the same asset a polygon draws
    #[property(asset = "mesh")]
    pub mesh: String,
    /// Take neighbouring triangles into account for contacts, so a body does not catch on the seam between two of them
    #[property(default = true)]
    pub fix_internal_edges: bool,
    /// fix_internal_edges for a mesh hit from both sides: a contact from behind a triangle is kept and smoothed, not dropped
    pub two_sided_edges: bool,
}

/// A convex 2D shape's source mesh, rounded or not.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct RoundMeshCollider2d {
    /// Points and triangles: the same asset a polygon draws
    #[property(asset = "mesh")]
    pub mesh: String,
    /// Rounds the shape, or each piece, by this radius
    #[property(min = 0.0)]
    pub edge_radius: f32,
}

/// A 2D polyline's source mesh and which of its edges it takes.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct PolylineCollider2d {
    /// Points and triangles: the same asset a polygon draws
    #[property(asset = "mesh")]
    pub mesh: String,
    /// Which edges the line takes: the points in order, the outline of the triangles, or every edge of them
    #[property(default = Edges2d::Chain)]
    pub edges: Edges2d,
}

/// The mesh a 2D shape is fitted to, and the shape.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct FitCollider2d {
    /// Points and triangles: the same asset a polygon draws
    #[property(asset = "mesh")]
    pub mesh: String,
    /// The shape fitted to the mesh; a fitted box keeps the pose it was fitted at
    #[property(default = Fit2d::ConvexHull)]
    pub fit: Fit2d,
}

/// A convex polygon's outline mesh.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct ConvexPolygonCollider {
    /// Points in order, counter-clockwise: the same asset a polygon draws
    #[property(asset = "mesh")]
    pub mesh: String,
    /// Rounds the polygon by this radius
    #[property(min = 0.0)]
    pub edge_radius: f32,
    /// Keep the points that lie on a straight edge instead of dropping them
    pub keep_collinear: bool,
}

/// A row of heights and its scale.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct HeightfieldCollider2d {
    /// A side-scroller's ground, with the asset's holes cut out
    #[property(asset = "heightfield")]
    pub heightfield: String,
    /// Width and height scale
    #[property(default = [1.0, 1.0])]
    pub scale: [f32; 2],
}

/// A 2D voxel grid and its cell size.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct VoxelsCollider2d {
    /// Filled cells; a script may dig into them while the game runs
    #[property(asset = "voxels")]
    pub voxels: String,
    /// How big one cell is; 0 keeps the asset's own cell size
    #[property(min = 0.0)]
    pub voxel_size: f32,
}

/// A 2D mesh filled into cells, and their size.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct VoxelizedCollider2d {
    /// Points and triangles: the same asset a polygon draws
    #[property(asset = "mesh")]
    pub mesh: String,
    /// How big one cell is; 0 is 0.25
    #[property(min = 0.0)]
    pub voxel_size: f32,
}
