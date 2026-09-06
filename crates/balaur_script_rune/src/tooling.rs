//! What an editor asks about a caret: what completes here, what is under it,
//! where it is defined, what this file declares.
//!
//! One provider, two fronts. `balaur lsp` renders the answers as LSP JSON and
//! the editor's `script` module renders them as Rune objects, so a popup in
//! the Script persona and a popup in VS Code say the same thing.
//!
//! Nothing here runs during a frame. Rune offers no incremental parse, so
//! what completes is decided from the text around the caret by [`classify`],
//! and the candidates come from four places: the `ApiEntry` list behind
//! `balaur api`, the component registry and its `acts_on` map, the compiled
//! unit's debug info, and Rune's own context.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use rune::compile::meta;
use rune::{Source, Sources};

use crate::RuneHost;
use crate::api::collect_modules;

/// What a completion is, so a client can pick an icon for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Module,
    Function,
    Method,
    Constant,
    Field,
    Property,
    Keyword,
    Variable,
}

impl Kind {
    /// The LSP `CompletionItemKind` number, and the word a script sees.
    pub fn lsp(self) -> u8 {
        match self {
            Self::Module => 9,
            Self::Function => 3,
            Self::Method => 2,
            Self::Constant => 21,
            Self::Field => 5,
            Self::Property => 10,
            Self::Keyword => 14,
            Self::Variable => 6,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Function => "function",
            Self::Method => "method",
            Self::Constant => "constant",
            Self::Field => "field",
            Self::Property => "property",
            Self::Keyword => "keyword",
            Self::Variable => "variable",
        }
    }
}

/// One thing that could be typed here.
#[derive(Clone, Debug)]
pub struct Completion {
    pub label: String,
    pub kind: Kind,
    /// `(args) -> returns`, or the constant's value. Empty when unknown.
    pub detail: String,
    pub doc: String,
    /// What replacing the prefix writes; equal to `label` unless a snippet.
    pub insert: String,
}

/// What is under the caret, for a hover card.
#[derive(Clone, Debug)]
pub struct Hover {
    pub title: String,
    pub detail: String,
    pub doc: String,
}

/// A `pub fn` or an `exports()` property, for an outline.
#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: String,
    pub kind: Kind,
    pub detail: String,
    /// 1-based, so a gutter can point at it.
    pub line: usize,
    pub column: usize,
}

/// Somewhere to jump. `file` is project-relative when the definition is in
/// the project, and empty when it is engine API, which `url` then names.
#[derive(Clone, Debug)]
pub struct Location {
    pub file: String,
    pub line: usize,
    pub column: usize,
    /// The reference page for a definition with no file to land in.
    pub url: String,
}

/// What the caret sits after, which decides what may follow it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum At {
    /// `physics2d::ray|` — an engine module's own name is known.
    Module { module: String, prefix: String },
    /// `node.body2d.app|` — the receiver named a registered component.
    Handle { component: String, prefix: String },
    /// `this.spe|` — the script's own properties and functions.
    This { prefix: String },
    /// `something.fo|` — a receiver whose type the text does not give.
    Instance { prefix: String },
    /// `phys|` — anything at all may follow.
    Bare { prefix: String },
}

/// Rune keywords worth offering; the rest are never what a game script wants
/// completed mid-expression.
const KEYWORDS: &[&str] = &[
    "async", "await", "break", "const", "continue", "else", "false", "fn", "for", "if", "impl",
    "in", "let", "loop", "match", "mod", "not", "pub", "return", "select", "struct", "true", "use",
    "while", "yield",
];

/// The byte offset of a 1-based line and column in `source`, clamped to the
/// line's end so a column past it lands there rather than in the next line.
pub fn offset_of(source: &str, line: usize, column: usize) -> usize {
    let mut at = 0;
    for (n, text) in source.split('\n').enumerate() {
        if n + 1 == line.max(1) {
            let want = column.saturating_sub(1);
            let mut taken = 0;
            for (i, _) in text.char_indices() {
                if taken == want {
                    return at + i;
                }
                taken += 1;
            }
            return at + text.len();
        }
        at += text.len() + 1;
    }
    source.len()
}

