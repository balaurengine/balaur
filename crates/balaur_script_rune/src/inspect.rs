//! What a tool asks the host about a script: its diagnostics, its `pub fn`s,
//! and the properties it declares tunable.
//!
//! Split out of `lib.rs`, which keeps loading, instancing and hot reload.
//! Nothing here runs during a frame — the editor's inspector, the script
//! checker and `script::functions` are the callers.

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use rune::ast::Spanned as _;
use rune::runtime::VmResult;
use rune::{Diagnostics, Source, Sources};

use crate::packed::PackSourceLoader;
use crate::{RuneHost, value};

/// A `pub fn` a script declares, read off its source text. A `pub fn`
/// starting a line is the whole public surface of the script model; its
/// parameter list may run on to the next line.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct PublicSignature {
    pub(crate) name: String,
    pub(crate) arity: usize,
    pub(crate) is_async: bool,
    /// 1-based, so a gutter can point at it.
    pub(crate) line: usize,
}

pub(crate) fn public_functions(source: &str) -> Vec<PublicSignature> {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    for (at, line) in lines.iter().enumerate() {
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
        if name.is_empty() {
            continue;
        }
        let Some(params) = parameters(&lines, at, &rest[open + 1..]) else {
            continue;
        };
        out.push(PublicSignature {
            name,
            arity: params.split(',').filter(|p| !p.trim().is_empty()).count(),
            is_async,
            line: at + 1,
        });
    }
    out
}

