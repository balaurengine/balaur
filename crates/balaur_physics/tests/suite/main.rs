//! Every integration test this crate has, in one binary.
//!
//! A file under `tests/` is a crate of its own and links the engine again;
//! one binary per crate links it once, and nextest still gives each test
//! its own process.

/// The log buffer is one per process, so every test that reads it takes this
/// lock: a mutex per module cannot exclude the other modules.
pub(crate) static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A dev run with the file watcher off: it reports the script a test wrote a
/// moment before booting, and the reload that starts logs over a test's lines.
pub(crate) fn dev_config(dir: &std::path::Path) -> balaur::AppConfig {
    let mut config = balaur::AppConfig::dev(dir.to_string_lossy().as_ref());
    config.watch = false;
    config
}

mod api;
mod bodies;
mod body_events;
mod body_rows;
mod boot;
mod characters_and_vehicles;
mod collider2d_rows;
mod collider_rows;
mod colliders;
mod collision_events;
mod constants;
mod convex_decomposition;
mod debug_draw;
mod determinism;
mod follow;
mod freeing;
mod internal_edges;
mod joint_axes;
mod joint_settings;
mod joints_and_characters;
mod paused;
mod queries;
mod ragdoll;
mod script_api;
mod settings;
mod shapes_and_geometry;
mod snapshots;
mod soft_bodies;
mod soft_body_layouts;
mod soft_body_particles;
mod soft_body_parts;
mod surfaces;
mod warnings;
// The thread count only means anything with the solver on rayon, which is
// what `parallel` brings in.
#[cfg(feature = "parallel")]
mod threads;
mod tile_collision;
mod units;
mod voxels_2d;
mod voxels_3d;
mod world_tuning;