/// The 1-based line and column of a byte offset.
pub fn line_col_of(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |at| at + 1);
    (line, source[start..offset].chars().count() + 1)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The identifier ending at `offset`, and where it starts.
fn word_before(source: &str, offset: usize) -> (&str, usize) {
    let head = &source[..offset.min(source.len())];
    let start = head
        .char_indices()
        .rev()
        .find(|(_, c)| !is_word(*c))
        .map_or(0, |(i, c)| i + c.len_utf8());
    (&head[start..], start)
}

/// Decide what the caret may be completing, from the text before it.
///
/// A receiver chain is walked backwards one segment at a time, so
/// `node.body2d.` reports the component and `get_node("Hip").` does not.
pub fn classify(source: &str, offset: usize) -> At {
    let (prefix, start) = word_before(source, offset);
    let prefix = prefix.to_string();
    let head = &source[..start];
    if let Some(before) = head.strip_suffix("::") {
        let (module, _) = word_before(before, before.len());
        if !module.is_empty() {
            return At::Module {
                module: module.to_string(),
                prefix,
            };
        }
        return At::Bare { prefix };
    }
    let Some(before) = head.strip_suffix('.') else {
        return At::Bare { prefix };
    };
    let (receiver, receiver_start) = word_before(before, before.len());
    if receiver == "this" {
        return At::This { prefix };
    }
    // `node.body2d.` — the segment before the receiver has to be a `.` too, or
    // the receiver is a plain local that happens to share a component's name.
    if !receiver.is_empty() && source[..receiver_start].ends_with('.') {
        return At::Handle {
            component: receiver.to_string(),
            prefix,
        };
    }
    At::Instance { prefix }
}

/// The methods each component's handle offers, from what every function
/// declared it acts on. The same map `value::component` builds the handle
/// from, so the popup and the call agree.
fn handle_methods() -> BTreeMap<String, BTreeMap<String, (String, String)>> {
    let mut out: BTreeMap<String, BTreeMap<String, (String, String)>> = BTreeMap::new();
    for (module_name, module) in collect_modules() {
        for (name, components) in &module.acts_on {
            let signature = signature_display(module.signatures.get(name));
            let doc = module.docs.get(name).cloned().unwrap_or_default();
            for component in components {
                out.entry(component.clone()).or_default().insert(
                    name.clone(),
                    (format!("{module_name}::{name}{signature}"), doc.clone()),
                );
            }
        }
    }
    out
}

/// The six a handle answers whatever it is on, from `value::component`.
const GENERIC: &[(&str, &str)] = &[
    ("get", "Read one of the component's properties by name."),
    ("set", "Write one of the component's properties by name."),
    ("has", "Whether the node carries this component."),
    ("add", "Give the node this component, with the given properties."),
    ("remove", "Take this component off the node."),
    ("props", "Every property of this component, as an object."),
];

impl RuneHost {
    /// Every completion valid at `line`:`column` of `source`, best first.
    ///
    /// # Errors
    /// If the context cannot be built.
    pub fn complete(
        &self,
        key: &str,
        source: &str,
        line: usize,
        column: usize,
    ) -> Result<Vec<Completion>> {
        let at = classify(source, offset_of(source, line, column));
        let mut out = Vec::new();
        match &at {
            At::Module { module, prefix } => self.complete_module(module, prefix, &mut out),
            At::Handle { component, prefix } => {
                Self::complete_handle(component, prefix, &mut out);
            }
            At::This { prefix } => self.complete_this(key, source, prefix, &mut out),
            At::Instance { prefix } => self.complete_instance(prefix, &mut out)?,
            At::Bare { prefix } => self.complete_bare(key, source, prefix, &mut out)?,
        }
        out.sort_by(|a, b| a.label.cmp(&b.label));
        out.dedup_by(|a, b| a.label == b.label && a.kind == b.kind);
        Ok(out)
    }

