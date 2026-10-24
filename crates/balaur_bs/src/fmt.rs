//! The layout `balaur fmt` and the editor's format key give a `.bs` file.
//!
//! The author's line breaks stay. Each line is indented by the brackets open
//! above it, the space between two tokens is the house's, a comment stays
//! where it was, and a run of blank lines becomes one. No token changes, and
//! a layout that would change one is refused rather than written.

use crate::parse::{PUNCT, lex_number, lex_string};
use crate::{Error, Pos};

const INDENT: &str = "    ";

/// Words after which `(` and `[` start a value rather than call or index
/// the one before.
const CONTROL: [&str; 9] = [
    "if", "else", "while", "for", "in", "return", "loop", "let", "await",
];

/// Operators that join two values: spaced on both sides, and a line that
/// starts with one continues the line above.
const BINARY: [&str; 19] = [
    "==", "!=", "<=", ">=", "&&", "||", "+=", "-=", "*=", "/=", "%=", "+", "-", "*", "/", "%", "=",
    "<", ">",
];

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Word,
    Num,
    Str,
    Punct,
    Comment,
}

struct Tok<'a> {
    kind: Kind,
    text: &'a str,
    /// Line breaks in the gap before it.
    breaks: usize,
    /// The gap itself, which a comment after code keeps.
    gap: &'a str,
}

impl Tok<'_> {
    fn is(&self, p: &str) -> bool {
        self.kind == Kind::Punct && self.text == p
    }

    fn opens(&self) -> bool {
        self.kind == Kind::Punct && matches!(self.text, "(" | "[" | "{")
    }

    fn closes(&self) -> bool {
        self.kind == Kind::Punct && matches!(self.text, ")" | "]" | "}")
    }

    fn binary(&self) -> bool {
        self.kind == Kind::Punct && BINARY.contains(&self.text)
    }

    /// Whether a value ends here, so a `-` after it subtracts and a `(` or
    /// `[` after it calls or indexes it.
    fn ends_value(&self) -> bool {
        match self.kind {
            Kind::Word => !CONTROL.contains(&self.text),
            Kind::Num | Kind::Str => true,
            Kind::Punct => matches!(self.text, ")" | "]" | "?"),
            Kind::Comment => false,
        }
    }
}

/// `src` laid out. An error where it does not scan, the compiler's message.
pub fn format(src: &str) -> Result<String, Error> {
    let toks = scan(src)?;
    let out = layout(&toks);
    let same = scan(&out)?
        .iter()
        .map(|t| t.text)
        .eq(toks.iter().map(|t| t.text));
    if !same {
        return Err(Error {
            pos: Pos { line: 1, col: 1 },
            msg: "internal: the layout would change a token, so the file is left as it is".into(),
        });
    }
    Ok(out)
}

