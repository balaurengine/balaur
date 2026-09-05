//! The Gamend wire client: auth, REST and realtime, engine-free.
//!
//! Blocking on native, where worker threads own the pacing; in a browser the
//! same protocol code is driven from callbacks. Nothing in this module knows
//! about nodes, values or the frame loop; it is the layer that could ship as
//! a standalone Rust SDK, kept as a sealed submodule so the protocol code
//! stays separable from the engine glue around it.
//!
//! Three layers, each usable alone:
//! - [`rest`]: an authenticated JSON caller for `/api/v1/*`.
//! - [`auth`]: login, token refresh, and the [`auth::Session`] both layers
//!   borrow.
//! - [`phoenix`]: the realtime connection — Phoenix Channels protocol V2 —
//!   with joins, pushes, replies and heartbeats.

pub mod auth;
pub mod phoenix;
pub mod rest;

pub use auth::{Credentials, Session};
#[cfg(not(target_family = "wasm"))]
pub use phoenix::Socket;
pub use phoenix::{Protocol, SocketEvent};
pub use rest::Client;
