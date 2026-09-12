//! Every integration test this crate has, in one binary.
//!
//! A file under `tests/` is a crate of its own and links the engine again;
//! one binary per crate links it once, and nextest still gives each test
//! its own process.

/// The log buffer is one per process, so every test that reads it takes this
/// lock: a mutex per module cannot exclude the other modules.
pub(crate) static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
mod settings;
mod shapes_and_geometry;
// The thread count only means anything with the solver on rayon, which is
// what `parallel` brings in.
#[cfg(feature = "parallel")]
mod threads;
mod tile_collision;
mod voxels_2d;
