//! `ui::write` and `ui::patch`: a control table written onto a widget node,
//! each key to the component its kind reads and only what moved, and a
//! reader's edit reaching the method the table names.

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use balaur_core::hecs::Entity;

/// A `Root` running `body`, over a `Bar` row holding two plain widgets, `A`
/// and `B`, to write.
fn app_with(body: &str) -> (tempfile::TempDir, App) {
    app_with_bar(body, None)
}

/// The same with `bar` as the row's own script, a node nearer every control
/// than the root.
fn app_with_bar(body: &str, bar: Option<&str>) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        format!(
            "[[nodes]]\nid = \"root\"\nname = \"Root\"\nscript = {{ source = \"root.bs\" }}\n\n\
         [[nodes]]\nid = \"bar\"\nname = \"Bar\"\nparent = \"root\"\nwidget = {{ kind = \"row\", x = 0.0, y = 0.0 }}\n{}\n\
         [[nodes]]\nid = \"a\"\nname = \"A\"\nparent = \"bar\"\nwidget = {{ kind = \"label\" }}\n\n\
         [[nodes]]\nid = \"b\"\nname = \"B\"\nparent = \"bar\"\nwidget = {{ kind = \"label\" }}\n",
            if bar.is_some() { "script = { source = \"bar.bs\" }" } else { "" }
        ),
    )
    .unwrap();
    std::fs::write(dir.path().join("root.bs"), body).unwrap();
    if let Some(bar) = bar {
        std::fs::write(dir.path().join("bar.bs"), bar).unwrap();
    }
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

fn node(app: &App, path: &str) -> Entity {
    let world = app.engine.world();
    balaur_core::scene::find_node(&world, app.engine.root(), path).expect("the node")
}

fn prop(app: &App, node: Entity, component: &str, key: &str) -> Option<toml::Value> {
    balaur_core::components::property(&app.engine, node, component, key)
}

fn number(app: &App, name: &str) -> Option<f64> {
    crate::support::script_number(app, node(app, "Root"), name)
}

fn flag(app: &App, name: &str) -> Option<balaur_script::Value> {
    app.engine
        .script_host()
        .and_then(|host| host.script_field(balaur_core::node_id_of(node(app, "Root")), name))
}

