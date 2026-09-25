//! An `AnimationTree` whose root is an `AnimationNodeStateMachine`, as a
//! `state_machine` asset and the component that runs it.
//!
//! Godot's states are nodes of the tree, each an `AnimationNodeAnimation`
//! naming a clip; here a state is a name and the clip it plays. `Start` is not
//! a state here: the transition leaving it names the machine's `start`, and
//! `End` is the machine's `end`. A nested state machine becomes a table of
//! its own under its state's name. A blend space or a blend tree has no
//! equivalent and is reported.

use std::fmt::Write as _;

use balaur::animation::machine::{GROUP_SEPARATOR, MACHINE_ASSET_TYPE};
use balaur::animation::{keys as k, words as w};
use balaur_plugin::toml;
use toml::Value as Toml;

use crate::godot::nodes::Resources;
use crate::godot::{Section, Value};

/// The key every asset document names its type under.
const TYPE: &str = "type";

/// Godot's own entry and exit states.
const START: &str = "Start";
const END: &str = "End";

/// The Godot node that holds a machine.
const TREE: &str = "AnimationTree";

/// The two Godot node types a state can be here.
const ANIMATION_NODE: &str = "AnimationNodeAnimation";
const STATE_MACHINE_NODE: &str = "AnimationNodeStateMachine";

/// How many points a drawn fade curve is sampled at, per segment.
const CURVE_STEPS: usize = 8;

/// The machine file, and what would not carry.
pub(crate) struct Converted {
    pub toml: String,
    pub notes: Vec<String>,
    /// Each `advance_expression`, as the `check` method a script answers it
    /// through and the expression that method returns.
    pub checks: Vec<(String, String)>,
}

/// Convert the state machine an AnimationTree's `tree_root` names, or `None`
/// when its root is not one.
pub(crate) fn convert(tree: &Section, res: &Resources<'_>) -> Option<Converted> {
    let root = tree.field("tree_root").and_then(|r| res.sub(r))?;
    if root.attr_str("type") != Some(STATE_MACHINE_NODE) {
        return None;
    }
    let mut out = Converted {
        toml: String::new(),
        notes: Vec::new(),
        checks: Vec::new(),
    };
    let mut document = toml::Table::new();
    document.insert(TYPE.into(), Toml::String(MACHINE_ASSET_TYPE.into()));
    document.extend(level(root, res, "", &mut out));
    let _ = writeln!(
        out.toml,
        "# Converted from a Godot AnimationTree by `balaur import`."
    );
    out.toml
        .push_str(&toml::to_string(&Toml::Table(document)).ok()?);
    Some(out)
}

/// The `check` method an `advance_expression` is answered through: named by
/// the expression, so a scene and the script it runs against agree on it.
pub(crate) fn check_name(expression: &str) -> String {
    let hash = expression.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("advance_check_{:08x}", hash & 0xffff_ffff)
}

/// Every scene's `advance_expression`s, by the project-relative script of
/// the node each is evaluated against. Read before any script is converted,
/// so the script converter can add the methods that answer them.
pub(crate) fn expression_scripts(
    root: &std::path::Path,
    files: &[String],
    project: &crate::godot::nodes::Project,
) -> std::collections::BTreeMap<String, Vec<(String, String)>> {
    let mut out: std::collections::BTreeMap<String, Vec<(String, String)>> =
        std::collections::BTreeMap::new();
    for scene in files
        .iter()
        .filter(|f| crate::godot::files::has_extension(f, "tscn"))
    {
        let Ok(text) = crate::godot::io::text(&root.join(scene)) else {
            continue;
        };
        // Most scenes have none, and a scene is only parsed when it might.
        if !text.contains("advance_expression") {
            continue;
        }
        let Ok(document) = crate::godot::parse(&text) else {
            continue;
        };
        let res = crate::godot::nodes::resources_of(&document, root, project);
        for tree in document
            .each("node")
            .filter(|s| s.attr_str("type") == Some(TREE))
        {
            let Some(converted) = convert(tree, &res) else {
                continue;
            };
            let Some(script) = base_script(&document, &res, tree) else {
                continue;
            };
            let known = out.entry(script).or_default();
            for check in converted.checks {
                if !known.contains(&check) {
                    known.push(check);
                }
            }
        }
    }
    out
}

