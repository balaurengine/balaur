//! The named constants, the runtime matchers and the scene-file schemas all
//! describe the same closed sets. The constants are built from the same words
//! the schemas are, so this reads the live registry to check that the words a
//! script can spell are the ones a component actually declares.

use balaur_core::components::ComponentRegistry;
use balaur_core::{App, AppConfig};
use balaur_physics::{
    AXES, AXES_2D, BODY_KINDS, CELL_MODELS, COLLISION_PAIRS, COMBINE_RULES, CONSTANTS_2D,
    CONSTANTS_3D, DECOMPOSITION_METHODS, DECOMPOSITION_METHODS_2D, EDGE_MODES, EDGE_MODES_2D,
    EVENTS, FILL_MODES, FIT_MODES, FIT_MODES_2D, IGNORES, JOINT_KINDS, JOINT_KINDS_2D,
    LENGTH_MODES, MOTOR_MODELS, MOTOR_MODES, ORIENTATIONS, PLASTIC_FLOWS, PhysicsPlugin,
    ROTATION_AXES, ROTATION_AXES_2D, SHAPE_KINDS, SHAPE_KINDS_2D, SHAPE_MATCHING_MODES, SOFT_KINDS,
    SOFT_KINDS_2D, SOFT_SOLVERS, TENSION_MODES, TEST_MODES, TILE_FIT_MODES_2D,
};

