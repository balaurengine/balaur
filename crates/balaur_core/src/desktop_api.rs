//! `engine.open_url` and `engine.reveal`: the two bindings that reach the OS
//! shell rather than the simulation.
//!
//! Kept out of `engine_api` because both reach the world outside the
//! simulation. `open_url` works on a desktop and in a tab, not on a phone
//! yet; `reveal` needs a desktop, being the only shell with a file manager.

use anyhow::Result;
use balaur_script::Value;

use crate::Engine;

/// Neither reaches the world on a replay: a session replayed for a bug report
/// must not open the reporter's browser.
pub(crate) fn open_url(eng: &Engine, args: &[Value]) -> Result<Value> {
    if !crate::replay::suppressed(eng) {
        crate::desktop::open_url(crate::engine_api::text(args, 0)?)?;
    }
    Ok(Value::Nil)
}

#[cfg(not(any(target_family = "wasm", target_os = "ios", target_os = "android")))]
pub(crate) fn reveal(eng: &Engine, args: &[Value]) -> Result<Value> {
    if !crate::replay::suppressed(eng) {
        crate::desktop::reveal(std::path::Path::new(crate::engine_api::text(args, 0)?))?;
    }
    Ok(Value::Nil)
}

/// Neither a tab nor a phone has a file manager to show a file in.
#[cfg(any(target_family = "wasm", target_os = "ios", target_os = "android"))]
pub(crate) fn reveal(_: &Engine, _: &[Value]) -> Result<Value> {
    Err(anyhow::anyhow!("engine.reveal needs a desktop"))
}
