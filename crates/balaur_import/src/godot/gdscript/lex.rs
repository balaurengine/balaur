//! GDScript as tokens. Its blocks are indentation, so the lexer owns the
//! indent stack and emits `Indent` and `Dedent` for the parser to nest on.

/// A token's payload. `Op` holds a static spelling so the parser compares by
/// pointer-free equality without allocating.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Tok {
    Newline,
    Indent,
    Dedent,
    Name(String),
    Int(String),
    Float(String),
    /// A string's decoded content, quotes and escapes already resolved.
    Str(String),
    /// `$Path/To/Node` or `$"quoted path"`.
    NodePath(String),
    /// `%UniqueName`, Godot's scene-unique lookup.
    Unique(String),
    /// `@export`, `@onready` and the rest, without the `@`.
    Annotation(String),
    Op(&'static str),
    Eof,
}

#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub kind: Tok,
    pub line: usize,
}

/// Longest first: `**=` must not lex as `**` then `=`.
const OPS: &[&str] = &[
    "**=", "<<=", ">>=", "**", "==", "!=", "<=", ">=", "&&", "||", "<<", ">>", "+=", "-=", "*=",
    "/=", "%=", "&=", "|=", "^=", ":=", "->", "+", "-", "*", "/", "%", "=", "<", ">", "!", "&",
    "|", "^", "~", ".", ",", ":", ";", "(", ")", "[", "]", "{", "}",
];

/// A `%` or `$` starts a node lookup only where a value is expected, which is
/// everywhere the previous token does not already end one.
fn ends_a_value(kind: &Tok) -> bool {
    matches!(
        kind,
        Tok::Name(_)
            | Tok::Int(_)
            | Tok::Float(_)
            | Tok::Str(_)
            | Tok::NodePath(_)
            | Tok::Unique(_)
            | Tok::Op(")" | "]" | "}")
    )
}

pub(crate) fn lex(source: &str) -> Result<Vec<Token>, String> {
    Lexer {
        out: Vec::new(),
        indents: vec![0],
        depth: 0,
        line: 0,
    }
    .run(source)
}

struct Lexer {
    out: Vec<Token>,
    indents: Vec<usize>,
    /// Open brackets: inside them a newline is whitespace and indentation is
    /// not a block.
    depth: usize,
    line: usize,
}

impl Lexer {
    fn run(mut self, source: &str) -> Result<Vec<Token>, String> {
        for (index, raw) in source.lines().enumerate() {
            self.line = index + 1;
            let line = raw.trim_end();
            if self.depth == 0 {
                let trimmed = line.trim_start();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                let width = line.len() - trimmed.len();
                self.indentation(width)?;
            }
            self.line_tokens(line)?;
            if self.depth == 0
                && !matches!(
                    self.out.last().map(|t| &t.kind),
                    None | Some(Tok::Newline | Tok::Indent)
                )
            {
                self.push(Tok::Newline);
            }
        }
        while self.indents.len() > 1 {
            self.indents.pop();
            self.push(Tok::Dedent);
        }
        self.push(Tok::Eof);
        Ok(self.out)
    }

    fn push(&mut self, kind: Tok) {
        let line = self.line;
        self.out.push(Token { kind, line });
    }

    /// One line's leading whitespace against the stack: deeper opens a block,
    /// shallower closes as many as it unwinds.
    fn indentation(&mut self, width: usize) -> Result<(), String> {
        let current = *self.indents.last().unwrap_or(&0);
        if width > current {
            self.indents.push(width);
            self.push(Tok::Indent);
            return Ok(());
        }
        while width < *self.indents.last().unwrap_or(&0) {
            self.indents.pop();
            self.push(Tok::Dedent);
        }
        if width != *self.indents.last().unwrap_or(&0) {
            return Err(format!("line {}: indentation matches no block", self.line));
        }
        Ok(())
    }