/// The enum or flags options a registered component actually declares. A
/// dotted `field` reaches into a list of records: `axes.motor`.
fn registered_options(component: &str, field: &str) -> Vec<String> {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    let registry = app.engine.resource::<ComponentRegistry>();
    let registry = registry.borrow();
    let def = registry
        .def(component)
        .unwrap_or_else(|| panic!("`{component}` is registered"));
    let mut path = field.split('.');
    let first = def.schema.get(path.next().unwrap_or_default());
    let spec = path.fold(first, |spec, name| {
        spec.and_then(|s| s.get("of"))
            .and_then(|of| of.get("fields"))
            .and_then(|fields| fields.get(name))
    });
    spec.and_then(|f| f.get("options"))
        .and_then(toml::Value::as_array)
        .unwrap_or_else(|| panic!("`{component}.{field}` declares options"))
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

/// One constant table under test: its name, its entries, and the component
/// property whose schema has to agree with it.
type TableCase = (
    &'static str,
    &'static [(&'static str, &'static str)],
    &'static str,
    &'static str,
);

/// Read from the live registry, not from a copy of the schema literal: the
/// point is to catch the two drifting apart.
#[test]
fn every_constant_table_matches_the_registered_schema() {
    let tables: &[TableCase] = &[
        ("BODY_KINDS", BODY_KINDS, "body3d", "kind"),
        ("BODY_KINDS", BODY_KINDS, "body2d", "kind"),
        ("SHAPE_KINDS", SHAPE_KINDS, "collider3d", "kind"),
        ("SHAPE_KINDS_2D", SHAPE_KINDS_2D, "collider2d", "kind"),
        ("JOINT_KINDS", JOINT_KINDS, "joint3d", "kind"),
        ("JOINT_KINDS_2D", JOINT_KINDS_2D, "joint2d", "kind"),
        (
            "COMBINE_RULES",
            COMBINE_RULES,
            "collider3d",
            "friction_combine",
        ),
        (
            "COMBINE_RULES",
            COMBINE_RULES,
            "collider2d",
            "restitution_combine",
        ),
        ("MOTOR_MODES", MOTOR_MODES, "joint3d", "axes.motor"),
        ("MOTOR_MODES", MOTOR_MODES, "joint2d", "axes.motor"),
        ("MOTOR_MODELS", MOTOR_MODELS, "joint3d", "axes.motor_model"),
        ("ORIENTATIONS", ORIENTATIONS, "softbody3d", "orientation"),
        ("ORIENTATIONS", ORIENTATIONS, "softbody2d", "orientation"),
        (
            "SHAPE_MATCHING_MODES",
            SHAPE_MATCHING_MODES,
            "softbody3d",
            "shape_matching",
        ),
        ("TENSION_MODES", TENSION_MODES, "softbody3d", "tension_only"),
        ("TENSION_MODES", TENSION_MODES, "softbody2d", "tension_only"),
        ("IGNORES", IGNORES, "character3d", "ignore"),
        ("IGNORES", IGNORES, "character2d", "ignore"),
        ("IGNORES", IGNORES, "vehicle3d", "ignore"),
        (
            "LENGTH_MODES",
            LENGTH_MODES,
            "character3d",
            "safe_margin_lengths",
        ),
        (
            "LENGTH_MODES",
            LENGTH_MODES,
            "character2d",
            "floor_snap_lengths",
        ),
        ("FILL_MODES", FILL_MODES, "collider3d", "fill"),
        ("FILL_MODES", FILL_MODES, "collider2d", "fill"),
        ("FIT_MODES", FIT_MODES, "collider3d", "fit"),
        ("FIT_MODES", FIT_MODES, "tile_collision", "fit"),
        ("FIT_MODES_2D", FIT_MODES_2D, "collider2d", "fit"),
        (
            "DECOMPOSITION_METHODS",
            DECOMPOSITION_METHODS,
            "collider3d",
            "method",
        ),
        (
            "DECOMPOSITION_METHODS_2D",
            DECOMPOSITION_METHODS_2D,
            "collider2d",
            "method",
        ),
        ("TEST_MODES", TEST_MODES, "collider3d", "collision_test"),
        ("TEST_MODES", TEST_MODES, "collider2d", "solver_test"),
        ("EDGE_MODES", EDGE_MODES, "collider3d", "edges"),
        ("EDGE_MODES_2D", EDGE_MODES_2D, "collider2d", "edges"),
        ("AXES", AXES, "collider3d", "up_axis"),
        ("AXES_2D", AXES_2D, "collider2d", "up_axis"),
        ("EVENTS", EVENTS, "collider3d", "events"),
        (
            "COLLISION_PAIRS",
            COLLISION_PAIRS,
            "collider2d",
            "contact_pairs",
        ),
        ("AXES", AXES, "joint3d", "lock_rotation"),
        ("AXES_2D", AXES_2D, "joint2d", "lock_translation"),
        ("SOFT_KINDS", SOFT_KINDS, "softbody3d", "kind"),
        ("SOFT_KINDS_2D", SOFT_KINDS_2D, "softbody2d", "kind"),
        ("SOFT_SOLVERS", SOFT_SOLVERS, "softbody3d", "solver"),
        ("SOFT_SOLVERS", SOFT_SOLVERS, "softbody2d", "solver"),
        ("CELL_MODELS", CELL_MODELS, "softbody3d", "cell_model"),
        (
            "PLASTIC_FLOWS",
            PLASTIC_FLOWS,
            "softbody2d",
            "edge_plastic_flow",
        ),
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

/// A joint call's axis is one of the words an `axes` record takes, spelled by
/// the linear axis constants and the rotation ones together.
#[test]
fn the_axis_constants_spell_every_joint_axis() {
    for (component, linear, turns) in [
        ("joint3d", AXES, ROTATION_AXES),
        ("joint2d", AXES_2D, ROTATION_AXES_2D),
    ] {
        let declared: Vec<&str> = linear.iter().chain(turns).map(|(_, v)| *v).collect();
        assert_eq!(declared, registered_options(component, "axes.axis"));
    }
}

/// physics2d spells every fit a tile's polygon takes, the one a 2D collider
/// cannot among them.
#[test]
fn the_2d_fit_constants_spell_every_tile_fit() {
    let declared: Vec<&str> = FIT_MODES_2D
        .iter()
        .chain(TILE_FIT_MODES_2D)
        .map(|(_, v)| *v)
        .collect();
    assert_eq!(declared, registered_options("tile_collision", "fit"));
}

#[test]
fn every_constant_is_screaming_snake_and_unique_in_its_world() {
    for (world, tables) in [("physics3d", CONSTANTS_3D), ("physics2d", CONSTANTS_2D)] {
        let mut seen = std::collections::BTreeSet::new();
        for (name, value) in tables.iter().flat_map(|t| t.iter()) {
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
                "{world}.{name} is not SCREAMING_SNAKE_CASE"
            );
            assert!(seen.insert(*name), "{world}.{name} is declared twice");
            assert!(!value.is_empty(), "{world}.{name} has no value");
        }
    }
}

/// A name both worlds spell means the same thing in each: `SHAPE_CAPSULE` is
/// a capsule in 2D and 3D alike, and a name that is not, such as
/// `SHAPE_SPHERE`, exists in one world only.
#[test]
fn a_name_the_two_worlds_share_has_one_meaning() {
    let three: std::collections::BTreeMap<&str, &str> = CONSTANTS_3D
        .iter()
        .flat_map(|t| t.iter().copied())
        .collect();
    for (name, value) in CONSTANTS_2D.iter().flat_map(|t| t.iter()) {
        if let Some(other) = three.get(name) {
            assert_eq!(
                value, other,
                "{name} is `{value}` in 2D and `{other}` in 3D"
            );
        }
    }
}

/// Every enum or flags property of one world's components, with the words
/// it offers, a record's fields inside a list included: the layer numbers
/// aside, which are numbers and not words.
fn offered_words(dimension: &str) -> Vec<(String, std::collections::BTreeSet<String>)> {
    fn walk(
        spec: &toml::Value,
        at: &str,
        out: &mut Vec<(String, std::collections::BTreeSet<String>)>,
    ) {
        if let Some(options) = spec.get("options").and_then(toml::Value::as_array) {
            let words: std::collections::BTreeSet<String> = options
                .iter()
                .filter_map(toml::Value::as_str)
                .filter(|word| word.parse::<u32>().is_err())
                .map(str::to_string)
                .collect();
            if !words.is_empty() {
                out.push((at.to_string(), words));
            }
        }
        if let Some(of) = spec.get("of") {
            walk(of, at, out);
        }
        if let Some(fields) = spec.get("fields").and_then(toml::Value::as_table) {
            for (name, field) in fields {
                walk(field, &format!("{at}.{name}"), out);
            }
        }
    }
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    let registry = app.engine.resource::<ComponentRegistry>();
    let registry = registry.borrow();
    let mut out = Vec::new();
    for (name, def) in registry.iter() {
        if !def.tags.contains(&balaur_core::components::tag::PHYSICS)
            || !def.tags.contains(&dimension)
        {
            continue;
        }
        for (key, spec) in def.schema.as_table().into_iter().flatten() {
            walk(spec, &format!("{name}.{key}"), &mut out);
        }
    }
    out
}

/// A property's words are spelled by its world's constant tables: by whole
/// tables that hold nothing else, as `AXES` and `ROTATION_AXES` spell a joint
/// axis, or by one table holding them all. A word some unrelated table
/// happens to spell does not count.
#[test]
fn every_word_a_schema_offers_has_a_constant_in_its_world() {
    for (dimension, tables) in [
        (balaur_core::components::tag::DIM_3D, CONSTANTS_3D),
        (balaur_core::components::tag::DIM_2D, CONSTANTS_2D),
    ] {
        let sets: Vec<std::collections::BTreeSet<String>> = tables
            .iter()
            .map(|t| t.iter().map(|(_, v)| (*v).to_string()).collect())
            .collect();
        let mut missing = Vec::new();
        for (at, words) in offered_words(dimension) {
            let whole: std::collections::BTreeSet<String> = sets
                .iter()
                .filter(|set| set.is_subset(&words))
                .flatten()
                .cloned()
                .collect();
            let one = sets.iter().any(|set| words.is_subset(set));
            if whole != words && !one {
                let unspelled: Vec<&String> = words.difference(&whole).collect();
                missing.push(format!("{at}: {unspelled:?}"));
            }
        }
        assert!(
            missing.is_empty(),
            "no {dimension} table spells {missing:#?}"
        );
    }
}
