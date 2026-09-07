//! Every integration test this crate has, in one binary.
//!
//! A file under `tests/` is a crate of its own and links the engine again;
//! one binary per crate links it once, and nextest still gives each test
//! its own process.

/// The log buffer is one per process, so every test that reads it takes this
/// lock: a mutex per module cannot exclude the other modules.
pub(crate) static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

mod api;
mod aseprite;
mod boolean;
mod camera;
mod cloner;
mod light;
mod material;
mod morph_weights;
mod particles;
mod picking;
mod polygon;
mod script_api;
mod sheet;
mod sprite;
mod tilemap;
