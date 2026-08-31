//! A Gamend backend client for Rust games: auth, REST and realtime.
//!
//! Blocking on purpose — the caller owns the thread and the pacing, which is
//! exactly what a game engine pumping a frame loop (or a worker thread beside
//! one) wants. No async runtime anywhere.
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
