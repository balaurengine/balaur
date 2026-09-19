//! Addons mounted as native modules.
//!
//! `addons/<name>/<file>.rn` is reachable from every script as
//! `<name>::<file>`: its `pub fn`s as functions and its `pub const`s as
//! constants, a `pub mod` block's constants one path deeper. Each function
//! forwards to the addon's own unit when called, so a hot reload reaches every
//! caller; the paths themselves are fixed when the context is built.

use std::fmt::Write as _;
use std::path::Path;

use rune::alloc::clone::TryClone as _;
use rune::runtime::{VmError, VmResult};

use crate::inspect::public_functions;
use crate::tooling::{Completion, Hover, Kind};
use crate::{HOSTS, RuneHost, packed};

/// Where addons live under a project root.
const ADDONS: &str = "addons";

/// One addon file, as its mount exposes it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Mount {
    /// The addon and the file: `["gamend", "lobbies"]`.
    pub(crate) path: [String; 2],
    /// The key the host loads the file by.
    pub(crate) key: String,
    pub(crate) functions: Vec<Mounted>,
    /// Each `pub const` by its path below the file: `["lobby", "UPDATED"]`.
    pub(crate) constants: Vec<(Vec<String>, Constant)>,
}

/// One `pub fn` of a mounted file. A compiled pack keeps only the name and
/// the arity; the parameters and the doc are for the editor's tooling.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Mounted {
    pub(crate) name: String,
    pub(crate) arity: usize,
    /// The parameter list as written, without its parentheses.
    pub(crate) params: String,
    /// The `///` lines above it, joined.
    pub(crate) doc: String,
}

/// A value a native module can hold as a constant.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Constant {
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(String),
}

impl std::fmt::Display for Constant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Num(n) => write!(f, "{n}"),
            Self::Str(s) => write!(f, "{s:?}"),
        }
    }
}

impl RuneHost {
    /// Every addon file the host can reach. A later root replaces an earlier
    /// one at the same path: the game the editor opened last is the one
    /// playing.
    pub(crate) fn discover_mounts(&self) -> Vec<Mount> {
        let mut found: Vec<Mount> = Vec::new();
        let (pack_files, own_root) = {
            let state = self.state.borrow();
            let files = state.pack.as_ref().map(|pack| {
                pack.scripts
                    .iter()
                    .filter_map(|(key, bytes)| Some((mount_path(key)?, key.clone(), bytes.clone())))
                    .collect::<Vec<_>>()
            });
            (files, state.project_root.clone())
        };
        let roots = balaur_core::file_api::project_roots(&self.engine);
        match pack_files {
            Some(files) => {
                for (path, key, bytes) in files {
                    found.push(mount_of(path, key, &bytes));
                }
            }
            None => self.mounts_under(&own_root, false, &mut found),
        }
        for root in roots.iter().skip(1) {
            self.mounts_under(root, true, &mut found);
        }
        let mut kept: Vec<Mount> = Vec::new();
        for mount in found {
            kept.retain(|m| m.path != mount.path);
            kept.push(mount);
        }
        kept
    }

    /// The addon files under one root. The project's own are keyed relative
    /// to it; another root's by absolute path, as `script::require` keys them.
    fn mounts_under(&self, root: &Path, absolute: bool, out: &mut Vec<Mount>) {
        let files = balaur_core::files::backend(&self.engine);
        let addons = root.join(ADDONS);
        let mut addon_dirs = files.list(&addons);
        addon_dirs.sort();
        for (addon, is_dir) in addon_dirs {
            if !is_dir {
                continue;
            }
            let mut entries = files.list(&addons.join(&addon));
            entries.sort();
            for (file, is_dir) in entries {
                let relative = format!("{ADDONS}/{addon}/{file}");
                let Some(path) = mount_path(&relative).filter(|_| !is_dir) else {
                    continue;
                };
                let full = root.join(&relative);
                let Ok(bytes) = files.read(&full) else {
                    continue;
                };
                let key = if absolute {
                    full.to_string_lossy().replace('\\', "/")
                } else {
                    relative
                };
                out.push(mount_of(path, key, &bytes));
            }
        }
    }

    /// The native modules for `mounts`, each named for the error its install
    /// would report.
    pub(crate) fn mount_modules(&self, mounts: &[Mount]) -> Vec<(String, rune::Module)> {
        let slot = HOSTS.with(|hosts| {
            let mut hosts = hosts.borrow_mut();
            hosts.push(self.clone());
            hosts.len() - 1
        });
        let mut out = Vec::new();
        for mount in mounts {
            let label = mount.path.join("::");
            match module_of(mount, slot) {
                Ok(modules) => out.extend(modules.into_iter().map(|m| (label.clone(), m))),
                Err(err) => tracing::error!("addon {label}: {err}"),
            }
        }
        out
    }
}

