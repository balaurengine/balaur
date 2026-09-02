//! Preset definitions parsed from a project's `presets.toml`.
//!
//! Only the parsing lives here: applying a preset needs the plugins that own
//! the components, so those tests are in the facade crate.

use balaur_core::components::ComponentDef;
use balaur_core::{components, presets, scene, App, AppConfig};

fn app() -> App {
    App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap()
}

/// Two components that behave like real ones -- each owns storage, so
/// "present" means present. `lonely` applies fine on its own but only does
/// something when `partner` is there. No built-in component is shaped like
/// this today, so the mechanism is exercised against one the test registers.
struct Lonely;
struct Partner;

fn empty_table() -> toml::Value {
    toml::Value::Table(toml::map::Map::new())
}

fn register_pair(app: &mut App) {
    app.register_component(
        "lonely",
        ComponentDef {
            doc: "",
            schema: ComponentDef::parse_schema(
                "lonely",
                r#"on = { type = "bool", default = true }"#,
            ),
            tags: &["test"],
            expects: &["partner"],
            apply: Box::new(|eng, entity, _| {
                let _ = eng.world_mut().insert_one(entity, Lonely);
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Lonely>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                eng.world()
                    .get::<&Lonely>(entity)
                    .ok()
                    .map(|_| empty_table())
            }),
        },
    );
    app.register_component(
        "partner",
        ComponentDef {
            doc: "",
            schema: ComponentDef::parse_schema(
                "partner",
                r#"on = { type = "bool", default = true }"#,
            ),
            tags: &["test"],
            expects: &[],
            apply: Box::new(|eng, entity, _| {
                let _ = eng.world_mut().insert_one(entity, Partner);
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Partner>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                eng.world()
                    .get::<&Partner>(entity)
                    .ok()
                    .map(|_| empty_table())
            }),
        },
    );
}

/// The warning fires only while the companion is missing, and never blocks:
/// the component applied, and adding the partner later clears it.
#[test]
fn an_expectation_warns_while_unmet_and_clears_when_met() {
    let mut app = app();
    register_pair(&mut app);
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), "N", root);

    components::add(&app.engine, entity, "lonely", None).unwrap();
    let unmet = presets::unmet_expectations(&app.engine, entity);
    assert_eq!(unmet.len(), 1, "{unmet:?}");
    assert_eq!(unmet[0].0, "lonely");
    assert_eq!(unmet[0].1, vec!["partner".to_string()]);

    components::add(&app.engine, entity, "partner", None).unwrap();
    assert!(
        presets::unmet_expectations(&app.engine, entity).is_empty(),
        "adding the companion later clears it"
    );
}

/// Nothing to warn about is the normal case, and has to stay quiet.
#[test]
fn a_component_with_no_expectations_never_warns() {
    let mut app = app();
    register_pair(&mut app);
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), "N", root);
    components::add(&app.engine, entity, "partner", None).unwrap();
    assert!(presets::unmet_expectations(&app.engine, entity).is_empty());
}

#[test]
fn a_project_preset_is_parsed_from_toml() {
    let body = r#"
description = "A patrolling enemy"
tags = ["2d"]
components = [
  { component = "shape2d", kind = "rect" },
  { component = "color" },
]
"#;
    let body: toml::Value = toml::from_str(body).unwrap();
    let def = presets::from_toml("enemy", &body).unwrap();
    assert_eq!(def.description, "A patrolling enemy");
    assert_eq!(def.tags, vec!["2d".to_string()]);
    assert_eq!(def.parts.len(), 2);
    assert_eq!(def.parts[0].component, "shape2d");
    // The discriminant is stripped; what is left is the component's own table.
    let params = def.parts[0].params.as_ref().unwrap();
    assert!(params.get("component").is_none());
    assert_eq!(params["kind"].as_str().unwrap(), "rect");
    assert!(def.parts[1].params.is_none(), "no properties means none");
}

#[test]
fn a_project_preset_without_components_is_an_error() {
    let body: toml::Value = toml::from_str("description = \"x\"").unwrap();
    let err = presets::from_toml("broken", &body).unwrap_err();
    assert!(err.to_string().contains("components"), "{err}");
}
