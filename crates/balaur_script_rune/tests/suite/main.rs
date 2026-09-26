//! Every integration test this crate has, in one binary.
//!
//! A file under `tests/` is a crate of its own and links the engine again;
//! one binary per crate links it once, and nextest still gives each test
//! its own process.

mod backend;
mod binary_assets;
mod cache;
mod debugger;
mod engine_api;
mod lifecycle;
mod modules;
mod mounts;
mod packed;
mod pause;
mod pow;
mod props;
mod replay_api;
mod rollback;
mod saves;
mod values;