    /// `physics2d::` — that module's functions and constants, and nothing
    /// else: a module path admits no locals.
    fn complete_module(&self, module: &str, prefix: &str, out: &mut Vec<Completion>) {
        let modules = collect_modules();
        let Some(found) = modules.get(module) else {
            return;
        };
        for name in &found.functions {
            if !name.starts_with(prefix) {
                continue;
            }
            out.push(Completion {
                label: name.clone(),
                kind: Kind::Function,
                detail: signature_display(found.signatures.get(name)),
                doc: found.docs.get(name).cloned().unwrap_or_default(),
                insert: name.clone(),
            });
        }
        for (name, value) in &found.constants {
            if !name.starts_with(prefix) {
                continue;
            }
            out.push(Completion {
                label: name.clone(),
                kind: Kind::Constant,
                detail: value.clone(),
                doc: String::new(),
                insert: name.clone(),
            });
        }
    }

    /// `node.body2d.` — what that component's handle answers.
    fn complete_handle(component: &str, prefix: &str, out: &mut Vec<Completion>) {
        if let Some(methods) = handle_methods().get(component) {
            for (name, (detail, doc)) in methods {
                if !name.starts_with(prefix) {
                    continue;
                }
                out.push(Completion {
                    label: name.clone(),
                    kind: Kind::Method,
                    detail: detail.clone(),
                    doc: doc.clone(),
                    insert: name.clone(),
                });
            }
        }
        for (name, doc) in GENERIC {
            if !name.starts_with(prefix) {
                continue;
            }
            out.push(Completion {
                label: (*name).to_string(),
                kind: Kind::Method,
                detail: String::new(),
                doc: (*doc).to_string(),
                insert: (*name).to_string(),
            });
        }
    }

    /// `this.` — the script's own `exports()` properties and `pub fn`s. Both
    /// are read off the file rather than an instance: there is no game here.
    fn complete_this(&self, key: &str, source: &str, prefix: &str, out: &mut Vec<Completion>) {
        for (name, value) in self.exports(key).unwrap_or_default() {
            if !name.starts_with(prefix) {
                continue;
            }
            out.push(Completion {
                label: name.clone(),
                kind: Kind::Property,
                detail: format!("{value:?}"),
                doc: String::new(),
                insert: name,
            });
        }
        for declared in crate::inspect::public_functions(source) {
            if !declared.name.starts_with(prefix) {
                continue;
            }
            out.push(Completion {
                label: declared.name.clone(),
                kind: Kind::Method,
                detail: format!("({} arguments)", declared.arity),
                doc: String::new(),
                insert: declared.name,
            });
        }
    }

    /// A receiver whose type the text does not give: every component name a
    /// node offers as a field, `node`'s own methods, and Rune's instance
    /// methods. Long, but a short list that hides the right answer is worse.
    fn complete_instance(&self, prefix: &str, out: &mut Vec<Completion>) -> Result<()> {
        for (component, _) in balaur_core::components::schemas(&self.engine) {
            if !component.starts_with(prefix) {
                continue;
            }
            out.push(Completion {
                label: component.clone(),
                kind: Kind::Field,
                detail: "component handle".to_string(),
                doc: String::new(),
                insert: component,
            });
        }
        let modules = collect_modules();
        if let Some(node) = modules.get("node") {
            for name in &node.functions {
                if !name.starts_with(prefix) {
                    continue;
                }
                out.push(Completion {
                    label: name.clone(),
                    kind: Kind::Method,
                    detail: signature_display(node.signatures.get(name)),
                    doc: node.docs.get(name).cloned().unwrap_or_default(),
                    insert: name.clone(),
                });
            }
        }
        self.complete_context_instances(prefix, out)
    }

