//! The `inspector` kind: a component's rows filled from the registry and the
//! node, and an edit on one reaching `on_change` as the value now there.

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use balaur_core::hecs::Entity;
use balaur_script::Value;

/// `Root` carries a transform and a script that keeps what `on_change`
/// hears; `Form` is an inspector over `component` with `extra` stated.
fn app_with(component: &str, extra: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        format!(
            "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = {{ source = \"s.rn\" }}\n\
             [nodes.transform]\nposition = [1.0, 2.0, 3.0]\n\n\
             [[nodes]]\nid = \"l\"\nname = \"Shown\"\nparent = \"n\"\n\
             widget = {{ kind = \"label\", text = \"hi\", anchor = \"bottom_right\" }}\n\n\
             [[nodes]]\nid = \"f\"\nname = \"Form\"\nparent = \"n\"\n\
             widget = {{ kind = \"inspector\", anchor = \"fill\", label_width = 80, \
             target = \"Root{}\", component = \"{component}\", on_change = \"edited\"{extra} }}\n",
            if component == "widget" { "/Shown" } else { "" }
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("s.rn"),
        "pub fn init(this) { this.edits = 0.0; this.seen = \"\"; this.depth = -1.0; }\n\
         pub fn edited(this, edit) {\n\
             this.edits += 1.0;\n\
             this.depth = edit.path.len() as f64;\n\
             this.seen = format!(\"{} {:?}\", edit.property, edit.value);\n\
         }\n\
         pub fn update(this, dt) {}\n",
    )
    .unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

fn node(app: &App, path: &str) -> Option<Entity> {
    balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), path)
}

fn prop(app: &App, node: Entity, key: &str) -> Option<toml::Value> {
    balaur_core::components::property(&app.engine, node, "widget", key)
}

/// The control `n` of row `row`, as the pool names them.
fn control(app: &App, row: usize, n: usize) -> Entity {
    node(app, &format!("Root/Form/R{row}/H/C/C{n}")).expect("the row's control")
}

fn label(app: &App, row: usize) -> Option<toml::Value> {
    prop(app, node(app, &format!("Root/Form/R{row}/L"))?, "text")
}

fn shown_rows(app: &App) -> usize {
    let world = app.engine.world();
    let Some(form) = node(app, "Root/Form") else {
        return 0;
    };
    let kids = world
        .get::<&balaur_core::scene::Children>(form)
        .map(|c| c.0.clone())
        .unwrap_or_default();
    drop(world);
    kids.iter()
        .filter(|row| prop(app, **row, "visible") == Some(toml::Value::Boolean(true)))
        .count()
}

fn number(app: &App, name: &str) -> Option<f64> {
    let root = node(app, "Root").expect("the scene has a Root node");
    balaur::script_rune::rune_of(&app.engine).number_field(root, name)
}

/// The last edit the script heard: its property, then its value as Rune
/// writes it out.
fn seen(app: &App) -> Option<String> {
    let root = node(app, "Root").expect("the scene has a Root node");
    balaur::script_rune::rune_of(&app.engine).text_field(root, "seen")
}

/// Ticks and passes enough for the rows to be filled and drawn.
fn settle(app: &mut App, ctx: &egui::Context) {
    for _ in 0..3 {
        app.tick(1.0 / 60.0);
        crate::support::pass(app, ctx, vec![]);
    }
}

#[test]
fn an_inspector_draws_a_row_for_every_property_its_component_declares() {
    let (_dir, mut app) = app_with("transform", "");
    let ctx = egui::Context::default();
    settle(&mut app, &ctx);
    assert_eq!(shown_rows(&app), 4, "position, rotation, scale and skew");
    let labels: Vec<_> = (0..4).map(|row| label(&app, row)).collect();
    assert_eq!(
        labels,
        ["Position", "Rotation euler", "Scale", "Skew"]
            .map(|text| Some(toml::Value::String(text.into())))
    );
    assert_eq!(
        prop(&app, control(&app, 0, 1), "value"),
        Some(toml::Value::Float(2.0))
    );
    assert_eq!(
        prop(&app, control(&app, 0, 1), "prefix"),
        Some(toml::Value::String("y".into()))
    );
}

#[test]
fn options_name_the_properties_and_their_order() {
    let (_dir, mut app) = app_with("transform", ", options = [\"skew\", \"position\"]");
    let ctx = egui::Context::default();
    settle(&mut app, &ctx);
    assert_eq!(shown_rows(&app), 2);
    assert_eq!(label(&app, 0), Some(toml::Value::String("Skew".into())));
    assert_eq!(label(&app, 1), Some(toml::Value::String("Position".into())));
}