/// The text between a signature's parentheses, gathered across lines.
///
/// A signature broken over two lines used to be invisible here, which took
/// the function out of `script::require`, out of the editor's hooks list, and
/// out of the list a plugin's `register` is looked for in.
fn parameters(lines: &[&str], at: usize, first: &str) -> Option<String> {
    let mut gathered = String::from(first);
    let mut scan = at;
    while !gathered.contains(')') {
        scan += 1;
        gathered.push(' ');
        gathered.push_str(lines.get(scan)?);
        // A `{` before any `)` means this was never a signature.
        if gathered.contains('{') && !gathered.contains(')') {
            return None;
        }
    }
    let close = gathered.find(')')?;
    Some(gathered[..close].to_string())
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

/// `script::exports`' answer: one row per declared property, its name beside
/// everything the spec declares — `type`, `default`, and whatever else of
/// `min`, `max`, `step`, `options`, `asset`, `help` and `order` was written.
pub(crate) fn export_rows(declared: &[(String, balaur_script::Value)]) -> Result<rune::Value> {
    let mut rows = Vec::with_capacity(declared.len());
    for (name, spec) in declared {
        let mut row = rune::runtime::Object::new();
        row.insert(
            rune::alloc::String::try_from("name")?,
            rune::to_value(name.clone())?,
        )?;
        if let balaur_script::Value::Map(fields) = spec {
            for (key, value) in fields {
                row.insert(
                    rune::alloc::String::try_from(key.as_str())?,
                    value::from_neutral(value)?,
                )?;
            }
        }
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
        // the spec form below says otherwise.
        _ => "string",
    }
}

/// The `default` an export declares, which is what the host writes onto an
/// instance before `init`.
#[must_use]
pub(crate) fn export_default(spec: &balaur_script::Value) -> balaur_script::Value {
    let balaur_script::Value::Map(fields) = spec else {
        return spec.clone();
    };
    fields
        .iter()
        .find(|(k, _)| k == "default")
        .map_or(balaur_script::Value::Nil, |(_, v)| v.clone())
}

/// One exported property as a spec, whichever way it was written.
///
/// **A table carrying `type` is a spec; anything else is a bare default**,
/// lifted into one so every reader sees the same shape. That is the whole
/// rule, and it is why a plain `speed: 2.0` keeps working.
fn spec_of(key: &str, name: &str, value: &balaur_script::Value) -> Result<balaur_script::Value> {
    use balaur_script::Value;
    let declared = match value {
        Value::Map(fields) if fields.iter().any(|(k, _)| k == "type") => fields.clone(),
        bare => vec![
            ("type".to_string(), Value::Str(export_type(bare).into())),
            ("default".to_string(), bare.clone()),
        ],
    };
    let spec = Value::Map(declared);
    if let Err(why) = balaur_core::node_api::validate_property_spec(&spec) {
        return Err(anyhow!("[{key}] exports: property '{name}': {why}"));
    }
    Ok(spec)
}

/// Where a spec asks to sit on the page; everything unordered sorts after,
/// keeping the name order `to_plain` produced.
fn order_of(spec: &balaur_script::Value) -> f64 {
    use balaur_script::Value;
    let Value::Map(fields) = spec else {
        return f64::MAX;
    };
    match fields.iter().find(|(k, _)| k == "order").map(|(_, v)| v) {
        Some(Value::Num(n)) => *n,
        Some(Value::Int(i)) => *i as f64,
        _ => f64::MAX,
    }
}

impl RuneHost {
    /// Log a runtime error at the line that threw, with the script backtrace
    /// under it.
    ///
    /// `VmError` on its own prints the message and nothing else. Rendering it
    /// against the unit's sources is what turns "field not found" into a file,
    /// a line and the frames that led there.
    pub(crate) fn report(&self, key: &str, label: &str, err: &rune::runtime::VmError) {
        let sources = self
            .state
            .borrow()
            .scripts
            .get(key)
            .and_then(|s| s.sources.clone());
        // A packed script has no sources; there is nothing to render against.
        let Some(sources) = sources else {
            tracing::error!("[{key}] {label}: {err}");
            return;
        };
        let mut buf = rune::termcolor::Buffer::no_color();
        if err.emit(&mut buf, &sources).is_err() {
            tracing::error!("[{key}] {label}: {err}");
            return;
        }
        let rendered = String::from_utf8_lossy(buf.as_slice());
        tracing::error!("[{key}] {label}:\n{}", rendered.trim_end());
    }

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
    /// so the list is sorted by the spec's `order` and then by name, which is
    /// the order the inspector shows and a scene's `props` are written in.
    pub fn exports(&self, key: &str) -> Result<Vec<(String, balaur_script::Value)>> {
        if let Some(hit) = self
            .state
            .borrow()
            .scripts
            .get(key)
            .and_then(|s| s.exports.clone())
        {
            return hit.map_err(|why| anyhow!(why));
        }
        let outcome = self.read_exports(key);
        // The failure is cached with the success: a broken `exports` that
        // re-ran per attach would report itself once per node.
        let cached = match &outcome {
            Ok(declared) => Ok(declared.clone()),
            Err(err) => Err(format!("{err:#}")),
        };
        if let Some(script) = self.state.borrow_mut().scripts.get_mut(key) {
            script.exports = Some(cached);
        }
        outcome
    }

    /// Evaluate `exports()` and normalise every entry into a spec.
    fn read_exports(&self, key: &str) -> Result<Vec<(String, balaur_script::Value)>> {
        let written = match self.method(key, "exports") {
            None => Vec::new(),
            Some(f) => match f.call::<rune::Value>(()) {
                VmResult::Ok(v) => match value::to_plain(&v) {
                    Some(balaur_script::Value::Map(fields)) => fields,
                    _ => return Err(anyhow!("[{key}] exports must return an object of defaults")),
                },
                VmResult::Err(err) => return Err(anyhow!("[{key}] exports: {err}")),
            },
        };
        let mut declared = Vec::with_capacity(written.len());
        for (name, value) in written {
            let spec = spec_of(key, &name, &value)?;
            declared.push((name, spec));
        }
        // `to_plain` sorted by name, which is the tie-break; `order` is what a
        // script says when the rows belong in an order of its own.
        declared.sort_by(|a, b| order_of(&a.1).total_cmp(&order_of(&b.1)));
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
