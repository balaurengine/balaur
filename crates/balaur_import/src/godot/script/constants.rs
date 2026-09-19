//! A class's enums and constants, as module items: a value that stands on
//! its own is a `const`, anything else a function computing it.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use super::{Context, assigned, gdscript, name_of, shim_binding, top_level};

/// A GDScript `enum` as a module constant: named, an object whose fields are
/// its members, so `StageState.IDLE` reads unchanged; unnamed, one constant
/// per member.
pub(super) fn write_enums(out: &mut String, source: &str, emitted: &mut BTreeSet<String>) {
    // A member's `## doc` may hold commas and braces, so comments go first.
    let text: String = source
        .replace('\t', " ")
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    let mut rest = text.as_str();
    while let Some(at) = rest.find("enum ") {
        // Only a declaration at column 0 is the class's own.
        let starts_line = rest[..at].ends_with('\n') || at == 0;
        rest = &rest[at + 5..];
        if !starts_line {
            continue;
        }
        let Some(open) = rest.find('{') else { continue };
        let name = rest[..open].trim().to_string();
        let Some(close) = rest.find('}') else {
            continue;
        };
        let members = enum_members(&rest[open + 1..close]);
        rest = &rest[close + 1..];
        if members.is_empty() || !emitted.insert(name.clone()) {
            continue;
        }
        let entries: Vec<String> = members
            .iter()
            .map(|(name, value)| format!("\"{name}\": {value}"))
            .collect();
        out.push('\n');
        if name.is_empty() {
            for (member, value) in &members {
                if emitted.insert(member.clone()) {
                    let _ = writeln!(out, "pub const {member} = {value};");
                }
            }
            continue;
        }
        let _ = writeln!(out, "pub const {name} = #{{ {} }};", entries.join(", "));
    }
}

/// An enum's members and their values, numbered from zero where Godot left
/// them implicit.
pub(super) fn enum_members(body: &str) -> Vec<(String, i64)> {
    let mut out = Vec::new();
    let mut next = 0;
    for entry in body.split(',') {
        let entry = entry.split('#').next().unwrap_or_default().trim();
        if entry.is_empty() {
            continue;
        }
        let (name, value) = match entry.split_once('=') {
            // A value the scan cannot read keeps its position, so the enum is
            // still emitted and only that member is wrong.
            Some((name, value)) => (name.trim(), enum_value(value.trim()).unwrap_or(next)),
            None => (entry, next),
        };
        if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        next = value + 1;
        out.push((name.to_string(), value));
    }
    out
}

/// An enum member's value: an integer, or the `1 << n` its flags are written
/// as.
fn enum_value(text: &str) -> Option<i64> {
    if let Ok(value) = text.parse::<i64>() {
        return Some(value);
    }
    let (left, right) = text.split_once("<<")?;
    let left = left.trim().parse::<i64>().ok()?;
    let right = right.trim().parse::<u32>().ok()?;
    left.checked_shl(right)
}

/// A class constant is a module constant here, so a body reads it bare.
pub(super) fn write_constants(
    out: &mut String,
    source: &str,
    context: &Context,
    emitted: &mut BTreeSet<String>,
) {
    let mut wrote = false;
    for line in top_level(source) {
        let Some(rest) = line.strip_prefix("const ") else {
            continue;
        };
        let name = name_of(rest);
        let Some(value) = assigned(rest) else {
            continue;
        };
        if !emitted.insert(name.clone()) {
            continue;
        }
        let body = gdscript::body(
            &[format!("var _x = {value}")],
            context,
            0,
            &[],
            true,
            true,
            "",
        );
        let Some(text) = body
            .rune
            .trim()
            .strip_prefix("let _x = ")
            .and_then(|t| t.strip_suffix(';'))
        else {
            continue;
        };
        if !wrote {
            out.push('\n');
            wrote = true;
        }
        // A value that stands alone at load is a constant; one that needs the
        // shim or a name is a function, read by calling it. The declaration
        // scan judged the same text, so the two agree.
        if self_contained(value) {
            let _ = writeln!(out, "pub const {name} = {text};");
            continue;
        }
        let binding = if body.uses_shim {
            shim_binding(1)
        } else {
            String::new()
        };
        let _ = writeln!(out, "pub fn {name}() {{\n{binding}    {text}\n}}");
    }
}

/// Whether an expression stands on its own at load: no shim, which is bound
/// per body, and no name, which needs a node the engine does not have yet.
pub(super) fn self_contained(text: &str) -> bool {
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '\\' {
                chars.next();
            } else if c == '"' {
                quoted = false;
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            // A dictionary keyed by anything but strings, or empty, is the
            // shim's hash map, which a constant cannot build.
            '{' => {
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                if chars.peek() != Some(&'"') {
                    return false;
                }
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut word = String::from(c);
                while chars
                    .peek()
                    .is_some_and(|c| c.is_alphanumeric() || *c == '_')
                {
                    word.push(chars.next().unwrap_or_default());
                }
                // `true`, `false` and a numeric suffix are values; anything
                // else is a name this cannot resolve at load.
                if !matches!(word.as_str(), "true" | "false" | "f64" | "i64") {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}
