//! Every integration test this crate has, in one binary.
//!
//! A file under `tests/` is a crate of its own and links the engine again;
//! one binary per crate links it once, and nextest still gives each test
//! its own process.

/// The log buffer is one per process, so every test that reads it takes this
/// lock: a mutex per module cannot exclude the other modules.
pub(crate) static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Two colours or vectors equal to within rounding.
pub(crate) fn same<const N: usize>(a: [f32; N], b: [f32; N]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

mod api;
mod aseprite;
mod boolean;
mod camera;
mod light;
mod material;
mod mesh;
mod morph_weights;
mod multimesh;
mod notifier;
mod particles;
mod picking;
mod polygon;
mod reflection;
mod script_api;
mod sheet;
mod sprite;
mod surfaces;
mod text;
mod tilemap;
