//! `components::patch`: writing one property without rewriting the component.
//!
//! The fixture is deliberately shaped like `balaur_render`'s `shape`: a
//! tagged union whose `get` reports only the properties its current `kind`
//! actually has. That is the case `patch` exists for — anything driving one
//! property over time has to leave the rest of the component where it found
//! it, and merging over the schema defaults does not.

use balaur_core::components::{self, ComponentDef};
use balaur_core::{App, AppConfig, Engine};

/// What the fixture component stores, as the properties it was applied with.
struct Dial(toml::Value);

fn app_with_dial() -> App {
    let mut app = App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    app.register_component(
        "dial",
        ComponentDef {
            schema: ComponentDef::parse_schema(
                "dial",
                r#"kind = { type = "enum", default = "round", options = ["round", "square"] }
radius = { type = "float", default = 0.5 }
half_extents = { type = "vec3", default = [0.5, 0.5, 0.5] }
label = { type = "string", default = "none" }"#,
            ),
            apply: Box::new(|eng: &Engine, entity, params| {
                eng.world_mut()
                    .insert_one(entity, Dial(params.clone()))
                    .map_err(|_| anyhow::anyhow!("dead node"))?;
                Ok(())
            }),
            remove: Box::new(|eng: &Engine, entity| {
                let _ = eng.world_mut().remove_one::<Dial>(entity);
                Ok(())
            }),
            // Reports only what the current `kind` has, exactly as `shape`
            // does: a round dial has no half extents to report.
            get: Box::new(|eng: &Engine, entity| {
                let world = eng.world();
                let dial = world.get::<&Dial>(entity).ok()?;
                let mut out = toml::map::Map::new();
                let kind = dial.0.get("kind")?.clone();
                let square = kind.as_str() == Some("square");
                out.insert("kind".into(), kind);
                out.insert("label".into(), dial.0.get("label")?.clone());
                let carried = if square { "half_extents" } else { "radius" };
                out.insert(carried.into(), dial.0.get(carried)?.clone());
                Some(toml::Value::Table(out))
            }),
        },
    );
    app
}

fn spawn(app: &App) -> hecs::Entity {
    let root = app.engine.root();
    balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Node", root)
}

fn table(text: &str) -> toml::Value {
    toml::from_str(text).unwrap()
}

fn read(app: &App, entity: hecs::Entity, prop: &str) -> toml::Value {
    let world = app.engine.world();
    let dial = world.get::<&Dial>(entity).expect("the node has a dial");
    dial.0.get(prop).expect("the property is set").clone()
}

#[test]
fn patch_writes_one_property_and_leaves_the_others_where_they_were() {
    let app = app_with_dial();
    let entity = spawn(&app);
    components::add(
        &app.engine,
        entity,
        "dial",
        Some(&table(
            r#"kind = "square"
half_extents = [2.0, 3.0, 4.0]
label = "gauge""#,
        )),
    )
    .unwrap();

    components::patch(&app.engine, entity, "dial", &table("radius = 1.25")).unwrap();

    assert_eq!(read(&app, entity, "radius"), toml::Value::Float(1.25));
    assert_eq!(
        read(&app, entity, "half_extents"),
        table("v = [2.0, 3.0, 4.0]").get("v").unwrap().clone(),
        "patching one property put another back to its default"
    );
    assert_eq!(
        read(&app, entity, "label"),
        toml::Value::String("gauge".into())
    );
}

#[test]
fn add_rewrites_the_whole_component_where_patch_does_not() {
    let app = app_with_dial();
    let entity = spawn(&app);
    let set = r#"kind = "square"
half_extents = [2.0, 3.0, 4.0]"#;
    components::add(&app.engine, entity, "dial", Some(&table(set))).unwrap();

    components::add(&app.engine, entity, "dial", Some(&table("radius = 1.25"))).unwrap();

    assert_eq!(
        read(&app, entity, "kind"),
        toml::Value::String("round".into()),
        "`add` describes a component whole, so an unmentioned property goes \
         back to its default — which is the reason `patch` exists"
    );
}

#[test]
fn a_property_the_component_never_reports_still_survives_a_patch() {
    let app = app_with_dial();
    let entity = spawn(&app);
    components::add(
        &app.engine,
        entity,
        "dial",
        Some(&table("kind = \"round\"\nradius = 3.0")),
    )
    .unwrap();

    components::patch(&app.engine, entity, "dial", &table("label = \"dial\"")).unwrap();

    assert_eq!(read(&app, entity, "radius"), toml::Value::Float(3.0));
    assert_eq!(
        read(&app, entity, "kind"),
        toml::Value::String("round".into())
    );
}

#[test]
fn patching_a_node_without_the_component_adds_it_over_the_defaults() {
    let app = app_with_dial();
    let entity = spawn(&app);

    components::patch(&app.engine, entity, "dial", &table("radius = 9.0")).unwrap();

    assert_eq!(read(&app, entity, "radius"), toml::Value::Float(9.0));
    assert_eq!(
        read(&app, entity, "label"),
        toml::Value::String("none".into()),
        "with nothing to read back, the defaults are the current value"
    );
}

#[test]
fn patching_a_component_nothing_registered_says_which_one() {
    let app = app_with_dial();
    let entity = spawn(&app);
    let why = components::patch(&app.engine, entity, "knob", &table("radius = 1.0"))
        .unwrap_err()
        .to_string();
    assert!(why.contains("knob"), "unhelpful: {why}");
}

#[test]
fn a_hex_colour_reaches_apply_expanded_through_patch_as_well_as_add() {
    let mut app = App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    app.register_component(
        "tint",
        ComponentDef {
            schema: ComponentDef::parse_schema(
                "tint",
                r#"rgba = { type = "color", default = [0.0, 0.0, 0.0, 1.0] }"#,
            ),
            apply: Box::new(|eng: &Engine, entity, params| {
                eng.world_mut()
                    .insert_one(entity, Dial(params.clone()))
                    .map_err(|_| anyhow::anyhow!("dead node"))?;
                Ok(())
            }),
            remove: Box::new(|_, _| Ok(())),
            get: Box::new(|eng: &Engine, entity| {
                eng.world().get::<&Dial>(entity).ok().map(|d| d.0.clone())
            }),
        },
    );
    let entity = spawn(&app);

    components::patch(&app.engine, entity, "tint", &table("rgba = \"#ff0000\"")).unwrap();

    assert_eq!(
        read(&app, entity, "rgba"),
        table("v = [1.0, 0.0, 0.0, 1.0]").get("v").unwrap().clone()
    );
}