/// The project-relative script of the node a tree evaluates its
/// expressions against, when that node carries one in this scene.
pub(crate) fn base_script(
    document: &crate::godot::Document,
    res: &Resources<'_>,
    tree: &Section,
) -> Option<String> {
    let base = tree
        .field("advance_expression_base_node")
        .and_then(crate::godot::anim::node_path)
        .unwrap_or_default();
    let tree_path = node_path_of(tree);
    let mut path: Vec<&str> = tree_path.split('/').filter(|s| !s.is_empty()).collect();
    for step in base.split('/').filter(|s| !s.is_empty() && *s != ".") {
        if step == ".." {
            path.pop()?;
        } else {
            path.push(step);
        }
    }
    let wanted = path.join("/");
    let node = document.each("node").find(|n| node_path_of(n) == wanted)?;
    let script = res.path(node.field("script")?)?;
    Some(crate::godot::relative_path(script).to_string())
}

/// A node section's path from the scene root, the root itself being empty.
fn node_path_of(node: &Section) -> String {
    let name = node.attr_str("name").unwrap_or_default();
    match node.attr_str("parent") {
        None => String::new(),
        Some(".") => name.to_string(),
        Some(parent) => format!("{parent}/{name}"),
    }
}

/// The GDScript functions that answer a script's checks, added to its
/// source before it is converted, so each expression is translated in the
/// class it was written against.
pub(crate) fn check_functions(checks: &[(String, String)]) -> String {
    let mut out = String::new();
    for (name, expression) in checks {
        let _ = write!(out, "\n\nfunc {name}():\n\treturn {expression}\n");
    }
    out
}

/// One `AnimationNodeStateMachine` as a machine table; `path` is where it
/// sits in the tree, empty for the root.
fn level(section: &Section, res: &Resources<'_>, path: &str, out: &mut Converted) -> toml::Table {
    let mut states = toml::Table::new();
    for (key, value) in &section.fields {
        let Some(name) = key
            .strip_prefix("states/")
            .and_then(|k| k.strip_suffix("/node"))
        else {
            continue;
        };
        let Some(node) = res.sub(value) else {
            continue;
        };
        match node.attr_str("type") {
            Some(ANIMATION_NODE) => {
                let clip = node
                    .field("animation")
                    .and_then(Value::as_str)
                    .unwrap_or(name);
                states.insert(name.to_string(), Toml::String(clip.to_string()));
            }
            Some(STATE_MACHINE_NODE) => {
                let inner = format!("{path}{name}{GROUP_SEPARATOR}");
                states.insert(name.to_string(), Toml::Table(level(node, res, &inner, out)));
            }
            other => out.notes.push(format!(
                "state `{path}{name}` is a {}, not one clip or a state machine; it was dropped",
                other.unwrap_or("node")
            )),
        }
    }
    let known = |name: &str| {
        states.contains_key(name)
            || name
                .split_once(GROUP_SEPARATOR)
                .is_some_and(|(group, _)| states.get(group).is_some_and(Toml::is_table))
    };
    let mut start = None;
    let mut transitions = Vec::new();
    let flat = section
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
        let to = if to == END {
            if !path.is_empty() {
                out.notes.push(format!(
                    "`{path}{from}` -> End: a nested machine is left by a transition from its state, not ended; dropped"
                ));
                continue;
            }
            w::END
        } else {
            to
        };
        if !known(from) || (to != w::END && !known(to)) {
            continue;
        }
        let mut row = toml::Table::new();
        row.insert(k::FROM.into(), Toml::String(from.to_string()));
        row.insert(k::TO.into(), Toml::String(to.to_string()));
        if let Some(section) = res.sub(transition) {
            describe(section, res, &mut row, out);
        }
        transitions.push(Toml::Table(row));
    }
    let mut table = toml::Table::new();
    if let Some(start) = start.filter(|s| known(s)) {
        table.insert(k::START.into(), Toml::String(start));
    }
    table.insert(k::STATES.into(), Toml::Table(states));
    table.insert(k::TRANSITIONS.into(), Toml::Array(transitions));
    table
}

