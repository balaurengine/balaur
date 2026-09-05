//! The `script` module: how one script reaches another.
//!
//! Split out of `lib.rs`, whose `RuneHost::context` folds this module in
//! alongside `balaur` and `task` the first time a unit is compiled.

use anyhow::Result;
use rune::runtime::Function;

use crate::inspect::{export_rows, finding_rows};
use crate::{HOSTS, RuneHost, SHARED_FNS, trampoline};

/// Everything a script may ask about — or borrow from — another script.
///
/// The host is registered in the thread's `HOSTS` table and reached by slot,
/// because a rune closure has to be `'static` and the host is not.
pub(crate) fn script_module(host: &RuneHost) -> Result<rune::Module> {
    // `script::require("scripts/lib.rn")` — an object of another
    // script's public functions, cached and hot-reloaded in place:
    // `let lib = script::require("scripts/lib.rn"); lib["helper"](x)`.
    let slot = HOSTS.with(|hosts| {
        let mut hosts = hosts.borrow_mut();
        hosts.push(host.clone());
        hosts.len() - 1
    });
    let mut script = rune::Module::with_crate("script")?;
    script
        .function("require", move |path: &str| {
            let host = HOSTS.with(|hosts| hosts.borrow()[slot].clone());
            match host.require_module(path) {
                Ok(value) => value,
                Err(err) => {
                    tracing::error!("script::require({path}): {err}");
                    rune::to_value(()).expect("unit always converts")
                }
            }
        })
        .build()?;
    // `script::functions("scripts/lib.rn")` — what that script declares,
    // so a tool reads the host's own signatures instead of parsing.
    script
        .function("functions", move |path: &str| {
            let host = HOSTS.with(|hosts| hosts.borrow()[slot].clone());
            match host.public_signatures(path) {
                Ok(value) => value,
                Err(err) => {
                    tracing::error!("script::functions({path}): {err}");
                    rune::to_value(()).expect("unit always converts")
                }
            }
        })
        .build()?;
    // `script::check(path, source)` — every diagnostic about that source, as
    // `[#{ file, line, column, severity, message }]`. The source is the
    // caller's, so an editor checks the buffer it is showing, not the file.
    script
        .function("check", move |path: &str, source: &str| {
            let host = HOSTS.with(|hosts| hosts.borrow()[slot].clone());
            match host
                .check_source(&RuneHost::normalize_key(path), source)
                .and_then(|found| finding_rows(&found))
            {
                Ok(value) => value,
                Err(err) => {
                    tracing::error!("script::check({path}): {err}");
                    rune::to_value(()).expect("unit always converts")
                }
            }
        })
        .build()?;
    // `script::exports(path)` — what that script declares tunable, as
    // `[#{ name, default, type }]` in name order, the type named in the schema
    // vocabulary so the inspector reaches for an editor it already has.
    script
        .function("exports", move |path: &str| {
            let host = HOSTS.with(|hosts| hosts.borrow()[slot].clone());
            match host
                .exports(&RuneHost::normalize_key(path))
                .and_then(|declared| export_rows(&declared))
            {
                Ok(value) => value,
                Err(err) => {
                    tracing::error!("script::exports({path}): {err}");
                    rune::to_value(()).expect("unit always converts")
                }
            }
        })
        .build()?;
    // `script::shared(f, arity)` — a callback made in this unit, callable
    // from another unit's VM. Arity is explicit: a wrapper is typed.
    script
        .function("shared", |f: Function, arity: i64| -> rune::Value {
            let arity = usize::try_from(arity).unwrap_or(usize::MAX);
            let wrapped = SHARED_FNS.with(|shared| {
                let mut shared = shared.borrow_mut();
                shared.push(f);
                trampoline(shared.len() - 1, arity)
            });
            if let Some(function) = wrapped {
                return rune::to_value(function).expect("a function always converts");
            }
            tracing::error!("script::shared: arity {arity} is past the five rune allows");
            rune::to_value(()).expect("unit always converts")
        })
        .build()?;
    // `let (ok, value) = script::attempt(|| risky())`: the closure's
    // error becomes a value instead of ending the caller, which is what
    // a tool wants from a call that may legitimately fail.
    script
        .function("attempt", |f: Function| -> rune::Value {
            let outcome = match f.call::<rune::Value>(()).into_result() {
                Ok(value) => rune::to_value((true, value)),
                Err(err) => rune::to_value((false, err.to_string())),
            };
            outcome.expect("a tuple always converts")
        })
        .build()?;
    Ok(script)
}
