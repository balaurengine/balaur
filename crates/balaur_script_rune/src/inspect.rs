//! What a tool asks the host about a script: its diagnostics, its `pub fn`s,
//! and the properties it declares tunable.
//!
//! Split out of `lib.rs`, which keeps loading, instancing and hot reload.
//! Nothing here runs during a frame — the editor's inspector, the script
//! checker and `script::functions` are the callers.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use rune::ast::Spanned as _;
use rune::runtime::VmResult;
use rune::{Diagnostics, Source, Sources};

use hecs::Entity;
use rustc_hash::FxHashMap;

use crate::handles;
use crate::packed::PackSourceLoader;
use crate::value::Node;
use crate::{RuneHost, value};

/// The function `script::require` reads a module's constants through.
pub(crate) const CONSTANTS_FN: &str = "__balaur_constants";

/// The source with one more function, returning every top-level `pub const`
/// by name: Rune keeps constants inside the unit, where a caller holding the
/// module cannot reach them. A source with none comes back as it was.
///
/// The names come off a parse rather than the text, so `pub const` inside a
/// string is not one, and a name a `pub fn` already owns stays the function.
pub(crate) fn with_constants(source: &str) -> std::borrow::Cow<'_, str> {
    let taken: Vec<String> = public_functions(source)
        .into_iter()
        .map(|f| f.name)
        .collect();
    let names: Vec<&str> = public_constants(source)
        .into_iter()
        .filter(|name| !taken.iter().any(|f| f == name))
        .collect();
    if names.is_empty() {
        return source.into();
    }
    let fields: Vec<String> = names.iter().map(|n| format!("{n}: {n}")).collect();
    format!(
        "{source}\npub fn {CONSTANTS_FN}() {{ #{{ {} }} }}\n",
        fields.join(", ")
    )
    .into()
}

