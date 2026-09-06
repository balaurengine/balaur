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
            let signature = module.signatures.get(name).cloned().unwrap_or_default();
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
                detail: found.signatures.get(name).cloned().unwrap_or_default(),
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
                    detail: node.signatures.get(name).cloned().unwrap_or_default(),
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
