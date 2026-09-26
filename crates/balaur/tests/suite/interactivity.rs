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

/// A project with scripts beside its scene.
fn app_with_scripts(scene: &str, scripts: &[(&str, &str)]) -> (tempfile::TempDir, balaur::App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), scene).unwrap();
    for (path, text) in scripts {
        std::fs::write(dir.path().join(path), text).unwrap();
    }
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

/// Two scripted nodes counting the hooks they hear. A broadcast walks the
/// last child first, so B hears before A.
const HEARERS: &str = r#"
[variables]
hits = { type = "int", value = 0 }

[[nodes]]
id = "n_scene"
name = "Scene"

[[nodes]]
id = "n_a"
name = "A"
parent = "n_scene"
script = { source = "scripts/a.rn" }

[[nodes]]
id = "n_b"
name = "B"
parent = "n_scene"
script = { source = "scripts/b.rn" }
"#;

/// A hears what reaches it and takes nothing.
const A_HEARS: &str = "pub fn on_key_down(this, key) {\n\
    scene::set_variable(\"hits\", scene::variable(\"hits\") + 1);\n\
}\n\
pub fn on_pointer_down(this, button) {\n\
    scene::set_variable(\"hits\", scene::variable(\"hits\") + 10);\n\
}\n";

/// B hears first, takes the key, and lets every press through.
const B_HEARS: &str = "pub fn on_key_down(this, key) {\n\
    scene::set_variable(\"hits\", scene::variable(\"hits\") + 1);\n\
    true\n\
}\n\
pub fn on_pointer_down(this, button) {\n\
    scene::set_variable(\"hits\", scene::variable(\"hits\") + 10);\n\
    false\n\
}\n";

fn hits(app: &balaur::App) -> i64 {
    let variables = app.engine.resource::<Variables>();
    let held = variables.borrow();
    held.get("hits")
        .map_or(-1.0, balaur_core::variables::as_num) as i64
}

/// A that feeds a press from its own update, once, and hears like A.
const A_FEEDS: &str = "pub fn update(this, dt) {\n\
    if scene::variable(\"hits\") == 0 {\n\
        input::feed_mouse_button(0, true);\n\
    }\n\
}\n\
pub fn on_pointer_down(this, button) {\n\
    scene::set_variable(\"hits\", scene::variable(\"hits\") + 10);\n\
}\n";

#[test]
fn a_press_a_script_feeds_reaches_the_hooks_on_the_next_frame() {
    let (_dir, mut app) = app_with_scripts(
        HEARERS,
        &[("scripts/a.rn", A_FEEDS), ("scripts/b.rn", B_HEARS)],
    );
    app.tick(1.0 / 60.0);
    assert_eq!(hits(&app), 0, "fed during the tick, so not this frame's");
    app.tick(1.0 / 60.0);
    assert_eq!(
        hits(&app),
        20,
        "the frame began with the press, both heard it"
    );
    app.tick(1.0 / 60.0);
    assert_eq!(hits(&app), 20, "an edge is one frame's");
}

#[test]
fn a_hook_that_answers_true_ends_the_broadcast() {
    let (_dir, mut app) = app_with_scripts(
        HEARERS,
        &[("scripts/a.rn", A_HEARS), ("scripts/b.rn", B_HEARS)],
    );
    app.tick(1.0 / 60.0);
    {
        let input = app.engine.resource::<balaur::input::InputSnapshot>();
        let mut input = input.borrow_mut();
        input.begin_frame();
        input.key_event("Space", true);
    }
    app.tick(1.0 / 60.0);
    assert_eq!(hits(&app), 1, "B took the key, so A never heard it");
}

