//! Every integration test this crate has, in one binary.
//!
//! A file under `tests/` is a crate of its own and links the engine again;
//! one binary per crate links it once, and nextest still gives each test
//! its own process.

mod app_events;
mod bs;
mod bs_host;
mod bs_types;
mod bs_units;
mod chapter;
mod components;
mod dap;
mod debug_holds;
mod dependencies;
mod digest_cover;
mod editor_theme;
mod event_names;
mod extensions;
mod facade;
mod fixed_update;
mod gamepad;
mod interactivity;
mod parked_scenes;
mod presets;
mod replay;
mod rest;
mod rotations;
mod save_addon;
mod screenshot_events;
mod script_check;
mod script_pause;
mod script_reload;
mod script_tooling;
mod store;
mod tool_modules;
mod touch_replay;
mod tree_events;
