//! A scene that reacts with no script in it: variables, states and bindings,
//! and the hook dispatch that reaches them.
//!
//! The door `examples/hello` opens on the third click, proved here rather than
//! by clicking: a pointer needs a viewport, and this runs headless.

use balaur::{AppConfig, standard_app};
use balaur_core::bindings;
use balaur_core::variables::Variables;
use balaur_script::Value;

/// An app booted from a scene written for the test.
fn app_from(scene: &str) -> (tempfile::TempDir, balaur::App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), scene).unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

/// The door scene, spelled as `examples/hello` spells it.
const DOOR: &str = r#"
[variables]
score = { type = "int", value = 0 }

[[nodes]]
id = "n_scene"
name = "Scene"

[[nodes]]
id = "n_ball"
name = "Ball"
parent = "n_scene"

[[nodes.bindings]]
event = "pointer_click"
action = "add_variable"
target = "score"
value = 1

[[nodes.bindings]]
event = "pointer_click"
when = "score >= 3"
action = "state"
target = "../Door"
value = "open"

[[nodes]]
id = "n_door"
name = "Door"
parent = "n_scene"

[nodes.shape3d]
kind = "cuboid"
color = [0.45, 0.32, 0.17, 1]

[nodes.states]
current = "shut"

[nodes.states.shut]
shape3d = { color = [0.45, 0.32, 0.17, 1] }

[nodes.states.open]
shape3d = { color = [0.3, 0.75, 0.4, 1] }
"#;

fn node(app: &balaur::App, name: &str) -> balaur::hecs::Entity {
    let world = app.engine.world();
    // Every node in these scenes hangs off the document's one root, `Scene`.
    balaur::scene::find_node(&world, app.engine.root(), &format!("Scene/{name}"))
        .unwrap_or_else(|| panic!("no node `{name}`"))
}

fn state_of(app: &balaur::App, entity: balaur::hecs::Entity) -> String {
    let world = app.engine.world();
    world
        .get::<&balaur_core::states::States>(entity)
        .map(|states| states.current.clone())
        .unwrap_or_default()
}

/// The score as the whole number it is declared as: an `int` variable is
/// truncated on every write, so comparing one is comparing a count.
fn score(app: &balaur::App) -> i64 {
    let variables = app.engine.resource::<Variables>();
    let held = variables.borrow();
    held.get("score")
        .map_or(-1.0, balaur_core::variables::as_num) as i64
}

/// Three clicks on the ball, and nothing in the project is a script.
#[test]
fn the_third_click_opens_the_door() {
    let (_dir, app) = app_from(DOOR);
    let ball = node(&app, "Ball");
    let door = node(&app, "Door");
    assert_eq!(state_of(&app, door), "shut", "the scene's own state");
    assert_eq!(score(&app), 0);

    for expected in [1, 2] {
        bindings::fire(&app.engine, ball, "pointer_click", &[]);
        assert_eq!(score(&app), expected, "the click counts");
        assert_eq!(
            state_of(&app, door),
            "shut",
            "and the door stays shut below three"
        );
    }
    bindings::fire(&app.engine, ball, "pointer_click", &[]);
    assert_eq!(score(&app), 3);
    assert_eq!(state_of(&app, door), "open", "the third click opens it");
}

/// A state patches only the properties it names, so the door keeps its size.
#[test]
fn a_state_leaves_what_it_does_not_name_alone() {
    let (_dir, app) = app_from(DOOR);
    let door = node(&app, "Door");
    let before = balaur_core::components::get(&app.engine, door, "shape3d").unwrap();
    let kind = before.get("kind").and_then(toml::Value::as_str).unwrap();
    assert_eq!(kind, "cuboid");

    balaur_core::states::go(&app.engine, door, "open").unwrap();
    let after = balaur_core::components::get(&app.engine, door, "shape3d").unwrap();
    assert_eq!(
        after.get("kind").and_then(toml::Value::as_str),
        Some("cuboid"),
        "the state said nothing about the shape's kind"
    );
    let green = after
        .get("color")
        .and_then(toml::Value::as_array)
        .map(|c| balaur_core::components::as_f64(&c[1]).unwrap())
        .unwrap();
    assert!(green > 0.7, "and the colour it did name changed: {green}");
}