#[test]
fn a_press_over_nothing_reaches_every_node() {
    let (_dir, mut app) = app_with_scripts(
        HEARERS,
        &[("scripts/a.rn", A_HEARS), ("scripts/b.rn", B_HEARS)],
    );
    app.tick(1.0 / 60.0);
    {
        let input = app.engine.resource::<balaur::input::InputSnapshot>();
        let mut input = input.borrow_mut();
        input.begin_frame();
        input.mouse_button_event(0, true);
    }
    app.tick(1.0 / 60.0);
    assert_eq!(
        hits(&app),
        20,
        "nothing under the pointer, so both heard the press"
    );
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

[[nodes.bindings.rows]]
event = "pointer_click"
action = "add_variable"
target = "score"
value = 1

[[nodes.bindings.rows]]
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
kind = "box"
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
    assert_eq!(kind, "box");

    balaur_core::states::go(&app.engine, door, "open").unwrap();
    let after = balaur_core::components::get(&app.engine, door, "shape3d").unwrap();
    assert_eq!(
        after.get("kind").and_then(toml::Value::as_str),
        Some("box"),
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

[[nodes.bindings.rows]]
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
keys = [ { time = 0.0, value = [0, 0, 0] }, { time = 0.5, value = [0, 1, 0] } ]

[[nodes.bindings.rows]]
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
    // Timeouts at a quarter, a half and three quarters of the second; the
    // fourth falls just past the last tick. The half-second clip ends once.
    assert_eq!(score(&app), 3 + 100);
}

/// A sensor with a crate resting in it, a row on each end of the contact,
/// and a door whose state answers the score changing.
const CONTACT: &str = r#"
[variables]
score = { type = "int", value = 0 }
opened = { type = "bool", value = false }

[[nodes]]
id = "n_scene"
name = "Scene"

[[nodes]]
id = "n_zone"
name = "Zone"
parent = "n_scene"

[nodes.collider3d]
kind = "box"
size = [4.0, 4.0, 4.0]
sensor = true
events = ["collision"]

[[nodes.bindings.rows]]
event = "collision_enter"
action = "add_variable"
target = "score"
value = 1

[[nodes.bindings.rows]]
event = "collision_exit"
action = "add_variable"
target = "score"
value = 10

[[nodes]]
id = "n_crate"
name = "Crate"
parent = "n_scene"
body3d = { kind = "dynamic", gravity_scale = 0.0 }

[nodes.collider3d]
kind = "sphere"
radius = 0.5

[[nodes]]
id = "n_door"
name = "Door"
parent = "n_scene"

[[nodes.bindings.rows]]
event = "variable_changed"
action = "state"
value = "open"

[[nodes.bindings.rows]]
event = "state_changed"
action = "set_variable"
target = "opened"
value = true

[nodes.states]
current = "shut"

[nodes.states.shut]
transform = { position = [0, 0, 0] }

[nodes.states.open]
transform = { position = [0, 1, 0] }
"#;

#[test]
fn collision_rows_run_with_no_script_and_a_freed_node_still_ends_its_contact() {
    let (_dir, mut app) = app_from(CONTACT);
    for _ in 0..10 {
        app.tick(1.0 / 60.0);
    }
    assert_eq!(score(&app), 1, "the crate starts inside the zone");
    balaur_core::scene::free_node(&app.engine, node(&app, "Crate"));
    for _ in 0..5 {
        app.tick(1.0 / 60.0);
    }
    assert_eq!(score(&app), 11, "freeing the crate ends its contact");
}

#[test]
fn variable_and_state_rows_run_with_no_script() {
    let (_dir, mut app) = app_from(CONTACT);
    for _ in 0..10 {
        app.tick(1.0 / 60.0);
    }
    let door = node(&app, "Door");
    assert_eq!(state_of(&app, door), "open", "the score changing opened it");
    let variables = app.engine.resource::<Variables>();
    let opened = variables.borrow().get("opened").cloned();
    assert_eq!(
        opened,
        Some(Value::Bool(true)),
        "and the state change was heard"
    );
}

/// A clip whose node's script also subscribes to its own event.
const ONCE: &str = r#"
[variables]
score = { type = "int", value = 0 }

[[nodes]]
id = "n_scene"
name = "Scene"

[[nodes]]
id = "n_wave"
name = "Wave"
parent = "n_scene"
script = { source = "scenes/wave.rn" }

[nodes.transform]
position = [0, 0, 0]

[nodes.animation]
autoplay = "rise"

[nodes.animation.library.clips.rise]
length = 0.2
[[nodes.animation.library.clips.rise.tracks]]
property = "position"
keys = [ { time = 0.0, value = [0, 0, 0] }, { time = 0.2, value = [0, 1, 0] } ]
"#;

#[test]
fn a_finished_clip_calls_its_own_node_once_when_the_node_also_subscribes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), ONCE).unwrap();
    std::fs::write(
        dir.path().join("scenes/wave.rn"),
        "pub fn init(this) { events::subscribe(this.node, \"animation_finished\", this.node); }\n\
         pub fn on_animation_finished(this, clip) {\n\
         \x20   scene::set_variable(\"score\", scene::variable(\"score\") + 1);\n\
         }\n",
    )
    .unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    for _ in 0..40 {
        app.tick(1.0 / 60.0);
    }
    assert_eq!(score(&app), 1);
}

