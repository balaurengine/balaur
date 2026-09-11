//! Every integration test this crate has, in one binary.
//!
//! A file under `tests/` is a crate of its own and links the engine again;
//! one binary per crate links it once, and nextest still gives each test
//! its own process.

mod support;

mod constants;
mod glyph_mesh;
mod glyphs;
mod pass;
mod splash;
mod widget_anchor;
mod widget_focus;
mod widget_kinds;
mod widget_layer;
mod widget_tree;
mod widget_window;
