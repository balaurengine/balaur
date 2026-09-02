//! The script API as JSON, from what the host was asked to declare.
//!
//! Rune's context cannot be walked from outside, so the bindings layer
//! records every function and constant as it is declared; the modules the
//! host installs itself are listed here by hand.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use balaur_core::Engine;

use crate::bindings::api_entries;
use crate::RuneHost;

/// The Rune host behind an engine's script host.
///
/// For code written against this backend on purpose. Code that should work
/// with any language goes through `balaur_script::ScriptHost` instead.
///
/// # Panics
/// If the engine's script host is not the Rune one.
pub fn rune_of(eng: &Engine) -> RuneHost {
    eng.script_host()
        .expect("the engine always has a script host")
        .as_any()
        .downcast_ref::<RuneHost>()
        .expect("the engine is not running the Rune backend")
        .clone()
}

#[derive(Default)]
struct Module {
    functions: BTreeSet<String>,
    constants: BTreeMap<String, String>,
}

/// Every module scripts can reach, with its functions and its constants:
/// `{"modules": [{"name", "functions": [..], "constants": [{"name", "value"}]}]}`.
pub fn api_json(_host: &RuneHost) -> Result<String> {
    let mut modules: BTreeMap<String, Module> = BTreeMap::new();
    for entry in api_entries() {
        let module = modules.entry(entry.module).or_default();
        match entry.constant {
            Some(value) => {
                module.constants.insert(entry.name, value);
            }
            None => {
                module.functions.insert(entry.name);
            }
        }
    }
    for (module, name) in [
        ("script", "require"),
        ("script", "attempt"),
        ("task", "wait"),
    ] {
        modules
            .entry(module.to_string())
            .or_default()
            .functions
            .insert(name.to_string());
    }
    let mut out = String::from("{\n  \"modules\": [\n");
    let last = modules.len();
    for (i, (name, module)) in modules.iter().enumerate() {
        use std::fmt::Write as _;
        let _ = write!(out, "    {{\n      \"name\": {},\n", quote(name));
        out.push_str("      \"functions\": [");
        out.push_str(
            &module
                .functions
                .iter()
                .map(|f| quote(f))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str("],\n      \"constants\": [");
        out.push_str(
            &module
                .constants
                .iter()
                .map(|(k, v)| format!("{{\"name\": {}, \"value\": {}}}", quote(k), quote(v)))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str("]\n    }");
        if i + 1 < last {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n}");
    Ok(out)
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
