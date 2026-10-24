// The Rune source as text: lines, comments (which the AST drops) and string
// literals rewritten for `.bs`.

/// Byte offset to line, both ways.
pub(crate) struct Lines {
    starts: Vec<usize>,
}

impl Lines {
    pub(crate) fn new(src: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(src.match_indices('\n').map(|(i, _)| i + 1));
        Self { starts }
    }

    /// 1-based.
    pub(crate) fn line(&self, at: usize) -> u32 {
        (self.starts.partition_point(|s| *s <= at)) as u32
    }
}

/// A comment as written, with where it starts.
#[derive(Clone, Debug)]
pub(crate) struct Comment {
    pub at: usize,
    pub line: u32,
    pub text: String,
    /// Code precedes it on its line.
    pub trailing: bool,
}

/// Every `//` and `/* */` comment outside string and char literals.
pub(crate) fn comments(src: &str, lines: &Lines) -> Vec<Comment> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => i = skip_quoted(b, i, b'"'),
            b'`' => i = skip_quoted(b, i, b'`'),
            b'\'' => {
                // A char literal: 'x' or '\n' or '\u{1f}'; a label is not closed.
                if b.get(i + 1) == Some(&b'\\') {
                    let end = src[i + 2..].find('\'').map_or(b.len(), |e| i + 3 + e);
                    i = end;
                } else if let Some(c) = src[i + 1..].chars().next() {
                    let after = i + 1 + c.len_utf8();
                    i = if b.get(after) == Some(&b'\'') {
                        after + 1
                    } else {
                        i + 1
                    };
                } else {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                let end = src[i..].find('\n').map_or(b.len(), |e| i + e);
                out.push(Comment {
                    at: i,
                    line: lines.line(i),
                    text: src[i..end].trim_end().to_string(),
                    trailing: !line_before(src, i).trim().is_empty(),
                });
                i = end;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let end = src[i + 2..].find("*/").map_or(b.len(), |e| i + 4 + e);
                out.push(Comment {
                    at: i,
                    line: lines.line(i),
                    text: src[i..end].to_string(),
                    trailing: !line_before(src, i).trim().is_empty(),
                });
                i = end;
            }
            _ => i += 1,
        }
    }
    out
}

fn line_before(src: &str, at: usize) -> &str {
    let start = src[..at].rfind('\n').map_or(0, |s| s + 1);
    &src[start..at]
}

fn skip_quoted(b: &[u8], at: usize, q: u8) -> usize {
    let mut i = at + 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            c if c == q => return i + 1,
            _ => i += 1,
        }
    }
    b.len()
}

/// Lines of code: not blank, not only a comment.
pub(crate) fn code_lines(src: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let mut in_block = false;
    for (n, line) in src.lines().enumerate() {
        let t = line.trim();
        if in_block {
            if let Some(e) = t.find("*/") {
                in_block = false;
                if !t[e + 2..].trim().is_empty() {
                    out.push(n as u32 + 1);
                }
            }
            continue;
        }
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        if t.starts_with("/*") {
            in_block = !t.contains("*/");
            continue;
        }
        out.push(n as u32 + 1);
    }
    out
}

