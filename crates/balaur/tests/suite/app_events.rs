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
    assert_eq!(said(&app), "suspended true;low memory;insets 47.0;portrait;");
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