    /// Rune's own instance methods, from the context the fork opened up.
    fn complete_context_instances(&self, prefix: &str, out: &mut Vec<Completion>) -> Result<()> {
        let (ctx, _) = self.context()?;
        for (found, _) in ctx.iter_functions() {
            let meta::Kind::Function {
                associated: Some(meta::AssociatedKind::Instance(name)),
                ..
            } = &found.kind
            else {
                continue;
            };
            let name = name.as_ref();
            if !name.starts_with(prefix) {
                continue;
            }
            out.push(Completion {
                label: name.to_string(),
                kind: Kind::Method,
                detail: found
                    .item
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                doc: String::new(),
                insert: name.to_string(),
            });
        }
        Ok(())
    }

    /// A bare prefix: module names, this file's own functions and the locals
    /// its unit compiled, `use` paths, and keywords.
    fn complete_bare(
        &self,
        key: &str,
        source: &str,
        prefix: &str,
        out: &mut Vec<Completion>,
    ) -> Result<()> {
        for (name, module) in collect_modules() {
            if !name.starts_with(prefix) {
                continue;
            }
            out.push(Completion {
                label: name.clone(),
                kind: Kind::Module,
                detail: String::new(),
                doc: module.doc.clone(),
                insert: name,
            });
        }
        for declared in crate::inspect::public_functions(source) {
            if !declared.name.starts_with(prefix) {
                continue;
            }
            out.push(Completion {
                label: declared.name.clone(),
                kind: Kind::Function,
                detail: format!("({} arguments)", declared.arity),
                doc: String::new(),
                insert: declared.name,
            });
        }
        for name in self.unit_functions(key, source) {
            if !name.starts_with(prefix) {
                continue;
            }
            out.push(Completion {
                label: name.clone(),
                kind: Kind::Function,
                detail: String::new(),
                doc: String::new(),
                insert: name,
            });
        }
        for name in locals_before(source, prefix) {
            out.push(Completion {
                label: name.clone(),
                kind: Kind::Variable,
                detail: String::new(),
                doc: String::new(),
                insert: name,
            });
        }
        for word in KEYWORDS {
            if word.starts_with(prefix) {
                out.push(Completion {
                    label: (*word).to_string(),
                    kind: Kind::Keyword,
                    detail: String::new(),
                    doc: String::new(),
                    insert: (*word).to_string(),
                });
            }
        }
        Ok(())
    }

    /// `source` formatted, or the message saying why it could not be.
    ///
    /// Rune's own formatter, so a Balaur script and a Rune script are laid out
    /// the same way. A source that will not parse comes back unchanged.
    ///
    /// # Errors
    /// If the source will not parse.
    pub fn format(&self, key: &str, source: &str) -> Result<String> {
        let mut sources = Sources::new();
        sources.insert(Source::new(key, source)?)?;
        let formatted = rune::fmt::prepare(&sources)
            .format()
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        Ok(formatted
            .into_iter()
            .next()
            .map_or_else(|| source.to_string(), |(_, text)| text.into_std()))
    }

    /// What is under the caret at `line`:`column`, for a hover card.
    ///
    /// # Errors
    /// If the context cannot be built.
    pub fn hover(
        &self,
        key: &str,
        source: &str,
        line: usize,
        column: usize,
    ) -> Result<Option<Hover>> {
        let offset = offset_of(source, line, column);
        // The caret may sit inside the word rather than after it, so the hover
        // reads the whole identifier the offset touches.
        let end = source[offset.min(source.len())..]
            .char_indices()
            .find(|(_, c)| !is_word(*c))
            .map_or(source.len(), |(i, _)| offset + i);
        let (name, start) = word_before(source, end);
        if name.is_empty() {
            return Ok(None);
        }
        Ok(self.describe(key, source, &source[..start], name))
    }