    fn line_tokens(&mut self, line: &str) -> Result<(), String> {
        let bytes: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i];
            if c == ' ' || c == '\t' {
                i += 1;
                continue;
            }
            if c == '#' {
                break;
            }
            // A trailing `\` joins the next source line; the caller feeds that
            // line straight after, so dropping the mark is the whole of it.
            if c == '\\' && bytes[i + 1..].iter().all(|c| c.is_whitespace()) {
                break;
            }
            if let Some(taken) = self.string(&bytes, i)? {
                i = taken;
                continue;
            }
            if c.is_ascii_digit()
                || (c == '.' && bytes.get(i + 1).is_some_and(char::is_ascii_digit))
            {
                i = self.number(&bytes, i);
                continue;
            }
            if c == '_' || c.is_alphabetic() {
                i = self.word(&bytes, i);
                continue;
            }
            if c == '@' {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && (bytes[end] == '_' || bytes[end].is_alphanumeric()) {
                    end += 1;
                }
                let name: String = bytes[start..end].iter().collect();
                self.push(Tok::Annotation(name));
                i = end;
                continue;
            }
            if c == '$' {
                i = self.node_path(&bytes, i)?;
                continue;
            }
            if c == '%' && !self.out.last().is_some_and(|t| ends_a_value(&t.kind)) {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && (bytes[end] == '_' || bytes[end].is_alphanumeric()) {
                    end += 1;
                }
                if end > start {
                    let name: String = bytes[start..end].iter().collect();
                    self.push(Tok::Unique(name));
                    i = end;
                    continue;
                }
            }
            let rest: String = bytes[i..].iter().collect();
            let Some(op) = OPS.iter().find(|op| rest.starts_with(**op)) else {
                return Err(format!("line {}: `{c}` means nothing here", self.line));
            };
            match *op {
                "(" | "[" | "{" => self.depth += 1,
                ")" | "]" | "}" => self.depth = self.depth.saturating_sub(1),
                _ => {}
            }
            self.push(Tok::Op(op));
            i += op.chars().count();
        }
        Ok(())
    }

    fn word(&mut self, bytes: &[char], start: usize) -> usize {
        let mut end = start;
        while end < bytes.len() && (bytes[end] == '_' || bytes[end].is_alphanumeric()) {
            end += 1;
        }
        let word: String = bytes[start..end].iter().collect();
        self.push(Tok::Name(word));
        end
    }

    fn number(&mut self, bytes: &[char], start: usize) -> usize {
        let mut end = start;
        let mut float = false;
        if bytes[start] == '0' && matches!(bytes.get(start + 1), Some('x' | 'X' | 'b' | 'B')) {
            end = start + 2;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == '_') {
                end += 1;
            }
            let text: String = bytes[start..end].iter().filter(|c| **c != '_').collect();
            self.push(Tok::Int(text));
            return end;
        }
        while end < bytes.len() {
            let c = bytes[end];
            if c.is_ascii_digit() || c == '_' {
                end += 1;
            } else if c == '.' && !float && bytes.get(end + 1).is_some_and(char::is_ascii_digit) {
                float = true;
                end += 1;
            } else if (c == 'e' || c == 'E')
                && bytes
                    .get(end + 1)
                    .is_some_and(|d| d.is_ascii_digit() || *d == '-' || *d == '+')
            {
                float = true;
                end += 2;
            } else {
                break;
            }
        }
        let text: String = bytes[start..end].iter().filter(|c| **c != '_').collect();
        self.push(if float {
            Tok::Float(text)
        } else {
            Tok::Int(text)
        });
        end
    }

    /// `$Node/Path`, `$"quoted path"` or `$%Unique`.
    fn node_path(&mut self, bytes: &[char], start: usize) -> Result<usize, String> {
        let mut i = start + 1;
        if bytes.get(i) == Some(&'"') || bytes.get(i) == Some(&'\'') {
            let quote = bytes[i];
            let mut end = i + 1;
            let mut text = String::new();
            while end < bytes.len() && bytes[end] != quote {
                text.push(bytes[end]);
                end += 1;
            }
            self.push(Tok::NodePath(text));
            return Ok(end + 1);
        }
        let mut text = String::new();
        while i < bytes.len()
            && (bytes[i] == '_' || bytes[i] == '/' || bytes[i] == '%' || bytes[i].is_alphanumeric())
        {
            text.push(bytes[i]);
            i += 1;
        }
        if text.is_empty() {
            return Err(format!("line {}: `$` names no node", self.line));
        }
        self.push(Tok::NodePath(text));
        Ok(i)
    }

    /// A string literal, with the `r`, `&` and `^` prefixes Godot allows. The
    /// token carries decoded text, so the emitter re-escapes for Rune rather
    /// than copying one language's escapes into another.
    fn string(&mut self, bytes: &[char], start: usize) -> Result<Option<usize>, String> {
        let mut i = start;
        let mut raw = false;
        if matches!(bytes[i], 'r' | 'R') && matches!(bytes.get(i + 1), Some('"' | '\'')) {
            raw = true;
            i += 1;
        } else if matches!(bytes[i], '&' | '^') && matches!(bytes.get(i + 1), Some('"' | '\'')) {
            i += 1;
        }
        let Some(quote) = bytes.get(i).copied().filter(|c| *c == '"' || *c == '\'') else {
            return Ok(None);
        };
        // A triple quote opens a block string; the game uses them for doc text
        // that fits one line, and an unterminated one is reported.
        let triple = bytes.get(i + 1) == Some(&quote) && bytes.get(i + 2) == Some(&quote);
        let close = if triple { 3 } else { 1 };
        i += close;
        let mut text = String::new();
        loop {
            let Some(c) = bytes.get(i).copied() else {
                return Err(format!("line {}: the string never closes", self.line));
            };
            if c == quote {
                let closed = (0..close).all(|n| bytes.get(i + n) == Some(&quote));
                if closed {
                    i += close;
                    break;
                }
            }
            if c == '\\' && !raw {
                i += 1;
                let Some(escape) = bytes.get(i).copied() else {
                    return Err(format!("line {}: the string never closes", self.line));
                };
                text.push(match escape {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    '0' => '\0',
                    'a' => '\u{7}',
                    'b' => '\u{8}',
                    'f' => '\u{c}',
                    'v' => '\u{b}',
                    'u' | 'U' => {
                        let width = if escape == 'u' { 4 } else { 8 };
                        let hex: String = bytes[i + 1..(i + 1 + width).min(bytes.len())]
                            .iter()
                            .collect();
                        let point = u32::from_str_radix(&hex, 16)
                            .ok()
                            .and_then(char::from_u32)
                            .ok_or_else(|| {
                                format!("line {}: `\\{escape}{hex}` is no character", self.line)
                            })?;
                        i += width + 1;
                        text.push(point);
                        continue;
                    }
                    other => other,
                });
                i += 1;
                continue;
            }
            text.push(c);
            i += 1;
        }
        self.push(Tok::Str(text));
        Ok(Some(i))
    }
}

