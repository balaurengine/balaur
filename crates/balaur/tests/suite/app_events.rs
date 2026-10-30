//! What every script hears about the app and the player's hands: every mouse
//! button, an action let go, the device's changes and a script's own.

use balaur::{AppConfig, standard_app};
use balaur_core::variables::Variables;

fn app_with(script: &str) -> (tempfile::TempDir, balaur::App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"scenes/main.toml\"\n\n\
         [input.actions]\njump = [\"Space\"]\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), SCENE).unwrap();
    std::fs::write(dir.path().join("scenes/ear.bs"), script).unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

const SCENE: &str = r#"
[variables]
heard = { type = "int", value = 0 }
said = { type = "text", value = "" }

[[nodes]]
id = "n_ear"
name = "Ear"
script = { source = "scenes/ear.bs" }
"#;

fn variable(app: &balaur::App, name: &str) -> balaur_script::Value {
    let variables = app.engine.resource::<Variables>();
    let held = variables.borrow();
    held.get(name).cloned().unwrap_or(balaur_script::Value::Nil)
}

fn heard(app: &balaur::App) -> i64 {
    balaur_core::variables::as_num(&variable(app, "heard")) as i64
}

fn said(app: &balaur::App) -> String {
    match variable(app, "said") {
        balaur_script::Value::Str(text) => text,
        other => panic!("said is {other:?}"),
    }
}

/// Appends what it heard to `said`, so the order shows too.
const TELL: &str = r#"fn tell(what: str) {
    scene::set_variable("heard", scene::variable("heard").as_int() + 1);
    let said = scene::variable("said").as_str();
    scene::set_variable("said", said + what + ";");
}
"#;

/// The ear's script: `tell` and a node script whose methods are `methods`.
fn ear(methods: &str) -> String {
    format!("{TELL}pub class Ear {{ node: Node }}\nimpl Ear {{\n{methods}}}\n")
}