/// What the editor's tooling offers along a mounted path.
impl RuneHost {
    /// `gamend::` — a mounted addon's files; `gamend::lobbies::` — that
    /// file's functions, constants and `pub mod` blocks, and so on down.
    pub(crate) fn complete_mounted(
        &self,
        path: &[String],
        prefix: &str,
        out: &mut Vec<Completion>,
    ) {
        // Building the context is what finds the mounts.
        let _ = self.context();
        let state = self.state.borrow();
        for mount in &state.mounts {
            let [addon, file] = &mount.path;
            if path.first() != Some(addon) {
                continue;
            }
            if path.len() == 1 {
                if file.starts_with(prefix) {
                    out.push(Completion {
                        label: file.clone(),
                        kind: Kind::Module,
                        detail: mount.key.clone(),
                        doc: String::new(),
                        insert: file.clone(),
                    });
                }
                continue;
            }
            if &path[1] != file {
                continue;
            }
            let within = &path[2..];
            if within.is_empty() {
                for function in &mount.functions {
                    if function.name.starts_with(prefix) {
                        out.push(Completion {
                            label: function.name.clone(),
                            kind: Kind::Function,
                            detail: format!("({})", function.params),
                            doc: function.doc.clone(),
                            insert: function.name.clone(),
                        });
                    }
                }
            }
            for (at, value) in &mount.constants {
                let Some((name, parent)) = at.split_last() else {
                    continue;
                };
                if parent == within && name.starts_with(prefix) {
                    out.push(Completion {
                        label: name.clone(),
                        kind: Kind::Constant,
                        detail: value.to_string(),
                        doc: String::new(),
                        insert: name.clone(),
                    });
                } else if parent.len() > within.len()
                    && parent.starts_with(within)
                    && parent[within.len()].starts_with(prefix)
                {
                    let module = parent[within.len()].clone();
                    out.push(Completion {
                        label: module.clone(),
                        kind: Kind::Module,
                        detail: String::new(),
                        doc: String::new(),
                        insert: module,
                    });
                }
            }
        }
    }

    /// The hover for `name` after a mounted path, when it names one.
    pub(crate) fn describe_mounted(&self, path: &[String], name: &str) -> Option<Hover> {
        let _ = self.context();
        let state = self.state.borrow();
        let title = format!("{}::{name}", path.join("::"));
        for mount in &state.mounts {
            let [addon, file] = &mount.path;
            if path.first() != Some(addon) {
                continue;
            }
            if path.len() == 1 && file == name {
                return Some(Hover {
                    title,
                    detail: mount.key.clone(),
                    doc: String::new(),
                });
            }
            if path.get(1) != Some(file) {
                continue;
            }
            let within = &path[2..];
            if within.is_empty()
                && let Some(function) = mount.functions.iter().find(|f| f.name == name)
            {
                return Some(Hover {
                    title,
                    detail: format!("({})", function.params),
                    doc: function.doc.clone(),
                });
            }
            let constant = mount.constants.iter().find(|(at, _)| {
                at.split_last()
                    .is_some_and(|(last, parent)| last == name && parent == within)
            });
            if let Some((_, value)) = constant {
                return Some(Hover {
                    title,
                    detail: value.to_string(),
                    doc: String::new(),
                });
            }
        }
        None
    }
}

/// `["gamend", "lobbies"]` for `addons/gamend/lobbies.rn`; `None` for a file
/// deeper down, or a name Rune cannot spell as a path.
fn mount_path(key: &str) -> Option<[String; 2]> {
    let mut parts = key.split('/');
    let (Some(ADDONS), Some(addon), Some(file), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let stem = file.strip_suffix(".rn")?;
    (identifier(addon) && identifier(stem)).then(|| [addon.to_string(), stem.to_string()])
}

/// Whether `name` is a Rune identifier and not a keyword.
fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && rune::parse::parse_all::<rune::ast::Ident>(name, rune::SourceId::EMPTY, false).is_ok()
}

/// What one file exposes. A compiled pack keeps its functions' signatures
/// but not its constants, which every unit using them already holds.
fn mount_of(path: [String; 2], key: String, bytes: &[u8]) -> Mount {
    let (functions, constants) = if packed::is_encoded(bytes) {
        let signatures = packed::decode(bytes).map(|(_, f)| f).unwrap_or_default();
        let functions = signatures
            .into_iter()
            .map(|f| Mounted {
                name: f.name,
                arity: f.arity,
                params: String::new(),
                doc: String::new(),
            })
            .collect();
        (functions, Vec::new())
    } else {
        let source = String::from_utf8_lossy(bytes);
        let functions = public_functions(&source)
            .into_iter()
            .map(|f| {
                let (params, doc) = declaration(&source, f.line);
                Mounted {
                    name: f.name,
                    arity: f.arity,
                    params,
                    doc,
                }
            })
            .collect();
        (functions, constants(&key, &source))
    };
    Mount {
        path,
        key,
        functions,
        constants,
    }
}

