//! Every integration test this crate has, in one binary.
//!
//! A file under `tests/` is a crate of its own and links the engine again;
//! one binary per crate links it once, and nextest still gives each test
//! its own process.

mod components;
mod dap;
mod extensions;
mod facade;
mod fixed_update;
mod interactivity;
mod presets;
mod replay;
mod script_tooling;
