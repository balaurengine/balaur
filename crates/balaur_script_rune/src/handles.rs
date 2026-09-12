//! What a component handle answers, and the check that a script's calls are
//! in it.
//!
//! One table, three readers: `value::component` builds the handle from it,
//! `tooling` completes from it, and [`check`] refuses a call that is not in
//! it. A second copy would let the checker and the run time disagree, which
//! is the one thing a checker may never do.
//!
//! What the pass can see is narrow on purpose. `node.body2d.apply_impulse()`
//! resolves its method by component name at call time, so the component has
//! to be a literal for any of this to be knowable: `this.node.<component>`,
//! and a local the script bound to `this.node`. Anything else — a component
//! off a variable, a node out of a loop — is the run time's answer still.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use balaur_core::Engine;

use crate::bindings::api_docs;
use crate::inspect::Finding;

/// An operation every handle answers whatever component it names, and the
/// node operation behind it.
pub(crate) struct Generic {
    pub(crate) method: &'static str,
    pub(crate) node_op: &'static str,
    pub(crate) doc: &'static str,
}

pub(crate) const GENERIC: &[Generic] = &[
    Generic {
        method: "get",
        node_op: "get_component",
        doc: "Read the component's properties, as an object.",
    },
    Generic {
        method: "set",
        node_op: "set_component",
        doc: "Give the node this component, with the given properties.",
    },
    Generic {
        method: "patch",
        node_op: "patch_component",
        doc: "Write some of the component's properties, leaving the rest.",
    },
    Generic {
        method: "has",
        node_op: "has_component",
        doc: "Whether the node carries this component.",
    },
    Generic {
        method: "remove",
        node_op: "remove_component",
        doc: "Take this component off the node.",
    },
];

/// Whether a name can be written after a dot, which is what a handle's field
/// and method names have to be.
pub(crate) fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Which module drives which component with which method: `method ->
/// component -> module`, from what every function declared it acts on.
pub(crate) fn drives() -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for entry in api_docs() {
        if entry.acts_on.is_empty() {
            continue;
        }
        if GENERIC.iter().any(|g| g.method == entry.name) {
            tracing::warn!(
                "{}::{} hides the handle's own `{}`",
                entry.module,
                entry.name,
                entry.name
            );
            continue;
        }
        let targets = out.entry(entry.name.clone()).or_default();
        for component in entry.acts_on {
            if targets
                .insert(component.clone(), entry.module.clone())
                .is_some()
            {
                tracing::warn!("two modules act on `{component}` with a `{}`", entry.name);
            }
        }
    }
    out
}

/// Every schema property and the components declaring it: `property ->
/// components`, which is the direction dispatch reads it in.
pub(crate) struct Properties {
    pub(crate) owners: BTreeMap<String, HashSet<String>>,
    /// The same, narrowed to where the schema says `vec3`.
    pub(crate) vectors: BTreeMap<String, HashSet<String>>,
}

pub(crate) fn properties(eng: &Engine) -> Properties {
    let mut owners: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    let mut vectors: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    for (component, schema) in balaur_core::components::schemas(eng) {
        let Some(table) = schema.as_table() else {
            continue;
        };
        for (prop, spec) in table {
            if !is_identifier(prop) {
                tracing::warn!("`{component}.{prop}` is not a script identifier; no field");
                continue;
            }
            owners
                .entry(prop.clone())
                .or_default()
                .insert(component.clone());
            if spec.get("type").and_then(|v| v.as_str()) == Some("vec3") {
                vectors
                    .entry(prop.clone())
                    .or_default()
                    .insert(component.clone());
            }
        }
    }
    Properties { owners, vectors }
}