/// A Rune string literal's body (between its quotes, escapes as written) as
/// the body of a `.bs` one. `.bs` knows `\n \t \" \\` and reads `{` as the
/// start of a value, so braces double. Returns the text and what it could
/// not carry over.
pub(crate) fn bs_string_body(raw: &str, template: bool) -> (String, Option<String>) {
    let mut out = String::new();
    let mut problem = None;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => out.push_str("{{"),
            '}' => out.push_str("}}"),
            '\\' => match chars.next() {
                Some('n') => out.push_str("\\n"),
                Some('t') => out.push_str("\\t"),
                Some('"') => out.push_str("\\\""),
                Some('\\') => out.push_str("\\\\"),
                Some('\'') => out.push('\''),
                Some('`') if template => out.push('`'),
                Some('$') if template => out.push('$'),
                Some('0') => {
                    out.push('\u{0}');
                    problem = Some("`\\0`: .bs strings have no escape for it".to_string());
                }
                Some('r') => {
                    out.push('\r');
                    problem = Some("`\\r`: .bs strings have no escape for it".to_string());
                }
                Some('u') => {
                    let mut hex = String::new();
                    if chars.peek() == Some(&'{') {
                        chars.next();
                        for h in chars.by_ref() {
                            if h == '}' {
                                break;
                            }
                            hex.push(h);
                        }
                    }
                    match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        Some(ch) if !ch.is_control() => out.push(ch),
                        Some(ch) => {
                            out.push(ch);
                            problem = Some(format!(
                                "`\\u{{{hex}}}` written as the raw character: .bs strings have no `\\u` escape"
                            ));
                        }
                        None => {
                            problem = Some(format!("bad escape `\\u{{{hex}}}`"));
                        }
                    }
                }
                Some('x') => {
                    let hex: String = chars.by_ref().take(2).collect();
                    if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        out.push(ch);
                    }
                    problem = Some(format!("`\\x{hex}`: .bs strings have no `\\x` escape"));
                }
                Some('\n') => {
                    // A line continuation: Rune drops the newline and the
                    // indentation after it.
                    while chars.peek().is_some_and(|c| c.is_whitespace()) {
                        chars.next();
                    }
                }
                Some(o) => {
                    out.push(o);
                    problem = Some(format!("unknown escape `\\{o}`"));
                }
                None => {}
            },
            '\n' => {
                out.push_str("\\n");
            }
            '\r' => {}
            c => out.push(c),
        }
    }
    (out, problem)
}

/// `snake_case` or `kebab` to `PascalCase`.
pub(crate) fn pascal(s: &str) -> String {
    let mut out = String::new();
    let mut up = true;
    for c in s.chars() {
        if c == '_' || c == '-' || c == ' ' || c == ':' || c == '/' || c == '.' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, 'T');
    }
    out
}

/// `controls` → `control`, for an array's element. Plain English only.
pub(crate) fn singular(s: &str) -> String {
    if let Some(stem) = s.strip_suffix("ies") {
        return format!("{stem}y");
    }
    for suf in ["sses", "shes", "ches", "xes"] {
        if s.ends_with(suf) {
            return s[..s.len() - 2].to_string();
        }
    }
    if s.ends_with('s') && !s.ends_with("ss") && s.len() > 3 {
        return s[..s.len() - 1].to_string();
    }
    s.to_string()
}

/// Words `.bs` reserves that Rune lets a name be.
pub(crate) fn safe_ident(name: &str) -> String {
    match name {
        "fn" | "let" | "if" | "else" | "while" | "loop" | "for" | "in" | "return" | "break"
        | "continue" | "struct" | "impl" | "pub" | "extern" | "const" | "true" | "false"
        | "await" | "async" | "as" | "self" | "use" | "nil" | "int" | "float" | "bool" | "str"
        | "any" | "map" => format!("{name}_"),
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_double_their_braces_and_keep_known_escapes() {
        assert_eq!(
            bs_string_body(r#"a {b} \"c\"\n"#, false).0,
            r#"a {{b}} \"c\"\n"#
        );
        let (s, p) = bs_string_body(r"\u{1f}", false);
        assert_eq!(s, "\u{1f}");
        assert!(p.is_some());
    }

    #[test]
    fn comments_skip_strings_and_chars() {
        let src = "let a = \"// no\"; // yes\nlet c = '/'; /* also */\n";
        let c = comments(src, &Lines::new(src));
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].text, "// yes");
        assert!(c[0].trailing);
        assert_eq!(c[1].line, 2);
    }

    #[test]
    fn names() {
        assert_eq!(pascal("history_frame"), "HistoryFrame");
        assert_eq!(singular("controls"), "control");
        assert_eq!(singular("entries"), "entry");
        assert_eq!(singular("boxes"), "box");
    }
}