/// The parameter list of the `pub fn` on 1-based `line`, and the `///` lines
/// right above it.
fn declaration(source: &str, line: usize) -> (String, String) {
    let lines: Vec<&str> = source.lines().collect();
    let at = line.saturating_sub(1);
    let params = lines
        .get(at)
        .and_then(|text| text.find('(').map(|open| &text[open + 1..]))
        .and_then(|first| crate::inspect::parameters(&lines, at, first))
        .map(|params| params.trim().to_string())
        .unwrap_or_default();
    let mut doc: Vec<&str> = lines[..at.min(lines.len())]
        .iter()
        .rev()
        .map(|text| text.trim_start())
        .take_while(|text| text.starts_with("///"))
        .map(|text| text.trim_start_matches("///").trim())
        .collect();
    doc.reverse();
    (params, doc.join(" "))
}

/// Every `pub const` in a source, a `pub mod` block's by their path within
/// it, evaluated by compiling the constants alone: the rest of the file may
/// call the very paths being built.
fn constants(key: &str, source: &str) -> Vec<(Vec<String>, Constant)> {
    let Ok(file) = rune::parse::parse_all::<rune::ast::File>(source, rune::SourceId::EMPTY, false)
    else {
        return Vec::new();
    };
    let mut snippet = String::new();
    let mut paths = Vec::new();
    gather(&file, source, &[], true, &mut snippet, &mut paths);
    if paths.is_empty() {
        return Vec::new();
    }
    let listed: Vec<String> = paths.iter().map(|p| p.join("::")).collect();
    let _ = writeln!(
        snippet,
        "pub fn {VALUES_FN}() {{ [{}] }}",
        listed.join(", ")
    );
    match evaluate(&snippet) {
        Ok(values) => paths
            .into_iter()
            .zip(values)
            .filter_map(|(path, value)| {
                let constant = constant_of(&value);
                if constant.is_none() {
                    tracing::error!(
                        "{key}: `{}` is not a bool, number or string, so it is not mounted",
                        path.join("::")
                    );
                }
                Some((path, constant?))
            })
            .collect(),
        Err(err) => {
            tracing::error!("{key}: its constants did not evaluate: {err}");
            Vec::new()
        }
    }
}

/// The function a constants-only source reports its values through.
const VALUES_FN: &str = "__balaur_mount_values";

/// The source of every constant and every inline module holding one, and
/// the public paths among them.
fn gather(
    file: &rune::ast::File,
    source: &str,
    prefix: &[String],
    public: bool,
    snippet: &mut String,
    paths: &mut Vec<Vec<String>>,
) {
    use rune::ast::{Item, ItemModBody, Spanned as _, Visibility};
    for (item, _) in &file.items {
        match item {
            Item::Const(declared) => {
                let (Some(text), Some(name)) = (
                    source.get(declared.span().range()),
                    source.get(declared.name.span().range()),
                ) else {
                    continue;
                };
                snippet.push_str(text);
                snippet.push_str(";\n");
                if public && matches!(declared.visibility, Visibility::Public(_)) {
                    let mut path = prefix.to_vec();
                    path.push(name.to_string());
                    paths.push(path);
                }
            }
            Item::Mod(module) => {
                let ItemModBody::InlineBody(body) = &module.body else {
                    continue;
                };
                let Some(name) = source.get(module.name.span().range()) else {
                    continue;
                };
                let mut inner = prefix.to_vec();
                inner.push(name.to_string());
                let _ = writeln!(snippet, "pub mod {name} {{");
                let shown = public && matches!(module.visibility, Visibility::Public(_));
                gather(&body.file, source, &inner, shown, snippet, paths);
                snippet.push_str("}\n");
            }
            _ => {}
        }
    }
}

/// Compile a constants-only source with Rune's own modules and read back
/// what [`VALUES_FN`] returns.
fn evaluate(snippet: &str) -> anyhow::Result<Vec<rune::Value>> {
    let context = rune::Context::with_default_modules()?;
    let mut sources = rune::Sources::new();
    sources.insert(rune::Source::memory(snippet)?)?;
    let mut diagnostics = rune::Diagnostics::without_warnings();
    let unit = rune::prepare(&mut sources)
        .with_context(&context)
        .with_diagnostics(&mut diagnostics)
        .build()
        .map_err(|_| anyhow::anyhow!("{}", crate::inspect::render(&diagnostics, &sources)))?;
    let mut vm = rune::Vm::new(
        std::sync::Arc::new(context.runtime()?),
        std::sync::Arc::new(unit),
    );
    let values = vm.call([VALUES_FN], ())?;
    Ok(rune::from_value::<Vec<rune::Value>>(values)?)
}

