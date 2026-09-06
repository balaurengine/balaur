//! The `script` module: how one script reaches another.
//!
//! Split out of `lib.rs`, whose `RuneHost::context` folds this module in
//! alongside `balaur` and `task` the first time a unit is compiled.

use anyhow::Result;
use rune::runtime::Function;

use crate::inspect::{export_rows, finding_rows};
use crate::shared::{SHARED_FNS, trampoline};
use crate::tooling::{completion_rows, hover_row, location_row, location_rows, symbol_rows};
use crate::{HOSTS, RuneHost};

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
    inspection_verbs(&mut script, slot)?;
    tooling_verbs(&mut script, slot)?;
    Ok(script)
}

/// What a tool asks about a file: what it declares, what is wrong with it,
/// and what it exports. Split from `script_module` so each stays about one
/// thing.
fn inspection_verbs(script: &mut rune::Module, slot: usize) -> Result<()> {
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
    // `script::complete(path, source, line, column)` — what may be typed at
    // that caret, as `[#{ label, kind, detail, doc, insert }]`. The same
    // answer `balaur lsp` gives an editor outside Balaur.
    script
        .function(
            "complete",
            move |path: &str, source: &str, line: i64, column: i64| {
                let host = HOSTS.with(|hosts| hosts.borrow()[slot].clone());
                let (line, column) = at(line, column);
                match host
                    .complete(&RuneHost::normalize_key(path), source, line, column)
                    .and_then(|found| completion_rows(&found))
                {
                    Ok(value) => value,
                    Err(err) => {
                        tracing::error!("script::complete({path}): {err}");
                        rune::to_value(()).expect("unit always converts")
                    }
                }
            },
        )
        .build()?;
    // `script::hover(path, source, line, column)` — what is under that caret,
    // as `#{ title, detail, doc }`, or `()`.
    script
        .function(
            "hover",
            move |path: &str, source: &str, line: i64, column: i64| {
                let host = HOSTS.with(|hosts| hosts.borrow()[slot].clone());
                let (line, column) = at(line, column);
                match host
                    .hover(&RuneHost::normalize_key(path), source, line, column)
                    .and_then(|found| hover_row(found.as_ref()))
                {
                    Ok(value) => value,
                    Err(err) => {
                        tracing::error!("script::hover({path}): {err}");
                        rune::to_value(()).expect("unit always converts")
                    }
                }
            },
        )
        .build()?;
    // `script::signature(path, source, line, column)` — the call the caret is
    // inside, as `#{ title, detail, doc, active }`, or `()`.
    script
        .function(
            "signature",
            move |path: &str, source: &str, line: i64, column: i64| {
                let host = HOSTS.with(|hosts| hosts.borrow()[slot].clone());
                let (line, column) = at(line, column);
                match host
                    .signature_help(&RuneHost::normalize_key(path), source, line, column)
                    .and_then(signature_row)
                {
                    Ok(value) => value,
                    Err(err) => {
                        tracing::error!("script::signature({path}): {err}");
                        rune::to_value(()).expect("unit always converts")
                    }
                }
            },
        )
        .build()?;
    Ok(())
}

