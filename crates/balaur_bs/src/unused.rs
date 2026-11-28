//! Code nothing names: a private `fn` or `const` its file never names, a
//! `use` whose module the file never reaches, and a `pub` one nothing in the
//! project names.
//!
//! Read from the file's words, not its build: every name a token, a string
//! or a string's `{..}` spells counts as named, so a name that is only
//! shadowed or spelled in passing is kept. A declaration a comment
//! `// keep: <why>` stands above, or ends the line of, is kept too: what
//! names it is somewhere this check cannot see.

use std::collections::BTreeSet;

use crate::Error;
use crate::ast::{Pos, Program};
use crate::parse::{self, Tok};
use crate::vocabulary::words::{hooks, keywords, packages};

/// The comment that keeps a declaration nothing here names, with why.
pub(crate) const KEEP: &str = "// keep:";

const CONST: &str = "const";
const USE: &str = "use";
const PATH: &str = "::";

/// What one file declares `pub` or `internal`, and every name it spells:
/// the file's share of [`unused_exports`].
#[derive(Clone, Debug, Default)]
pub struct Exports {
    /// Each `pub` or `internal` function and constant nothing keeps, by the
    /// name written and where it is written. A method the engine calls by
    /// name is not one.
    pub declared: Vec<(String, Pos)>,
    /// Every name the file spells, but where it declares one.
    pub named: BTreeSet<String>,
}

/// The words of a file, read once.
struct Words {
    /// Every token, with `{..}` in a string read as the tokens it holds.
    toks: Vec<(Tok, Pos)>,
    /// Where a `fn` or a `const` writes its name: no use of it.
    declares: Vec<Pos>,
    /// The file's lines, for a `// keep:`.
    lines: Vec<String>,
}

impl Words {
    fn read(src: &str) -> Result<Self, Error> {
        let mut toks = vec![];
        spread(parse::lex(src, Pos { line: 1, col: 1 })?, &mut toks);
        let declares = toks
            .windows(2)
            .filter_map(|pair| match pair {
                [(Tok::Ident(word), _), (Tok::Ident(_), at)]
                    if word == keywords::FN || word == CONST =>
                {
                    Some(*at)
                }
                _ => None,
            })
            .collect();
        Ok(Words {
            toks,
            declares,
            lines: src.lines().map(str::to_string).collect(),
        })
    }

    /// The names spelled anywhere but where a `fn` or `const` declares one:
    /// a token, or a string that is a name whole.
    fn named(&self) -> BTreeSet<String> {
        self.toks
            .iter()
            .filter(|(_, at)| !self.declares.contains(at))
            .filter_map(|(tok, _)| match tok {
                Tok::Ident(word) => Some(word),
                Tok::Str(text, _) if is_name(text) => Some(text),
                _ => None,
            })
            // copy: the names outlive the tokens, as `Exports` keeps them
            .cloned()
            .collect()
    }

