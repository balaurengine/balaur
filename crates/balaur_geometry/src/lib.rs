//! Balaur geometry: the mesh data a model or a definition describes, and the
//! shapes built from it, primitives, paths, strokes, CSG, tiles, voxels,
//! heightfields and bones. Pure data and the assets and components that name
//! it; reading a glTF file is `balaur_asset`'s.

pub mod csg;
pub mod geometry2d;
pub mod heightfield;
pub mod mesh;
pub mod model;
pub mod path;
pub mod primitive;
pub mod skeleton;
pub mod stroke;
pub mod tiles;
pub mod triangulate;
mod vocabulary;
pub mod voxels;
