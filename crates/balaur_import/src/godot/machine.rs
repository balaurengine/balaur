//! An `AnimationTree` whose root is an `AnimationNodeStateMachine`, as a
//! `state_machine` asset and the component that runs it.
//!
//! Godot's states are nodes of the tree, each an `AnimationNodeAnimation`
//! naming a clip; here a state is a name and the clip it plays. `Start` is not
//! a state here: the transition leaving it names the machine's `start`. A
//! state that is anything but one clip (a blend space, a nested machine) has
//! no equivalent and is reported.

use std::fmt::Write as _;

use balaur_plugin::toml;
use toml::Value as Toml;

use crate::godot::{Section, Value};
use crate::godot::nodes::Resources;

/// Godot's own entry and exit states.
const START: &str = "Start";
const END: &str = "End";

/// The machine file, and what would not carry.
pub(crate) struct Converted {
    pub toml: String,
    pub notes: Vec<String>,
}

/// Convert the state machine an AnimationTree's `tree_root` names, or `None`
/// when its root is not one.
pub(crate) fn convert(tree: &Section, res: &Resources<'_>) -> Option<Converted> {
    let root = tree.field("tree_root").and_then(|r| res.sub(r))?;
    if root.attr_str("type") != Some("AnimationNodeStateMachine") {
        return None;
    }
    let mut notes = Vec::new();
    let mut states = toml::Table::new();
    for (key, value) in &root.fields {
        let Some(name) = key
            .strip_prefix("states/")
            .and_then(|k| k.strip_suffix("/node"))
        else {
            continue;
        };
        let Some(node) = res.sub(value) else {
            continue;
        };
        if node.attr_str("type") != Some("AnimationNodeAnimation") {
            notes.push(format!(
                "state `{name}` is a {}, not one clip; it was dropped",
                node.attr_str("type").unwrap_or("node")
            ));
            continue;
        }
        let clip = node
            .field("animation")
            .and_then(Value::as_str)
            .unwrap_or(name);
        states.insert(name.to_string(), Toml::String(clip.to_string()));
    }
    let mut start = None;
    let mut transitions = Vec::new();
    let flat = root
        .field("transitions")
        .and_then(Value::as_array)
        .unwrap_or_default();
    for triple in flat.chunks(3) {
        let [from, to, transition] = triple else {
            continue;
        };
        let (Some(from), Some(to)) = (from.as_str(), to.as_str()) else {
            continue;
        };
        if from == START {
            start = Some(to.to_string());
            continue;
        }
        if to == END {
            notes.push(format!(
                "`{from}` -> End: the machine here has no end state; it stays in `{from}`"
            ));
            continue;
        }
        if !states.contains_key(from) || !states.contains_key(to) {
            continue;
        }
        let mut row = toml::Table::new();
        row.insert("from".into(), Toml::String(from.to_string()));
        row.insert("to".into(), Toml::String(to.to_string()));
        if let Some(section) = res.sub(transition) {
            describe(section, &mut row, &mut notes);
        }
        transitions.push(Toml::Table(row));
    }
    let mut document = toml::Table::new();
    document.insert("type".into(), Toml::String("state_machine".into()));
    if let Some(start) = start.filter(|s| states.contains_key(s)) {
        document.insert("start".into(), Toml::String(start));
    }
    document.insert("states".into(), Toml::Table(states));
    document.insert("transitions".into(), Toml::Array(transitions));
    let mut text = String::new();
    let _ = writeln!(
        text,
        "# Converted from a Godot AnimationTree by `balaur import`."
    );
    text.push_str(&toml::to_string(&Toml::Table(document)).ok()?);
    Some(Converted { toml: text, notes })
}

/// One `AnimationNodeStateMachineTransition`'s fade, advance, switch and
/// condition, at Godot's defaults where it leaves them out.
fn describe(section: &Section, row: &mut toml::Table, notes: &mut Vec<String>) {
    if let Some(fade) = section.field("xfade_time").and_then(Value::as_f64) {
        row.insert("fade".into(), Toml::Float(fade));
    }
    // Godot's enums: advance 0 disabled, 1 enabled, 2 auto; switch 0
    // immediate, 1 sync, 2 at the end.
    let advance = match section.field("advance_mode").and_then(Value::as_i64) {
        Some(0) => "disabled",
        Some(2) => "auto",
        _ => "enabled",
    };
    row.insert("advance".into(), Toml::String(advance.into()));
    let switch = match section.field("switch_mode").and_then(Value::as_i64) {
        Some(1) => "sync",
        Some(2) => "at_end",
        _ => "immediate",
    };
    row.insert("switch".into(), Toml::String(switch.into()));
    if let Some(condition) = section
        .field("advance_condition")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
    {
        row.insert("condition".into(), Toml::String(condition.to_string()));
    }
    if section
        .field("advance_expression")
        .and_then(Value::as_str)
        .is_some_and(|e| !e.is_empty())
    {
        notes.push(
            "a transition's `advance_expression` has no equivalent; set a condition from a script"
                .into(),
        );
    }
}