/// Every handle call in `source` that the run time would refuse.
///
/// `carried` is what the scene says the nodes attaching this script hold;
/// `None` when no scene attaches it, and the node's components are unknown.
pub(crate) fn check(
    eng: &Engine,
    file: &str,
    source: &str,
    carried: Option<&BTreeSet<String>>,
) -> Vec<Finding> {
    let registered: BTreeSet<String> = balaur_core::components::schemas(eng)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    // A build with no components registered is a headless tool, not a game
    // whose scripts this can say anything about.
    if registered.is_empty() {
        return Vec::new();
    }
    let (tokens, strings) = lex(source);
    let driven = drives();
    let props = properties(eng);
    let mut found = Vec::new();
    for used in handle_uses(&tokens) {
        let (component, method) = (used.component.text, used.member.text);
        if !registered.contains(component) {
            found.push(warn(
                file,
                used.component,
                format!("`{component}` is not a component, so the node has no field of that name"),
            ));
            continue;
        }
        // A component the script adds itself is named as a string somewhere in
        // the file, and the scene is not the whole story about the node.
        if let Some(carried) = carried
            && !carried.contains(component)
            && !strings.contains(component)
        {
            found.push(warn(
                file,
                used.component,
                format!("no node this script is attached to has a `{component}`"),
            ));
            continue;
        }
        let is_property = props
            .owners
            .get(method)
            .is_some_and(|owners| owners.contains(component));
        if used.call {
            if driven
                .get(method)
                .is_some_and(|targets| targets.contains_key(component))
                || GENERIC.iter().any(|g| g.method == method)
            {
                continue;
            }
            let message = if is_property {
                format!("`{component}.{method}` is a property, not a method; drop the `()`")
            } else {
                format!("`{component}` has no `{method}`; no module driving it declares one")
            };
            found.push(warn(file, used.member, message));
        } else if !is_property {
            found.push(warn(
                file,
                used.member,
                format!("`{component}` has no property `{method}`"),
            ));
        }
    }
    found
}

fn warn(file: &str, at: &Token<'_>, message: String) -> Finding {
    Finding {
        file: file.to_string(),
        line: at.line,
        column: at.column,
        end_line: at.line,
        end_column: at.column + at.text.chars().count(),
        severity: "warning",
        message,
    }
}

/// One token the pass reads: a name, or one character of everything else.
struct Token<'a> {
    text: &'a str,
    /// 1-based, both, so a gutter and a caret can point at them.
    line: usize,
    column: usize,
    is_name: bool,
}

impl Token<'_> {
    fn name(&self, want: &str) -> bool {
        self.is_name && self.text == want
    }

    fn punct(&self, want: char) -> bool {
        !self.is_name && self.text.starts_with(want)
    }
}

/// One `<node>.<component>.<member>` a script wrote, and whether it called it.
struct Use<'a> {
    component: &'a Token<'a>,
    member: &'a Token<'a>,
    call: bool,
}

