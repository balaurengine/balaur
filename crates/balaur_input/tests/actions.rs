//! Named actions over the raw snapshot: what a project declares, what a
//! player rebinds, and the edges a game reads.

use balaur_core::{App, AppConfig};
use balaur_input::{InputActions, InputPlugin, InputSnapshot};

const MANIFEST: &str = r#"
name = "actions test"
main_scene = "main.toml"

[input.actions]
jump = ["Space", "gamepad:South"]
move_x = ["keys:A,D", "axis:LeftStickX"]
fire = ["mouse:left"]
"#;

/// A booted app with the input plugin and a project that declares actions.
/// The main scene is empty: nothing here needs a node.
fn app(manifest: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.toml"), manifest).unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    let mut app = App::new(AppConfig {
        project_root: dir.path().to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    app.add_plugin(InputPlugin).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

/// One frame: the backend's `begin_frame`, then whatever the test presses,
/// then the tick that derives the actions from it.
fn frame(app: &mut App, press: &[(&str, bool)]) {
    {
        let input = app.engine.resource::<InputSnapshot>();
        let mut input = input.borrow_mut();
        input.begin_frame();
        for (key, down) in press {
            input.key_event(key, *down);
        }
    }
    app.tick(1.0 / 60.0);
}

fn value(app: &App, name: &str) -> f32 {
    app.engine.resource::<InputActions>().borrow().value(name)
}

fn pressed(app: &App, name: &str) -> bool {
    app.engine.resource::<InputActions>().borrow().is_pressed(name)
}

fn just_pressed(app: &App, name: &str) -> bool {
    app.engine
        .resource::<InputActions>()
        .borrow()
        .just_pressed(name)
}

fn just_released(app: &App, name: &str) -> bool {
    app.engine
        .resource::<InputActions>()
        .borrow()
        .just_released(name)
}

#[test]
fn a_declared_action_reads_the_key_it_is_bound_to() {
    let (_dir, mut app) = app(MANIFEST);
    frame(&mut app, &[]);
    assert!(!pressed(&app, "jump"));

    frame(&mut app, &[("Space", true)]);
    assert!(pressed(&app, "jump"));
    assert!(just_pressed(&app, "jump"));
    assert!((value(&app, "jump") - 1.0).abs() < 1e-6);
}

#[test]
fn an_edge_fires_for_one_frame_the_way_a_key_does() {
    let (_dir, mut app) = app(MANIFEST);
    frame(&mut app, &[("Space", true)]);
    assert!(just_pressed(&app, "jump"));

    frame(&mut app, &[]);
    assert!(!just_pressed(&app, "jump"), "still just-pressed a frame on");
    assert!(pressed(&app, "jump"), "the key is still held");

    frame(&mut app, &[("Space", false)]);
    assert!(!pressed(&app, "jump"));
    assert!(just_released(&app, "jump"));

    frame(&mut app, &[]);
    assert!(!just_released(&app, "jump"));
}

#[test]
fn a_key_pair_reads_as_an_axis() {
    let (_dir, mut app) = app(MANIFEST);
    frame(&mut app, &[("D", true)]);
    assert!((value(&app, "move_x") - 1.0).abs() < 1e-6);

    frame(&mut app, &[("D", false), ("A", true)]);
    assert!((value(&app, "move_x") + 1.0).abs() < 1e-6);

    // Both ends at once cancel, which is what a keyboard "axis" should do.
    frame(&mut app, &[("D", true)]);
    assert!(value(&app, "move_x").abs() < 1e-6);
}

#[test]
fn an_undeclared_action_reads_zero_rather_than_failing() {
    let (_dir, mut app) = app(MANIFEST);
    frame(&mut app, &[("Space", true)]);
    assert!(value(&app, "crouch").abs() < 1e-6);
    assert!(!pressed(&app, "crouch"));
}

#[test]
fn a_binding_that_does_not_parse_is_dropped_and_the_rest_still_work() {
    let (_dir, mut app) = app(
        r#"
name = "actions test"
main_scene = "main.toml"

[input.actions]
jump = ["Spacebar", "Space"]
"#,
    );
    frame(&mut app, &[("Space", true)]);
    assert!(pressed(&app, "jump"), "the one good binding still fires");
}

#[test]
fn rebinding_replaces_what_the_project_declared() {
    let (_dir, mut app) = app(MANIFEST);
    // The map loads on the first tick, so give it one before rebinding.
    frame(&mut app, &[]);
    app.engine
        .resource::<InputActions>()
        .borrow_mut()
        .rebind("jump", &[String::from("J")])
        .unwrap();

    frame(&mut app, &[("Space", true)]);
    assert!(!pressed(&app, "jump"), "the old binding still fires");
    frame(&mut app, &[("Space", false), ("J", true)]);
    assert!(pressed(&app, "jump"));
}

#[test]
fn rebinding_to_something_unparseable_is_refused() {
    let (_dir, mut app) = app(MANIFEST);
    frame(&mut app, &[]);
    let err = app
        .engine
        .resource::<InputActions>()
        .borrow_mut()
        .rebind("jump", &[String::from("gamepad:Nope")])
        .unwrap_err();
    assert!(err.contains("gamepad button"), "{err}");
}

#[test]
fn the_declared_actions_are_listed_in_a_stable_order() {
    let (_dir, mut app) = app(MANIFEST);
    frame(&mut app, &[]);
    let names = app.engine.resource::<InputActions>().borrow().names();
    assert_eq!(names, vec!["fire", "jump", "move_x"]);
}

#[test]
fn a_project_declaring_no_actions_is_not_an_error() {
    let (_dir, mut app) = app("name = \"t\"\nmain_scene = \"main.toml\"\n");
    frame(&mut app, &[("Space", true)]);
    assert!(app.engine.resource::<InputActions>().borrow().names().is_empty());
}

/// The bar from `docs/PLAN-batteries.md` phase 1: a replay records keys, and
/// the actions come back from them. Two runs of the same key sequence must
/// produce the same action values, which is what a recording replayed against
/// the same project gets.
#[test]
fn the_same_keys_produce_the_same_actions_every_run() {
    let keys = [
        vec![("Space", true)],
        vec![],
        vec![("D", true)],
        vec![("Space", false), ("D", false), ("A", true)],
    ];
    let mut runs = Vec::new();
    for _ in 0..2 {
        let (_dir, mut app) = app(MANIFEST);
        let mut seen = Vec::new();
        for step in &keys {
            frame(&mut app, step);
            seen.push((
                value(&app, "jump"),
                value(&app, "move_x"),
                just_pressed(&app, "jump"),
                just_released(&app, "jump"),
            ));
        }
        runs.push(seen);
    }
    assert_eq!(runs[0], runs[1]);
}
