//! What a tool asks the host about a script: its diagnostics, its `pub fn`s,
//! and the properties it declares tunable.
//!
//! Split out of `lib.rs`, which keeps loading, instancing and hot reload.
//! Nothing here runs during a frame — the editor's inspector, the script
//! checker and `script::functions` are the callers.

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use rune::ast::Spanned as _;
use rune::runtime::VmResult;
use rune::{Diagnostics, Source, Sources};

use crate::{value, PackSourceLoader, RuneHost};

/// A `pub fn` a script declares, read off its source text. The host owns
/// the text, and a `pub fn` starting a line, signature on that line, is the
/// whole public surface of the script model.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct PublicSignature {
    pub(crate) name: String,
    pub(crate) arity: usize,
    pub(crate) is_async: bool,
    /// 1-based, so a gutter can point at it.
    pub(crate) line: usize,
}

pub(crate) fn public_functions(source: &str) -> Vec<PublicSignature> {
    let mut out = Vec::new();
    for (at, line) in source.lines().enumerate() {
        let mut rest = line.trim_start();
        rest = match rest.strip_prefix("pub ") {
            Some(rest) => rest.trim_start(),
            None => continue,
        };
        let is_async = rest.starts_with("async ");
        if let Some(after) = rest.strip_prefix("async ") {
            rest = after.trim_start();
        }
        let Some(rest) = rest.strip_prefix("fn ") else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        let Some(open) = rest.find('(') else { continue };
        let Some(close) = rest.find(')') else {
            continue;
        };
        let arity = rest[open + 1..close]
            .split(',')
            .filter(|p| !p.trim().is_empty())
            .count();
        if !name.is_empty() {
            out.push(PublicSignature {
                name,
                arity,
                is_async,
                line: at + 1,
            });
        }
    }
    out
}

/// One compiler finding, at the file and line the author wrote.
#[derive(Clone, Debug)]
pub struct Finding {
    /// The source key the compiler read this from, which for a `mod` is the
    /// submodule's own file, not the root's.
    pub file: String,
    /// 1-based, all four, so a gutter and a caret can point at them.
    pub line: usize,
    pub column: usize,
    /// Where the span ends, for a client that underlines a range rather than
    /// a line. Equal to the start for a diagnostic that carries no span.
    pub end_line: usize,
    pub end_column: usize,
    /// `"error"` or `"warning"`.
    pub severity: &'static str,
    pub message: String,
}

/// Resolve a diagnostic's source and span into a [`Finding`]. A span-less
/// diagnostic (a link error) lands on line 0, which means "the whole file".
fn finding(
    sources: &Sources,
    id: rune::SourceId,
    span: Option<rune::ast::Span>,
    severity: &'static str,
    message: &str,
) -> Finding {
    let source = sources.get(id);
    let at = |offset: usize| {
        source.map_or((0, 0), |source| {
            let (line, column) = source.pos_to_utf8_linecol(offset);
            (line + 1, column + 1)
        })
    };
    let (start, end) = match span {
        Some(span) => (at(span.start.into_usize()), at(span.end.into_usize())),
        None => ((0, 0), (0, 0)),
    };
    Finding {
        file: source.map_or_else(String::new, |source| source.name().to_string()),
        line: start.0,
        column: start.1,
        end_line: end.0,
        end_column: end.1,
        severity,
        message: message.to_string(),
    }
}

pub(crate) fn render(diagnostics: &Diagnostics, sources: &Sources) -> String {
    let mut buf = rune::termcolor::Buffer::no_color();
    if diagnostics.emit(&mut buf, sources).is_err() {
        return "unprintable diagnostics".into();
    }
    String::from_utf8_lossy(buf.as_slice()).trim().to_string()
}

/// `script::check`'s answer: one row per diagnostic, in the order the
/// compiler reported them.
pub(crate) fn finding_rows(found: &[Finding]) -> Result<rune::Value> {
    let mut rows = Vec::with_capacity(found.len());
    for one in found {
        let mut row = rune::runtime::Object::new();
        for (key, value) in [
            ("file", rune::to_value(one.file.clone())?),
            (
                "line",
                rune::to_value(i64::try_from(one.line).unwrap_or(0))?,
            ),
            (
                "column",
                rune::to_value(i64::try_from(one.column).unwrap_or(0))?,
            ),
            ("severity", rune::to_value(one.severity)?),
            ("message", rune::to_value(one.message.clone())?),
        ] {
            row.insert(rune::alloc::String::try_from(key)?, value)?;
        }
        rows.push(rune::to_value(row)?);
    }
    Ok(rune::to_value(rows)?)
}

/// `script::exports`' answer: one row per declared property, carrying the name,
/// the default and the type to draw it at.
pub(crate) fn export_rows(declared: &[(String, balaur_script::Value)]) -> Result<rune::Value> {
    let mut rows = Vec::with_capacity(declared.len());
    for (name, default) in declared {
        let mut row = rune::runtime::Object::new();
        row.insert(
            rune::alloc::String::try_from("name")?,
            rune::to_value(name.clone())?,
        )?;
        row.insert(
            rune::alloc::String::try_from("default")?,
            value::from_neutral(default)?,
        )?;
        row.insert(
            rune::alloc::String::try_from("type")?,
            rune::to_value(export_type(default))?,
        )?;
        rows.push(rune::to_value(row)?);
    }
    Ok(rune::to_value(rows)?)
}