    /// The one thing `name` means, given what precedes it. The same four
    /// sources completion reads, asked for one answer instead of a list.
    fn describe(&self, key: &str, source: &str, head: &str, name: &str) -> Option<Hover> {
        let modules = collect_modules();
        if let Some(before) = head.strip_suffix("::") {
            let (module, _) = word_before(before, before.len());
            let found = modules.get(module)?;
            if let Some(value) = found.constants.get(name) {
                return Some(Hover {
                    title: format!("{module}::{name}"),
                    detail: value.clone(),
                    doc: String::new(),
                });
            }
            return Some(Hover {
                title: format!("{module}::{name}"),
                detail: signature_display(found.signatures.get(name)),
                doc: found.docs.get(name).cloned().unwrap_or_default(),
            });
        }
        if let Some(before) = head.strip_suffix('.') {
            let (receiver, receiver_start) = word_before(before, before.len());
            if receiver == "this" {
                return Some(Hover {
                    title: format!("this.{name}"),
                    detail: "this script".to_string(),
                    doc: String::new(),
                });
            }
            if !receiver.is_empty() && source[..receiver_start].ends_with('.') {
                let methods = handle_methods();
                if let Some((detail, doc)) = methods.get(receiver).and_then(|m| m.get(name)) {
                    return Some(Hover {
                        title: format!("{receiver}.{name}"),
                        detail: detail.clone(),
                        doc: doc.clone(),
                    });
                }
            }
            if let Some((generic, doc)) = GENERIC.iter().find(|(g, _)| *g == name) {
                return Some(Hover {
                    title: (*generic).to_string(),
                    detail: "component handle".to_string(),
                    doc: (*doc).to_string(),
                });
            }
            let node = modules.get("node")?;
            if node.functions.contains(name) {
                return Some(Hover {
                    title: format!("node.{name}"),
                    detail: signature_display(node.signatures.get(name)),
                    doc: node.docs.get(name).cloned().unwrap_or_default(),
                });
            }
            return None;
        }
        if let Some(module) = modules.get(name) {
            return Some(Hover {
                title: name.to_string(),
                detail: format!("{} functions", module.functions.len()),
                doc: module.doc.clone(),
            });
        }
        // A name with no qualifier is this file's own, or nothing we know.
        let declared = crate::inspect::public_functions(source)
            .into_iter()
            .find(|d| d.name == name)?;
        Some(Hover {
            title: format!("{}({})", declared.name, declared.arity),
            detail: format!("{key}:{}", declared.line),
            doc: String::new(),
        })
    }

    /// Where the name at the caret is defined.
    ///
    /// A `pub fn` in the project has a file and a line. Engine API has
    /// neither, so it carries the reference page's URL instead and the client
    /// opens that.
    ///
    /// # Errors
    /// If the context cannot be built.
    pub fn definition(
        &self,
        key: &str,
        source: &str,
        line: usize,
        column: usize,
    ) -> Result<Option<Location>> {
        let offset = offset_of(source, line, column);
        let end = source[offset.min(source.len())..]
            .char_indices()
            .find(|(_, c)| !is_word(*c))
            .map_or(source.len(), |(i, _)| offset + i);
        let (name, start) = word_before(source, end);
        if name.is_empty() {
            return Ok(None);
        }
        let head = &source[..start];
        // An engine module's own function: the reference page, not a file.
        if let Some(before) = head.strip_suffix("::") {
            let (module, _) = word_before(before, before.len());
            if collect_modules().contains_key(module) {
                return Ok(Some(Location {
                    file: String::new(),
                    line: 0,
                    column: 0,
                    url: reference_url(module),
                }));
            }
        }
        if collect_modules().contains_key(name) && !head.ends_with('.') {
            return Ok(Some(Location {
                file: String::new(),
                line: 0,
                column: 0,
                url: reference_url(name),
            }));
        }
        // This file's own, then every file its `mod` graph reaches.
        if let Some(found) = crate::inspect::public_functions(source)
            .into_iter()
            .find(|d| d.name == name)
        {
            return Ok(Some(Location {
                file: key.to_string(),
                line: found.line,
                column: 1,
                url: String::new(),
            }));
        }
        for rel in self.module_graph(key, source) {
            let Some(text) = self.source_of(&rel).ok() else {
                continue;
            };
            if let Some(found) = crate::inspect::public_functions(&text)
                .into_iter()
                .find(|d| d.name == name)
            {
                return Ok(Some(Location {
                    file: rel,
                    line: found.line,
                    column: 1,
                    url: String::new(),
                }));
            }
        }
        Ok(None)
    }

