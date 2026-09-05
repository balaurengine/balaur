//! The wall clock the engine reads for frame timing, the profiler and the
//! log's timestamps.
//!
//! `std::time::Instant` has no implementation on `wasm32-unknown-unknown` and
//! panics on first use — the first line of `main`, in a browser build. `web_time`
//! is the drop-in that reads `performance.now()` there and *is* `std::time`
//! everywhere else, so every crate reads its clock through here.
#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{Instant, SystemTime, UNIX_EPOCH};
#[cfg(target_arch = "wasm32")]
pub use web_time::{Instant, SystemTime, UNIX_EPOCH};
