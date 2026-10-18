//! What every script hears about the app and the player's hands: every mouse
//! button, an action let go, the device's changes and a script's own.

use balaur::{AppConfig, standard_app};
use balaur_core::variables::Variables;

fn app_with(script: &str) -> (tempfile::TempDir, balaur::App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"scenes/main.toml\"\n\n\
         [input.actions]\njump = [\"Space\"]\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), SCENE).unwrap();
    std::fs::write(dir.path().join("scenes/hear.rn"), script).unwrap();
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
script = { source = "scenes/hear.rn" }
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
const TELL: &str = "fn tell(what) {\n\
    scene::set_variable(\"heard\", scene::variable(\"heard\") + 1);\n\
    scene::set_variable(\"said\", `${scene::variable(\"said\")}${what};`);\n\
}\n";

#[test]
fn the_right_and_middle_buttons_reach_a_node_as_the_left_does() {
    let script = format!(
        "{TELL}pub fn on_pointer_down(this, button) {{ tell(`down ${{button}}`); }}\n\
         pub fn on_pointer_up(this, button) {{ tell(`up ${{button}}`); }}\n"
    );
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
    let script = format!(
        "{TELL}pub fn update(this, dt) {{\n\
             if scene::variable(\"heard\") == 0 {{ input::feed_action(\"jump\", 1.0); }}\n\
         }}\n\
         pub fn on_action(this, name) {{ tell(`pressed ${{name}}`); }}\n\
         pub fn on_action_released(this, name) {{ tell(`released ${{name}}`); }}\n"
    );
    let (_dir, mut app) = app_with(&script);
    for _ in 0..6 {
        app.tick(1.0 / 60.0);
    }
    assert_eq!(said(&app), "pressed jump;released jump;");
}

#[test]
fn the_device_s_changes_reach_every_script() {
    let script = format!(
        "{TELL}pub fn on_suspended_changed(this, suspended) {{ tell(`suspended ${{suspended}}`); }}\n\
         pub fn on_safe_area_changed(this, insets) {{ tell(`insets ${{insets[1]}}`); }}\n\
         pub fn on_orientation_changed(this, way) {{ tell(way); }}\n\
         pub fn on_low_memory(this) {{ tell(\"low memory\"); }}\n"
    );
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
    let script = format!(
        "{TELL}pub fn update(this, dt) {{\n\
             if scene::variable(\"said\") == \"\" {{\n\
                 scene::set_variable(\"said\", \"asked;\");\n\
                 settings::set(\"audio/volume\", 0.5);\n\
                 strings::set_locale(\"fr\");\n\
             }}\n\
         }}\n\
         pub fn on_setting_changed(this, change) {{ tell(change[\"path\"]); }}\n\
         pub fn on_locale_changed(this, locale) {{ tell(locale); }}\n"
    );
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
    let script = format!(
        "{TELL}pub fn init(this) {{ events::listen(this.node, \"logged\"); }}\n\
         pub fn update(this, dt) {{\n\
             if scene::variable(\"said\") == \"\" {{\n\
                 scene::set_variable(\"said\", \"asked;\");\n\
                 log::info(\"a line for the log view\");\n\
             }}\n\
         }}\n\
         pub fn on_logged(this, total) {{ tell(\"logged\"); }}\n"
    );
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
    let script = format!(
        "{TELL}pub fn init(this) {{ events::listen(this.node, \"assets_reloaded\"); }}\n\
         pub fn update(this, dt) {{\n\
             if scene::variable(\"said\") == \"\" {{\n\
                 scene::set_variable(\"said\", \"asked;\");\n\
                 assets::invalidate();\n\
             }}\n\
         }}\n\
         pub fn on_assets_reloaded(this, generation) {{ tell(\"reloaded\"); }}\n"
    );
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
    let script = format!(
        "{TELL}pub fn init(this) {{ events::listen(this.node, \"saved\"); }}\n\
         pub fn update(this, dt) {{\n\
             if scene::variable(\"said\") == \"\" {{\n\
                 scene::set_variable(\"said\", \"asked;\");\n\
                 save::write(\"probe\", #{{ n: 1 }});\n\
             }}\n\
         }}\n\
         pub fn on_saved(this, count) {{ tell(\"saved\"); }}\n"
    );
    let (dir, mut app) = app_with(&script);
    // Kept in the test's own folder, not the user's data directory.
    balaur_core::save::set_home(&app.engine, Some(dir.path().join("data")));
    for _ in 0..3 {
        app.tick(1.0 / 60.0);
    }
    assert!(said(&app).contains("saved;"), "{}", said(&app));
}

/// A camera listens for `pointer_scrolled` instead of reading the wheel each
/// frame: a turn is heard at the pump of the frame it happened in.
#[test]
fn a_script_listening_for_pointer_scrolled_hears_the_wheel() {
    let script = format!(
        "{TELL}pub fn init(this) {{ events::listen(this.node, \"pointer_scrolled\"); }}\n\
         pub fn on_pointer_scrolled(this, turn) {{ tell(`scrolled ${{turn[1]}}`); }}\n"
    );
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
    let script = format!(
        "{TELL}pub fn init(this) {{ events::listen(this.node, \"files_dropped\"); }}\n\
         pub fn on_files_dropped(this, paths) {{ tell(`dropped ${{paths[0]}}`); }}\n"
    );
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
