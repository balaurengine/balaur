//! Every test that needs a live Gamend. A plain run leaves this module out
//! and counts it skipped; `--profile ci` runs it, one test at a time
//! (`.config/nextest.toml`).

mod client;
mod flows;
mod replay;