/// A lamp hidden by a row on another node, and a sign hidden by its own clip,
/// each with a row that answers its `visibility_changed`.
const SHOWN: &str = r#"
[variables]
score = { type = "int", value = 0 }
dark = { type = "bool", value = false }

[[nodes]]
id = "n_scene"
name = "Scene"

[[nodes]]
id = "n_switch"
name = "Switch"
parent = "n_scene"

[[nodes.bindings.rows]]
event = "variable_changed"
action = "visible"
target = "../Lamp"
value = false

[[nodes]]
id = "n_lamp"
name = "Lamp"
parent = "n_scene"

[[nodes.bindings.rows]]
event = "emitted:visibility_changed"
action = "add_variable"
target = "score"
value = 1

[[nodes]]
id = "n_sign"
name = "Sign"
parent = "n_scene"

[nodes.animation]
autoplay = "blink"

[nodes.animation.library.clips.blink]
length = 0.2
[[nodes.animation.library.clips.blink.tracks]]
property = "visible"
keys = [ { time = 0.0, value = 1.0 }, { time = 0.1, value = 0.0 } ]

[[nodes.bindings.rows]]
event = "emitted:visibility_changed"
action = "add_variable"
target = "score"
value = 10
"#;

#[test]
fn a_row_and_a_clip_that_hide_a_node_announce_it() {
    let (_dir, mut app) = app_from(SHOWN);
    {
        let variables = app.engine.resource::<Variables>();
        variables
            .borrow_mut()
            .set("dark", &Value::Bool(true))
            .unwrap();
    }
    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    assert_eq!(score(&app), 11, "the lamp once and the sign once");
}

#[test]
fn a_node_offers_the_events_its_components_announce() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scenes/main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Clock\"\nscript = { source = \"scenes/c.rn\" }\n\n[nodes.timer]\nwait_time = 1.0\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scenes/c.rn"),
        "pub fn offered(this) { scene::bindable_events(this.node) }\n",
    )
    .unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    let clock = {
        let world = app.engine.world();
        balaur::scene::find_node(&world, app.engine.root(), "Clock").unwrap()
    };
    let host = app.engine.script_host().unwrap();
    let offered = host.call_on(balaur::node_id_of(clock), "offered", &[]);
    let Some(Value::List(offered)) = offered else {
        panic!("no list of events: {offered:?}");
    };
    for wanted in [
        "pointer_click",
        "emitted:timeout",
        "emitted:visibility_changed",
    ] {
        assert!(
            offered.contains(&Value::Str(wanted.into())),
            "`{wanted}` is not offered: {offered:?}"
        );
    }
}
