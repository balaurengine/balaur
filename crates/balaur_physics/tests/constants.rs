//! The named constants, the runtime matchers and the scene-file schemas all
//! describe the same closed sets. They are written in three places, so this
//! checks they agree — a constant nobody accepts is worse than a raw string,
//! because it looks official.

use balaur_core::components::ComponentRegistry;
use balaur_core::{App, AppConfig};
use balaur_physics::{PhysicsPlugin, BODY_KINDS, SHAPE_KINDS, SHAPE_KINDS_2D};

/// The enum options a registered component actually declares.
fn registered_options(component: &str, field: &str) -> Vec<String> {
    let mut app = App::new(AppConfig::bare("."))
    .unwrap();
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
        .unwrap_or_else(|| panic!("`{component}.{field}` declares enum options"))
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

/// Read from the live registry, not from a copy of the schema literal: the
/// point is to catch the two drifting apart.
#[test]
fn body_constants_match_the_registered_schema() {
    let declared: Vec<&str> = BODY_KINDS.iter().map(|(_, v)| *v).collect();
    for component in ["body3d", "body2d"] {
        assert_eq!(
            declared,
            registered_options(component, "kind"),
            "physics3d.BODY_* and the `{component}` component's options disagree"
        );
    }
}

#[test]
fn every_constant_is_screaming_snake_and_unique() {
    let mut seen = std::collections::BTreeSet::new();
    for (name, value) in BODY_KINDS.iter().chain(SHAPE_KINDS).chain(SHAPE_KINDS_2D) {
        assert!(
            name.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
            "{name} is not SCREAMING_SNAKE_CASE"
        );
        assert!(seen.insert(*name), "{name} is declared twice");
        assert!(!value.is_empty(), "{name} has no value");
    }
}

#[test]
fn the_two_worlds_share_body_kinds_and_differ_on_shapes() {
    let three: Vec<&str> = SHAPE_KINDS.iter().map(|(n, _)| *n).collect();
    let two: Vec<&str> = SHAPE_KINDS_2D.iter().map(|(n, _)| *n).collect();
    assert!(
        three.iter().all(|n| !two.contains(n)),
        "a shape name means different things in 2D and 3D: {three:?} vs {two:?}"
    );
}