    /// The modules the file reaches: each name a `::` follows outside a `use`.
    fn reached(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut in_use = false;
        for (at, (tok, _)) in self.toks.iter().enumerate() {
            match tok {
                Tok::Ident(word) if word == USE && !in_use => in_use = true,
                Tok::P(";") => in_use = false,
                Tok::Ident(word) if !in_use => {
                    if matches!(self.toks.get(at + 1), Some((Tok::P(PATH), _))) {
                        out.insert(word.clone());
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Where the `fn` declared at `at` writes its name: the first name a
    /// `fn` writes on its line after it.
    fn fn_name(&self, at: Pos) -> Option<Pos> {
        self.declares
            .iter()
            .find(|pos| pos.line == at.line && pos.col > at.col)
            .copied()
    }

    /// Whether a `// keep: <why>` ends line `line` or stands among the
    /// comments right above it.
    fn kept(&self, line: u32) -> bool {
        let Some(own) = (line as usize)
            .checked_sub(1)
            .and_then(|at| self.lines.get(at))
        else {
            return false;
        };
        if keeps(own) {
            return true;
        }
        self.lines[..line as usize - 1]
            .iter()
            .rev()
            .map(|text| text.trim_start())
            .take_while(|text| text.starts_with("//"))
            .any(keeps)
    }
}

/// Whether `text` holds a `// keep:` that says why.
fn keeps(text: &str) -> bool {
    text.find(KEEP)
        .is_some_and(|at| !text[at + KEEP.len()..].trim().is_empty())
}

fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `toks`, each string followed by the tokens of each `{..}` in it.
fn spread(toks: Vec<(Tok, Pos)>, out: &mut Vec<(Tok, Pos)>) {
    for (tok, at) in toks {
        let inner = match &tok {
            Tok::Str(text, start) => interpolated(text, *start),
            _ => vec![],
        };
        out.push((tok, at));
        spread(inner, out);
    }
}

/// The tokens of each `{..}` in a string's text; `{{` is a brace.
fn interpolated(text: &str, start: Pos) -> Vec<(Tok, Pos)> {
    let mut out = vec![];
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'{' && b.get(i + 1) == Some(&b'{') {
            i += 2;
            continue;
        }
        if b[i] == b'{'
            && let Some(close) = parse::closing_brace(text, i)
        {
            let at = Pos {
                line: start.line,
                col: start.col + i as u32 + 1,
            };
            if let Ok(toks) = parse::lex(&text[i + 1..close], at) {
                out.extend(toks.into_iter().filter(|(tok, _)| *tok != Tok::Eof));
            }
            i = close + 1;
            continue;
        }
        i += 1;
    }
    out
}

/// A method the engine calls by its name on a node's script.
fn is_hook(name: &str) -> bool {
    [hooks::INIT, hooks::UPDATE, hooks::FIXED_UPDATE, hooks::SHOW].contains(&name)
        || name.starts_with(hooks::ON)
}

/// The private functions and constants `src` never names, and each `use`
/// whose module it never reaches, as warnings where each is written.
pub(crate) fn unused(src: &str) -> Vec<Error> {
    let (Ok(prog), Ok(words)) = (parse::parse(src), Words::read(src)) else {
        return vec![];
    };
    let named = words.named();
    let reached = words.reached();
    let mut found = vec![];
    for f in prog.fns.iter().filter(|f| !f.public && !f.internal) {
        let Some(at) = words.fn_name(f.pos) else {
            continue;
        };
        if !named.contains(&f.name) && !words.kept(at.line) {
            found.push(Error {
                pos: at,
                msg: format!(
                    "`{}` is never called: remove it, or say what calls it with `{KEEP} <why>` above it",
                    f.name
                ),
            });
        }
    }
    for (name, at) in private_consts(&prog) {
        if !named.contains(name) && !words.kept(at.line) {
            found.push(Error {
                pos: at,
                msg: format!(
                    "`{name}` is never read: remove it, or say what reads it with `{KEEP} <why>` above it"
                ),
            });
        }
    }
    for (path, at) in &prog.uses {
        let alias = path.rsplit(PATH).next().unwrap_or(path);
        if !reached.contains(alias) && !words.kept(at.line) {
            found.push(Error {
                pos: *at,
                msg: format!(
                    "`use {path};` is never reached: nothing in this file names `{alias}::`, so remove it"
                ),
            });
        }
    }
    found.sort_by_key(|e| (e.pos.line, e.pos.col));
    found
}

/// Each constant declared without `pub` or `internal`, and where its name is.
fn private_consts(prog: &Program) -> impl Iterator<Item = (&String, Pos)> {
    prog.consts.iter().filter_map(|(name, _)| {
        let private = prog
            .kept
            .iter()
            .any(|(kept, internal)| kept == name && !internal);
        let at = prog.decls.iter().find(|(declared, _)| declared == name)?.1;
        private.then_some((name, at))
    })
}

/// What `src` declares `pub` or `internal` and every name it spells, for
/// [`unused_exports`].
///
/// # Errors
/// A file that does not parse.
pub fn exports(src: &str) -> Result<Exports, Error> {
    let prog = parse::parse(src)?;
    let words = Words::read(src)?;
    let mut declared = vec![];
    for f in prog.fns.iter().filter(|f| f.public || f.internal) {
        if f.owner.is_some() && is_hook(&f.name) {
            continue;
        }
        if let Some(at) = words.fn_name(f.pos)
            && !words.kept(at.line)
        {
            declared.push((f.name.clone(), at));
        }
    }
    for (name, _) in &prog.consts {
        let private = prog
            .kept
            .iter()
            .any(|(kept, internal)| kept == name && !internal);
        let at = prog.decls.iter().find(|(declared, _)| declared == name);
        if let Some((_, at)) = at.filter(|_| !private)
            && !words.kept(at.line)
        {
            declared.push((name.clone(), *at));
        }
    }
    Ok(Exports {
        declared,
        named: words.named(),
    })
}

/// Each `pub` or `internal` function and constant no file of `scripts`
/// names, by the file that declares it: a name a script spells, or one a
/// data file of `data` spells as a string (`value = "<fn>"` in a scene's
/// `bindings`) counts. An addon's or a dependency's are its own API, which
/// another project may call, and are not held to it.
#[must_use]
pub fn unused_exports(scripts: &[(String, Exports)], data: &[String]) -> Vec<(String, Error)> {
    let named: BTreeSet<&str> = scripts
        .iter()
        .flat_map(|(_, exports)| exports.named.iter().map(String::as_str))
        .collect();
    let in_data = |name: &str| {
        let (double, single) = (format!("\"{name}\""), format!("'{name}'"));
        data.iter()
            .any(|text| text.contains(&double) || text.contains(&single))
    };
    let library = |rel: &str| {
        [packages::ADDONS, packages::DEPS].iter().any(|dir| {
            rel.strip_prefix(dir)
                .is_some_and(|rest| rest.starts_with('/'))
        })
    };
    let mut found = vec![];
    for (rel, exports) in scripts.iter().filter(|(rel, _)| !library(rel)) {
        for (name, at) in &exports.declared {
            if !named.contains(name.as_str()) && !in_data(name) {
                found.push((
                    rel.clone(),
                    Error {
                        pos: *at,
                        msg: format!(
                            "`{name}` is shared, `pub` or `internal`, but nothing in the project names it: no script calls it, no scene binds it, and the engine does not call it; remove it, or say what does with `{KEEP} <why>` above it"
                        ),
                    },
                ));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{Exports, exports, unused, unused_exports};

    /// Each warning's line and the name it is about.
    fn lines(src: &str) -> Vec<(u32, String)> {
        unused(src)
            .into_iter()
            .map(|e| {
                let name = e.msg.split('`').nth(1).unwrap_or_default().to_string();
                (e.pos.line, name)
            })
            .collect()
    }

    #[test]
    fn a_private_fn_or_const_nothing_names_is_reported_where_its_name_is() {
        let src = "const USED: int = 1;
const UNUSED: int = 2;

fn helper() -> int {
    USED
}

fn stray() -> int {
    3
}

pub fn run() -> int {
    helper()
}
";
        assert_eq!(
            lines(src),
            [(2, "UNUSED".to_string()), (8, "stray".to_string())]
        );
        let at = unused(src)[1].pos;
        assert_eq!((at.line, at.col), (8, 4), "the name, not `fn`");
    }

    #[test]
    fn a_name_in_a_string_or_a_string_s_braces_or_handed_on_counts() {
        let src = "const LABEL: str = \"hi\";

fn shown() -> str {
    \"x\"
}

fn by_name() {}

fn handed() {}

pub fn run() -> str {
    events::listen(handed);
    let call = \"by_name\";
    \"{LABEL} {shown()}\"
}
";
        assert!(lines(src).is_empty(), "{:?}", lines(src));
    }

    #[test]
    fn a_private_method_nothing_calls_is_reported_and_one_called_with_a_dot_is_not() {
        let src = "pub class Box {
    node: Node,
}

impl Box {
    pub fn init() {
        self.used();
    }

    fn used() {}

    fn stray() {}
}
";
        assert_eq!(lines(src), [(12, "stray".to_string())]);
    }

    #[test]
    fn a_use_nothing_reaches_is_reported_and_one_reached_in_a_type_or_a_string_is_not() {
        let src = "use stores::unused_store;
use stores::typed;
use stores::spelled;
use called;

pub fn run(card: typed::Card) -> str {
    called::go();
    \"{spelled::NAME}\"
}
";
        assert_eq!(lines(src), [(1, "use stores::unused_store;".to_string())]);
    }

    #[test]
    fn a_keep_comment_with_a_reason_keeps_a_declaration_and_one_without_does_not() {
        let src = "use stores::kept; // keep: linked for its class

/// What the test calls.
// keep: the store's tests call it by name
fn kept() {}

// keep:
fn bare() {}

const ALSO: int = 1; // keep: read by the web page
";
        assert_eq!(lines(src), [(8, "bare".to_string())]);
    }

    fn exported(rel: &str, src: &str) -> (String, Exports) {
        (rel.to_string(), exports(src).unwrap())
    }

    #[test]
    fn a_pub_item_nothing_in_the_project_names_is_reported_by_its_file() {
        let lib = "pub fn called() {}

pub fn bound() {}

pub fn stray() {}

pub const LIMIT: int = 3;

internal fn inside() {}

// keep: the server calls it by name
pub fn remote() {}
";
        let game = "use lib;

pub class Game {
    node: Node,
}

impl Game {
    pub fn init() {
        lib::called();
    }

    pub fn on_resize() {}

    pub fn clicked() {}
}

pub fn handlers() -> str {
    \"{lib::LIMIT}\"
}
";
        let scripts = [
            exported("scripts/lib.bs", lib),
            exported("scripts/game.bs", game),
            exported("addons/a/api.bs", "pub fn offered() {}\n"),
            exported(
                "scripts/menu.bs",
                "pub fn open() -> str {\n    \"clicked\"\n}\n",
            ),
        ];
        let scene = "[[nodes]]\nbindings = [{ action = \"call\", value = \"bound\" }]\n";
        let found: Vec<(String, u32, String)> = unused_exports(&scripts, &[scene.to_string()])
            .into_iter()
            .map(|(rel, e)| {
                (
                    rel,
                    e.pos.line,
                    e.msg.split('`').nth(1).unwrap().to_string(),
                )
            })
            .collect();
        let expected = [
            ("scripts/lib.bs", 5, "stray"),
            ("scripts/lib.bs", 9, "inside"),
            ("scripts/game.bs", 17, "handlers"),
            ("scripts/menu.bs", 1, "open"),
        ]
        .map(|(rel, line, name)| (rel.to_string(), line, name.to_string()));
        assert_eq!(found, expected);
    }
}
