//! A class's enums and constants, as module items: a scalar literal is a
//! `const`, anything else a function computing it.

use std::collections::{BTreeMap, BTreeSet};

use super::render::Shape;
use super::{Context, assigned, expression, name_of, top_level};
use crate::godot::gdscript::{Bt, enum_member};

/// A GDScript `enum`. Named, each member is a constant `Name__MEMBER`, and
/// the name alone a function answering the members as a map, so
/// `StageState.IDLE` is a constant and `StageState.keys()` still reads;
/// unnamed, one constant per member.
pub(super) fn write_enums(items: &mut Vec<String>, source: &str, emitted: &mut BTreeSet<String>) {
    for (name, members) in enum_blocks(source) {
        if members.is_empty() || !emitted.insert(name.clone()) {
            continue;
        }
        if name.is_empty() {
            for (member, value) in &members {
                if emitted.insert(member.clone()) {
                    items.push(format!("pub const {member}: int = {value};"));
                }
            }
            continue;
        }
        let mut text = String::new();
        let mut table = String::from("map()");
        for (member, value) in &members {
            text.push_str(&format!(
                "pub const {}: int = {value};\n",
                enum_member(&name, member)
            ));
            table.push_str(&format!(".with(\"{member}\", {value})"));
        }
        text.push_str(&format!(
            "\n/// `{name}` as Godot's dictionary of its members.\npub fn {name}() -> any {{\n    {table}\n}}\n"
        ));
        items.push(text);
    }
}

/// Each named enum a file declares, with its members.
pub(super) fn enums(source: &str) -> BTreeMap<String, BTreeSet<String>> {
    enum_blocks(source)
        .into_iter()
        .filter(|(name, _)| !name.is_empty())
        .map(|(name, members)| (name, members.into_iter().map(|(m, _)| m).collect()))
        .collect()
}

// A file's `enum` blocks at column 0: each name, empty for an unnamed one,
// and its members' values.
fn enum_blocks(source: &str) -> Vec<(String, Vec<(String, i64)>)> {
    // A member's `## doc` may hold commas and braces, so comments go first.
    let text: String = source
        .replace('\t', " ")
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = Vec::new();
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
        out.push((name, members));
    }
    out
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

/// A class constant is a module constant here, so a body reads it bare; one
/// that is not a scalar literal is a function, read by calling it.
pub(super) fn write_constants(
    shape: &mut Shape,
    source: &str,
    context: &Context,
    emitted: &mut BTreeSet<String>,
    notes: &mut Vec<String>,
) {
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
        // A script preloaded under a name is that class's module, not a value.
        if super::preloaded_script_file(&line).is_some() {
            continue;
        }
        if let Some((kind, text)) = scalar_literal(value) {
            shape
                .items
                .push(format!("pub const {name}: {} = {text};", kind.text()));
            continue;
        }
        let Some((text, _)) = expression(value, context, true) else {
            notes.push(format!("constant `{name}` did not translate"));
            continue;
        };
        shape
            .items
            .push(format!("pub fn {name}() -> any {{\n    {text}\n}}"));
    }
}

/// A scalar literal, as Balaur Script spells it, with its type: what a
/// `const` or a field's default may be.
pub(super) fn scalar_literal(text: &str) -> Option<(Bt, String)> {
    let text = text.trim();
    if text == "true" || text == "false" {
        return Some((Bt::Bool, text.to_string()));
    }
    if let Some(inner) = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .filter(|t| !t.contains(['"', '\\']))
    {
        return Some((Bt::Str, crate::godot::gdscript::quoted(inner)));
    }
    let digits = text.strip_prefix('-').unwrap_or(text);
    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit() || c == '_') {
        let clean = text.replace('_', "");
        clean.parse::<i64>().ok()?;
        return Some((Bt::Int, clean));
    }
    let float = !digits.is_empty()
        && digits.contains('.')
        && digits
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '_');
    if float {
        let clean = text.replace('_', "");
        clean.parse::<f64>().ok()?;
        let clean = if clean.ends_with('.') {
            format!("{clean}0")
        } else if clean.starts_with('.') || clean.starts_with("-.") {
            clean.replacen('.', "0.", 1)
        } else {
            clean
        };
        return Some((Bt::Float, clean));
    }
    None
}
