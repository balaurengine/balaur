//! Every integration test this crate has, in one binary.
//!
//! A file under `tests/` is a crate of its own and links the engine again;
//! thirty-five of those cost more than the isolation they bought, and nextest
//! gives each test its own process anyway.

mod asset_ids;
mod assets;
mod change_ticks;
mod classes;
mod commands;
mod components;
mod dependencies;
mod derive;
mod digest;
mod engine;
mod engine_api;
mod fixed_update;
mod frozen;
mod interpolation;
mod kept_changes;
mod kernel_manifest;
mod lifecycle;
mod messages;
mod node_api;
mod observability;
mod observers;
mod packs;
mod patch;
mod pause;
mod plugins;
mod prefabs;
mod presets;
mod relationships;
mod replay;
mod required;
mod rng;
mod robustness;
mod rollback;
mod scene;
mod scene_ids;
mod scene_toml;
mod settings;
mod snapshot;
mod standalone;
mod strings;
mod task;
mod timings;
mod variant;