// Each row goes to the component its kind reads, and a node that turns into
// another kind drops what the last one wore.
#[test]
fn a_control_s_row_goes_to_the_component_its_kind_reads() {
    let (_dir, mut app) = app_with(
        "pub class Root { node: Node, frames: int }
impl Root {
    pub fn update(dt: float) {
        self.frames += 1;
        let a = self.node.get_node(\"Bar/A\");
        if self.frames == 1 {
            ui::write([a], [ui::Control { kind: \"text_field\", prefix: \"at\" }]);
        } else {
            ui::write([a], [ui::Control { kind: \"label\", text: \"plain\" }]);
        }
        ui::write([self.node.get_node(\"Bar/B\")], [ui::Control { kind: \"number_field\", prefix: \"at\" }]);
    }
}
",
    );
    app.tick(1.0 / 60.0);
    let (a, b) = (node(&app, "Root/Bar/A"), node(&app, "Root/Bar/B"));
    let at = Some(toml::Value::String("at".into()));
    assert_eq!(prop(&app, a, "widget_field", "prefix"), at);
    assert_eq!(prop(&app, b, "widget_range", "prefix"), at);
    assert!(!balaur_core::components::has(
        &app.engine,
        a,
        "widget_range"
    ));
    app.tick(1.0 / 60.0);
    assert!(
        !balaur_core::components::has(&app.engine, a, "widget_field"),
        "a node that became a label still wears its field's rows"
    );
    assert_eq!(
        prop(&app, a, "widget", "text"),
        Some(toml::Value::String("plain".into()))
    );
}

#[test]
fn a_reader_s_click_reaches_its_method_and_a_script_s_write_does_not() {
    let (_dir, mut app) = app_with(
        "pub class Root { node: Node, on: bool, edits: int }
impl Root {
    pub fn update(dt: float) {
        ui::write([self.node.get_node(\"Bar/A\")], [ui::Control { kind: \"checkbox\", checked: self.on, on_change: \"toggled\", target: self.node }]);
    }
    pub fn toggled(data: any, v: any) {
        self.edits += 1;
        self.on = v.as_bool();
    }
}
",
    );
    app.tick(1.0 / 60.0);
    let boxed = node(&app, "Root/Bar/A");
    assert_eq!(
        prop(&app, boxed, "widget_check", "checked"),
        Some(toml::Value::Boolean(false))
    );
    assert!(balaur_ui::click(&app.engine, boxed, false));
    app.tick(1.0 / 60.0);
    assert_eq!(number(&app, "edits"), Some(1.0));
    assert_eq!(
        prop(&app, boxed, "widget_check", "checked"),
        Some(toml::Value::Boolean(true))
    );
    // A write that is not the reader's is no edit.
    let mut write = toml::map::Map::new();
    write.insert("checked".into(), toml::Value::Boolean(false));
    balaur_core::components::patch(&app.engine, boxed, "widget_check", &write.into()).unwrap();
    app.tick(1.0 / 60.0);
    assert_eq!(
        number(&app, "edits"),
        Some(1.0),
        "a script's write is not an edit"
    );
    assert_eq!(
        prop(&app, boxed, "widget_check", "checked"),
        Some(toml::Value::Boolean(false)),
        "and `ui.write` writes only what its own table changed"
    );
}

// The handler's name is the writer's to keep: the label senses the pointer's
// click for it, the method is handed the table's data, a `true` and the
// control, and the widget holds no such row.
#[test]
fn a_label_s_click_calls_the_method_its_table_names() {
    let (_dir, mut app) = app_with(
        "pub class Root { node: Node, picked: str, with: bool }
impl Root {
    pub fn update(dt: float) {
        ui::write([self.node.get_node(\"Bar/A\")], [ui::Control { kind: \"label\", text: \"One\", width: 80, on_click: \"pick\", target: self.node, data: \"one\" }]);
        ui::write([self.node.get_node(\"Bar/B\")], [ui::Control { kind: \"label\", text: \"Two\", width: 80 }]);
    }
    pub fn pick(data: str, on: bool, node: Node) {
        self.picked = \"{data} from {node.name()}\";
        self.with = on;
    }
}
",
    );
    let ctx = egui::Context::default();
    app.tick(1.0 / 60.0);
    crate::support::settle(&app, &ctx);
    let (a, b) = (node(&app, "Root/Bar/A"), node(&app, "Root/Bar/B"));
    assert_eq!(prop(&app, a, "widget", "on_click"), None, "no row holds it");
    assert_eq!(prop(&app, a, "widget", "target"), None, "nor its target");
    let root = node(&app, "Root");
    let picked = |app: &App| crate::support::script_text(app, root, "picked");
    let click = |app: &mut App, at: Entity| {
        let at = balaur_ui::widget_rect(at).expect("drawn").center();
        crate::support::pass(app, &ctx, crate::support::press(at, true));
        crate::support::pass(app, &ctx, crate::support::press(at, false));
        app.tick(1.0 / 60.0);
    };
    click(&mut app, b);
    assert_eq!(
        picked(&app),
        Some(String::new()),
        "a label naming none lets it by"
    );
    click(&mut app, a);
    assert_eq!(picked(&app), Some("one from A".into()));
    assert_eq!(
        flag(&app, "with"),
        Some(balaur_script::Value::Bool(true)),
        "a click is a `true`"
    );
}

// A node written again wears only what its new table states. The range an
// earlier control left on it clamped the next one's number to 0.
#[test]
fn a_control_keeps_nothing_the_last_one_on_its_node_stated() {
    let (_dir, mut app) = app_with(
        "pub class Root { node: Node, frames: int }
impl Root {
    pub fn update(dt: float) {
        self.frames += 1;
        let control: ui::Control = if self.frames < 3 {
            ui::Control { kind: \"number_field\", value: 0.5, min: 0.01, max: 2.0, prefix: \"px\", tooltip_key: \"back\", scale: vec2(1.05, 1.05) }
        } else {
            ui::Control { kind: \"number_field\", value: -24.0, tooltip: \"Start\" }
        };
        ui::write([self.node.get_node(\"Bar/A\")], [control]);
    }
}
",
    );
    for _ in 0..4 {
        app.tick(1.0 / 60.0);
    }
    let field = node(&app, "Root/Bar/A");
    assert_eq!(
        prop(&app, field, "widget_range", "value"),
        Some(toml::Value::Float(-24.0))
    );
    assert_eq!(
        prop(&app, field, "widget_range", "max"),
        Some(toml::Value::Float(1.0)),
        "the range went with the control that stated it"
    );
    assert_eq!(
        prop(&app, field, "widget_range", "prefix"),
        Some(toml::Value::String(String::new())),
        "and so did its letter"
    );
    assert_eq!(
        prop(&app, field, "widget", "tooltip_key"),
        Some(toml::Value::String(String::new())),
        "and the key that would win over its words"
    );
    let scale: Option<Vec<f64>> = prop(&app, field, "widget", "scale").and_then(|v| {
        v.as_array()?
            .iter()
            .map(|axis| {
                axis.as_float()
                    .or_else(|| axis.as_integer().map(|i| i as f64))
            })
            .collect()
    });
    assert_eq!(scale, Some(vec![1.0, 1.0]), "drawn at its size");
}

/// A label written, then its text patched twice, then a write asking for
/// what it first held.
const PATCHED_THEN_WRITTEN: &str =
    "pub class Root { node: Node, frame: int, wrote: bool, again: bool }
impl Root {
    pub fn update(dt: float) {
        self.frame += 1;
        let a = self.node.get_node(\"Bar/A\");
        if self.frame == 1 {
            ui::write([a], [ui::Control { kind: \"label\", text: \"first\" }]);
        } else if self.frame == 2 {
            self.wrote = ui::patch(a, ui::Control { text: \"moved\" });
        } else if self.frame == 3 {
            self.again = ui::patch(a, ui::Control { text: \"moved\" });
        } else if self.frame == 4 {
            ui::write([a], [ui::Control { kind: \"label\", text: \"first\" }]);
        }
    }
}
";

#[test]
fn a_patch_writes_what_moved_and_the_next_write_compares_against_it() {
    let (_dir, mut app) = app_with(PATCHED_THEN_WRITTEN);
    let text = |app: &App| prop(app, node(app, "Root/Bar/A"), "widget", "text");
    app.tick(1.0 / 60.0);
    assert_eq!(text(&app), Some(toml::Value::String("first".into())));
    app.tick(1.0 / 60.0);
    assert_eq!(text(&app), Some(toml::Value::String("moved".into())));
    assert_eq!(flag(&app, "wrote"), Some(balaur_script::Value::Bool(true)));
    app.tick(1.0 / 60.0);
    assert_eq!(
        flag(&app, "again"),
        Some(balaur_script::Value::Bool(false)),
        "what it holds already is not written"
    );
    app.tick(1.0 / 60.0);
    assert_eq!(
        text(&app),
        Some(toml::Value::String("first".into())),
        "a write reads the patch, not what it last wrote itself"
    );
}

#[test]
fn a_click_calls_its_target_and_not_a_nearer_node_with_the_same_method() {
    let (_dir, mut app) = app_with_bar(
        "pub class Root { node: Node, frames: int, picks: int }
impl Root {
    pub fn update(dt: float) {
        self.frames += 1;
        let a = self.node.get_node(\"Bar/A\");
        ui::write([a], [ui::Control { kind: \"button\", text: \"Go\", on_click: \"pick\", target: self.node }]);
        if self.frames == 2 {
            ui::click(a);
        }
    }
    pub fn pick(data: any, on: any) { self.picks += 1; }
}
",
        Some(
            "pub class Bar { node: Node, stolen: int }
impl Bar {
    pub fn pick(data: any, on: any) { self.stolen += 1; }
}
",
        ),
    );
    for _ in 0..4 {
        app.tick(1.0 / 60.0);
    }
    assert_eq!(number(&app, "picks"), Some(1.0));
    assert_eq!(
        crate::support::script_number(&app, node(&app, "Root/Bar"), "stolen"),
        Some(0.0),
        "the row's own script declares `pick` too, and is not called"
    );
}

#[test]
fn a_control_naming_a_handler_and_no_target_is_refused() {
    balaur_core::logbuf::capture_for_test();
    let (_dir, mut app) = app_with(
        "pub class Root { node: Node, frames: int, written: int }
impl Root {
    pub fn update(dt: float) {
        self.frames += 1;
        ui::write([self.node.get_node(\"Bar/A\")], [ui::Control { kind: \"button\", text: \"Go\", on_click: \"pick\" }]);
        self.written += 1;
    }
    pub fn pick(data: any, on: any) {}
}
",
    );
    app.tick(1.0 / 60.0);
    assert!(
        number(&app, "frames").is_some_and(|n| n > 0.0),
        "the script ran"
    );
    assert_eq!(number(&app, "written"), Some(0.0));
    let said: Vec<String> = balaur_core::logbuf::heard()
        .into_iter()
        .map(|e| e.message)
        .collect();
    assert!(
        said.iter()
            .any(|line| line.contains("pick") && line.contains("target")),
        "the error names the method and what is missing: {said:#?}"
    );
}

// `scene.repeat` shows every node it hands back, and a heading's label is
// written hidden: the next write of the same table hides it again.
#[test]
fn a_hidden_control_shown_by_another_stays_hidden_on_its_next_write() {
    let (_dir, mut app) = app_with(
        "pub class Root { node: Node, frames: int }
impl Root {
    pub fn update(dt: float) {
        self.frames += 1;
        let a = self.node.get_node(\"Bar/A\");
        if self.frames == 2 {
            a.set_visible(true);
        }
        ui::write([a], [ui::Control { kind: \"label\", text: \"\", visible: false }]);
    }
}
",
    );
    for _ in 0..3 {
        app.tick(1.0 / 60.0);
    }
    assert_eq!(number(&app, "frames"), Some(3.0));
    assert!(
        !crate::support::own_visible(&app, node(&app, "Root/Bar/A")),
        "shown by another write, and hidden again by its own"
    );
}

// The range a number wore goes when its next table names none, whether or
// not the reader edited it between.
#[test]
fn an_edit_between_two_writes_keeps_what_the_node_wore_from_lingering() {
    let (_dir, mut app) = app_with(
        "pub class Root { node: Node, frames: int }
impl Root {
    pub fn update(dt: float) {
        self.frames += 1;
        let a = self.node.get_node(\"Bar/A\");
        let control: ui::Control = if self.frames < 3 {
            ui::Control { kind: \"number_field\", value: 0.5, min: 0.01, max: 2.0, prefix: \"px\" }
        } else {
            ui::Control { kind: \"number_field\", value: -24.0 }
        };
        ui::write([a], [control]);
        if self.frames == 2 {
            ui::edit(a, 0.75);
        }
    }
}
",
    );
    for _ in 0..5 {
        app.tick(1.0 / 60.0);
    }
    let field = node(&app, "Root/Bar/A");
    assert_eq!(
        prop(&app, field, "widget_range", "value"),
        Some(toml::Value::Float(-24.0))
    );
    assert_eq!(
        prop(&app, field, "widget_range", "max"),
        Some(toml::Value::Float(1.0)),
        "the edit did not keep the range the next table dropped"
    );
}
