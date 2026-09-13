//! Touch controls end to end: a scripted game steered by a thumb on a
//! `touch_stick`, recorded, and played back against its own digest chain.
//!
//! The script reads an action and never mentions touch. That is the claim
//! the controls exist to make, and the replay is the claim that a control
//! hit-tested in the tick is as reproducible as a key.

use balaur::input::{InputSnapshot, TouchPhase};
use balaur::{App, AppConfig, FIXED_DT, digest, replay, standard_app};

const SCRIPT: &str = "pub fn fixed_update(this, dt) {
    this.node.translate(input::action_value(\"move_x\") * dt, 0.0, 0.0);
}
";

const SCENE: &str = r#"[[nodes]]
id = "root"
name = "Game"

[[nodes]]
id = "n"
name = "Runner"
parent = "root"
script = { source = "scripts/s.rn" }

[nodes.transform]
position = [0, 0, 0]

[[nodes]]
id = "s"
name = "Stick"
parent = "root"

[nodes.touch_stick]
action_x = "move_x"
anchor = "bottom_left"
offset = [150.0, -150.0]
radius = 100.0
deadzone = 0.0
visibility = "always"
"#;

fn project(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.toml"), SCENE).unwrap();
    std::fs::write(dir.join("scripts").join("s.rn"), SCRIPT).unwrap();
}

/// A booted game on a 1000 x 600 screen, so the stick sits at (150, 450).
fn booted(dir: &std::path::Path) -> App {
    let mut app = standard_app(AppConfig::dev(dir.to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    balaur_core::facts::update_device(&app.engine, |f| {
        f.screen_size = [1000.0, 600.0];
        f.ui_scale = 1.0;
    });
    app
}

fn walked(app: &App) -> f32 {
    let world = app.engine.world();
    let node = balaur_core::scene::find_node(&world, app.engine.root(), "Game/Runner").unwrap();
    let t = world.get::<&balaur_core::Transform>(node).unwrap();
    t.position.x
}

/// A thumb lands on the stick at tick 5, pushes right over ticks 6..15, and
/// lifts at tick 20.
fn record(dir: &std::path::Path) -> Vec<(serde_json::Map<String, serde_json::Value>, u64)> {
    let mut app = booted(dir);
    let mut frames = Vec::new();
    for tick in 0..30u16 {
        {
            let input = app.engine.resource::<InputSnapshot>();
            let mut input = input.borrow_mut();
            input.begin_frame();
            match tick {
                5 => input.touch_event(7, 150.0, 450.0, TouchPhase::Start),
                6..=15 => {
                    let x = 150.0 + f32::from(tick - 5) * 10.0;
                    input.touch_event(7, x, 450.0, TouchPhase::Move);
                }
                20 => input.touch_event(7, 250.0, 450.0, TouchPhase::End),
                _ => {}
            }
        }
        app.tick(FIXED_DT);
        frames.push((replay::capture(&app.engine), digest::digest(&app.engine).0));
    }
    assert!(
        walked(&app) > 0.0,
        "the recording is worthless if the thumb never moved the runner"
    );
    frames
}

#[test]
fn a_thumb_on_a_stick_steers_a_script_that_reads_an_action() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    record(dir.path());
}

#[test]
fn a_replayed_touch_session_reproduces_every_tick_digest() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let recorded = record(dir.path());

    let mut app = booted(dir.path());
    for (tick, (sources, digest)) in recorded.iter().enumerate() {
        replay::restore(&app.engine, sources);
        app.tick(FIXED_DT);
        assert_eq!(
            digest::digest(&app.engine).0,
            *digest,
            "replay parted from the recording at tick {tick}"
        );
    }
    assert!(walked(&app) > 0.0);
}

/// The control: the same game with nobody touching it stays put, so the
/// test above is checking the thumb and not two identical idle runs.
#[test]
fn nobody_touching_the_stick_leaves_the_runner_still() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut app = booted(dir.path());
    for _ in 0..30 {
        app.tick(FIXED_DT);
    }
    assert!(walked(&app).abs() < 1e-9);
}

/// The verbs a script reaches: feeding a finger and an action, and reading
/// the four gestures, each of which must answer with nothing happening.
const VERBS: &str = "pub fn update(this, dt) {
    input::feed_action(\"jump\", 1.0);
    input::feed_touch(3, 500.0, 300.0, \"start\");
    let zoom = 1.0;
    if let Some(scale) = input::pinch().get(\"scale\") { zoom *= scale; }
    let pan = input::pan();
    if let Some(dx) = input::swipe().get(\"x\") { zoom += dx; }
    if input::long_press().get(\"x\").is_some() { zoom = 1.0; }
    input::feed_action(\"reached\", 1.0);
}
pub fn fixed_update(this, dt) {
    if input::action_pressed(\"jump\") { this.node.translate(dt, 0.0, 0.0); }
}
";

#[test]
fn a_script_can_feed_a_finger_and_an_action() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    std::fs::write(dir.path().join("scripts").join("s.rn"), VERBS).unwrap();
    let mut app = booted(dir.path());
    for _ in 0..30 {
        app.tick(FIXED_DT);
    }
    assert!(walked(&app) > 0.0, "a fed action presses like a bound one");
    // Fed after every gesture read: a read that threw would stop short of it,
    // and a script error is only logged.
    let reached = app
        .engine
        .resource::<balaur::input::InputActions>()
        .borrow()
        .value("reached");
    assert!(reached > 0.5, "every verb in `update` ran");
    let input = app.engine.resource::<InputSnapshot>();
    assert!(
        input.borrow().touches().iter().any(|(id, _, _)| *id == 3),
        "a fed finger is on the screen"
    );
}

/// A host running a project other than its own hands over that project's
/// `[input]` table, and the settings in it take hold.
#[test]
fn a_host_can_declare_another_projects_input_config() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut app = booted(dir.path());
    let hosted = toml::Value::Table(toml::Table::from_iter([(
        "emulate_mouse_from_touch".to_string(),
        toml::Value::Boolean(false),
    )]));
    balaur::input::actions::declare_manifest(&app.engine, &hosted)
        .expect("a hosted project's input declares");
    app.tick(FIXED_DT);
    {
        let input = app.engine.resource::<InputSnapshot>();
        let mut input = input.borrow_mut();
        input.begin_frame();
        input.touch_event(1, 300.0, 200.0, TouchPhase::Start);
    }
    app.tick(FIXED_DT);
    let input = app.engine.resource::<InputSnapshot>();
    assert!(
        !input.borrow().is_mouse_down(0),
        "the finger stayed a finger"
    );
}