/// An unknown state is an error naming the ones the node has, not silence.
#[test]
fn going_to_a_state_a_node_does_not_have_says_which_it_does() {
    let (_dir, app) = app_from(DOOR);
    let door = node(&app, "Door");
    let err = balaur_core::states::go(&app.engine, door, "ajar")
        .unwrap_err()
        .to_string();
    assert!(err.contains("no state `ajar`"), "{err}");
    assert!(err.contains("open"), "{err}");
}

/// Writing a variable reaches `on_variable_changed`, once per change.
#[test]
fn a_variable_change_reaches_the_nodes_that_declare_the_hook() {
    let (dir, mut app) = app_from(DOOR);
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("scripts/watch.rn"),
        r"
pub fn init(this) {
    this.seen = 0;
    this.last = 0.0;
}

pub fn on_variable_changed(this, name, value) {
    this.seen += 1;
    this.last = value;
}
",
    )
    .unwrap();
    let ball = node(&app, "Ball");
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur::node_id_of(ball), "scripts/watch.rn")
        .unwrap();

    {
        let variables = app.engine.resource::<Variables>();
        variables
            .borrow_mut()
            .set("score", &Value::Num(7.0))
            .unwrap();
        // The same value again is not a change, so it dispatches nothing.
        variables
            .borrow_mut()
            .set("score", &Value::Num(7.0))
            .unwrap();
    }
    // One frame, so the dispatch at the end of the tick runs.
    app.tick(0.016);

    let rune = balaur::rune::rune_of(&app.engine);
    assert_eq!(
        rune.number_field(ball, "seen"),
        Some(1.0),
        "one change, one call: {:#?}",
        balaur::logbuf::recent(10)
    );
    assert_eq!(rune.number_field(ball, "last"), Some(7.0));
}

/// Every action core cannot run itself has a runner in a standard build, so
/// a binding naming one does the thing rather than logging that nothing can.
#[test]
fn every_deferred_action_has_a_runner() {
    let (_dir, app) = app_from(DOOR);
    let runners = app.engine.resource::<bindings::Runners>();
    let held = runners.borrow();
    for (word, action) in bindings::ACTIONS {
        // The six core runs itself; the rest are filled at load.
        let own = matches!(
            action,
            bindings::Action::State
                | bindings::Action::SetVariable
                | bindings::Action::AddVariable
                | bindings::Action::Free
                | bindings::Action::Visible
                | bindings::Action::Call
        );
        if own {
            continue;
        }
        // Audio is a cargo feature, so its runner is only in a build with it.
        if matches!(action, bindings::Action::Sound) && cfg!(not(feature = "audio")) {
            continue;
        }
        assert!(
            held.has(*action),
            "`{word}` has no runner: a binding naming it would log rather than act"
        );
    }
}

/// A timer's `timeout` and a clip's `animation_finished`, each counted by a
/// binding row: Godot connections to those signals, with no script at all.
const SIGNALS: &str = r#"
[variables]
score = { type = "int", value = 0 }

[[nodes]]
id = "n_scene"
name = "Scene"

[[nodes]]
id = "n_clock"
name = "Clock"
parent = "n_scene"

[nodes.timer]
wait_time = 0.25
autostart = true

[[nodes.bindings]]
event = "emitted:timeout"
action = "add_variable"
target = "score"
value = 1

[[nodes]]
id = "n_wave"
name = "Wave"
parent = "n_scene"

[nodes.transform]
position = [0, 0, 0]

[nodes.animation]
autoplay = "rise"

[nodes.animation.library.clips.rise]
length = 0.5
[[nodes.animation.library.clips.rise.tracks]]
property = "position"
keys = [ { t = 0.0, value = [0, 0, 0] }, { t = 0.5, value = [0, 1, 0] } ]

[[nodes.bindings]]
event = "emitted:animation_finished"
action = "add_variable"
target = "score"
value = 100
"#;

#[test]
fn a_timer_and_a_finished_clip_drive_bindings_with_no_script() {
    let (_dir, mut app) = app_from(SIGNALS);
    for _ in 0..60 {
        app.tick(1.0 / 60.0);
    }
    // Timeouts at a quarter, a half and three quarters of the second, each
    // heard the frame after; the one at the full second is still in flight.
    // The half-second clip ends once.
    assert_eq!(score(&app), 3 + 100);
}