fn constant_of(value: &rune::Value) -> Option<Constant> {
    let owned = || value.try_clone().ok();
    if let Some(Ok(b)) = owned().map(rune::from_value::<bool>) {
        return Some(Constant::Bool(b));
    }
    if let Some(Ok(i)) = owned().map(rune::from_value::<i64>) {
        return Some(Constant::Int(i));
    }
    if let Some(Ok(n)) = owned().map(rune::from_value::<f64>) {
        return Some(Constant::Num(n));
    }
    if let Some(Ok(s)) = owned().map(rune::from_value::<String>) {
        return Some(Constant::Str(s));
    }
    None
}

/// The file's module and one per `pub mod` block holding constants.
fn module_of(mount: &Mount, slot: usize) -> anyhow::Result<Vec<rune::Module>> {
    let [addon, file] = &mount.path;
    let mut top = rune::Module::with_crate_item(addon.as_str(), [file.as_str()])?;
    for function in &mount.functions {
        let (name, arity) = (&function.name, function.arity);
        if let Err(err) = forward(&mut top, slot, &mount.key, name, arity) {
            tracing::error!("{}: {err}", mount.key);
        }
    }
    let mut nested: Vec<(Vec<String>, rune::Module)> = Vec::new();
    for (path, value) in &mount.constants {
        let (name, within) = path.split_last().expect("a constant has a name");
        let module = if within.is_empty() {
            &mut top
        } else {
            if !nested.iter().any(|(at, _)| at == within) {
                let mut item = vec![file.as_str()];
                item.extend(within.iter().map(String::as_str));
                nested.push((
                    within.to_vec(),
                    rune::Module::with_crate_item(addon.as_str(), item)?,
                ));
            }
            &mut nested
                .iter_mut()
                .find(|(at, _)| at == within)
                .expect("inserted above")
                .1
        };
        add_constant(module, name, value)?;
    }
    let mut out = vec![top];
    out.extend(nested.into_iter().map(|(_, module)| module));
    Ok(out)
}

fn add_constant(module: &mut rune::Module, name: &str, value: &Constant) -> anyhow::Result<()> {
    match value {
        Constant::Bool(b) => module.constant(name, *b).build()?,
        Constant::Int(i) => module.constant(name, *i).build()?,
        Constant::Num(n) => module.constant(name, *n).build()?,
        Constant::Str(s) => module.constant(name, s.as_str()).build()?,
    };
    Ok(())
}

/// A native function that runs `name` from `key`'s current unit.
fn forward(
    module: &mut rune::Module,
    slot: usize,
    key: &str,
    name: &str,
    arity: usize,
) -> anyhow::Result<()> {
    type V = rune::Value;
    let call = Call {
        slot,
        key: key.to_string(),
        name: name.to_string(),
    };
    match arity {
        0 => module
            .function(name, move || call.run(Vec::new()))
            .build()?,
        1 => module
            .function(name, move |a: V| call.run(vec![a]))
            .build()?,
        2 => module
            .function(name, move |a: V, b: V| call.run(vec![a, b]))
            .build()?,
        3 => module
            .function(name, move |a: V, b: V, c: V| call.run(vec![a, b, c]))
            .build()?,
        4 => module
            .function(name, move |a: V, b: V, c: V, d: V| {
                call.run(vec![a, b, c, d])
            })
            .build()?,
        5 => module
            .function(name, move |a: V, b: V, c: V, d: V, e: V| {
                call.run(vec![a, b, c, d, e])
            })
            .build()?,
        _ => anyhow::bail!(
            "`{name}` takes {arity} arguments; a mounted function takes at most {}",
            crate::shared::MOST_ARGS
        ),
    };
    Ok(())
}

/// Which function a mounted path runs, found again on every call.
#[derive(Clone)]
struct Call {
    slot: usize,
    key: String,
    name: String,
}

impl Call {
    fn run(&self, args: Vec<rune::Value>) -> VmResult<rune::Value> {
        let host = HOSTS.with(|hosts| hosts.borrow()[self.slot].clone());
        if let Err(err) = host.load(&self.key) {
            return VmResult::Err(VmError::panic(format!("{}: {err}", self.key)));
        }
        let Some(function) = host.method(&self.key, &self.name) else {
            return VmResult::Err(VmError::panic(format!(
                "{} has no `pub fn {}`",
                self.key, self.name
            )));
        };
        match function.call::<rune::Value>(args).into_result() {
            Ok(value) => VmResult::Ok(value),
            Err(err) => VmResult::Err(VmError::panic(format!(
                "{}: {}: {err}",
                self.key, self.name
            ))),
        }
    }
}