    /// What this file declares: its `pub fn`s and its `exports()` properties.
    ///
    /// # Errors
    /// Never; the signature matches the other verbs so one caller fits all.
    pub fn symbols(&self, key: &str, source: &str) -> Result<Vec<Symbol>> {
        let mut out = Vec::new();
        for declared in crate::inspect::public_functions(source) {
            out.push(Symbol {
                name: declared.name.clone(),
                kind: Kind::Function,
                detail: format!(
                    "{}({} arguments)",
                    if declared.is_async { "async " } else { "" },
                    declared.arity
                ),
                line: declared.line,
                column: 1,
            });
        }
        for (name, value) in self.exports(key).unwrap_or_default() {
            out.push(Symbol {
                name,
                kind: Kind::Property,
                detail: format!("{value:?}"),
                line: 0,
                column: 1,
            });
        }
        Ok(out)
    }

    /// Every place `name` appears as a whole word, across the files this
    /// file's `mod` graph reaches.
    ///
    /// Textual. Rune keeps no cross-file semantic index, so a match is a
    /// token match and the caller shows the list before writing anything.
    ///
    /// # Errors
    /// If the context cannot be built.
    pub fn references(&self, key: &str, source: &str, name: &str) -> Result<Vec<Location>> {
        let mut out = Vec::new();
        let mut files = vec![(key.to_string(), source.to_string())];
        for rel in self.module_graph(key, source) {
            if rel == key {
                continue;
            }
            if let Some(text) = self.source_of(&rel).ok() {
                files.push((rel, text));
            }
        }
        for (rel, text) in files {
            for (line, row) in text.split('\n').enumerate() {
                let mut from = 0;
                while let Some(hit) = row[from..].find(name) {
                    let at = from + hit;
                    let before = row[..at].chars().next_back().is_none_or(|c| !is_word(c));
                    let after = row[at + name.len()..]
                        .chars()
                        .next()
                        .is_none_or(|c| !is_word(c));
                    if before && after {
                        out.push(Location {
                            file: rel.clone(),
                            line: line + 1,
                            column: row[..at].chars().count() + 1,
                            url: String::new(),
                        });
                    }
                    from = at + name.len().max(1);
                }
            }
        }
        Ok(out)
    }

