//! Physics stores angles in radians, rapier's unit, and every schema property
//! holding one declares `unit = "degrees"` so the inspector draws degrees.

use balaur_core::components::ComponentRegistry;
use balaur_core::{App, AppConfig};
use balaur_physics::PhysicsPlugin;

/// Whether a property's name says it holds an angle or a turning speed;
/// `angular_damping` is a rate, and no angle.
fn angular(name: &str) -> bool {
    name.ends_with("_rotation")
        || name.contains("angle")
        || name.contains("angular_velocity")
        || name.contains("angular_threshold")
}

/// Every number-typed physics property, a list's record fields included, as
/// `component.property` beside its spec.
fn numbers() -> Vec<(String, toml::Value)> {
    fn walk(spec: &toml::Value, at: &str, out: &mut Vec<(String, toml::Value)>) {
        let kind = spec.get("type").and_then(toml::Value::as_str).unwrap_or("");
        if matches!(kind, "float" | "vec2" | "vec3") {
            out.push((at.to_string(), spec.clone()));
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
        if !def.tags.contains(&balaur_core::components::tag::PHYSICS) {
            continue;
        }
        for (key, spec) in def.schema.as_table().into_iter().flatten() {
            walk(spec, &format!("{name}.{key}"), &mut out);
        }
    }
    out
}

#[test]
fn every_angle_a_physics_schema_stores_is_drawn_in_degrees() {
    let bare: Vec<String> = numbers()
        .into_iter()
        .filter(|(at, _)| angular(at.rsplit('.').next().unwrap_or_default()))
        .filter(|(_, spec)| spec.get("unit").and_then(toml::Value::as_str) != Some("degrees"))
        .map(|(at, _)| at)
        .collect();
    assert!(bare.is_empty(), "radians drawn as radians: {bare:#?}");
}