fn layout(toks: &[Tok<'_>]) -> String {
    let mut out = String::new();
    // Each open bracket, and the indent of the line it opened on.
    let mut open: Vec<(&str, usize)> = vec![];
    // For each depth of brackets, the indent of the last line there that
    // began something rather than went on with it.
    let mut bases: Vec<Option<usize>> = vec![None];
    let (mut indent, mut depth, mut block_at) = (0usize, 0usize, 0usize);
    for (i, t) in toks.iter().enumerate() {
        let prev = i.checked_sub(1).map(|k| &toks[k]);
        let starts = prev.is_none() || t.breaks > 0;
        if let Some(p) = prev.filter(|_| starts) {
            out.push('\n');
            if t.breaks > 1 && !p.opens() && !t.closes() {
                out.push('\n');
            }
        }
        if starts {
            let last = last_code(&toks[..i]);
            depth = open.len();
            indent = if t.kind == Kind::Comment {
                // A comment sits where the code under it does, and inside a
                // block it closes rather than out with the closing bracket.
                match toks[i..].iter().find(|n| n.kind != Kind::Comment) {
                    Some(next) if !next.closes() => place(next, last, &open, bases[depth]).0,
                    _ => open.last().map_or(0, |(_, at)| at + 1),
                }
            } else {
                let (at, begins) = place(t, last, &open, bases[depth]);
                if begins {
                    bases[depth] = Some(at);
                }
                at
            };
            // A block opened at the end of a condition that went on over
            // lines is the statement's, not the last line's.
            block_at = bases[depth].unwrap_or(indent);
            for _ in 0..indent {
                out.push_str(INDENT);
            }
        } else if t.kind == Kind::Comment {
            out.push_str(if t.gap.is_empty() { " " } else { t.gap });
        } else if prev.is_some_and(|p| spaced(p, t, i.checked_sub(2).map(|k| &toks[k]))) {
            out.push(' ');
        }
        out.push_str(if t.kind == Kind::Comment {
            t.text.trim_end()
        } else {
            t.text
        });
        if t.opens() {
            let at = if t.is("{") && open.len() == depth {
                block_at
            } else {
                indent
            };
            open.push((t.text, at));
            bases.push(None);
        } else if t.closes() && open.pop().is_some() {
            bases.pop();
        }
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

// The last token on the lines so far that is not a comment.
fn last_code<'a, 'b>(toks: &'b [Tok<'a>]) -> Option<&'b Tok<'a>> {
    toks.iter().rev().find(|t| t.kind != Kind::Comment)
}

// A line's indent, and whether it begins something: one step inside its
// bracket, or one past the line it goes on with after `.` or an operator.
fn place(
    first: &Tok<'_>,
    last: Option<&Tok<'_>>,
    open: &[(&str, usize)],
    base: Option<usize>,
) -> (usize, bool) {
    if first.closes() {
        return (open.last().map_or(0, |(_, at)| *at), false);
    }
    let inside = open.last().map_or(0, |(_, at)| at + 1);
    let binary = first.binary() && last.is_some_and(Tok::ends_value);
    if last.is_some_and(Tok::binary) || first.is(".") || binary {
        return (base.map_or(inside, |at| at + 1), false);
    }
    (inside, true)
}

// Whether a space goes between `a` and `b` on one line; `before` is the
// token before `a`, which says whether a `-` or `&` there is a prefix.
fn spaced(a: &Tok<'_>, b: &Tok<'_>, before: Option<&Tok<'_>>) -> bool {
    let prefix =
        |t: &Tok<'_>| t.is("!") || t.is("&") || (t.is("-") && !before.is_some_and(Tok::ends_value));
    if a.is("(") || a.is("[") || prefix(a) {
        return false;
    }
    if b.is(")") || b.is("]") || b.is(",") || b.is(";") || b.is(":") || b.is("?") {
        return false;
    }
    if a.is("{") {
        return !b.is("}");
    }
    if b.is("}") {
        return true;
    }
    for p in [".", "::", ".."] {
        if a.is(p) || b.is(p) {
            return false;
        }
    }
    if b.is("(") || b.is("[") {
        // `fn(int) -> int` is a type: its parameters sit against it.
        let fn_type = a.kind == Kind::Word && a.text == "fn";
        return !(a.ends_value() || fn_type);
    }
    true
}

// Every token with the gap before it, comments among them.
fn scan(src: &str) -> Result<Vec<Tok<'_>>, Error> {
    let b = src.as_bytes();
    let (mut i, mut line, mut col) = (0usize, 1u32, 1u32);
    let mut out = Vec::new();
    let mut gap_start = 0usize;
    while i < b.len() {
        let c = b[i];
        if c == b'\n' {
            i += 1;
            line += 1;
            col = 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            col += 1;
            continue;
        }
        let pos = Pos { line, col };
        let start = i;
        let kind = if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            Kind::Comment
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            Kind::Word
        } else if c.is_ascii_digit() {
            i = lex_number(src, start, pos)?.1;
            Kind::Num
        } else if c == b'"' {
            i = lex_string(src, start, pos)?.1;
            Kind::Str
        } else if let Some(p) = PUNCT.iter().find(|p| src[i..].starts_with(**p)) {
            i += p.len();
            Kind::Punct
        } else {
            return Err(Error {
                pos,
                msg: format!("unexpected character `{}`", c as char),
            });
        };
        let gap = &src[gap_start..start];
        out.push(Tok {
            kind,
            text: &src[start..i],
            breaks: gap.matches('\n').count(),
            gap: gap.rsplit('\n').next().unwrap_or(""),
        });
        col += src[start..i].chars().count() as u32;
        gap_start = i;
    }
    Ok(out)
}
