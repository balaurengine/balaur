//! Every integration test this crate has, in one binary.
//!
//! A file under `tests/` is a crate of its own and links the engine again;
//! one binary per crate links it once, and nextest still gives each test
//! its own process.

mod api;
mod bodies;
mod colliders;
mod constants;
mod determinism;
mod freeing;
mod internal_edges;
mod joints_and_characters;
mod queries;
mod ragdoll;
mod script_api;
mod shapes_and_geometry;
mod threads;
mod tile_collision;
mod voxels_2d;