#[cfg(test)]
mod tests {
    use super::{Tok, lex};

    fn kinds(source: &str) -> Vec<Tok> {
        lex(source).unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn a_block_opens_on_indent_and_closes_on_dedent() {
        let out = kinds("func a():\n\tvar x = 1\n\tif x:\n\t\tpass\nvar y = 2\n");
        let indents = out.iter().filter(|t| **t == Tok::Indent).count();
        let dedents = out.iter().filter(|t| **t == Tok::Dedent).count();
        assert_eq!((indents, dedents), (2, 2), "{out:?}");
    }

    #[test]
    fn a_newline_inside_brackets_is_whitespace() {
        let out = kinds("var a = [\n\t1,\n\t2,\n]\n");
        assert_eq!(
            out.iter().filter(|t| **t == Tok::Newline).count(),
            1,
            "{out:?}"
        );
        assert!(
            !out.contains(&Tok::Indent),
            "no block inside a list: {out:?}"
        );
    }

    #[test]
    fn percent_is_a_unique_node_only_where_a_value_starts() {
        assert!(kinds("var a = %Ship\n").contains(&Tok::Unique("Ship".into())));
        assert!(kinds("var a = b % c\n").contains(&Tok::Op("%")));
        assert!(kinds("var a = \"%s\" % [x]\n").contains(&Tok::Op("%")));
    }

    #[test]
    fn a_string_carries_decoded_text() {
        assert!(
            kinds(
                r#"var a = "a\tb\u0041"
"#
            )
            .contains(&Tok::Str("a\tbA".into()))
        );
        assert!(kinds("var a = 'single'\n").contains(&Tok::Str("single".into())));
    }

    #[test]
    fn a_node_path_is_one_token() {
        assert!(kinds("var a = $Ship/Mast\n").contains(&Tok::NodePath("Ship/Mast".into())));
        assert!(kinds("var a = $\"Ship 2\"\n").contains(&Tok::NodePath("Ship 2".into())));
    }

    #[test]
    fn numbers_keep_their_kind() {
        let out = kinds("var a = 1\nvar b = 2.5\nvar c = 0xff\nvar d = 1_000\nvar e = 1e3\n");
        assert!(out.contains(&Tok::Int("1".into())), "{out:?}");
        assert!(out.contains(&Tok::Float("2.5".into())));
        assert!(out.contains(&Tok::Int("0xff".into())));
        assert!(out.contains(&Tok::Int("1000".into())));
        assert!(out.contains(&Tok::Float("1e3".into())));
    }
}