#[test]
fn an_edit_on_one_channel_reports_the_whole_vector_and_writes_nothing_itself() {
    let (_dir, mut app) = app_with("transform", "");
    let ctx = egui::Context::default();
    settle(&mut app, &ctx);
    assert!(balaur_ui::edit(
        &app.engine,
        control(&app, 0, 1),
        &Value::Num(5.0)
    ));
    app.tick(1.0 / 60.0);
    assert_eq!(number(&app, "edits"), Some(1.0));
    assert_eq!(number(&app, "depth"), Some(0.0));
    assert_eq!(seen(&app).as_deref(), Some("position [1.0, 5.0, 3.0]"));
    // The script did not write it, so the node and the row keep what it was.
    crate::support::pass(&app, &ctx, vec![]);
    assert_eq!(
        prop(&app, control(&app, 0, 1), "value"),
        Some(toml::Value::Float(2.0))
    );
}

#[test]
fn a_property_in_degrees_shows_degrees_and_reports_radians() {
    let (_dir, mut app) = app_with("transform", ", options = [\"skew\"]");
    let ctx = egui::Context::default();
    settle(&mut app, &ctx);
    assert!(balaur_ui::edit(
        &app.engine,
        control(&app, 0, 0),
        &Value::Num(90.0)
    ));
    app.tick(1.0 / 60.0);
    let seen = seen(&app).unwrap();
    let radians: f64 = seen.trim_start_matches("skew ").parse().unwrap();
    assert!(
        (radians - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
        "{seen}"
    );
}

/// Nothing is read back each pass: the node moved by something else keeps
/// its old rows until whoever moved it says so, and then they follow.
#[test]
fn the_rows_follow_the_node_once_told_it_moved() {
    let (_dir, mut app) = app_with("transform", "");
    let ctx = egui::Context::default();
    settle(&mut app, &ctx);
    let root = node(&app, "Root").unwrap();
    let mut write = toml::map::Map::new();
    write.insert(
        "position".into(),
        toml::Value::Array(vec![7.0.into(), 8.0.into(), 9.0.into()]),
    );
    balaur_core::components::patch(&app.engine, root, "transform", &write.into()).unwrap();
    crate::support::pass(&app, &ctx, vec![]);
    assert_eq!(
        prop(&app, control(&app, 0, 2), "value"),
        Some(toml::Value::Float(3.0)),
        "a pass read the component without being told"
    );
    // Named by the node above it, as a panel names itself for its whole form.
    assert!(balaur_ui::reread(&app.engine, root));
    crate::support::pass(&app, &ctx, vec![]);
    assert_eq!(
        prop(&app, control(&app, 0, 2), "value"),
        Some(toml::Value::Float(9.0))
    );
}

#[test]
fn a_set_s_pill_reports_the_set_with_its_option_turned_round() {
    let (_dir, mut app) = app_with("widget", ", options = [\"safe_area\"]");
    let ctx = egui::Context::default();
    settle(&mut app, &ctx);
    let top = control(&app, 0, 1);
    assert_eq!(
        prop(&app, top, "text"),
        Some(toml::Value::String("top".into()))
    );
    assert!(balaur_ui::click(&app.engine, top, false));
    app.tick(1.0 / 60.0);
    assert_eq!(number(&app, "edits"), Some(1.0));
    assert_eq!(seen(&app).as_deref(), Some("safe_area [\"top\"]"));
}

#[test]
fn a_list_s_rows_add_and_take_out_an_entry_at_their_own_path() {
    let (_dir, mut app) = app_with("widget", ", options = [\"options\"]");
    let ctx = egui::Context::default();
    let shown = node(&app, "Root/Shown").unwrap();
    let mut write = toml::map::Map::new();
    write.insert(
        "options".into(),
        toml::Value::Array(vec!["a".into(), "b".into()]),
    );
    balaur_core::components::patch(&app.engine, shown, "widget", &write.into()).unwrap();
    settle(&mut app, &ctx);
    assert_eq!(shown_rows(&app), 3, "the list's note, then its two entries");
    assert_eq!(label(&app, 2), Some(toml::Value::String("1".into())));
    // The entry's own field, then the chip that takes it out.
    assert!(balaur_ui::click(&app.engine, control(&app, 2, 1), false));
    app.tick(1.0 / 60.0);
    assert_eq!(seen(&app).as_deref(), Some("options [\"a\"]"));
    assert!(balaur_ui::click(&app.engine, control(&app, 0, 1), false));
    app.tick(1.0 / 60.0);
    assert_eq!(number(&app, "edits"), Some(2.0));
    assert_eq!(seen(&app).as_deref(), Some("options [\"a\", \"b\", \"\"]"));
}

/// What the editor does: a pooled form row holding one inspector, named by
/// the path the node reports for itself.
#[test]
fn an_inspector_a_script_pools_fills_and_draws_its_rows() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = { source = \"s.rn\" }\n\
         [nodes.transform]\nposition = [1.0, 2.0, 3.0]\n\n\
         [[nodes]]\nid = \"b\"\nname = \"Bar\"\nparent = \"n\"\n\
         widget = { kind = \"column\", anchor = \"fill\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("s.rn"),
        "pub fn init(this) { this.edits = 0.0; this.path = \"\"; }\n\
         pub fn update(this, dt) {\n\
             this.path = scene::get_node(\"Root\").path();\n\
             ui::fill_rows(\"Root/Bar\", [#{ full: true, height: 0, controls: [#{\n\
                 kind: \"inspector\", grow: 1, gap: [2, 2], target: this.path,\n\
                 component: \"transform\", label_width: 60.0,\n\
                 on: |edit| { this.edits += 1.0; },\n\
             }] }], 60.0, false);\n\
         }\n",
    )
    .unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    let ctx = egui::Context::default();
    settle(&mut app, &ctx);
    let root = node(&app, "Root").unwrap();
    let said = balaur::script_rune::rune_of(&app.engine).text_field(root, "path");
    let inspector = node(&app, "Root/Bar/R0/H/C/C0").expect("the pooled control");
    assert_eq!(
        prop(&app, inspector, "kind"),
        Some(toml::Value::String("inspector".into())),
        "path {said:?}"
    );
    // `node.path()` starts with the root's own name, which `get_node` does not.
    let first = node(&app, "Root/Bar/R0/H/C/C0/R0/H/C/C0").expect("its first row's control");
    assert_eq!(prop(&app, first, "value"), Some(toml::Value::Float(1.0)));
    let drawn = balaur_ui::widget_rect(first).expect("drawn");
    assert!(drawn.width() > 0.0 && drawn.height() > 0.0, "{drawn:?}");
    assert!(balaur_ui::edit(&app.engine, first, &Value::Num(4.0)));
    app.tick(1.0 / 60.0);
    assert_eq!(number(&app, "edits"), Some(1.0));
}

