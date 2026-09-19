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

use std::sync::OnceLock;

/// The header a REST call names its run in; the socket takes the same id as
/// its `client_session` param.
pub const RUN_HEADER: &str = "x-gamend-session";

/// This process's id, the same on every call, so the server files a run's
/// client lines and the server lines it caused together.
pub fn run_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        let mut high = balaur_core::digest::Hasher::new();
        high.write_u64(clock_nanos());
        high.write_u64(salt());
        let mut low = high;
        low.write_u64(salt());
        format!("{}{}", high.finish(), low.finish())
    })
}

#[allow(
    clippy::disallowed_methods,
    reason = "seeds an id made once per process, outside any tick"
)]
fn clock_nanos() -> u64 {
    balaur_core::time::SystemTime::now()
        .duration_since(balaur_core::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

#[cfg(not(all(target_family = "wasm", not(target_os = "emscripten"))))]
fn salt() -> u64 {
    u64::from(std::process::id())
}

#[cfg(all(target_family = "wasm", not(target_os = "emscripten")))]
fn salt() -> u64 {
    js_sys::Math::random().to_bits()
}

pub use auth::{Credentials, Session};
#[cfg(not(target_family = "wasm"))]
pub use phoenix::Socket;
pub use phoenix::{Protocol, SocketEvent};
pub use rest::Client;