/// A default's type, in the same vocabulary a component schema uses, so the
/// inspector draws an export with the editor it already has for that type.
///
/// `int` is not one of `PROPERTY_TYPES` — no schema declares it — but the
/// distinction has to survive to the editor, which rounds an edit back to a
/// whole number rather than turning a count into 2.0.
fn export_type(default: &balaur_script::Value) -> &'static str {
    use balaur_script::Value;
    match default {
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Num(_) => "float",
        Value::Vec2(_) => "vec2",
        Value::Vec3(_) => "vec3",
        Value::Color(_) => "color",
        // A node reference and anything structured are typed by hand until
        // an attribute says otherwise; `PLAN-scripting.md` phase 3.
        _ => "string",
    }
}

impl RuneHost {
    /// Compile `key` from `source` and report every diagnostic instead of
    /// the first error's rendered text — the caller wants a list, not a page.
    ///
    /// The unit is dropped, so a check never disturbs the instances running
    /// the old one, and the source is the caller's (an unsaved buffer), not
    /// the file. A submodule's diagnostic is reported against the submodule:
    /// each one carries the `SourceId` the compiler read it from.
    ///
    /// # Errors
    /// If the context cannot be built.
    pub fn check_source(&self, key: &str, source: &str) -> Result<Vec<Finding>> {
        let (ctx, _) = self.context()?;
        let (path, packed) = {
            let state = self.state.borrow();
            match &state.pack {
                Some(pack) => (PathBuf::from(key), Some(pack.scripts.clone())),
                None => (state.project_root.join(key), None),
            }
        };
        let mut sources = Sources::new();
        sources.insert(Source::with_path(key, source, path)?)?;
        // The one place warnings are wanted: an error report should be the
        // error, but a check is exactly the language server's business.
        let mut diagnostics = Diagnostics::new();
        let mut loader = PackSourceLoader {
            scripts: packed.clone().unwrap_or_default(),
        };
        let mut prepared = rune::prepare(&mut sources)
            .with_context(&ctx)
            .with_diagnostics(&mut diagnostics);
        if packed.is_some() {
            prepared = prepared.with_source_loader(&mut loader);
        }
        drop(prepared.build());
        let mut findings = Vec::new();
        for diagnostic in diagnostics.diagnostics() {
            findings.push(match diagnostic {
                rune::diagnostics::Diagnostic::Fatal(fatal) => {
                    // `FatalDiagnostic::span` is private; the kind is not, and
                    // only a compile error has a span at all.
                    let span = match fatal.kind() {
                        rune::diagnostics::FatalDiagnosticKind::CompileError(error) => {
                            Some(error.span())
                        }
                        _ => None,
                    };
                    finding(
                        &sources,
                        fatal.source_id(),
                        span,
                        "error",
                        &fatal.to_string(),
                    )
                }
                rune::diagnostics::Diagnostic::Warning(warning) => finding(
                    &sources,
                    warning.source_id(),
                    Some(warning.span()),
                    "warning",
                    &warning.to_string(),
                ),
                _ => continue,
            });
        }
        Ok(findings)
    }

    /// The defaults `exports()` declares for `key`, evaluated once per file.
    ///
    /// Declaration order is not recoverable — Rune objects do not keep it —
    /// so the list is sorted by name, which is the order the inspector shows
    /// and the order a scene's `props` are written back in.
    pub fn exports(&self, key: &str) -> Result<Vec<(String, balaur_script::Value)>> {
        if let Some(hit) = self
            .state
            .borrow()
            .scripts
            .get(key)
            .and_then(|s| s.exports.clone())
        {
            return Ok(hit);
        }
        let declared = match self.method(key, "exports") {
            None => Vec::new(),
            Some(f) => match f.call::<rune::Value>(()) {
                VmResult::Ok(v) => match value::to_plain(&v) {
                    Some(balaur_script::Value::Map(fields)) => fields,
                    _ => return Err(anyhow!("[{key}] exports must return an object of defaults")),
                },
                VmResult::Err(err) => return Err(anyhow!("[{key}] exports: {err}")),
            },
        };
        if let Some(script) = self.state.borrow_mut().scripts.get_mut(key) {
            script.exports = Some(declared.clone());
        }
        Ok(declared)
    }

    /// `script::functions`: what a script declares, as `[#{ name, arity,
    /// is_async, line }]`. The host already reads every signature to build a
    /// module object; a tool asking for the same thing should not re-parse
    /// the source.
    ///
    /// # Errors
    /// If the script will not load.
    pub fn public_signatures(&self, path: &str) -> Result<rune::Value> {
        let key = Self::normalize_key(path);
        self.load(&key)?;
        let functions = self
            .state
            .borrow()
            .scripts
            .get(&key)
            .map(|s| s.functions.clone())
            .ok_or_else(|| anyhow!("{key} did not load"))?;
        let mut out = rune::runtime::Vec::new();
        for declared in functions {
            let mut entry = rune::runtime::Object::new();
            entry.insert(
                rune::alloc::String::try_from("name")?,
                rune::to_value(declared.name)?,
            )?;
            entry.insert(
                rune::alloc::String::try_from("arity")?,
                rune::to_value(i64::try_from(declared.arity).unwrap_or(i64::MAX))?,
            )?;
            entry.insert(
                rune::alloc::String::try_from("is_async")?,
                rune::to_value(declared.is_async)?,
            )?;
            entry.insert(
                rune::alloc::String::try_from("line")?,
                rune::to_value(i64::try_from(declared.line).unwrap_or(i64::MAX))?,
            )?;
            out.push(rune::to_value(entry)?)?;
        }
        Ok(rune::to_value(out)?)
    }
}
