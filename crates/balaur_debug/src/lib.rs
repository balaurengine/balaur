//! Balaur debugging: the DAP server an editor attaches to, and the
//! `debugger` and `profile` script modules. The pause itself, a frame held
//! still, is the kernel's.

#[cfg(not(target_family = "wasm"))]
pub mod dap;
pub mod debugger_api;
pub mod profile_api;
mod vocabulary;
