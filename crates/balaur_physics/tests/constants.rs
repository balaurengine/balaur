//! The named constants, the runtime matchers and the scene-file schemas all
//! describe the same closed sets. The constants are built from the same words
//! the schemas are, so this reads the live registry to check that the words a
//! script can spell are the ones a component actually declares.

use balaur_core::components::ComponentRegistry;
use balaur_core::{App, AppConfig};
use balaur_physics::{
    AXES, AXES_2D, BODY_KINDS, COLLISION_PAIRS, COMBINE_RULES, CONSTANTS_2D, CONSTANTS_3D, EVENTS,
    FILL_MODES, FIT_MODES, JOINT_KINDS, JOINT_KINDS_2D, JOINT_SOLVERS, LENGTH_MODES, MOTOR_MODELS,
    MOTOR_MODES, PhysicsPlugin, SHAPE_KINDS, SHAPE_KINDS_2D,
};

/// The enum or flags options a registered component actually declares.
fn registered_options(component: &str, field: &str) -> Vec<String> {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    let registry = app.engine.resource::<ComponentRegistry>();
    let registry = registry.borrow();
    let def = registry
        .def(component)
        .unwrap_or_else(|| panic!("`{component}` is registered"));
    def.schema
        .get(field)
        .and_then(|f| f.get("options"))
        .and_then(toml::Value::as_array)
        .unwrap_or_else(|| panic!("`{component}.{field}` declares options"))
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

/// Read from the live registry, not from a copy of the schema literal: the
/// point is to catch the two drifting apart.
#[test]
fn every_constant_table_matches_the_registered_schema() {
    let tables: &[(&str, &[(&str, &str)], &str, &str)] = &[
        ("BODY_KINDS", BODY_KINDS, "body3d", "kind"),
        ("BODY_KINDS", BODY_KINDS, "body2d", "kind"),
        ("SHAPE_KINDS", SHAPE_KINDS, "collider3d", "kind"),
        ("SHAPE_KINDS_2D", SHAPE_KINDS_2D, "collider2d", "kind"),
        ("JOINT_KINDS", JOINT_KINDS, "joint3d", "kind"),
        ("JOINT_KINDS_2D", JOINT_KINDS_2D, "joint2d", "kind"),
        ("COMBINE_RULES", COMBINE_RULES, "collider3d", "friction_combine"),
        ("COMBINE_RULES", COMBINE_RULES, "collider2d", "restitution_combine"),
        ("MOTOR_MODES", MOTOR_MODES, "joint3d", "motor"),
        ("MOTOR_MODES", MOTOR_MODES, "joint2d", "motor"),
        ("MOTOR_MODELS", MOTOR_MODELS, "joint3d", "motor_model"),
        ("JOINT_SOLVERS", JOINT_SOLVERS, "joint2d", "solver"),
        ("LENGTH_MODES", LENGTH_MODES, "character3d", "lengths"),
        ("LENGTH_MODES", LENGTH_MODES, "character2d", "lengths"),
        ("FILL_MODES", FILL_MODES, "collider3d", "fill"),
        ("FIT_MODES", FIT_MODES, "collider3d", "fit"),
        ("EVENTS", EVENTS, "collider3d", "events"),
        ("COLLISION_PAIRS", COLLISION_PAIRS, "collider2d", "active_collisions"),
        ("AXES", AXES, "joint3d", "locked_axes"),
        ("AXES_2D", AXES_2D, "joint2d", "locked_axes"),
    ];
    for (table_name, table, component, field) in tables {
        let declared: Vec<&str> = table.iter().map(|(_, v)| *v).collect();
        assert_eq!(
            declared,
            registered_options(component, field),
            "{table_name} and the `{component}.{field}` options disagree"
        );
    }
}

#[test]
fn every_constant_is_screaming_snake_and_unique_in_its_world() {
    for (world, tables) in [("physics3d", CONSTANTS_3D), ("physics2d", CONSTANTS_2D)] {
        let mut seen = std::collections::BTreeSet::new();
        for (name, value) in tables.iter().flat_map(|t| t.iter()) {
            assert!(
                name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
                "{world}.{name} is not SCREAMING_SNAKE_CASE"
            );
            assert!(seen.insert(*name), "{world}.{name} is declared twice");
            assert!(!value.is_empty(), "{world}.{name} has no value");
        }
    }
}

/// A name both worlds spell means the same thing in each: `SHAPE_CAPSULE` is
/// a capsule in 2D and 3D alike, and a name that is not, such as
/// `SHAPE_BALL`, exists in one world only.
#[test]
fn a_name_the_two_worlds_share_has_one_meaning() {
    let three: std::collections::BTreeMap<&str, &str> =
        CONSTANTS_3D.iter().flat_map(|t| t.iter().copied()).collect();
    for (name, value) in CONSTANTS_2D.iter().flat_map(|t| t.iter()) {
        if let Some(other) = three.get(name) {
            assert_eq!(value, other, "{name} is `{value}` in 2D and `{other}` in 3D");
        }
    }
}