#[test]
fn the_right_and_middle_buttons_reach_a_node_as_the_left_does() {
    let script = ear(r#"
    pub fn on_pointer_down(button: str) { tell("down {button}"); }
    pub fn on_pointer_up(button: str) { tell("up {button}"); }
"#);
    let (_dir, mut app) = app_with(&script);
    app.tick(1.0 / 60.0);
    for (button, down) in [(1, true), (2, true), (1, false), (2, false)] {
        {
            let input = app.engine.resource::<balaur::input::InputSnapshot>();
            let mut input = input.borrow_mut();
            input.begin_frame();
            input.mouse_button_event(button, down);
        }
        app.tick(1.0 / 60.0);
    }
    assert_eq!(said(&app), "down right;down middle;up right;up middle;");
}

#[test]
fn an_action_let_go_is_heard_as_one_pressed_is() {
    let script = ear(r#"
    fn update(dt: float) {
        if scene::variable("heard").as_int() == 0 { input::feed_action("jump", 1.0); }
    }
    pub fn on_action(name: str) { tell("pressed {name}"); }
    pub fn on_action_released(name: str) { tell("released {name}"); }
"#);
    let (_dir, mut app) = app_with(&script);
    for _ in 0..6 {
        app.tick(1.0 / 60.0);
    }
    assert_eq!(said(&app), "pressed jump;released jump;");
}

#[test]
fn the_device_s_changes_reach_every_script() {
    let script = ear(r#"
    pub fn on_suspended_changed(suspended: bool) { tell("suspended {suspended}"); }
    pub fn on_safe_area_changed(insets: any) { tell("insets {insets.at(1)}"); }
    pub fn on_orientation_changed(way: str) { tell(way); }
    pub fn on_low_memory() { tell("low memory"); }
"#);
    let (_dir, mut app) = app_with(&script);
    balaur_core::facts::update_device(&app.engine, |device| device.screen_size = [800.0, 600.0]);
    app.tick(1.0 / 60.0);
    assert_eq!(heard(&app), 0, "a first size is the screen appearing");
    balaur_core::facts::update_device(&app.engine, |device| {
        device.suspended = true;
        device.safe_area = [0.0, 47.0, 0.0, 34.0];
        device.screen_size = [600.0, 800.0];
        device.memory_warnings += 1;
    });
    app.tick(1.0 / 60.0);
    assert_eq!(
        said(&app),
        "suspended true;low memory;insets 47.0;portrait;"
    );
}

#[test]
fn a_setting_and_a_language_a_script_changes_are_heard_the_next_frame() {
    let script = ear(r#"
    fn update(dt: float) {
        if scene::variable("said").as_str() == "" {
            scene::set_variable("said", "asked;");
            settings::set("audio/volume", 0.5);
            strings::set_locale("fr");
        }
    }
    pub fn on_setting_changed(change: any) { tell(change.get("path").as_str()); }
    pub fn on_locale_changed(locale: str) { tell(locale); }
"#);
    let (_dir, mut app) = app_with(&script);
    for _ in 0..3 {
        app.tick(1.0 / 60.0);
    }
    assert_eq!(said(&app), "asked;audio/volume;fr;");
}

/// A log view listens for `logged` instead of reading the buffer each frame:
/// a line written is heard at the next pump.
#[test]
fn a_script_listening_for_logged_hears_a_line_written() {
    balaur_core::logbuf::capture_for_test();
    let script = ear(r#"
    fn init() { events::listen(self.node, "logged"); }
    fn update(dt: float) {
        if scene::variable("said").as_str() == "" {
            scene::set_variable("said", "asked;");
            log::info("a line for the log view");
        }
    }
    pub fn on_logged(total: any) { tell("logged"); }
"#);
    let (_dir, mut app) = app_with(&script);
    for _ in 0..3 {
        app.tick(1.0 / 60.0);
    }
    assert!(said(&app).contains("logged;"), "{}", said(&app));
}

/// A file view listens for `assets_reloaded` instead of listing the project
/// each frame: a file declared stale is heard at the next pump.
#[test]
fn a_script_listening_for_assets_reloaded_hears_an_invalidate() {
    let script = ear(r#"
    fn init() { events::listen(self.node, "assets_reloaded"); }
    fn update(dt: float) {
        if scene::variable("said").as_str() == "" {
            scene::set_variable("said", "asked;");
            assets::invalidate();
        }
    }
    pub fn on_assets_reloaded(generation: any) { tell("reloaded"); }
"#);
    let (_dir, mut app) = app_with(&script);
    for _ in 0..3 {
        app.tick(1.0 / 60.0);
    }
    assert!(said(&app).contains("reloaded;"), "{}", said(&app));
}

/// A view of a game's saves listens for `saved` instead of listing the
/// folder each frame: a slot written is heard at the next pump.
#[test]
fn a_script_listening_for_saved_hears_a_slot_written() {
    let script = ear(r#"
    fn init() { events::listen(self.node, "saved"); }
    fn update(dt: float) {
        if scene::variable("said").as_str() == "" {
            scene::set_variable("said", "asked;");
            save::write("probe", map().with("n", 1));
        }
    }
    pub fn on_saved(count: any) { tell("saved"); }
"#);
    let (dir, mut app) = app_with(&script);
    // Kept in the test's own folder, not the user's data directory.
    balaur_core::save::set_home(&app.engine, Some(dir.path().join("data")));
    for _ in 0..3 {
        app.tick(1.0 / 60.0);
    }
    assert!(said(&app).contains("saved;"), "{}", said(&app));
}

/// An editor's views listen for `changed` instead of comparing the scene
/// each frame: the writes of a frame are heard at the next pump, one entry
/// for each node and component however often it was written, a component a
/// write put on as `attached`, and a write is kept only from the frame after
/// someone listens.
#[test]
fn a_script_listening_for_changed_hears_what_the_writes_changed() {
    let script = ear(r#"
    fn init() { events::listen(self.node, "changed"); }
    fn update(dt: float) {
        let said = scene::variable("said").as_str();
        if said == "" {
            scene::set_variable("said", "waited;");
        } else if said == "waited;" {
            scene::set_variable("said", "wrote;");
            self.node.transform.position = vec3(1.0, 2.0, 3.0);
            self.node.transform.scale = vec3(2.0, 2.0, 2.0);
            self.node.transform.position = vec3(4.0, 5.0, 6.0);
            self.node.set_name("Renamed");
            let child = self.node.add_child("Child");
            child.queue_free();
        }
    }
    pub fn on_changed(entries: any) {
        let index = 0;
        while index < entries.len() {
            let entry = entries.at(index);
            let kind = entry.get("kind").as_str();
            if kind == "component" {
                tell(kind + " " + entry.get("component").as_str() + " " + entry.get("properties").text());
            } else {
                tell(kind);
            }
            index += 1;
        }
    }
"#);
    let (_dir, mut app) = app_with(&script);
    for _ in 0..4 {
        app.tick(1.0 / 60.0);
    }
    let said = said(&app);
    assert_eq!(
        said, r#"wrote;attached;component transform ["scale", "position"];renamed;added;freed;"#,
        "the transform put on by its first write, one entry for the writes after, then the tree's in order"
    );
}

/// Heard from a node, `changed` holds what happened under it alone: an
/// editor listening from the scene it edits does not hear its own widgets.
#[test]
fn a_script_listening_for_changed_under_a_node_hears_that_subtree() {
    let script = ear(r#"
    fn init() { events::listen(self.node, "changed", self.node); }
    fn update(dt: float) {
        let said = scene::variable("said").as_str();
        if said == "" {
            scene::set_variable("said", "waited;");
        } else if said == "waited;" {
            scene::set_variable("said", "wrote;");
            self.node.add_child("Inside");
            scene::root().add_child("Outside");
        }
    }
    pub fn on_changed(entries: any) {
        let index = 0;
        while index < entries.len() {
            let entry = entries.at(index);
            tell(entry.get("kind").as_str() + " " + entry.get("node").as_node().name());
            index += 1;
        }
    }
"#);
    let (_dir, mut app) = app_with(&script);
    for _ in 0..4 {
        app.tick(1.0 / 60.0);
    }
    assert_eq!(said(&app), "wrote;added Inside;");
}

/// A camera listens for `pointer_scrolled` instead of reading the wheel each
/// frame: a turn is heard at the pump of the frame it happened in.
#[test]
fn a_script_listening_for_pointer_scrolled_hears_the_wheel() {
    let script = ear(r#"
    fn init() { events::listen(self.node, "pointer_scrolled"); }
    pub fn on_pointer_scrolled(turn: any) { tell("scrolled {turn.at(1)}"); }
"#);
    let (_dir, mut app) = app_with(&script);
    app.tick(1.0 / 60.0);
    {
        let input = app.engine.resource::<balaur::input::InputSnapshot>();
        let mut input = input.borrow_mut();
        input.begin_frame();
        input.add_scroll(0.0, 2.0);
    }
    app.tick(1.0 / 60.0);
    assert_eq!(said(&app), "scrolled 2.0;");
}

/// An import listens for `files_dropped` instead of asking each frame: a
/// drop is heard at the pump of the frame it landed in, with its paths.
#[test]
fn a_script_listening_for_files_dropped_hears_the_paths() {
    let script = ear(r#"
    fn init() { events::listen(self.node, "files_dropped"); }
    pub fn on_files_dropped(paths: any) { tell("dropped {paths.at(0).as_str()}"); }
"#);
    let (_dir, mut app) = app_with(&script);
    app.tick(1.0 / 60.0);
    {
        let input = app.engine.resource::<balaur::input::InputSnapshot>();
        let mut input = input.borrow_mut();
        input.begin_frame();
        input.file_drop_event("/tmp/a.png".into());
    }
    app.tick(1.0 / 60.0);
    assert_eq!(said(&app), "dropped /tmp/a.png;");
}