/// One `AnimationNodeStateMachineTransition`'s fade, advance, switch,
/// condition, priority, reset and loop break, at Godot's defaults where it
/// leaves them out.
fn describe(section: &Section, res: &Resources<'_>, row: &mut toml::Table, out: &mut Converted) {
    if let Some(fade) = section.field("xfade_time").and_then(Value::as_f64) {
        row.insert(k::FADE.into(), Toml::Float(fade));
    }
    // Godot's enums: advance 0 disabled, 1 enabled, 2 auto; switch 0
    // immediate, 1 sync, 2 at the end.
    let advance = match section.field("advance_mode").and_then(Value::as_i64) {
        Some(0) => w::DISABLED,
        Some(2) => w::AUTO,
        _ => w::ENABLED,
    };
    row.insert(k::ADVANCE.into(), Toml::String(advance.into()));
    let switch = match section.field("switch_mode").and_then(Value::as_i64) {
        Some(1) => w::SYNC,
        Some(2) => w::AT_END,
        _ => w::IMMEDIATE,
    };
    row.insert(k::SWITCH.into(), Toml::String(switch.into()));
    if let Some(condition) = section
        .field("advance_condition")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
    {
        row.insert(k::CONDITION.into(), Toml::String(condition.to_string()));
    }
    if let Some(priority) = section.field("priority").and_then(Value::as_i64) {
        row.insert(k::PRIORITY.into(), Toml::Integer(priority));
    }
    for (godot, key) in [("reset", k::RESET), ("break_loop_at_end", k::BREAK_LOOP)] {
        if let Some(&Value::Bool(on)) = section.field(godot) {
            row.insert(key.into(), Toml::Boolean(on));
        }
    }
    if let Some(curve) = section.field("xfade_curve").and_then(|c| res.sub(c)) {
        match sampled(curve) {
            Some(points) => {
                row.insert(k::FADE_CURVE.into(), points);
            }
            None => out
                .notes
                .push("a transition's `xfade_curve` has no points; its fade is linear".into()),
        }
    }
    if let Some(expression) = section
        .field("advance_expression")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|e| !e.is_empty())
    {
        let name = check_name(expression);
        row.insert(k::CHECK.into(), Toml::String(name.clone()));
        if !out.checks.iter().any(|(known, _)| *known == name) {
            out.checks.push((name, expression.to_string()));
        }
    }
}

