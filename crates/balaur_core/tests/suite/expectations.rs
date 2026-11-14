//! What a component expects beside it, and the warning while it is missing.

use balaur_core::components::ComponentDef;
use balaur_core::{App, AppConfig, components, scene, warnings};

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
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
            tags: &["test"],
            expects: &["partner"],
            ..ComponentDef::by_hand(
                ComponentDef::parse_schema("lonely", r#"on = { type = "bool", default = true }"#),
                Box::new(|eng, entity, _| {
                    let _ = eng.world_mut().insert_one(entity, Lonely);
                    Ok(())
                }),
                Box::new(|eng, entity| {
                    let _ = eng.world_mut().remove_one::<Lonely>(entity);
                    Ok(())
                }),
                Box::new(|eng, entity| {
                    eng.world()
                        .get::<&Lonely>(entity)
                        .ok()
                        .map(|_| empty_table())
                }),
            )
        },
    );
    app.register_component(
        "partner",
        ComponentDef {
            tags: &["test"],
            ..ComponentDef::by_hand(
                ComponentDef::parse_schema("partner", r#"on = { type = "bool", default = true }"#),
                Box::new(|eng, entity, _| {
                    let _ = eng.world_mut().insert_one(entity, Partner);
                    Ok(())
                }),
                Box::new(|eng, entity| {
                    let _ = eng.world_mut().remove_one::<Partner>(entity);
                    Ok(())
                }),
                Box::new(|eng, entity| {
                    eng.world()
                        .get::<&Partner>(entity)
                        .ok()
                        .map(|_| empty_table())
                }),
            )
        },
    );
}

// The warning fires only while the companion is missing, and never blocks:
// the component applied, and adding the partner later clears it.
#[test]
fn an_expectation_warns_while_unmet_and_clears_when_met() {
    let mut app = app();
    register_pair(&mut app);
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), "N", root);

    components::add(&app.engine, entity, "lonely", None).unwrap();
    let unmet = warnings::unmet_expectations(&app.engine, entity);
    assert_eq!(unmet.len(), 1, "{unmet:?}");
    assert_eq!(unmet[0].0, "lonely");
    assert_eq!(unmet[0].1, vec!["partner".to_string()]);

    components::add(&app.engine, entity, "partner", None).unwrap();
    assert!(
        warnings::unmet_expectations(&app.engine, entity).is_empty(),
        "adding the companion later clears it"
    );
}

// Nothing to warn about is the normal case, and has to stay quiet.
#[test]
fn a_component_with_no_expectations_never_warns() {
    let mut app = app();
    register_pair(&mut app);
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), "N", root);
    components::add(&app.engine, entity, "partner", None).unwrap();
    assert!(warnings::unmet_expectations(&app.engine, entity).is_empty());
}