/// The verbs an editor asks about a caret: what completes, what is under
/// it, where it is defined, what a file declares, and the two that write.
/// Split from `script_module` so each stays about one thing.
fn tooling_verbs(script: &mut rune::Module, slot: usize) -> Result<()> {
    // `script::api()` — every module scripts can reach, as the JSON string
    // `balaur api` prints. The Docs dock reads the live engine through this,
    // so a plugin's own module is in the reference the editor shows.
    script
        .function("api", move || {
            let host = HOSTS.with(|hosts| hosts.borrow()[slot].clone());
            match crate::api::api_json(&host) {
                Ok(text) => rune::to_value(text).expect("a string always converts"),
                Err(err) => {
                    tracing::error!("script::api: {err}");
                    rune::to_value(String::new()).expect("a string always converts")
                }
            }
        })
        .build()?;
    // `script::definition(path, source, line, column)` — where the name at
    // that caret is defined, as `#{ file, line, column, url }`. Engine API has
    // no file, so it carries its reference page instead.
    script
        .function(
            "definition",
            move |path: &str, source: &str, line: i64, column: i64| {
                let host = HOSTS.with(|hosts| hosts.borrow()[slot].clone());
                let (line, column) = at(line, column);
                match host
                    .definition(&RuneHost::normalize_key(path), source, line, column)
                    .and_then(|found| location_row(found.as_ref()))
                {
                    Ok(value) => value,
                    Err(err) => {
                        tracing::error!("script::definition({path}): {err}");
                        rune::to_value(()).expect("unit always converts")
                    }
                }
            },
        )
        .build()?;
    // `script::symbols(path, source)` — what that file declares, as
    // `[#{ name, kind, detail, line, column }]`, for an outline.
    script
        .function("symbols", move |path: &str, source: &str| {
            let host = HOSTS.with(|hosts| hosts.borrow()[slot].clone());
            match host
                .symbols(&RuneHost::normalize_key(path), source)
                .and_then(|found| symbol_rows(&found))
            {
                Ok(value) => value,
                Err(err) => {
                    tracing::error!("script::symbols({path}): {err}");
                    rune::to_value(()).expect("unit always converts")
                }
            }
        })
        .build()?;
    // `script::references(path, source, name)` — every place that name
    // appears as a whole word across the `mod` graph. Textual, so a caller
    // shows the list before writing anything.
    script
        .function("references", move |path: &str, source: &str, name: &str| {
            let host = HOSTS.with(|hosts| hosts.borrow()[slot].clone());
            match host
                .references(&RuneHost::normalize_key(path), source, name)
                .and_then(|found| location_rows(&found))
            {
                Ok(value) => value,
                Err(err) => {
                    tracing::error!("script::references({path}): {err}");
                    rune::to_value(()).expect("unit always converts")
                }
            }
        })
        .build()?;
    // `script::rename(path, source, from, to)` — every file a rename would
    // rewrite, as `[#{ file, source }]`. Nothing is written: the caller shows
    // the list, then writes what it chooses.
    script
        .function(
            "rename",
            move |path: &str, source: &str, from: &str, to: &str| {
                let host = HOSTS.with(|hosts| hosts.borrow()[slot].clone());
                match host
                    .rename(&RuneHost::normalize_key(path), source, from, to)
                    .and_then(|written| rename_rows(&written))
                {
                    Ok(value) => value,
                    Err(err) => {
                        tracing::error!("script::rename({path}): {err}");
                        rune::to_value(()).expect("unit always converts")
                    }
                }
            },
        )
        .build()?;
    // `script::format(path, source)` — that source laid out by Rune's own
    // formatter, or the source unchanged when it will not parse.
    script
        .function("format", move |path: &str, source: &str| {
            let host = HOSTS.with(|hosts| hosts.borrow()[slot].clone());
            match host.format(&RuneHost::normalize_key(path), source) {
                Ok(text) => rune::to_value(text).expect("a string always converts"),
                Err(err) => {
                    tracing::error!("script::format({path}): {err}");
                    rune::to_value(source.to_string()).expect("a string always converts")
                }
            }
        })
        .build()?;
    Ok(())
}

/// A script counts lines and columns from one, and a negative one is a caller
/// that has not placed its caret yet.
fn at(line: i64, column: i64) -> (usize, usize) {
    (
        usize::try_from(line).unwrap_or(1).max(1),
        usize::try_from(column).unwrap_or(1).max(1),
    )
}

/// A signature-help answer with the argument the caret is in.
fn signature_row(found: Option<(crate::Hover, usize)>) -> Result<rune::Value> {
    let Some((one, active)) = found else {
        return Ok(rune::to_value(())?);
    };
    let mut object = rune::runtime::Object::new();
    for (key, value) in [
        ("title", rune::to_value(one.title)?),
        ("detail", rune::to_value(one.detail)?),
        ("doc", rune::to_value(one.doc)?),
        (
            "active",
            rune::to_value(i64::try_from(active).unwrap_or(0))?,
        ),
    ] {
        object.insert(rune::alloc::String::try_from(key)?, value)?;
    }
    Ok(rune::to_value(object)?)
}

/// One object per file a rename would rewrite.
fn rename_rows(written: &[(String, String)]) -> Result<rune::Value> {
    let mut rows = Vec::with_capacity(written.len());
    for (file, source) in written {
        let mut object = rune::runtime::Object::new();
        for (key, value) in [
            ("file", rune::to_value(file.clone())?),
            ("source", rune::to_value(source.clone())?),
        ] {
            object.insert(rune::alloc::String::try_from(key)?, value)?;
        }
        rows.push(rune::to_value(object)?);
    }
    Ok(rune::to_value(rows)?)
}