/// Every handle a script names off a node it bound to `this.node`.
fn handle_uses<'a>(tokens: &'a [Token<'a>]) -> Vec<Use<'a>> {
    let nodes = node_locals(tokens);
    let mut out = Vec::new();
    for at in 0..tokens.len() {
        if !names_a_node(tokens, at, &nodes) {
            continue;
        }
        let Some(window) = tokens.get(at + 1..at + 5) else {
            continue;
        };
        if !window[0].punct('.')
            || !window[1].is_name
            || !window[2].punct('.')
            || !window[3].is_name
        {
            continue;
        }
        out.push(Use {
            component: &window[1],
            member: &window[3],
            call: tokens.get(at + 5).is_some_and(|next| next.punct('(')),
        });
    }
    out
}

/// Whether the name at `at` is a node this pass can fold: `this.node`, or a
/// local the script bound to it.
fn names_a_node(tokens: &[Token<'_>], at: usize, locals: &HashSet<&str>) -> bool {
    let token = &tokens[at];
    if !token.is_name {
        return false;
    }
    if token.text == "node" && at >= 2 && tokens[at - 1].punct('.') && tokens[at - 2].name("this") {
        return true;
    }
    locals.contains(token.text) && !at_field(tokens, at)
}

/// Whether the name at `at` is being read off something else, in which case
/// it is that thing's field rather than the local of the same name.
fn at_field(tokens: &[Token<'_>], at: usize) -> bool {
    at > 0 && tokens[at - 1].punct('.')
}

/// The locals a script bound to `this.node` and never to anything else.
///
/// `let node = this.node;` is how a script that touches several components
/// is written, and folding it is what keeps the pass from seeing only the
/// one-liners.
fn node_locals<'a>(tokens: &'a [Token<'a>]) -> HashSet<&'a str> {
    let mut bound: HashSet<&str> = HashSet::new();
    let mut rebound: HashSet<&str> = HashSet::new();
    for (at, token) in tokens.iter().enumerate() {
        if !token.is_name || at_field(tokens, at) {
            continue;
        }
        // `<name> =`, and not `==`: an assignment, whatever declared the name.
        if !tokens.get(at + 1).is_some_and(|t| t.punct('='))
            || tokens.get(at + 2).is_some_and(|t| t.punct('='))
        {
            continue;
        }
        let is_node = tokens
            .get(at + 2..at + 5)
            .is_some_and(|w| w[0].name("this") && w[1].punct('.') && w[2].name("node"))
            && !tokens.get(at + 5).is_some_and(|t| t.punct('.'));
        if is_node && at > 0 && tokens[at - 1].name("let") {
            bound.insert(token.text);
        } else {
            rebound.insert(token.text);
        }
    }
    bound.retain(|name| !rebound.contains(name));
    bound
}

/// The tokens the pass matches on, and every string literal in the file.
///
/// Rune's own lexer is not public and its AST is a walk of every expression
/// kind; the pattern this pass folds is three tokens wide, so it reads them
/// itself. Comments and literals are skipped, so a call written inside a
/// string is not one.
fn lex(source: &str) -> (Vec<Token<'_>>, BTreeSet<&str>) {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut strings = BTreeSet::new();
    let (mut line, mut column, mut at) = (1usize, 1usize, 0usize);
    // Advancing over a slice, counting the lines and characters in it.
    macro_rules! step {
        ($to:expr) => {{
            let to = $to;
            for c in source[at..to].chars() {
                if c == '\n' {
                    line += 1;
                    column = 1;
                } else {
                    column += 1;
                }
            }
            at = to;
        }};
    }
    while at < bytes.len() {
        let c = bytes[at];
        if c.is_ascii_whitespace() {
            step!(at + 1);
            continue;
        }
        if source[at..].starts_with("//") {
            step!(source[at..].find('\n').map_or(bytes.len(), |end| at + end));
            continue;
        }
        if source[at..].starts_with("/*") {
            step!(
                source[at + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |end| at + 2 + end + 2)
            );
            continue;
        }
        if c == b'"' || c == b'`' || c == b'\'' {
            let quote = c as char;
            let (content, after) = closing(source, at + 1, quote);
            if quote == '"' {
                strings.insert(&source[at + 1..content]);
            }
            step!(after);
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let end = source[at..]
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                .map_or(bytes.len(), |len| at + len);
            tokens.push(Token {
                text: &source[at..end],
                line,
                column,
                is_name: true,
            });
            step!(end);
            continue;
        }
        if c.is_ascii_digit() {
            let end = source[at..]
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '_'))
                .map_or(bytes.len(), |len| at + len);
            step!(end);
            continue;
        }
        let end = at + source[at..].chars().next().map_or(1, char::len_utf8);
        tokens.push(Token {
            text: &source[at..end],
            line,
            column,
            is_name: false,
        });
        step!(end);
    }
    (tokens, strings)
}

/// Where the literal opened at `from` ends: the end of its content, and the
/// offset after its closing quote. An unclosed literal runs to end of file.
fn closing(source: &str, from: usize, quote: char) -> (usize, usize) {
    let mut chars = source[from..].char_indices();
    while let Some((offset, c)) = chars.next() {
        if c == '\\' {
            chars.next();
            continue;
        }
        if c == quote {
            return (from + offset, from + offset + c.len_utf8());
        }
    }
    (source.len(), source.len())
}