/// A Godot `Curve` as `[u, value]` points: each segment sampled on Godot's
/// own cubic, which runs straight in `u` and bends in `value` by the
/// tangents, and held flat out to 0 and 1.
fn sampled(curve: &Section) -> Option<Toml> {
    let data = curve.field("_data").and_then(Value::as_array)?;
    // Five values a point: the position, the two tangents, the two modes.
    let points: Vec<(f64, f64, f64, f64)> = data
        .chunks(5)
        .filter_map(|point| {
            let position = point.first()?.numbers()?;
            let tangent = |i: usize| point.get(i).and_then(Value::as_f64).unwrap_or(0.0);
            Some((
                *position.first()?,
                *position.get(1)?,
                tangent(1),
                tangent(2),
            ))
        })
        .collect();
    let (first, last) = (points.first()?, points.last()?);
    let mut out = vec![[0.0, first.1]];
    for pair in points.windows(2) {
        let ((ax, ay, _, right), (bx, by, left, _)) = (pair[0], pair[1]);
        let d = (bx - ax) / 3.0;
        let (c1, c2) = (ay + d * right, by - d * left);
        for step in 0..=CURVE_STEPS {
            let t = step as f64 / CURVE_STEPS as f64;
            let u = 1.0 - t;
            let y = u * u * u * ay + 3.0 * u * u * t * c1 + 3.0 * u * t * t * c2 + t * t * t * by;
            out.push([ax + (bx - ax) * t, y]);
        }
    }
    out.push([1.0, last.1]);
    // Rounded, and every `u` strictly after the one before.
    let mut kept: Vec<[f64; 2]> = Vec::new();
    for [u, y] in out {
        let (u, y) = ((u * 1e4).round() / 1e4, (y * 1e4).round() / 1e4);
        if kept.last().is_none_or(|last| u > last[0]) && (0.0..=1.0).contains(&u) {
            kept.push([u, y]);
        }
    }
    Some(Toml::Array(
        kept.into_iter()
            .map(|[u, y]| Toml::Array(vec![Toml::Float(u), Toml::Float(y)]))
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::godot::nodes::{Project, resources_of};

    /// A tree over a nested machine that ends, a drawn fade curve, an
    /// expression read off the character, and the machine's own `End`.
    const SCENE: &str = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://hero.gd" id="1_hero"]

[sub_resource type="AnimationNodeAnimation" id="Idle"]
animation = &"idle"

[sub_resource type="AnimationNodeAnimation" id="Walk"]
animation = &"walk"

[sub_resource type="AnimationNodeAnimation" id="Run"]
animation = &"run"

[sub_resource type="AnimationNodeStateMachineTransition" id="Enter"]
advance_mode = 2

[sub_resource type="AnimationNodeStateMachine" id="Move"]
states/walk/node = SubResource("Walk")
states/run/node = SubResource("Run")
transitions = ["Start", "walk", SubResource("Enter"), "walk", "run", SubResource("Enter"), "run", "End", SubResource("Enter")]

[sub_resource type="Curve" id="Soft"]
_data = [Vector2(0, 0), 0.0, 2.0, 0, 0, Vector2(1, 1), 0.0, 0.0, 0, 0]
point_count = 2

[sub_resource type="AnimationNodeStateMachineTransition" id="Go"]
advance_mode = 2
xfade_time = 0.3
xfade_curve = SubResource("Soft")
advance_expression = "velocity.length() > 1.0"

[sub_resource type="AnimationNodeStateMachineTransition" id="Stop"]
advance_mode = 2
advance_condition = &"dead"

[sub_resource type="AnimationNodeStateMachine" id="Root"]
states/idle/node = SubResource("Idle")
states/move/node = SubResource("Move")
transitions = ["Start", "idle", SubResource("Enter"), "idle", "move", SubResource("Go"), "move", "End", SubResource("Stop")]

[node name="Hero" type="CharacterBody2D"]
script = ExtResource("1_hero")

[node name="Tree" type="AnimationTree" parent="."]
tree_root = SubResource("Root")
advance_expression_base_node = NodePath("..")
"#;

    #[test]
    fn nesting_end_curves_and_expressions_carry() {
        let document = crate::godot::parse(SCENE).unwrap();
        let project = Project::default();
        let res = resources_of(&document, std::path::Path::new("."), &project);
        let tree = document
            .each("node")
            .find(|n| n.attr_str("type") == Some(TREE))
            .unwrap();
        let converted = convert(tree, &res).unwrap();
        let machine: Toml = toml::from_str(&converted.toml).unwrap();

        let nested = &machine["states"]["move"];
        assert_eq!(nested["start"].as_str(), Some("walk"));
        assert!(
            converted.notes.iter().any(|n| n.contains("move/run")),
            "a nested End is reported: {:?}",
            converted.notes
        );
        let transitions = machine["transitions"].as_array().unwrap();
        let go = &transitions[0];
        let name = check_name("velocity.length() > 1.0");
        assert_eq!(go["check"].as_str(), Some(name.as_str()));
        let curve = go["fade_curve"].as_array().unwrap();
        assert!(curve.len() > 4, "the curve is sampled: {curve:?}");
        let stop = &transitions[1];
        assert_eq!(stop["to"].as_str(), Some(w::END));

        let parsed = balaur::animation::machine::parse(&machine).unwrap();
        assert!(parsed.states.contains_key("move/walk"));
        assert_eq!(
            parsed.groups.get("move").map(String::as_str),
            Some("move/walk")
        );

        assert_eq!(
            base_script(&document, &res, tree).as_deref(),
            Some("hero.gd")
        );
        let source = format!(
            "extends CharacterBody2D\n{}",
            check_functions(&converted.checks)
        );
        let rune = crate::godot::script::convert(
            &source,
            "hero.gd",
            &crate::godot::exports::Classes::default(),
        )
        .rune;
        assert!(rune.contains(&format!("pub fn {name}(this)")), "{rune}");
    }
}
