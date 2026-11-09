//! Balaur projects shipped: a pack built from a project (every script
//! compiled, the scenes, assets and dependencies gathered), the report and
//! strip an export weighs it with, and the shell that opens a URL or reveals
//! a folder. The pack's data and wire format are the kernel's: a game runs
//! from one with nothing here.

// Opening a URL is a shell's job and a tab is one; revealing a folder is a
// desktop's alone, so only that half is gated, inside.
pub mod desktop;
pub mod desktop_api;
pub mod packing;
mod vocabulary;