    /// Every file a rename would rewrite, as `(file, new source)`.
    ///
    /// Textual, like [`references`](Self::references): the caller shows the
    /// list before any of it is written. A name that is not an identifier is
    /// refused rather than producing a file that will not parse.
    ///
    /// # Errors
    /// If `to` is not a Rune identifier, or the context cannot be built.
    pub fn rename(
        &self,
        key: &str,
        source: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<(String, String)>> {
        if to.is_empty() || to.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            anyhow::bail!("`{to}` cannot start an identifier");
        }
        if !to.chars().all(is_word) {
            anyhow::bail!("`{to}` is not an identifier");
        }
        let found = self.references(key, source, from)?;
        let mut by_file: BTreeMap<String, Vec<Location>> = BTreeMap::new();
        for one in found {
            by_file.entry(one.file.clone()).or_default().push(one);
        }
        let mut out = Vec::new();
        for (file, hits) in by_file {
            let text = if file == key {
                source.to_string()
            } else {
                match self.source_of(&file) {
                    Ok(text) => text,
                    Err(_) => continue,
                }
            };
            let mut lines: Vec<String> = text.split('\n').map(ToString::to_string).collect();
            // Right to left, so an earlier hit's column still points at the
            // character it did before a longer name was written after it.
            let mut sorted = hits;
            sorted.sort_by(|a, b| (b.line, b.column).cmp(&(a.line, a.column)));
            for hit in sorted {
                let Some(line) = lines.get_mut(hit.line.saturating_sub(1)) else {
                    continue;
                };
                let at = offset_of(line, 1, hit.column);
                if !line[at..].starts_with(from) {
                    continue;
                }
                line.replace_range(at..at + from.len(), to);
            }
            out.push((file, lines.join("\n")));
        }
        Ok(out)
    }

    /// Every file this one's `mod` declarations reach, itself included.
    ///
    /// Textual and transitive: a `mod name;` names `name.rn` beside the file
    /// that declares it, which is how Rune resolves one.
    pub(crate) fn module_graph(&self, key: &str, source: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut queue = vec![(key.to_string(), source.to_string())];
        while let Some((rel, text)) = queue.pop() {
            if !seen.insert(rel.clone()) {
                continue;
            }
            let dir = rel.rsplit_once('/').map_or("", |(head, _)| head);
            for line in text.lines() {
                let trimmed = line.trim_start();
                let trimmed = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
                let Some(rest) = trimmed.strip_prefix("mod ") else {
                    continue;
                };
                let name: String = rest.chars().take_while(|c| is_word(*c)).collect();
                if name.is_empty() {
                    continue;
                }
                let child = if dir.is_empty() {
                    format!("{name}.rn")
                } else {
                    format!("{dir}/{name}.rn")
                };
                if seen.contains(&child) {
                    continue;
                }
                if let Some(found) = self.source_of(&child).ok() {
                    queue.push((child, found));
                }
            }
        }
        seen
    }

    /// The function being called at the caret, and which argument the caret
    /// is in: `(signature, active)`. Walks back over balanced parentheses, so
    /// a nested call reports the inner one.
    ///
    /// # Errors
    /// If the context cannot be built.
    pub fn signature_help(
        &self,
        key: &str,
        source: &str,
        line: usize,
        column: usize,
    ) -> Result<Option<(Hover, usize)>> {
        let offset = offset_of(source, line, column).min(source.len());
        let mut depth = 0usize;
        let mut active = 0usize;
        let mut open = None;
        for (i, c) in source[..offset].char_indices().rev() {
            match c {
                ')' => depth += 1,
                '(' if depth == 0 => {
                    open = Some(i);
                    break;
                }
                '(' => depth -= 1,
                ',' if depth == 0 => active += 1,
                _ => {}
            }
        }
        let Some(open) = open else { return Ok(None) };
        let (name, start) = word_before(source, open);
        if name.is_empty() {
            return Ok(None);
        }
        Ok(self
            .describe(key, source, &source[..start], name)
            .map(|found| (found, active)))
    }

    /// Every function the unit compiled for this source, `mod` files
    /// included. A source that will not compile has none, which is why the
    /// caller also reads the file's own `pub fn`s.
    pub(crate) fn unit_functions(&self, key: &str, source: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let Ok(Some(unit)) = self.build_unit(key, source) else {
            return out;
        };
        let Some(debug) = unit.debug_info() else {
            return out;
        };
        for signature in debug.functions.values() {
            if let Some(base) = signature.path.base_name() {
                out.insert(base.to_string());
            }
        }
        out
    }
}

