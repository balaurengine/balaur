//! Every integration test this crate has, in one binary.
//!
//! A file under `tests/` is a crate of its own and links the engine again;
//! thirty-five of those cost more than the isolation they bought, and nextest
//! gives each test its own process anyway.

mod asset_ids;
mod assets;
mod cloner;
mod components;
mod csg;
mod digest;
mod engine;
mod engine_api;
mod faults;
mod fixed_update;
mod frozen;
mod glb;
mod morphs;
mod node_api;
mod observability;
mod pack;
mod pack_report;
mod patch;
mod path;
mod plugins;
mod polygon_mesh;
mod prefabs;
mod presets;
mod primitive;
mod replay;
mod rng;
mod rollback;
mod scene;
mod scene_ids;
mod settings;
mod skeleton;
mod snapshot;
mod standalone;
mod strings;
mod timings;
