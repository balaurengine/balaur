//! What `balaur check` says about the names a project hears and sends: a
//! handler, a listener or a row naming an event nothing announces, and an
//! `emit` its script does not declare. Each warning has a control beside it
//! that stays silent.

use balaur::{AppConfig, standard_app};
use balaur_script::Value;
use std::path::Path;

/// A project of one scene, `main.toml`, and the files given beside it.
fn project(scene: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    for (rel, text) in files {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

/// A scene whose one node runs `ball.bs`, with `rows` as its bindings.
fn scene(rows: &str) -> String {
    format!(
        "[[nodes]]\nid = \"n\"\nname = \"Ball\"\nscript = {{ source = \"ball.bs\" }}\n\
         bindings = {{ rows = [{rows}] }}\n"
    )
}

/// Every finding as `file:line:severity`, with its message.
fn findings(root: &Path) -> Vec<(String, String)> {
    balaur::check_project(root)
        .unwrap()
        .into_iter()
        .map(|one| {
            (
                format!("{}:{}:{}", one.file, one.line, one.severity),
                one.message,
            )
        })
        .collect()
}

/// The one finding of a project, where it is, checked to say `wanted`.
fn one_warning(root: &Path, wanted: &str) -> String {
    let found = findings(root);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].1.contains(wanted), "{found:?}");
    found[0].0.clone()
}

const STALE_HANDLER: &str = "pub class Ball { node: Node }
impl Ball {
    pub fn on_scorred(points: int) {}
}
";

#[test]
fn a_handler_for_an_event_nothing_announces_is_a_warning_on_its_line() {
    let dir = project(&scene(""), &[("ball.bs", STALE_HANDLER)]);
    let at = one_warning(dir.path(), "`on_scorred` hears nothing");
    assert_eq!(at, "ball.bs:3:warning");
}

#[test]
fn a_handler_is_heard_by_a_hook_a_component_a_node_a_module_and_a_declaration() {
    let ball = "/// The ball left the table.
event scored;

pub class Ball { node: Node }
impl Ball {
    pub fn on_scored(points: int) {}
    pub fn on_free() {}
    pub fn on_pointer_click(button: any) {}
    pub fn on_timeout(payload: any) {}
    pub fn on_child_added(child: any) {}
    pub fn on_tween_finished(tween: any) {}
    pub fn on_pointer_moved(at: any) {}
    pub fn on_websocket_event(event: any) {}
    pub fn on_raised(payload: any) {}
}
";
    let rows = "{ event = \"pointer_click\", action = \"emit\", value = \"raised\" }";
    let dir = project(&scene(rows), &[("ball.bs", ball)]);
    assert_eq!(findings(dir.path()), []);
}

