//! A `variant` property: one of several records, named by its `kind`, so a
//! row only ever holds the fields its kind reads.

use balaur_core::components::{self, ComponentDef};
use balaur_core::{App, AppConfig, Engine};

struct Held(toml::Value);

const SCHEMA: &str = r##"form = { type = "variant", default = { kind = "square", side = 1.0 }, variants = { square = { type = "record", fields = { side = { type = "float", default = 1.0 } } }, disc = { type = "record", fields = { radius = { type = "float", default = 0.5 }, rim = { type = "int", default = 32 } } } } }
tint = { type = "color", default = "#ffffff" }"##;

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    app.register_component(
        "plate",
        ComponentDef::by_hand(
            ComponentDef::parse_schema("plate", SCHEMA),
            Box::new(|eng: &Engine, entity, params| {
                eng.world_mut()
                    .insert_one(entity, Held(params.clone()))
                    .map_err(|_| anyhow::anyhow!("dead node"))
            }),
            Box::new(|eng: &Engine, entity| {
                let _ = eng.world_mut().remove_one::<Held>(entity);
                Ok(())
            }),
            Box::new(|eng: &Engine, entity| {
                eng.world().get::<&Held>(entity).ok().map(|h| h.0.clone())
            }),
        ),
    );
    app
}

fn form(app: &App, entity: balaur_core::hecs::Entity) -> toml::Table {
    components::get(&app.engine, entity, "plate").unwrap()["form"]
        .as_table()
        .unwrap()
        .clone()
}

fn table(text: &str) -> toml::Value {
    toml::from_str(text).unwrap()
}

fn spawn(app: &App) -> balaur_core::hecs::Entity {
    let root = app.engine.root();
    balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Plate", root)
}

#[test]
fn a_variant_holds_the_fields_of_its_kind_and_no_others() {
    let app = app();
    let e = spawn(&app);
    components::add(
        &app.engine,
        e,
        "plate",
        Some(&table(
            r#"form = { kind = "disc", radius = 2.0, side = 9.0 }"#,
        )),
    )
    .unwrap();
    let form = form(&app, e);
    assert_eq!(form["kind"].as_str(), Some("disc"));
    assert_eq!(form["radius"].as_float(), Some(2.0));
    assert_eq!(
        form["rim"].as_integer(),
        Some(32),
        "a field left out takes its default"
    );
    assert!(
        !form.contains_key("side"),
        "a square's field has no place on a disc: {form:?}"
    );
}

#[test]
fn a_write_naming_no_kind_changes_only_what_it_names() {
    let app = app();
    let e = spawn(&app);
    components::add(
        &app.engine,
        e,
        "plate",
        Some(&table(r#"form = { kind = "disc", radius = 2.0, rim = 8 }"#)),
    )
    .unwrap();
    components::patch(&app.engine, e, "plate", &table("form = { radius = 3.0 }")).unwrap();
    let form = form(&app, e);
    assert_eq!(form["kind"].as_str(), Some("disc"), "it stays a disc");
    assert_eq!(form["radius"].as_float(), Some(3.0));
    assert_eq!(
        form["rim"].as_integer(),
        Some(8),
        "and keeps what it did not name"
    );
}

#[test]
fn another_kind_starts_from_its_own_defaults() {
    let app = app();
    let e = spawn(&app);
    components::add(
        &app.engine,
        e,
        "plate",
        Some(&table(r#"form = { kind = "disc", radius = 2.0 }"#)),
    )
    .unwrap();
    components::patch(
        &app.engine,
        e,
        "plate",
        &table(r#"form = { kind = "square" }"#),
    )
    .unwrap();
    let form = form(&app, e);
    assert_eq!(form["kind"].as_str(), Some("square"));
    assert_eq!(form["side"].as_float(), Some(1.0));
    assert!(!form.contains_key("radius"), "{form:?}");
}

#[test]
fn a_variant_schema_is_held_to_its_rules() {
    for (spec, why) in [
        (
            r#"{ type = "variant", default = { kind = "a" } }"#,
            "variants",
        ),
        (
            r#"{ type = "variant", default = { kind = "a" }, variants = {} }"#,
            "empty",
        ),
        (
            r#"{ type = "variant", default = { kind = "a" }, variants = { a = { type = "float" } } }"#,
            "record",
        ),
        (
            r#"{ type = "variant", default = { kind = "b" }, variants = { a = { type = "record", fields = { x = { type = "float" } } } } }"#,
            "none of",
        ),
        (
            r#"{ type = "variant", default = { kind = "a" }, variants = { a = { type = "record", fields = { kind = { type = "float" } } } } }"#,
            "kind",
        ),
        (
            r#"{ type = "float", default = 0.0, variants = {} }"#,
            "belongs",
        ),
    ] {
        let parsed: toml::Value = toml::from_str(&format!("v = {spec}")).unwrap();
        let err = components::validate_property(&parsed["v"]).unwrap_err();
        assert!(
            err.contains(why),
            "`{spec}` refused for the wrong reason: {err}"
        );
    }
}
