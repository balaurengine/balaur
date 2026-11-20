//! The `input` calls no game or editor script makes yet, each run from a
//! script and checked by what it answered.

use balaur::input::{InputSnapshot, TouchPhase};
use balaur::{App, AppConfig, DEFAULT_FIXED_DT, standard_app};
use balaur_script::Value;

const SCENE: &str = r#"[[nodes]]
id = "n"
name = "Probe"
script = { source = "scripts/probe.bs" }
"#;

/// A game whose one node runs `script`, with `actions` as its
/// `[input.actions]` table.
fn booted(script: &str, actions: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!(
            "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n\n[input.actions]\n{actions}"
        ),
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), SCENE).unwrap();
    std::fs::write(dir.path().join("scripts").join("probe.bs"), script).unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

/// What the probe's script holds under `name`.
fn field(app: &App, name: &str) -> Option<Value> {
    let world = app.engine.world();
    let probe = balaur_core::scene::find_node(&world, app.engine.root(), "Probe")?;
    drop(world);
    app.engine
        .script_host()?
        .script_field(balaur::node_id_of(probe), name)
}

#[test]
fn every_declared_action_is_listed_for_a_rebinding_screen() {
    let (_dir, mut app) = booted(
        r#"pub class Probe { node: Node, listed: str }
impl Probe {
    pub fn init() {
        let names = input::actions();
        let joined = "";
        for name in names {
            joined = joined + name + ";";
        }
        self.listed = joined;
    }
}
"#,
        "jump = [\"Space\"]\nmove_x = [\"keys:KeyA,KeyD\"]\nsave = [\"cmd+KeyS\"]\n",
    );
    app.tick(DEFAULT_FIXED_DT);
    assert_eq!(
        field(&app, "listed"),
        Some(Value::Str("jump;move_x;save;".into()))
    );
}

// A script diffing `touches()` from one frame to the next would never see a
// finger that lands and lifts between two frames; these two do.
#[test]
fn a_finger_that_lands_and_lifts_in_one_frame_is_both_started_and_ended() {
    let (_dir, mut app) = booted(
        r#"pub class Probe { node: Node, seen: str }
impl Probe {
    pub fn update(dt: float) {
        let started = input::touches_just_started();
        let ended = input::touches_just_ended();
        self.seen = self.seen + "{started.len()}/{ended.len()} ";
        for id in started {
            self.seen = self.seen + "+{id} ";
        }
        for id in ended {
            self.seen = self.seen + "-{id} ";
        }
    }
}
"#,
        "",
    );
    for events in [
        vec![(7, TouchPhase::Start)],
        vec![],
        vec![(7, TouchPhase::End)],
        vec![(8, TouchPhase::Start), (8, TouchPhase::End)],
    ] {
        {
            let input = app.engine.resource::<InputSnapshot>();
            let mut input = input.borrow_mut();
            input.begin_frame();
            for (id, phase) in events {
                input.touch_event(id, 100.0, 100.0, phase);
            }
        }
        app.tick(DEFAULT_FIXED_DT);
    }
    assert_eq!(
        field(&app, "seen"),
        Some(Value::Str("1/0 +7 0/0 0/1 -7 1/1 +8 -8 ".into()))
    );
}

#[test]
fn a_vibration_with_no_motor_does_nothing_and_says_nothing() {
    balaur::logbuf::capture_for_test();
    balaur::logbuf::clear();
    let (_dir, mut app) = booted(
        r"pub class Probe { node: Node, done: int }
impl Probe {
    pub fn init() {
        input::vibrate(0.25);
        input::vibrate(-1.0);
        self.done = 1;
    }
}
",
        "",
    );
    app.tick(DEFAULT_FIXED_DT);
    assert_eq!(field(&app, "done"), Some(Value::Int(1)));
    // Only what the call said, or the script's own error: tests in one
    // process share the log.
    let loud: Vec<String> = balaur::logbuf::recent(200)
        .into_iter()
        .filter(|line| {
            line.level.eq_ignore_ascii_case("warn") || line.level.eq_ignore_ascii_case("error")
        })
        .map(|line| line.message)
        .filter(|message| message.contains("probe.bs] init") || message.contains("vibrat"))
        .collect();
    assert!(loud.is_empty(), "{loud:#?}");
}