/// Names bound by a `let` earlier in the file. Textual, so a name bound in
/// another function is offered too: over-offering beats a missing local.
fn locals_before(source: &str, prefix: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in source.lines() {
        let Some(rest) = line.trim_start().strip_prefix("let ") else {
            continue;
        };
        let name: String = rest.trim_start().chars().take_while(|c| is_word(*c)).collect();
        if !name.is_empty() && name.starts_with(prefix) {
            out.insert(name);
        }
    }
    out
}

/// One object per completion, for a script drawing the popup.
pub(crate) fn completion_rows(found: &[Completion]) -> Result<rune::Value> {
    let mut rows = Vec::with_capacity(found.len());
    for one in found {
        rows.push(row(&[
            ("label", rune::to_value(one.label.clone())?),
            ("kind", rune::to_value(one.kind.name())?),
            ("detail", rune::to_value(one.detail.clone())?),
            ("doc", rune::to_value(one.doc.clone())?),
            ("insert", rune::to_value(one.insert.clone())?),
        ])?);
    }
    Ok(rune::to_value(rows)?)
}

/// A hover as `#{ title, detail, doc }`, `()` when nothing is under the caret.
pub(crate) fn hover_row(found: Option<&Hover>) -> Result<rune::Value> {
    let Some(one) = found else {
        return Ok(rune::to_value(())?);
    };
    row(&[
        ("title", rune::to_value(one.title.clone())?),
        ("detail", rune::to_value(one.detail.clone())?),
        ("doc", rune::to_value(one.doc.clone())?),
    ])
}

/// One object per symbol, for the Outline dock.
pub(crate) fn symbol_rows(found: &[Symbol]) -> Result<rune::Value> {
    let mut rows = Vec::with_capacity(found.len());
    for one in found {
        rows.push(row(&[
            ("name", rune::to_value(one.name.clone())?),
            ("kind", rune::to_value(one.kind.name())?),
            ("detail", rune::to_value(one.detail.clone())?),
            ("line", rune::to_value(i64::try_from(one.line).unwrap_or(0))?),
            (
                "column",
                rune::to_value(i64::try_from(one.column).unwrap_or(0))?,
            ),
        ])?);
    }
    Ok(rune::to_value(rows)?)
}

/// A location as `#{ file, line, column, url }`, `()` when there is none.
pub(crate) fn location_row(found: Option<&Location>) -> Result<rune::Value> {
    let Some(one) = found else {
        return Ok(rune::to_value(())?);
    };
    row(&[
        ("file", rune::to_value(one.file.clone())?),
        ("line", rune::to_value(i64::try_from(one.line).unwrap_or(0))?),
        (
            "column",
            rune::to_value(i64::try_from(one.column).unwrap_or(0))?,
        ),
        ("url", rune::to_value(one.url.clone())?),
    ])
}

/// One object per location, for a references list.
pub(crate) fn location_rows(found: &[Location]) -> Result<rune::Value> {
    let mut rows = Vec::with_capacity(found.len());
    for one in found {
        rows.push(location_row(Some(one))?);
    }
    Ok(rune::to_value(rows)?)
}

fn row(fields: &[(&str, rune::Value)]) -> Result<rune::Value> {
    let mut object = rune::runtime::Object::new();
    for (key, value) in fields {
        object.insert(
            rune::alloc::String::try_from(*key)?,
            value.clone(),
        )?;
    }
    Ok(rune::to_value(object)?)
}

/// The reference page a module or one of its functions lives on. The site
/// lays the reference out one page per module, anchored by function name.
fn reference_url(module: &str) -> String {
    format!("https://balaurengine.org/docs/reference/modules/{module}")
}

/// A signature as it reads beside a name. The typed seam records
/// `(NodeId, f32) -> ()`, which follows the name directly; a raw registration
/// records only its types, and needs a separator or it runs into the name.
fn signature_display(signature: Option<&String>) -> String {
    let Some(signature) = signature else {
        return String::new();
    };
    if signature.starts_with('(') {
        signature.clone()
    } else {
        format!(": {signature}")
    }
}