#[test]
fn a_handler_something_calls_by_name_is_left_alone() {
    let ball = "use helper;

pub class Ball { node: Node }
impl Ball {
    pub fn init() {
        self.on_direct();
        self.node.call(\"on_literal\");
        self.node.call(helper::NAMED);
        self.node.call(\"on_{helper::KIND}_built\");
    }
    pub fn on_direct() {}
    pub fn on_literal() {}
    pub fn on_constant() {}
    pub fn on_any_built() {}
    pub fn on_pick(payload: any, source: any) {}
}
";
    let helper = "pub const NAMED: str = \"on_constant\";\npub const KIND: str = \"any\";\n";
    let rows = "{ event = \"pointer_click\", action = \"call\", value = \"on_pick\" }";
    let dir = project(&scene(rows), &[("ball.bs", ball), ("helper.bs", helper)]);
    assert_eq!(findings(dir.path()), []);
}

fn listener(first: &str, second: &str) -> String {
    format!(
        "const LOST: str = \"lost\";

event scored;
event lost;

pub class Ball {{ node: Node }}
impl Ball {{
    pub async fn init() {{
        events::listen(self.node, {first});
        events::listen(self.node, \"timeout\", self.node);
        let token = events::next({second});
        await task::wait(token);
    }}
}}
"
    )
}

#[test]
fn listening_for_an_event_nothing_announces_is_a_warning_on_its_line() {
    let dir = project(&scene(""), &[("ball.bs", &listener("\"scored\"", "LOST"))]);
    assert_eq!(findings(dir.path()), []);
    let dir = project(&scene(""), &[("ball.bs", &listener("\"scorred\"", "LOST"))]);
    let at = one_warning(dir.path(), "nothing announces `scorred`");
    assert_eq!(at, "ball.bs:9:warning");
    let dir = project(
        &scene(""),
        &[("ball.bs", &listener("\"scored\"", "\"gone\""))],
    );
    let at = one_warning(dir.path(), "nothing announces `gone`");
    assert_eq!(at, "ball.bs:11:warning");
}

const QUIET: &str = "event scored;\n\npub class Ball { node: Node }\n";

#[test]
fn a_row_answering_an_event_nothing_announces_is_a_warning_on_its_line() {
    let heard = "{ event = \"emitted:scored\", action = \"free\" }, \
                 { event = \"emitted:timeout\", action = \"free\" }";
    let dir = project(&scene(heard), &[("ball.bs", QUIET)]);
    assert_eq!(findings(dir.path()), []);
    let stale = "{ event = \"emitted:scorred\", action = \"free\" }";
    let dir = project(&scene(stale), &[("ball.bs", QUIET)]);
    let at = one_warning(
        dir.path(),
        "nothing announces `scorred`, which this row answers",
    );
    assert_eq!(at, "main.toml:5:warning");
}

#[test]
fn a_row_written_as_a_table_of_its_own_is_read_the_same() {
    let scene = "[[nodes]]\nid = \"n\"\nname = \"Ball\"\nscript = { source = \"ball.bs\" }\n\n\
                 [[nodes.bindings.rows]]\nevent = \"emitted:scorred\"\naction = \"free\"\n";
    let dir = project(scene, &[("ball.bs", QUIET)]);
    let at = one_warning(dir.path(), "nothing announces `scorred`");
    assert_eq!(at, "main.toml:7:warning");
}

fn sender(declared: &str) -> String {
    format!(
        "const SCORED: str = \"scored\";
{declared}
pub class Ball {{ node: Node }}
impl Ball {{
    pub fn init() {{
        self.node.emit(SCORED, 3);
    }}
}}
"
    )
}

#[test]
fn sending_an_event_the_script_does_not_declare_is_a_warning_on_its_line() {
    let dir = project(&scene(""), &[("ball.bs", &sender("event scored;"))]);
    assert_eq!(findings(dir.path()), []);
    let dir = project(&scene(""), &[("ball.bs", &sender(""))]);
    let at = one_warning(dir.path(), "`scored` is sent but not declared");
    assert_eq!(at, "ball.bs:6:warning");
}

#[test]
fn an_event_another_script_of_the_project_declares_is_announced() {
    let hearer = "pub class Ball { node: Node }
impl Ball {
    pub fn init() { events::listen(self.node, \"saved\"); }
    pub fn on_saved(count: int) {}
}
";
    let addon = "event saved;\n\npub fn write() { events::emit(\"saved\", 1); }\n";
    let dir = project(
        &scene(""),
        &[("ball.bs", hearer), ("addons/save/save.bs", addon)],
    );
    assert_eq!(findings(dir.path()), []);
}

#[test]
fn a_node_offers_the_events_its_script_declares() {
    let ball = "/// The ball left the table.
event scored;

pub class Ball { node: Node }
impl Ball { pub fn offered() -> [str] { scene::bindable_events(self.node) } }
";
    let dir = project(&scene(""), &[("ball.bs", ball)]);
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    let ball = {
        let world = app.engine.world();
        balaur::scene::find_node(&world, app.engine.root(), "Ball").unwrap()
    };
    let host = app.engine.script_host().unwrap();
    assert_eq!(
        host.events("ball.bs"),
        [("scored".to_string(), "The ball left the table.".to_string())]
    );
    let offered = host.call_on(balaur::node_id_of(ball), "offered", &[]);
    let Some(Value::List(offered)) = offered else {
        panic!("no list of events: {offered:?}");
    };
    for wanted in ["emitted:scored", "emitted:tween_finished"] {
        assert!(
            offered.contains(&Value::Str(wanted.into())),
            "`{wanted}` is not offered: {offered:?}"
        );
    }
}

#[test]
fn every_event_said_from_no_node_is_declared_with_its_payload() {
    let dir = project(&scene(""), &[("ball.bs", QUIET)]);
    let app = standard_app(AppConfig::export(dir.path())).unwrap();
    let said: Vec<&str> = balaur::events::said(&app.engine)
        .iter()
        .map(|(name, ..)| *name)
        .collect();
    for wanted in [
        "pointer_moved",
        "pointer_pressed",
        "pointer_released",
        "pointer_scrolled",
        "pointer_gestured",
        "files_dropped",
        "pasted",
        "image_pasted",
    ] {
        assert!(
            said.contains(&wanted),
            "`{wanted}` is not declared: {said:?}"
        );
    }
    for (name, payload, when) in balaur::events::said(&app.engine) {
        assert!(!payload.is_empty() && !when.is_empty(), "`{name}`");
    }
}