/// Every top-level `pub const` in a source, by name. A file Rune cannot parse
/// has none: the compile below reports that, and better than this could.
fn public_constants(source: &str) -> Vec<&str> {
    let Ok(file) = rune::parse::parse_all::<rune::ast::File>(source, rune::SourceId::EMPTY, false)
    else {
        return Vec::new();
    };
    file.items
        .iter()
        .filter_map(|(item, _)| match item {
            rune::ast::Item::Const(declared)
                if matches!(declared.visibility, rune::ast::Visibility::Public(_)) =>
            {
                source.get(declared.name.span().range())
            }
            _ => None,
        })
        .collect()
}

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
pub(crate) fn parameters(lines: &[&str], at: usize, first: &str) -> Option<String> {
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

/// The text the compiler read a source from: the caller's own buffer for the
/// root, and the file for a `mod` submodule, which the loader read from disk.
///
/// `Source` keeps its text to itself, so a pass that has to look at the code
/// under a diagnostic reads it back. `None` where there is no file to read —
/// a packed run — and the caller then keeps the diagnostic rather than
/// judging it blind.
fn read_source(
    sources: &Sources,
    id: rune::SourceId,
    root: rune::SourceId,
    buffer: &str,
) -> Option<String> {
    if id == root {
        return Some(buffer.to_string());
    }
    let read = balaur_core::files::default_backend()
        .read(sources.get(id)?.path()?)
        .ok()?;
    String::from_utf8(read).ok()
}

/// Whether a "Pattern might panic" is a tuple of names being unpacked.
///
/// Rune warns for every refutable pattern in a `let` or a `for`, and a tuple
/// is refutable: nothing proves a value's arity before it arrives. That is
/// every multiple return the language has — `let (x, y) = input::mouse_position()`,
/// `for (i, node) in nodes.iter().enumerate()` — so reporting it says nothing
/// and drowns the warnings that do. A pattern that tests a value rather than
/// spreading it (`Some(x)`, a list, an object) is still reported.
///
/// The warning's span is the pattern itself. Without the text — a packed run
/// has no file to read — the diagnostic is kept rather than judged blind.
fn unpacking_a_tuple(message: &str, text: Option<&str>, span: rune::ast::Span) -> bool {
    if message != "Pattern might panic" {
        return false;
    }
    let Some(pattern) = text.and_then(|text| text.get(span.range())) else {
        return false;
    };
    let Some(names) = pattern
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
    else {
        return false;
    };
    !names.is_empty()
        && names
            .split(',')
            .all(|name| handles::is_identifier(name.trim()))
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

/// Where a runtime error was thrown: the unit and instruction, or the message
/// when the error carries no location.
type ThrowSite = (usize, usize, String);

thread_local! {
    /// Each site's throws so far. The unit is held so its address cannot be
    /// reused by a reload's new unit, whose errors must render again.
    static THROWN: RefCell<FxHashMap<ThrowSite, (Option<Arc<rune::Unit>>, u64)>> =
        RefCell::new(FxHashMap::default());
}

/// Count this throw at its site and answer how many there have been.
///
/// A script throwing in `update` on every node throws thousands of times a
/// frame, and rendering each against its sources cost more than the frame.
fn tally(key: &str, label: &str, err: &rune::runtime::VmError) -> u64 {
    let (site, unit) = match err.first_location() {
        Some(at) => (
            (Arc::as_ptr(&at.unit) as usize, at.ip, label.to_string()),
            Some(at.unit.clone()),
        ),
        None => ((0, 0, format!("{key}\n{label}\n{err}")), None),
    };
    THROWN.with_borrow_mut(|thrown| {
        let entry = thrown.entry(site).or_insert((unit, 0));
        entry.1 += 1;
        entry.1
    })
}

fn is_power_of_ten(mut n: u64) -> bool {
    while n >= 10 && n % 10 == 0 {
        n /= 10;
    }
    n == 1
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
            rune::to_value(name.as_str())?,
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

/// A bare default as a whole spec, in the vocabulary a component schema uses,
/// so the inspector draws an export with the editor it already has.
///
/// Read from the Rune value rather than its plain form: a `struct` a script
/// declared is a `record` naming its class, and nothing plain says which.
/// `int` is not what a float default would be read back as, and the
/// distinction has to survive to the editor, which rounds an edit back to a
/// whole number rather than turning a count into 2.0.
fn inferred_spec(default: &rune::Value) -> Result<Vec<(String, balaur_script::Value)>> {
    use balaur_script::Value;
    if let Some((class, fields)) = default.struct_parts() {
        return Ok(vec![
            ("type".to_string(), Value::Str("record".into())),
            ("class".to_string(), Value::Str(class)),
            ("fields".to_string(), record_fields(&fields)?),
            ("default".to_string(), plain_of(default)?),
        ]);
    }
    let mut spec = vec![
        (
            "type".to_string(),
            Value::Str(inferred_type(default).into()),
        ),
        ("default".to_string(), plain_of(default)?),
    ];
    if let Ok(items) = default.borrow_ref::<rune::runtime::Vec>() {
        spec.push(("of".to_string(), Value::Map(list_of(&items)?)));
    } else if let Ok(object) = default.borrow_ref::<rune::runtime::Object>() {
        spec.push(("fields".to_string(), record_fields(&entries_of(&object))?));
    }
    Ok(spec)
}

/// A Rune object's entries, in name order, so one reader sees the same
/// record twice running.
fn entries_of(object: &rune::runtime::Object) -> Vec<(String, rune::Value)> {
    let mut out = Vec::with_capacity(object.len());
    for (name, value) in object {
        out.push((name.to_string(), value.clone()));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// A default as the plain value a schema holds. A class is its fields, which
/// is what a scene file can write and read back.
fn plain_of(default: &rune::Value) -> Result<balaur_script::Value> {
    if let Some((_, fields)) = default.struct_parts() {
        let mut out = Vec::with_capacity(fields.len());
        for (name, value) in &fields {
            out.push((name.clone(), plain_of(value)?));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        return Ok(balaur_script::Value::Map(out));
    }
    value::to_plain(default)
        .ok_or_else(|| anyhow!("a default has to be a value a scene file can hold"))
}

fn inferred_type(default: &rune::Value) -> &'static str {
    if default.borrow_ref::<rune::runtime::Vec>().is_ok() {
        return "list";
    }
    if default.borrow_ref::<rune::runtime::Object>().is_ok() {
        return "record";
    }
    match value::to_plain(default) {
        Some(balaur_script::Value::Bool(_)) => "bool",
        Some(balaur_script::Value::Int(_)) => "int",
        Some(balaur_script::Value::Num(_)) => "float",
        Some(balaur_script::Value::Vec2(_)) => "vec2",
        Some(balaur_script::Value::Vec3(_)) => "vec3",
        Some(balaur_script::Value::Color(_)) => "color",
        // A node reference is a string until the spec form says otherwise.
        _ => "string",
    }
}

/// What a bare list holds, taken from its first entry and required of the
/// rest: a row has to know which editor to draw in it.
fn list_of(items: &rune::runtime::Vec) -> Result<Vec<(String, balaur_script::Value)>> {
    let Some(first) = items.first() else {
        bail!(
            "an empty list cannot say what it holds; declare it as `#{{ type: \"list\", of: #{{ \
             type: \"string\" }}, default: [] }}`"
        );
    };
    let of = inferred_spec(first)?;
    let spec = balaur_script::Value::Map(of.clone());
    for (at, item) in items.iter().enumerate().skip(1) {
        let plain = plain_of(item)?;
        if let Err(why) = balaur_core::node_api::check_property_value(&spec, &plain) {
            bail!("entry {at}: {why}. A list holds one type");
        }
    }
    Ok(of)
}

/// A bare object's fields, or a class's, each inferred on its own: this is
/// how a script exports the data a type of its own holds.
fn record_fields(entries: &[(String, rune::Value)]) -> Result<balaur_script::Value> {
    if entries.is_empty() {
        bail!(
            "an empty object cannot say what it holds; declare it as `#{{ type: \"record\", \
             fields: #{{ .. }}, default: #{{ }} }}`"
        );
    }
    let mut fields = Vec::with_capacity(entries.len());
    for (name, value) in entries {
        fields.push((
            name.clone(),
            balaur_script::Value::Map(inferred_spec(value)?),
        ));
    }
    fields.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(balaur_script::Value::Map(fields))
}

/// A `record`'s declared fields, each beside the spec it takes.
pub(crate) fn record_fields_of(spec: &balaur_script::Value) -> Vec<(&str, &balaur_script::Value)> {
    let Some(balaur_script::Value::Map(fields)) = spec_key(spec, "fields") else {
        return Vec::new();
    };
    fields
        .iter()
        .map(|(name, field)| (name.as_str(), field))
        .collect()
}

/// What a `list` or a `map` export holds.
pub(crate) fn held_spec(spec: &balaur_script::Value) -> Option<&balaur_script::Value> {
    spec_key(spec, "of")
}

/// A name a spec carries beside its type: `class`, and `asset` before it.
fn spec_str<'a>(spec: &'a balaur_script::Value, key: &str) -> Option<&'a str> {
    match spec_key(spec, key) {
        Some(balaur_script::Value::Str(name)) => Some(name.as_str()),
        _ => None,
    }
}

fn spec_key<'a>(spec: &'a balaur_script::Value, key: &str) -> Option<&'a balaur_script::Value> {
    let balaur_script::Value::Map(fields) = spec else {
        return None;
    };
    fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// What an export's spec declares as its `type`, where it declares one.
fn declared_type(spec: &balaur_script::Value) -> Option<&str> {
    let balaur_script::Value::Map(fields) = spec else {
        return None;
    };
    fields.iter().find_map(|(k, v)| match v {
        balaur_script::Value::Str(kind) if k == "type" => Some(kind.as_str()),
        _ => None,
    })
}

/// The component a `node` export names, whose handle the script is handed
/// instead of the node: `#{ type: "node", component: "body2d" }`.
pub(crate) fn export_component(spec: &balaur_script::Value) -> Option<&str> {
    let balaur_script::Value::Map(fields) = spec else {
        return None;
    };
    fields.iter().find_map(|(k, v)| match v {
        balaur_script::Value::Str(name) if k == "component" => Some(name.as_str()),
        _ => None,
    })
}

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
fn spec_of(key: &str, name: &str, value: &rune::Value) -> Result<balaur_script::Value> {
    use balaur_script::Value;
    // A table carrying `type` is written out as it stands; anything else is a
    // bare default, and its own shape says what it is.
    let written = value
        .borrow_ref::<rune::runtime::Object>()
        .ok()
        .filter(|fields| fields.contains_key("type"))
        .and_then(|_| value::to_plain(value));
    let declared = match written {
        Some(Value::Map(fields)) => fields,
        _ => inferred_spec(value)
            .map_err(|why| anyhow!("[{key}] exports: property '{name}': {why:#}"))?,
    };
    checked_spec(key, name, declared)
}

/// One spec against the schema vocabulary, and back with every nested
/// `default` filled in.
fn checked_spec(
    key: &str,
    name: &str,
    declared: Vec<(String, balaur_script::Value)>,
) -> Result<balaur_script::Value> {
    balaur_core::node_api::checked_property_spec(&balaur_script::Value::Map(declared))
        .map_err(|why| anyhow!("[{key}] exports: property '{name}': {why}"))
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
    /// One `node` export's value: the node its path names from `entity`, its
    /// handle for the `component` the spec asks for, or nil.
    pub(crate) fn node_prop(
        &self,
        entity: Entity,
        key: &str,
        name: &str,
        path: &str,
        spec: &balaur_script::Value,
    ) -> Result<rune::Value> {
        let found = (!path.is_empty())
            .then(|| balaur_core::scene::find_node(&self.engine.world(), entity, path))
            .flatten();
        if found.is_none() && !path.is_empty() {
            tracing::warn!("[{key}] node property '{name}' names '{path}', which is not there");
        }
        let component = export_component(spec);
        if let (Some(node), Some(component)) = (found, component)
            && balaur_core::components::get(&self.engine, node, component).is_none()
        {
            tracing::warn!(
                "[{key}] property '{name}' names '{path}', which carries no {component}"
            );
        }
        Ok(match (found, component) {
            (Some(node), Some(component)) => rune::to_value(value::component::Component {
                node: balaur_core::node_id_of(node).0,
                name: crate::value::component::intern(component),
                index: balaur_core::components::index_of(&self.engine, component)
                    .unwrap_or(usize::MAX) as u32,
            })?,
            (Some(node), None) => rune::to_value(Node {
                id: node.to_bits().get(),
            })?,
            (None, _) => rune::to_value(())?,
        })
    }

    /// One export's value as its spec says to build it: a node path becomes
    /// the node it names, a map keyed by numbers gets numbers for keys, and a
    /// composite is walked beside the specs it holds.
    pub(crate) fn export_value(
        &self,
        entity: Entity,
        key: &str,
        name: &str,
        spec: &balaur_script::Value,
        value: &balaur_script::Value,
    ) -> Result<rune::Value> {
        use balaur_script::Value;
        match (declared_type(spec), value) {
            (Some("node"), Value::Str(path)) => self.node_prop(entity, key, name, path, spec),
            (Some("list"), Value::List(items)) => {
                let Some(of) = held_spec(spec) else {
                    return value::from_neutral(value);
                };
                let mut out = rune::runtime::Vec::new();
                for item in items {
                    out.push(self.export_value(entity, key, name, of, item)?)?;
                }
                Ok(rune::to_value(out)?)
            }
            (Some("map"), Value::Map(entries)) => {
                let Some(of) = held_spec(spec) else {
                    return value::from_neutral(value);
                };
                let mut built = Vec::with_capacity(entries.len());
                for (at, inner) in entries {
                    built.push((at, self.export_value(entity, key, name, of, inner)?));
                }
                // TOML keys are text, so a map of whole numbers is written
                // `"7"` in the file and handed back keyed by the number.
                if spec_str(spec, "key") == Some("int") {
                    let mut out = rune::modules::collections::HashMap::new();
                    for (at, value) in built {
                        let Ok(number) = at.parse::<i64>() else {
                            bail!(
                                "[{key}] property '{name}' holds the key \"{at}\", which is not a whole number"
                            );
                        };
                        out.insert(rune::to_value(number)?, value).into_result()?;
                    }
                    return Ok(rune::to_value(out)?);
                }
                let mut out = rune::runtime::Object::new();
                for (at, value) in built {
                    out.insert(rune::alloc::String::try_from(at.as_str())?, value)?;
                }
                Ok(rune::to_value(out)?)
            }
            // Every declared field, so a scene naming one of two still hands
            // the script both. The record's shape is the spec's.
            (Some("record"), Value::Map(entries)) => {
                let mut built = Vec::new();
                for (field, governs) in record_fields_of(spec) {
                    let held = entries.iter().find(|(written, _)| written == field);
                    let held = held.map_or_else(|| export_default(governs), |(_, v)| v.clone());
                    built.push((field, self.export_value(entity, key, name, governs, &held)?));
                }
                // A record naming a class is that class: the script gets its
                // own type back, methods and all, not a look-alike object.
                if let Some(class) = spec_str(spec, "class") {
                    return self.new_class(key, name, class, &built);
                }
                let mut out = rune::runtime::Object::new();
                for (field, value) in built {
                    out.insert(rune::alloc::String::try_from(field)?, value)?;
                }
                Ok(rune::to_value(out)?)
            }
            _ => value::from_neutral(value),
        }
    }

    /// One `class` record as an instance of the script's own struct.
    ///
    /// The unit is the script's, so a class is looked up where it was
    /// declared. A name no `struct` answers to is a warning and an object,
    /// which is what the fields already are.
    fn new_class(
        &self,
        key: &str,
        name: &str,
        class: &str,
        fields: &[(&str, rune::Value)],
    ) -> Result<rune::Value> {
        let unit = {
            let state = self.state.borrow();
            match state.scripts.get(key) {
                Some(script) => script.unit.clone(),
                None => return Err(anyhow!("[{key}] is not loaded")),
            }
        };
        if let Some(built) = unit.new_struct(class, fields) {
            return Ok(built);
        }
        tracing::warn!(
            "[{key}] property '{name}' names class '{class}', which it does not declare"
        );
        let mut out = rune::runtime::Object::new();
        for (field, value) in fields {
            out.insert(rune::alloc::String::try_from(*field)?, value.clone())?;
        }
        Ok(rune::to_value(out)?)
    }

    /// Log a runtime error at the line that threw, with the script backtrace
    /// under it.
    ///
    /// `VmError` on its own prints the message and nothing else. Rendering it
    /// against the unit's sources is what turns "field not found" into a file,
    /// a line and the frames that led there.
    pub(crate) fn report(&self, key: &str, label: &str, err: &rune::runtime::VmError) {
        let times = tally(key, label, err);
        if times > 1 {
            if is_power_of_ten(times) {
                tracing::error!("[{key}] {label}: the same error, thrown {times} times");
            }
            return;
        }
        // An error thrown in a unit another script required renders against
        // that unit's sources: its line numbers mean nothing in the caller's.
        let thrown = err.first_location().map(|at| at.unit.clone());
        let sources = {
            let state = self.state.borrow();
            let owner = thrown.and_then(|unit| {
                state
                    .scripts
                    .values()
                    .find(|s| std::sync::Arc::ptr_eq(&s.unit, &unit))
                    .and_then(|s| s.sources.clone())
            });
            owner.or_else(|| state.scripts.get(key).and_then(|s| s.sources.clone()))
        };
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
        let rendered = rendered.trim_end();
        // An error against a source the unit does not hold renders to nothing,
        // and a blank line says less than the message it replaced.
        if rendered.is_empty() {
            tracing::error!("[{key}] {label}: {err}");
            return;
        }
        tracing::error!("[{key}] {label}:\n{rendered}");
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
        let mut findings = self.handle_findings(key, source);
        let mut sources = Sources::new();
        let root = sources.insert(Source::with_path(key, source, path)?)?;
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
        // Each warned-about source, read once: what a diagnostic means can
        // depend on the code under it, and `Source` does not hand its text out.
        let mut texts: BTreeMap<rune::SourceId, Option<String>> = BTreeMap::new();
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
                rune::diagnostics::Diagnostic::Warning(warning) => {
                    let id = warning.source_id();
                    let text = texts
                        .entry(id)
                        .or_insert_with(|| read_source(&sources, id, root, source));
                    if unpacking_a_tuple(&warning.to_string(), text.as_deref(), warning.span()) {
                        continue;
                    }
                    finding(
                        &sources,
                        id,
                        Some(warning.span()),
                        "warning",
                        &warning.to_string(),
                    )
                }
                _ => continue,
            });
        }
        Ok(findings)
    }

    /// What the scene beside the script says about a handle call in it.
    ///
    /// The compiler cannot see this: a component handle resolves its method
    /// by component name at call time, so `this.node.sprite.apply_impulse()`
    /// compiles clean and fails on the tick that runs it. A packed run has no
    /// scene tree to read and is checking a game that already shipped.
    fn handle_findings(&self, key: &str, source: &str) -> Vec<Finding> {
        let root = {
            let state = self.state.borrow();
            if state.pack.is_some() {
                return Vec::new();
            }
            state.project_root.clone()
        };
        let attached = balaur_core::attachments::scene_attachments(&root);
        let carried = attached.get(key).and_then(Option::as_ref);
        handles::check(&self.engine, key, source, carried)
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
        // The values are read as Rune's own, not as plain data: a script's
        // `struct` says which class it is, and that has to reach the spec.
        let written = match self.method(key, "exports") {
            None => Vec::new(),
            Some(f) => match f.call::<rune::Value>(()) {
                VmResult::Ok(v) => match v.borrow_ref::<rune::runtime::Object>() {
                    Ok(object) => {
                        // Rune objects keep no order of their own, so the
                        // rows are sorted and `order` moves one that asks.
                        entries_of(&object)
                    }
                    Err(_) => {
                        return Err(anyhow!("[{key}] exports must return an object of defaults"));
                    }
                },
                VmResult::Err(err) => return Err(anyhow!("[{key}] exports: {err}")),
            },
        };
        let mut declared = Vec::with_capacity(written.len());
        for (name, value) in written {
            let spec = spec_of(key, &name, &value)?;
            declared.push((name, spec));
        }
        for (name, spec) in self.attributed_exports(key)? {
            if declared.iter().any(|(seen, _)| *seen == name) {
                return Err(anyhow!(
                    "[{key}] property '{name}' is declared twice: once by `#[export]` and \
                     once in `exports()`. Keep one."
                ));
            }
            declared.push((name, spec));
        }
        // Name order is the tie-break; `order` is what a script says when the
        // rows belong in an order of its own.
        declared.sort_by(|a, b| order_of(&a.1).total_cmp(&order_of(&b.1)));
        Ok(declared)
    }

    /// The properties `#[export]` declares, as specs.
    ///
    /// The compiler wrote the names and kinds into the unit, so a typo is a
    /// build error rather than a property that silently never appears. Only
    /// the kind a default cannot carry is named: `node` and `asset` both look
    /// like a string until something says otherwise.
    fn attributed_exports(&self, key: &str) -> Result<Vec<(String, balaur_script::Value)>> {
        let unit = {
            let state = self.state.borrow();
            let Some(script) = state.scripts.get(key) else {
                return Ok(Vec::new());
            };
            script.unit.clone()
        };
        let mut out = Vec::new();
        for (name, kind, value) in unit
            .exported_constants()
            .map_err(|err| anyhow!("[{key}] reading `#[export]` constants: {err}"))?
        {
            // An asset property has to say which asset type it takes, and the
            // attribute has nowhere to put that, so it stays with the form
            // that does rather than building a spec the schema will refuse.
            if kind == "asset" {
                return Err(anyhow!(
                    "[{key}] `#[export(asset)]` on '{name}': an asset property has to name the \
                     asset type it takes, which the attribute cannot carry. Declare it in \
                     `exports()` as `#{{ type: \"asset\", asset: \"texture\", default: \"\" }}`."
                ));
            }
            // `value` leaves the default's own type to speak, which is what
            // `spec_of` does for a bare entry anyway.
            let spec = if kind == "value" {
                spec_of(key, name, &value)?
            } else {
                let Some(default) = value::to_plain(&value) else {
                    return Err(anyhow!(
                        "[{key}] `#[export]` on '{name}': a property's default has to be a plain \
                         value"
                    ));
                };
                checked_spec(
                    key,
                    name,
                    vec![
                        (
                            "type".to_string(),
                            balaur_script::Value::Str(kind.to_string()),
                        ),
                        ("default".to_string(), default),
                    ],
                )?
            };
            out.push((name.to_string(), spec));
        }
        Ok(out)
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
