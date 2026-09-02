//! The script API as JSON, from what the host was asked to declare.
//!
//! Rune's context cannot be walked from outside, so the bindings layer
//! records every function and constant as it is declared; the modules the
//! host installs itself are listed here by hand.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use balaur_core::Engine;

use crate::bindings::{api_docs, api_entries, api_module_docs};
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
    /// `args -> returns` per function that came through the typed seam.
    signatures: BTreeMap<String, String>,
    /// Components each function declared it acts on.
    acts_on: BTreeMap<String, BTreeSet<String>>,
    /// One line per function, saying what it does.
    docs: BTreeMap<String, String>,
    /// What the module as a whole is for.
    doc: String,
}

/// Every module scripts can reach, with its functions and its constants:
/// `{"modules": [{"name", "functions": [..], "constants": [{"name", "value"}],
/// "signatures": {name: "args -> returns"}, "components": [..]}]}`.
pub fn api_json(_host: &RuneHost) -> Result<String> {
    let mut modules: BTreeMap<String, Module> = BTreeMap::new();
    for entry in api_entries() {
        let module = modules.entry(entry.module).or_default();
        match entry.constant {
            Some(value) => {
                module.constants.insert(entry.name, value);
            }
            None => {
                if let Some(signature) = entry.signature {
                    module.signatures.insert(entry.name.clone(), signature);
                }
                module.functions.insert(entry.name);
            }
        }
    }
    for entry in api_docs() {
        let module = modules.entry(entry.module).or_default();
        module.docs.insert(entry.name.clone(), entry.doc);
        if !entry.acts_on.is_empty() {
            module
                .acts_on
                .entry(entry.name)
                .or_default()
                .extend(entry.acts_on);
        }
    }
    for (name, doc) in api_module_docs() {
        modules.entry(name).or_default().doc = doc;
    }
    for (module, name) in [
        ("script", "require"),
        ("script", "attempt"),
        ("script", "check"),
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
        let _ = write!(
            out,
            "    {{\n      \"name\": {},\n      \"doc\": {},\n",
            quote(name),
            quote(&module.doc)
        );
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
        out.push_str("],\n      \"signatures\": {");
        out.push_str(
            &module
                .signatures
                .iter()
                .map(|(k, v)| format!("{}: {}", quote(k), quote(v)))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str("},\n      \"docs\": {");
        out.push_str(
            &module
                .docs
                .iter()
                .map(|(k, v)| format!("{}: {}", quote(k), quote(v)))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str("},\n      \"acts_on\": {");
        out.push_str(
            &module
                .acts_on
                .iter()
                .map(|(k, v)| {
                    let list = v.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", ");
                    format!("{}: [{list}]", quote(k))
                })
                .collect::<Vec<_>>()
                .join(", "),
        );
        // The union, so a reader can ask what a module touches at a glance.
        let union: BTreeSet<&String> = module.acts_on.values().flatten().collect();
        out.push_str("},\n      \"components\": [");
        out.push_str(
            &union
                .iter()
                .map(|c| quote(c))
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
