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
    balaur::script_rune::rune_of(&app.engine).number_field(root, name)
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
fn a_reader_s_click_reaches_on_and_a_script_s_write_does_not() {
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

    assert!(balaur_ui::click(&app.engine, boxed, false));
    app.tick(1.0 / 60.0);
    assert_eq!(number(&app, "edits"), Some(1.0));
    assert_eq!(
        prop(&app, boxed, "checked"),
        Some(toml::Value::Boolean(true))
    );

    // A write that is not the reader's is no edit.
    let mut write = toml::map::Map::new();
    write.insert("checked".into(), toml::Value::Boolean(false));
    balaur_core::components::patch(&app.engine, boxed, "widget", &write.into()).unwrap();
    app.tick(1.0 / 60.0);
    assert_eq!(
        number(&app, "edits"),
        Some(1.0),
        "a script's write is not an edit"
    );
    assert_eq!(
        prop(&app, boxed, "checked"),
        Some(toml::Value::Boolean(false)),
        "and the pool writes only what its own spec changed"
    );
}

/// The editor hides its inspector's form with a plain write when nothing is
/// selected, and selecting the same node a third time drew an empty panel.
#[test]
fn a_host_a_script_hid_shows_again_every_time_it_is_filled() {
    let (_dir, mut app) = app_with(
        "pub fn init(this) { this.frames = 0.0; }\n\
         pub fn update(this, dt) {\n\
             this.frames += 1.0;\n\
             if this.frames % 2.0 == 0.0 {\n\
                 scene::get_node(\"Root/Bar\").patch_component(\"widget\", #{ visible: false });\n\
             } else {\n\
                 ui::fill_rows(\"Root/Bar\", [#{ label: \"a\", controls: [#{ kind: \"label\", text: \"x\" }] }], 40.0, false);\n\
             }\n\
         }\n",
    );
    for frame in 1..=7 {
        app.tick(1.0 / 60.0);
        let shown = frame % 2 == 1;
        assert_eq!(
            prop(&app, bar(&app), "visible"),
            Some(toml::Value::Boolean(shown)),
            "frame {frame}: {}",
            if shown {
                "filled, so shown"
            } else {
                "hidden by the script"
            }
        );
    }
    assert_eq!(number(&app, "frames"), Some(7.0));
}

/// A reused slot wears only what its new control states. The range an
/// earlier control left on it clamped the next one's number to 0.
#[test]
fn a_control_keeps_nothing_the_last_one_in_its_slot_stated() {
    let (_dir, mut app) = app_with(
        "pub fn init(this) {\n\
             this.frames = 0.0; this.edits = 0.0;\n\
             scene::get_node(\"Root/Bar\").patch_component(\"widget\", #{ kind: \"column\", anchor: \"fill\" });\n\
         }\n\
         pub fn update(this, dt) {\n\
             this.frames += 1.0;\n\
             let control = if this.frames < 3.0 {\n\
                 #{ kind: \"number_field\", grow: 1, value: 0.5, min: 0.01, max: 2.0, placeholder: \"px\", on: |v| { this.edits += 1.0; } }\n\
             } else {\n\
                 #{ kind: \"number_field\", grow: 1, value: -24.0, on: |v| { this.edits += 1.0; } }\n\
             };\n\
             ui::fill_rows(\"Root/Bar\", [#{ label: \"x\", controls: [control] }], 40.0, false);\n\
         }\n",
    );
    let ctx = egui::Context::default();
    for _ in 0..6 {
        app.tick(1.0 / 60.0);
        crate::support::pass(&app, &ctx, vec![]);
    }
    app.tick(1.0 / 60.0);
    let field = number_fields(&app);
    assert_eq!(field.len(), 1, "one number in the row");
    assert!(
        balaur_ui::widget_rect(field[0]).is_some_and(|r| r.width() > 0.0),
        "the number was drawn"
    );
    assert_eq!(
        prop(&app, field[0], "value"),
        Some(toml::Value::Float(-24.0))
    );
    assert_eq!(
        prop(&app, field[0], "max"),
        Some(toml::Value::Float(1.0)),
        "the range went with the control that stated it"
    );
    assert_eq!(
        prop(&app, field[0], "placeholder"),
        Some(toml::Value::String(String::new())),
        "and so did its letter"
    );
    assert_eq!(number(&app, "edits"), Some(0.0), "nobody edited the number");
}

/// Every `number_field` under the bar, however deep the rows put it.
fn number_fields(app: &App) -> Vec<Entity> {
    let world = app.engine.world();
    let mut out = Vec::new();
    let mut stack = vec![bar(app)];
    while let Some(node) = stack.pop() {
        if let Ok(kids) = world.get::<&balaur_core::scene::Children>(node) {
            stack.extend(kids.0.iter().copied());
        }
        if prop(app, node, "kind") == Some(toml::Value::String("number_field".into())) {
            out.push(node);
        }
    }
    out
}
