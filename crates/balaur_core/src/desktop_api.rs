//! `engine.open_url` and `engine.reveal`: the two bindings that reach the OS
//! shell rather than the simulation.
//!
//! Kept out of `engine_api` because the work behind them is a desktop's, and
//! a browser build has neither an opener nor a file manager to ask.

use anyhow::Result;
use balaur_script::Value;

use crate::Engine;

/// Neither reaches the world on a replay: a session replayed for a bug report
/// must not open the reporter's browser.
#[cfg(not(target_family = "wasm"))]
pub(crate) fn open_url(eng: &Engine, args: &[Value]) -> Result<Value> {
    if !crate::replay::suppressed(eng) {
        crate::desktop::open_url(crate::engine_api::text(args, 0)?)?;
    }
    Ok(Value::Nil)
}

#[cfg(not(target_family = "wasm"))]
pub(crate) fn reveal(eng: &Engine, args: &[Value]) -> Result<Value> {
    if !crate::replay::suppressed(eng) {
        crate::desktop::reveal(std::path::Path::new(crate::engine_api::text(args, 0)?))?;
    }
    Ok(Value::Nil)
}

/// A tab opens a URL with `web.open`, and has no file manager at all.
#[cfg(target_family = "wasm")]
pub(crate) fn open_url(_: &Engine, _: &[Value]) -> Result<Value> {
    Err(anyhow::anyhow!(
        "engine.open_url needs a desktop; a tab uses web.open"
    ))
}

#[cfg(target_family = "wasm")]
pub(crate) fn reveal(_: &Engine, _: &[Value]) -> Result<Value> {
    Err(anyhow::anyhow!("engine.reveal needs a desktop"))
}
