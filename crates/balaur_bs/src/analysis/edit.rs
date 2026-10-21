// The rest an editor asks of a file: what it declares, the call the cursor
// is in, where a name is used, a rename, and the file laid out.

use super::{Analysis, KEYWORDS, Kind, NATIVES, scan, scope, types};
use crate::ast::{ExprKind, Pos};

/// Something a file declares, for an outline. A script's methods are
/// functions, as the engine calls them; a `pub` field with a default is a
/// property, as a scene tunes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub kind: Kind,
    pub detail: String,
    pub pos: Pos,
}

fn is_name(s: &str) -> bool {
    s.chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !KEYWORDS.contains(&s)
}

// Braces and brackets a line opens less those it closes, outside strings and
// comments, and what it closes before anything else.
fn tally(line: &str) -> (i32, i32, i32, i32) {
    let lead_braces = line
        .chars()
        .take_while(|c| matches!(c, '}' | ')' | ']'))
        .filter(|c| *c == '}')
        .count();
    let lead_parens = line
        .chars()
        .take_while(|c| matches!(c, '}' | ')' | ']'))
        .filter(|c| *c != '}')
        .count();
    let (mut braces, mut parens, mut in_str, mut prev) = (0, 0, false, ' ');
    for c in line.chars() {
        if in_str {
            in_str = !(c == '"' && prev != '\\');
        } else if c == '/' && prev == '/' {
            break;
        } else {
            match c {
                '"' => in_str = true,
                '{' => braces += 1,
                '}' => braces -= 1,
                '(' | '[' => parens += 1,
                ')' | ']' => parens -= 1,
                _ => {}
            }
        }
        prev = if prev == '\\' && c == '\\' { ' ' } else { c };
    }
    (braces, parens, lead_braces as i32, lead_parens as i32)
}

impl Analysis {
    pub fn symbols(&self) -> Vec<Symbol> {
        let Ok(prog) = crate::parse::parse(&self.src) else {
            return vec![];
        };
        let mut out = vec![];
        for (name, fields) in &prog.structs {
            if let Some((_, pos)) = prog.decls.iter().find(|(n, _)| n == name) {
                out.push(Symbol {
                    name: name.clone(),
                    kind: Kind::Struct,
                    detail: format!("struct {name}"),
                    pos: *pos,
                });
            }
            for f in fields.iter().filter(|f| f.public && f.default.is_some()) {
                out.push(Symbol {
                    name: f.name.clone(),
                    kind: Kind::Property,
                    detail: crate::codegen::show(&f.ty),
                    pos: f.pos,
                });
            }
        }
        for f in &prog.fns {
            out.push(Symbol {
                name: f.name.clone(),
                kind: Kind::Function,
                detail: types::fn_signature(f),
                pos: f.pos,
            });
        }
        out.sort_by_key(|s| (s.pos.line, s.pos.col));
        out
    }

    /// The signature of the call at `line`, `col`, and which argument the
    /// cursor is on.
    pub fn signature(&self, line: u32, col: u32) -> Option<(String, usize)> {
        let at = scan::offset(&self.src, line, col);
        let (callee, active) = scan::call_at(&self.src, at)?;
        if callee.contains("::") {
            let f = self.host.function(&callee)?;
            return Some((types::host_signature(&callee, f, false), active));
        }
        let prog = self.patched(at, 0, &[")", "0)", ");", "0);"])?;
        let scope = scope::at(&prog, &self.host, Pos { line, col });
        let sig = match crate::parse::parse_expr(&callee).ok()?.kind {
            ExprKind::Field(base, name) => {
                let r = scope.resolve(&base)?;
                scope
                    .members(&r)
                    .into_iter()
                    .find(|c| c.label == name)?
                    .detail
            }
            ExprKind::Var(name) => {
                if let Some(f) = scope.free_fn(&name) {
                    types::fn_signature(f)
                } else if let Some(x) = prog.externs.iter().find(|x| x.name == name) {
                    let params: Vec<crate::Ty> = x.params.iter().map(|p| p.ty.clone()).collect();
                    types::signature(&name, &params, &x.ret)
                } else {
                    NATIVES.iter().find(|(n, _)| *n == name)?.1.to_string()
                }
            }
            _ => return None,
        };
        Some((sig, active))
    }

    /// Every use of `name` in the file, its declarations included: by name,
    /// as Rune's own references go, so a local and a field spelled alike are
    /// both found.
    pub fn references(&self, name: &str) -> Vec<Pos> {
        scan::occurrences(&self.src, name)
            .into_iter()
            .map(|at| {
                let (line, col) = scan::position(&self.src, at);
                Pos { line, col }
            })
            .collect()
    }

    /// The file with every use of `from` spelled `to`; `None` when `to` is no
    /// name or `from` is not used.
    pub fn rename(&self, from: &str, to: &str) -> Option<String> {
        if !is_name(to) || !is_name(from) {
            return None;
        }
        let found = scan::occurrences(&self.src, from);
        if found.is_empty() {
            return None;
        }
        let mut out = self.src.clone();
        for at in found.into_iter().rev() {
            out.replace_range(at..at + from.len(), to);
        }
        Some(out)
    }

    /// The file indented four spaces a level, a bracket's continuation one
    /// more, trailing space gone and one blank line kept between blocks.
    pub fn format(&self) -> String {
        let (mut out, mut braces, mut parens, mut blank) = (String::new(), 0i32, 0i32, false);
        for raw in self.src.lines() {
            let line = raw.trim();
            if line.is_empty() {
                if !blank && !out.is_empty() {
                    out.push('\n');
                }
                blank = true;
                continue;
            }
            blank = false;
            let (b, p, lead_b, lead_p) = tally(line);
            let mut level = braces - lead_b + i32::from(parens - lead_p > 0);
            if line.starts_with('.') {
                level += 1;
            }
            out.push_str(&"    ".repeat(level.max(0) as usize));
            out.push_str(line);
            out.push('\n');
            braces += b;
            parens += p;
        }
        while out.ends_with("\n\n") {
            out.pop();
        }
        out
    }
}
