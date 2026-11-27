//! Every integration test this crate has, in one binary.
//!
//! A file under `tests/` is a crate of its own and links the engine again;
//! one binary per crate links it once, and nextest still gives each test
//! its own process.

mod api;
mod device;
mod java;
mod purchases;
mod replay;
mod script_api;
mod secrets;
mod web_sign_in;
