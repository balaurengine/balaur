//! `ui::fill_strip`: widget nodes made, reused and hidden from a list a
//! script hands over every frame, and a reader's edit reaching `on`.

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use balaur_core::hecs::Entity;

/// A `Root` running `body` as its `update`, over a `Bar` row to fill.
fn app_with(body: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = { source = \"s.rn\" }\n\n\
         [[nodes]]\nid = \"b\"\nname = \"Bar\"\nparent = \"n\"\nwidget = { kind = \"row\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("s.rn"), body).unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

fn bar(app: &App) -> Entity {
    let world = app.engine.world();
    balaur_core::scene::find_node(&world, app.engine.root(), "Root/Bar").expect("the bar")
}

fn kids(app: &App) -> Vec<Entity> {
    let world = app.engine.world();
    world
        .get::<&balaur_core::scene::Children>(bar(app))
        .map(|c| c.0.clone())
        .unwrap_or_default()
}

fn prop(app: &App, node: Entity, key: &str) -> Option<toml::Value> {
    balaur_core::components::property(&app.engine, node, "widget", key)
}

fn number(app: &App, name: &str) -> Option<f64> {
    let root = balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), "Root")
        .expect("the scene has a Root node");
    balaur::rune::rune_of(&app.engine).number_field(root, name)
}

#[test]
fn a_strip_makes_a_node_a_control_and_hides_what_a_shorter_list_leaves() {
    let (_dir, mut app) = app_with(
        "pub fn init(this) { this.n = 3; this.frames = 0.0; }\n\
         pub fn update(this, dt) {\n\
             this.frames += 1.0;\n\
             let controls = [];\n\
             for i in 0..this.n { controls.push(#{ kind: \"label\", text: `L${i}` }); }\n\
             ui::fill_strip(\"Root/Bar\", controls);\n\
             this.n = 1;\n\
         }\n",
    );
    app.tick(1.0 / 60.0);
    let made = kids(&app);
    assert_eq!(made.len(), 3);
    assert_eq!(
        prop(&app, made[2], "text"),
        Some(toml::Value::String("L2".into()))
    );

    app.tick(1.0 / 60.0);
    assert_eq!(number(&app, "frames"), Some(2.0));
    let after = kids(&app);
    assert_eq!(after, made, "the nodes are reused, not made again");
    assert_eq!(
        prop(&app, after[0], "visible"),
        Some(toml::Value::Boolean(true))
    );
    assert_eq!(
        prop(&app, after[1], "visible"),
        Some(toml::Value::Boolean(false))
    );
    assert_eq!(
        prop(&app, after[2], "visible"),
        Some(toml::Value::Boolean(false))
    );
}

#[test]
fn a_reader_s_edit_reaches_on_and_the_pass_after_writes_what_the_model_says() {
    let (_dir, mut app) = app_with(
        "pub fn init(this) { this.on = false; this.edits = 0.0; }\n\
         pub fn update(this, dt) {\n\
             ui::fill_strip(\"Root/Bar\", [#{ kind: \"checkbox\", checked: this.on, on: |v| {\n\
                 this.edits += 1.0;\n\
                 this.on = v;\n\
             } }]);\n\
         }\n",
    );
    app.tick(1.0 / 60.0);
    let boxed = kids(&app)[0];
    assert_eq!(
        prop(&app, boxed, "checked"),
        Some(toml::Value::Boolean(false))
    );

    let mut edit = toml::map::Map::new();
    edit.insert("checked".into(), toml::Value::Boolean(true));
    balaur_core::components::patch(&app.engine, boxed, "widget", &edit.into()).unwrap();
    app.tick(1.0 / 60.0);
    assert_eq!(number(&app, "edits"), Some(1.0));
    assert_eq!(
        prop(&app, boxed, "checked"),
        Some(toml::Value::Boolean(true))
    );

    app.tick(1.0 / 60.0);
    assert_eq!(
        number(&app, "edits"),
        Some(1.0),
        "our own write is not an edit"
    );
    assert_eq!(
        prop(&app, boxed, "checked"),
        Some(toml::Value::Boolean(true))
    );
}
