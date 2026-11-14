//! `ui::fill_strip`: widget nodes made, reused and hidden from a list a
//! script hands over every frame, and a reader's edit reaching its keys.

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
        "[[nodes]]\nid = \"root\"\nname = \"Root\"\nscript = { source = \"root.bs\" }\n\n\
         [[nodes]]\nid = \"bar\"\nname = \"Bar\"\nparent = \"root\"\nwidget = { kind = \"row\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("root.bs"), body).unwrap();
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
    crate::support::script_number(app, root, name)
}

#[test]
fn a_strip_makes_a_node_a_control_and_hides_what_a_shorter_list_leaves() {
    let (_dir, mut app) = app_with(
        "pub class Root { node: Node, count: int, frames: int }
impl Root {
    fn init() { self.count = 3; }
    fn update(dt: float) {
        self.frames += 1;
        let controls: [any] = [];
        for index in 0..self.count { controls.push({ \"kind\": \"label\", \"text\": \"L{index}\" }); }
        ui::fill_strip(\"Root/Bar\", controls);
        self.count = 1;
    }
}
",
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
    assert!(crate::support::own_visible(&app, after[0]));
    assert!(!crate::support::own_visible(&app, after[1]));
    assert!(!crate::support::own_visible(&app, after[2]));
}

// A control's one table: each row goes to the component its kind reads, and
// a slot that turns into another kind drops what the last one wore.
#[test]
fn a_pooled_row_goes_to_the_component_its_kind_reads() {
    let (_dir, mut app) = app_with(
        "pub class Root { node: Node, frames: int }
impl Root {
    fn update(dt: float) {
        self.frames += 1;
        let controls: [any] = [];
        if self.frames == 1 {
            controls.push({ \"kind\": \"text_field\", \"prefix\": \"at\" });
        } else {
            controls.push({ \"kind\": \"label\", \"text\": \"plain\" });
        }
        controls.push({ \"kind\": \"number_field\", \"prefix\": \"at\" });
        ui::fill_strip(\"Root/Bar\", controls);
    }
}
",
    );
    app.tick(1.0 / 60.0);
    let made = kids(&app);
    let row = |node, component: &str| {
        balaur_core::components::property(&app.engine, node, component, "prefix")
    };
    let at = Some(toml::Value::String("at".into()));
    assert_eq!(row(made[0], "widget_field"), at);
    assert_eq!(row(made[1], "widget_range"), at);
    assert!(!balaur_core::components::has(
        &app.engine,
        made[0],
        "widget_range"
    ));
    app.tick(1.0 / 60.0);
    assert!(
        !balaur_core::components::has(&app.engine, made[0], "widget_field"),
        "a slot that became a label still wears its field's rows"
    );
}

#[test]
fn a_reader_s_click_reaches_its_key_and_a_script_s_write_does_not() {
    let (_dir, mut app) = app_with(
        "pub class Root { node: Node, on: bool, edits: int }
impl Root {
    fn update(dt: float) {
        let controls: [any] = [{ \"kind\": \"checkbox\", \"checked\": self.on, \"on_change\": \"toggled\" }];
        ui::fill_strip(\"Root/Bar\", controls);
    }
    pub fn toggled(data: any, v: any) {
        self.edits += 1;
        self.on = v.as_bool();
    }
}
",
    );
    app.tick(1.0 / 60.0);
    let boxed = kids(&app)[0];
    assert_eq!(
        balaur_core::components::property(&app.engine, boxed, "widget_check", "checked"),
        Some(toml::Value::Boolean(false))
    );

    assert!(balaur_ui::click(&app.engine, boxed, false));
    app.tick(1.0 / 60.0);
    assert_eq!(number(&app, "edits"), Some(1.0));
    assert_eq!(
        balaur_core::components::property(&app.engine, boxed, "widget_check", "checked"),
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
        balaur_core::components::property(&app.engine, boxed, "widget_check", "checked"),
        Some(toml::Value::Boolean(false)),
        "and the pool writes only what its own spec changed"
    );
}

// A spec's handler name is the pool's to keep: the pooled label senses the
// pointer's click for it, the method above is handed the spec's data, a
// `true` and the control, and the widget holds no such row.
#[test]
fn a_pooled_label_s_click_calls_the_method_its_spec_names() {
    let (_dir, mut app) = app_with(
        "pub class Root { node: Node, picked: str, with: bool }
impl Root {
    fn init() {
        scene::get_node(\"Root/Bar\").patch_component(\"widget\", { \"x\": 0.0, \"y\": 0.0 });
    }
    fn update(dt: float) {
        let controls: [any] = [
            { \"kind\": \"label\", \"text\": \"One\", \"width\": 80, \"on_click\": \"pick\", \"data\": \"one\" },
            { \"kind\": \"label\", \"text\": \"Two\", \"width\": 80 },
        ];
        ui::fill_strip(\"Root/Bar\", controls);
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
    let made = kids(&app);
    assert_eq!(prop(&app, made[0], "on_click"), None, "no row holds it");
    let root = balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), "Root")
        .expect("the scene has a Root node");
    let picked = |app: &App| crate::support::script_text(app, root, "picked");
    let click = |app: &mut App, slot: Entity| {
        let at = balaur_ui::widget_rect(slot).expect("drawn").center();
        crate::support::pass(app, &ctx, crate::support::press(at, true));
        crate::support::pass(app, &ctx, crate::support::press(at, false));
        app.tick(1.0 / 60.0);
    };
    click(&mut app, made[1]);
    assert_eq!(
        picked(&app),
        Some(String::new()),
        "a label naming none lets it by"
    );
    click(&mut app, made[0]);
    assert_eq!(picked(&app), Some("one from C0".into()));
    let host = app.engine.script_host().unwrap();
    assert_eq!(
        host.script_field(balaur_core::node_id_of(root), "with"),
        Some(balaur_script::Value::Bool(true)),
        "a pooled click is a `true`"
    );
}

// The editor hides its inspector's form with a plain write when nothing is
// selected, and selecting the same node a third time drew an empty panel.
#[test]
fn a_host_a_script_hid_shows_again_every_time_it_is_filled() {
    let (_dir, mut app) = app_with(
        "pub class Root { node: Node, frames: int }
impl Root {
    fn update(dt: float) {
        self.frames += 1;
        if self.frames % 2 == 0 {
            scene::get_node(\"Root/Bar\").set_visible(false);
        } else {
            let label: [any] = [{ \"kind\": \"label\", \"text\": \"x\" }];
            ui::fill_strip(\"Root/Bar\", label);
        }
    }
}
",
    );
    for frame in 1..=7 {
        app.tick(1.0 / 60.0);
        let shown = frame % 2 == 1;
        assert_eq!(
            crate::support::own_visible(&app, bar(&app)),
            shown,
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

// A reused slot wears only what its new control states. The range an
// earlier control left on it clamped the next one's number to 0.
#[test]
fn a_control_keeps_nothing_the_last_one_in_its_slot_stated() {
    let (_dir, mut app) = app_with(
        "pub class Root { node: Node, frames: int, edits: int }
impl Root {
    fn init() {
        scene::get_node(\"Root/Bar\").patch_component(\"widget\", { \"kind\": \"column\", \"anchor\": \"fill\" });
    }
    fn update(dt: float) {
        self.frames += 1;
        let control: any = if self.frames < 3 {
            {
                \"kind\": \"number_field\",
                \"grow\": 1,
                \"value\": 0.5,
                \"min\": 0.01,
                \"max\": 2.0,
                \"prefix\": \"px\",
                \"on_change\": \"edited\",
            }
        } else {
            { \"kind\": \"number_field\", \"grow\": 1, \"value\": -24.0, \"on_change\": \"edited\" }
        };
        ui::fill_strip(\"Root/Bar\", [control]);
    }
    pub fn edited(data: any, v: any) { self.edits += 1; }
}
",
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
        balaur_core::components::property(&app.engine, field[0], "widget_range", "value"),
        Some(toml::Value::Float(-24.0))
    );
    assert_eq!(
        balaur_core::components::property(&app.engine, field[0], "widget_range", "max"),
        Some(toml::Value::Float(1.0)),
        "the range went with the control that stated it"
    );
    assert_eq!(
        balaur_core::components::property(&app.engine, field[0], "widget_range", "prefix"),
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

/// What a slot's control says, "" for one that says nothing.
fn row_text(app: &App, slot: Entity) -> String {
    prop(app, slot, "text")
        .and_then(|text| text.as_str().map(ToOwned::to_owned))
        .unwrap_or_default()
}

/// What a group's first control says.
fn first_text(app: &App, group: Entity) -> String {
    let world = app.engine.world();
    let first = balaur_core::scene::find_node(&world, group, "C0");
    drop(world);
    first.map(|first| row_text(app, first)).unwrap_or_default()
}

/// One slot a frame: the third on the first, then the first, then the third
/// hidden, then the third again.
const ONE_ROW_A_FRAME: &str = "pub class Root { node: Node, frame: int }
fn row(said: str, y: float) -> any {
    { \"kind\": \"label\", \"text\": said, \"absolute\": true, \"inset\": ui::sides(0.0, y, 0.0, -1.0) }
}
impl Root {
    fn update(dt: float) {
        self.frame += 1;
        let host = scene::get_node(\"Root/Bar\");
        if self.frame == 1 {
            ui::fill_slot(host, 2, row(\"third\", 20.0));
        } else if self.frame == 2 {
            ui::fill_slot(host, 0, row(\"first\", 0.0));
        } else if self.frame == 3 {
            ui::fill_slot(host, 2);
        } else if self.frame == 4 {
            ui::fill_slot(host, 2, row(\"back\", 20.0));
        }
    }
}
";

#[test]
fn one_slot_is_made_with_those_before_it_and_placed_where_it_says() {
    let (_dir, mut app) = app_with(ONE_ROW_A_FRAME);
    app.tick(1.0 / 60.0);
    let rows = kids(&app);
    assert_eq!(
        rows.len(),
        3,
        "slot 2 needs slots 0 and 1 to stand before it"
    );
    assert_eq!(row_text(&app, rows[2]), "third");
    assert_eq!(
        prop(&app, rows[2], "absolute"),
        Some(toml::Value::Boolean(true)),
        "a control placed by hand"
    );
    let inset = prop(&app, rows[2], "inset").expect("its inset");
    assert_eq!(
        balaur_core::components::as_f64(&inset.as_array().unwrap()[1]),
        Some(20.0)
    );
}

#[test]
fn writing_one_slot_leaves_the_others_as_they_are() {
    let (_dir, mut app) = app_with(ONE_ROW_A_FRAME);
    app.tick(1.0 / 60.0);
    let rows = kids(&app);
    app.tick(1.0 / 60.0);
    assert_eq!(kids(&app), rows, "the rows are reused, not made again");
    assert_eq!(row_text(&app, rows[0]), "first");
    assert_eq!(row_text(&app, rows[2]), "third", "not asked about");
}

#[test]
fn a_slot_given_nothing_is_hidden_and_shows_again_when_it_is_filled() {
    let (_dir, mut app) = app_with(ONE_ROW_A_FRAME);
    app.tick(1.0 / 60.0);
    app.tick(1.0 / 60.0);
    let rows = kids(&app);
    assert!(crate::support::own_visible(&app, rows[2]));
    app.tick(1.0 / 60.0);
    assert!(!crate::support::own_visible(&app, rows[2]));
    assert!(
        crate::support::own_visible(&app, rows[0]),
        "the other stays"
    );
    app.tick(1.0 / 60.0);
    assert!(crate::support::own_visible(&app, rows[2]));
    assert_eq!(row_text(&app, rows[2]), "back");
}

/// A row filled, then its text patched, then a fill asking for what it first
/// held.
const PATCHED_THEN_FILLED: &str =
    "pub class Root { node: Node, frame: int, wrote: bool, again: bool }
fn row(said: str) -> any {
    { \"kind\": \"row\", \"controls\": [{ \"kind\": \"label\", \"text\": said }] }
}
impl Root {
    fn update(dt: float) {
        self.frame += 1;
        let host = scene::get_node(\"Root/Bar\");
        if self.frame == 1 {
            ui::fill_slot(host, 0, row(\"first\"));
        } else if self.frame == 2 {
            self.wrote = ui::patch_control(host, [0, 0], ui::Control { text: \"moved\" });
        } else if self.frame == 3 {
            self.again = ui::patch_control(host, [0, 0], ui::Control { text: \"moved\" });
        } else if self.frame == 4 {
            ui::fill_slot(host, 0, row(\"first\"));
        }
    }
}
";

#[test]
fn a_patch_writes_what_moved_and_the_next_fill_compares_against_it() {
    let (_dir, mut app) = app_with(PATCHED_THEN_FILLED);
    app.tick(1.0 / 60.0);
    let rows = kids(&app);
    assert_eq!(first_text(&app, rows[0]), "first");
    app.tick(1.0 / 60.0);
    assert_eq!(first_text(&app, rows[0]), "moved");
    let root =
        balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), "Root").unwrap();
    let flag = |app: &App, name: &str| {
        app.engine
            .script_host()
            .and_then(|host| host.script_field(balaur_core::node_id_of(root), name))
    };
    assert_eq!(flag(&app, "wrote"), Some(balaur_script::Value::Bool(true)));
    app.tick(1.0 / 60.0);
    assert_eq!(
        flag(&app, "again"),
        Some(balaur_script::Value::Bool(false)),
        "what it holds already is not written"
    );
    app.tick(1.0 / 60.0);
    assert_eq!(
        first_text(&app, rows[0]),
        "first",
        "a fill reads the patch, not what it last wrote itself"
    );
}
