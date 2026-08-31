//! The Gamend wire client: auth, REST and realtime, engine-free.
//!
//! Blocking on purpose — the worker threads own the pacing. Nothing in this
//! module knows about nodes, values or the frame loop; it is the layer that
//! could ship as a standalone Rust SDK, kept as a sealed submodule so the
//! protocol code stays separable from the engine glue around it.
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
pub use phoenix::{Socket, SocketEvent};
pub use rest::Client;