/// A pooled slot is an inspector for one selection and a group of controls
/// for the next. Its rows go with it, rather than being read as the group's
/// controls or left to anchor themselves, and come back with it.
#[test]
fn a_slot_that_stops_being_an_inspector_takes_its_rows_with_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = { source = \"s.rn\" }\n\
         [nodes.transform]\nposition = [1.0, 2.0, 3.0]\n\n\
         [[nodes]]\nid = \"b\"\nname = \"Bar\"\nparent = \"n\"\n\
         widget = { kind = \"column\", anchor = \"fill\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("s.rn"),
        "pub fn init(this) { this.inspecting = true; }\n\
         pub fn update(this, dt) {\n\
             let control = if this.inspecting {\n\
                 #{ kind: \"inspector\", grow: 1, target: \"Root\", component: \"transform\" }\n\
             } else {\n\
                 #{ controls: [#{ kind: \"label\", text: \"a\" }, #{ kind: \"label\", text: \"b\" }] }\n\
             };\n\
             ui::fill_rows(\"Root/Bar\", [#{ full: true, height: 0, controls: [control] }], 60.0, false);\n\
         }\n\
         pub fn flip(this) { this.inspecting = !this.inspecting; }\n",
    )
    .unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    let ctx = egui::Context::default();
    let slot = |app: &App| node(app, "Root/Bar/R0/H/C/C0").expect("the pooled slot");
    let kids = |app: &App| {
        let world = app.engine.world();
        world
            .get::<&balaur_core::scene::Children>(slot(app))
            .map_or(0, |c| c.0.len())
    };
    let flip = |app: &App| {
        let root = node(app, "Root").unwrap();
        balaur::script_rune::rune_of(&app.engine).call_on(root, "flip", &[]);
    };
    settle(&mut app, &ctx);
    assert_eq!(
        kids(&app),
        4,
        "a row for each of the transform's properties"
    );

    flip(&app);
    settle(&mut app, &ctx);
    assert_eq!(
        kids(&app),
        2,
        "the group's two labels, and none of the rows"
    );
    assert_eq!(
        prop(&app, node(&app, "Root/Bar/R0/H/C/C0/C1").unwrap(), "text"),
        Some(toml::Value::String("b".into()))
    );

    flip(&app);
    settle(&mut app, &ctx);
    assert_eq!(kids(&app), 4, "and back as an inspector, its rows again");
    let first = node(&app, "Root/Bar/R0/H/C/C0/R0/H/C/C0").expect("its first row's control");
    assert_eq!(prop(&app, first, "value"), Some(toml::Value::Float(1.0)));
}
