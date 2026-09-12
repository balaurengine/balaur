//! Every integration test this crate has, in one binary.
//!
//! A file under `tests/` is a crate of its own and links the engine again;
//! one binary per crate links it once, and nextest still gives each test
//! its own process.

mod common;

mod api;
mod authoring;
mod clip;
mod ease;
mod machine;
mod modifier;
mod retarget;
mod rollback;
mod script;
mod skeleton;
mod tracks;
mod tween;
