// The text around the cursor: what is being typed, and what it hangs off.

/// Byte offset of 1-based `line`, `col` (a character count) in `src`.
pub(crate) fn offset(src: &str, line: u32, col: u32) -> usize {
    let mut at = 0;
    for (i, l) in src.split_inclusive('\n').enumerate() {
        if i + 1 == line as usize {
            let chars = (col.max(1) - 1) as usize;
            let within = l
                .char_indices()
                .nth(chars)
                .map_or(l.trim_end_matches('\n').len(), |(b, _)| b);
            return at + within;
        }
        at += l.len();
    }
    src.len()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// What the cursor is completing.
#[derive(Debug, PartialEq)]
pub(crate) enum Context {
    /// `module::par`: the module, then the part typed.
    Path(String, String),
    /// `receiver.par`: the receiver's text, then the part typed.
    Member(String, String),
    /// A bare name being typed.
    Name(String),
}

pub(crate) fn context(src: &str, at: usize) -> Context {
    let before = &src[..at];
    let partial: String = before.chars().rev().take_while(|c| is_word(*c)).collect();
    let partial: String = partial.chars().rev().collect();
    let rest = &before[..before.len() - partial.len()];
    if let Some(head) = rest.strip_suffix("::") {
        let module: String = head.chars().rev().take_while(|c| is_word(*c)).collect();
        return Context::Path(module.chars().rev().collect(), partial);
    }
    if let Some(head) = rest.strip_suffix('.') {
        return Context::Member(receiver(head).to_string(), partial);
    }
    Context::Name(partial)
}

/// The expression ending at the end of `head`: names, dots, calls and
/// indexes, with any brackets and strings inside them balanced.
pub(crate) fn receiver(head: &str) -> &str {
    let b = head.as_bytes();
    let mut i = b.len();
    let mut depth = 0i32;
    while i > 0 {
        let c = b[i - 1];
        match c {
            b')' | b']' => depth += 1,
            b'(' | b'[' if depth > 0 => depth -= 1,
            b'"' if depth > 0 => {
                // Back to the quote that opens this string.
                i -= 1;
                while i > 0 && !(b[i - 1] == b'"' && (i < 2 || b[i - 2] != b'\\')) {
                    i -= 1;
                }
            }
            _ if depth > 0 => {}
            b'.' | b'_' => {}
            _ if (c as char).is_alphanumeric() => {}
            b':' if i >= 2 && b[i - 2] == b':' => i -= 1,
            _ => break,
        }
        i -= 1;
    }
    head[i..].trim_start()
}

/// The word under the cursor, and where it starts.
pub(crate) fn word_at(src: &str, at: usize) -> Option<(String, usize)> {
    let start = src[..at]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map_or(at, |(i, _)| i);
    let end = src[at..]
        .char_indices()
        .find(|(_, c)| !is_word(*c))
        .map_or(src.len(), |(i, _)| at + i);
    (end > start).then(|| (src[start..end].to_string(), start))
}

/// 1-based line and column of byte offset `at`.
pub(crate) fn position(src: &str, at: usize) -> (u32, u32) {
    let before = &src[..at];
    let line = before.matches('\n').count() as u32 + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) as u32 + 1;
    (line, col)
}

/// The call the cursor sits in: the callee's text, and which argument.
pub(crate) fn call_at(src: &str, at: usize) -> Option<(String, usize)> {
    let b = &src.as_bytes()[..at];
    let (mut depth, mut commas, mut i) = (0i32, 0usize, b.len());
    while i > 0 {
        i -= 1;
        match b[i] {
            b')' | b']' => depth += 1,
            b'(' if depth == 0 => {
                let callee = receiver(&src[..i]);
                return (!callee.is_empty()).then(|| (callee.to_string(), commas));
            }
            b'(' | b'[' => depth -= 1,
            b',' if depth == 0 => commas += 1,
            b';' | b'{' | b'}' if depth == 0 => return None,
            b'"' => {
                while i > 0 && b[i - 1] != b'"' {
                    i -= 1;
                }
                i = i.saturating_sub(1);
            }
            _ => {}
        }
    }
    None
}

/// Every identifier in `src` spelled `word`, as byte offsets: in code and in
/// a string's `{..}`, not in a string's text or a comment.
pub(crate) fn occurrences(src: &str, word: &str) -> Vec<usize> {
    let b = src.as_bytes();
    let mut out = vec![];
    // Brace depth inside each open string; empty means code.
    let mut strings: Vec<u32> = vec![];
    let mut i = 0;
    while i < b.len() {
        let in_text = strings.last().is_some_and(|d| *d == 0);
        match b[i] {
            b'\\' if in_text => i += 2,
            b'"' if in_text => {
                strings.pop();
                i += 1;
            }
            b'{' if in_text && b.get(i + 1) == Some(&b'{') => i += 2,
            b'}' if in_text && b.get(i + 1) == Some(&b'}') => i += 2,
            b'{' if in_text => {
                if let Some(depth) = strings.last_mut() {
                    *depth = 1;
                }
                i += 1;
            }
            _ if in_text => i += 1,
            b'"' => {
                strings.push(0);
                i += 1;
            }
            b'{' | b'}' if !strings.is_empty() => {
                if let Some(depth) = strings.last_mut() {
                    *depth = if b[i] == b'{' {
                        *depth + 1
                    } else {
                        depth.saturating_sub(1)
                    };
                }
                i += 1;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            c if is_word(c as char) => {
                let start = i;
                while i < b.len() && is_word(b[i] as char) {
                    i += 1;
                }
                if &src[start..i] == word {
                    out.push(start);
                }
            }
            _ => i += 1,
        }
    }
    out
}
