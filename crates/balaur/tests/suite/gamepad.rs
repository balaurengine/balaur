//! The gamepad contract, end to end and backend-free: every rule in
//! `docs/PLAN-input.md`'s contract checked through `input.feed_gamepad` and
//! the script verbs, then a session recorded and replayed against its own
//! digest chain.
//!
//! Fed pads use ids from 7 up, clear of the slots a real pad on the desk
//! would take, so a run on a machine with a controller plugged in is the same.

use balaur::input::{GamepadState, PadEvent, PadInfo};
use balaur::{App, AppConfig, DEFAULT_FIXED_DT, digest, replay, standard_app};

const PAD: i64 = 7;

fn project(dir: &std::path::Path, input: &str, script: &str) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("project.toml"),
        format!("[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n\n{input}"),
    )
    .unwrap();
    std::fs::write(
        dir.join("main.toml"),
        r#"[[nodes]]
id = "root"
name = "Game"

[[nodes]]
id = "n"
name = "Runner"
parent = "root"
script = { source = "scripts/runner.bs" }
"#,
    )
    .unwrap();
    std::fs::write(dir.join("scripts").join("runner.bs"), script).unwrap();
}

fn booted(dir: &std::path::Path) -> App {
    balaur::logbuf::capture_for_test();
    let mut app = standard_app(AppConfig::dev(dir.to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app
}

fn runner(app: &App) -> balaur_core::hecs::Entity {
    balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), "Game/Runner").unwrap()
}

fn field(app: &App, name: &str) -> Option<f64> {
    let host = app.engine.script_host().expect("a script host");
    match host.script_field(balaur_core::node_id_of(runner(app)), name)? {
        balaur_script::Value::Int(n) => Some(n as f64),
        balaur_script::Value::Num(n) => Some(n),
        other => panic!("{name} is {other:?}"),
    }
}

fn position(app: &App) -> [f32; 3] {
    let world = app.engine.world();
    let t = world.get::<&balaur_core::Transform>(runner(app)).unwrap();
    t.position.to_array()
}

/// Run `ticks` frames and require the script to have reached `self.done = 1`.
fn run_to_done(app: &mut App, ticks: usize) {
    for _ in 0..ticks {
        app.tick(DEFAULT_FIXED_DT);
    }
    assert_eq!(
        field(app, "done"),
        Some(1.0),
        "the script did not reach its end: {:#?}",
        balaur::logbuf::recent(10)
    );
}

const VERBS: &str = r#"
fn near(a: float, b: float) -> bool { math::abs(a - b) < 1e-4 }

pub class Runner { node: Node, tick: int, done: int, failed: bool }

impl Runner {
    fn update(dt: float) {
        self.tick += 1;
        let pad = 7;
        if self.tick == 1 {
            input::feed_gamepad(pad, map()
                .with("name", "Fed Pad").with("os_name", "Fed OS").with("guid", "0300aabb")
                .with("vendor", 1).with("product", 2)
                .with("mapping", input::GAMEPAD_MAPPING_SDL).with("rumble", true)
                .with("buttons", map().with("south", true).with("left_trigger", 0.3))
                .with("axes", map().with("left_x", 0.1).with("right_x", 1.0))
                .with("power", map().with("state", input::GAMEPAD_POWER_ON_BATTERY).with("level", 0.5))
                .with("touches", [map().with("id", 1).with("x", 0.25).with("y", 0.75)]));
        } else if self.tick == 2 {
            let pads = input::gamepads();
            self.check(pads.len() == 1 && pads.at(0).as_int() == pad, "the fed pad is listed by its id");
            self.check(input::gamepad_name(pad) == "Fed Pad", "its name");
            let south = input::GAMEPAD_BUTTON_SOUTH;
            self.check(input::gamepad_down(pad, south) && input::gamepad_just_pressed(pad, south), "south down");
            self.check(input::gamepad_repeated(pad, south), "a press is a repeat");
            let trigger = input::GAMEPAD_BUTTON_LEFT_TRIGGER;
            self.check(!input::gamepad_down(pad, trigger), "under the press threshold");
            self.check(near(input::gamepad_pressure(pad, trigger), 0.3), "trigger pressure");
            self.check(near(input::gamepad_axis(pad, input::GAMEPAD_AXIS_LEFT_X), 0.0), "inside the deadzone");
            self.check(near(input::gamepad_axis(pad, input::GAMEPAD_AXIS_RIGHT_X), 1.0), "right x");
            let info = input::gamepad_info(pad);
            self.check(info.get("guid").as_str() == "0300aabb" && info.get("vendor").as_int() == 1 && info.get("product").as_int() == 2, "info ids");
            self.check(info.get("os_name").as_str() == "Fed OS" && info.get("mapping").as_str() == "sdl", "info names");
            let power = input::gamepad_power(pad);
            self.check(power.get("state").as_str() == "on_battery" && near(power.get("level").as_float(), 0.5), "power");
            self.check(near(input::gamepad_touches(pad).at(0).get("x").as_float(), 0.25), "touch x");
            self.check(input::gamepad_can_rumble(pad), "can rumble");
            input::feed_gamepad(pad, map().with("buttons", map().with("south", false)));
        } else if self.tick == 3 {
            let south = input::GAMEPAD_BUTTON_SOUTH;
            self.check(input::gamepad_just_released(pad, south) && !input::gamepad_down(pad, south), "south released");
            input::feed_gamepad(pad, map().with("buttons", map().with("east", true)));
            input::feed_gamepad(pad, map().with("buttons", map().with("east", false)));
        } else if self.tick == 4 {
            let east = input::GAMEPAD_BUTTON_EAST;
            self.check(input::gamepad_just_pressed(pad, east), "a tap between frames presses");
            self.check(input::gamepad_just_released(pad, east), "and releases");
            self.check(!input::gamepad_down(pad, east), "east up");
            input::feed_gamepad(pad, map().with("connected", false));
        } else if self.tick == 5 {
            self.check(input::gamepads().len() == 0, "unplugged");
            self.check(input::gamepad_name(pad) == "" && !input::gamepad_can_rumble(pad), "no name, no rumble");
            let power = input::gamepad_power(pad);
            self.check(power.get("state").as_str() == "unknown" && near(power.get("level").as_float(), -1.0), "neutral");
            if !self.failed {
                self.done = 1;
            }
        }
    }

    fn check(ok: bool, what: str) {
        if !ok {
            self.failed = true;
            log::error("failed: {what}");
        }
    }
}
"#;

#[test]
fn a_fed_pad_reads_through_every_verb_and_answers_neutrally_once_gone() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), "", VERBS);
    let mut app = booted(dir.path());
    run_to_done(&mut app, 6);
}

const RUMBLE: &str = r#"
pub class Runner { node: Node, tick: int, started: int, refused: int, finished: int, finished_at: int }

impl Runner {
    fn init() {
        self.refused = 1;
    }

    fn update(dt: float) {
        self.tick += 1;
        if self.tick == 1 {
            input::feed_gamepad(7, map().with("rumble", true));
            input::feed_gamepad(8, map().with("rumble", false));
        } else if self.tick == 2 {
            let shaped = map().with("strong", 0.02).with("weak", 0.5).with("duration", 0.09)
                .with("attack", 0.03).with("pulse", 0.04).with("gap", 0.01);
            if input::gamepad_rumble(7, shaped) { self.started = self.tick; }
            if input::gamepad_rumble(8, map()) { self.refused = 0; }
        } else if self.tick == 20 {
            input::gamepad_rumble(7, map().with("duration", 1.0).with("position", [3.0, 0.0, 0.0]).with("falloff", "linear"));
            input::gamepad_set_listener(7, [0.0, 0.0, 0.0]);
        } else if self.tick == 21 {
            input::gamepad_stop_rumble(7);
        }
    }

    pub fn on_gamepad_rumble_finished(pad: int) {
        self.finished += 1;
        self.finished_at = self.tick;
    }
}
"#;

#[test]
fn a_rumble_finishes_on_tick_time_once_and_a_stopped_one_never_does() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), "", RUMBLE);
    let mut app = booted(dir.path());
    for _ in 0..90 {
        app.tick(DEFAULT_FIXED_DT);
    }
    let started = field(&app, "started").expect("the script ran");
    assert!(
        started > 0.0,
        "a pad with motors rumbles: {:#?}",
        balaur::logbuf::recent(10)
    );
    assert_eq!(
        field(&app, "refused"),
        Some(1.0),
        "a pad without motors refuses"
    );
    assert_eq!(
        field(&app, "finished"),
        Some(1.0),
        "once, and not for the stopped one"
    );
    let took = field(&app, "finished_at").unwrap() - started;
    let ticks = (0.09 / f64::from(DEFAULT_FIXED_DT)).ceil();
    assert!(
        (ticks..=ticks + 1.0).contains(&took),
        "heard {took} ticks after it started, for {ticks} ticks of rumble"
    );
}

fn feed(app: &mut App, events: Vec<PadEvent>) {
    app.engine
        .resource::<GamepadState>()
        .borrow_mut()
        .feed(events);
    app.tick(DEFAULT_FIXED_DT);
}

fn left_x(app: &App) -> f32 {
    let pads = app.engine.resource::<GamepadState>();
    let pads = pads.borrow();
    pads.pad(PAD).unwrap().axis("left_x")
}

fn trigger_down(app: &App) -> bool {
    let pads = app.engine.resource::<GamepadState>();
    let pads = pads.borrow();
    pads.pad(PAD).unwrap().is_down("left_trigger")
}

#[test]
fn the_project_s_numbers_shape_every_pad() {
    let dir = tempfile::tempdir().unwrap();
    let input = "[input]\ngamepad_deadzone = 0.5\ngamepad_press = 0.9\ngamepad_release = 0.2\n";
    project(dir.path(), input, "pub class Runner { node: Node }\n");
    let mut app = booted(dir.path());
    let trigger = balaur::input::PAD_BUTTON_NAMES
        .iter()
        .position(|name| *name == "left_trigger")
        .unwrap();
    feed(
        &mut app,
        vec![
            PadEvent::Connected(PAD, Box::default()),
            PadEvent::Axis(PAD, 0, 0.4),
        ],
    );
    assert!(left_x(&app).abs() < 1e-6, "inside 0.5");
    feed(
        &mut app,
        vec![
            PadEvent::Axis(PAD, 0, 0.75),
            PadEvent::Button(PAD, trigger, 0.85),
        ],
    );
    assert!((left_x(&app) - 0.5).abs() < 1e-5, "rescaled");
    assert!(!trigger_down(&app), "under 0.9");
    for (pressure, down) in [(0.95, true), (0.5, true), (0.1, false)] {
        feed(&mut app, vec![PadEvent::Button(PAD, trigger, pressure)]);
        assert_eq!(trigger_down(&app), down, "at {pressure}");
    }
}

const ACTIONS: &str = "[input.actions]\njump = [\"gamepad:south\"]\nmove_x = [\"axis:left_x\"]\n";

const PLAYER: &str = r#"
pub class Runner { node: Node }

impl Runner {
    fn fixed_update(dt: float) {
        self.node.transform.translate(input::action_value("move_x") * dt, 0.0, 0.0);
        if input::action_just_pressed("jump") {
            input::gamepad_rumble(7, map().with("duration", 0.05));
        }
    }

    pub fn on_gamepad_rumble_finished(pad: int) {
        self.node.transform.translate(0.0, 1.0, 0.0);
    }
}
"#;

/// A pad plugs in at tick 2, a stick pushes right over ticks 5..15, and the
/// jump button taps at tick 20, which rumbles; the rumble's end lifts the
/// runner. A second tap at tick 30 rumbles too, but the pad unplugs before
/// it ends, so that one lifts nothing.
fn record(dir: &std::path::Path) -> Vec<(serde_json::Map<String, serde_json::Value>, u64)> {
    let mut app = booted(dir);
    let south = balaur::input::PAD_BUTTON_NAMES
        .iter()
        .position(|name| *name == "south")
        .unwrap();
    let mut frames = Vec::new();
    for tick in 0..40u8 {
        let events = match tick {
            2 => vec![PadEvent::Connected(
                PAD,
                Box::new(PadInfo {
                    rumble: true,
                    ..PadInfo::default()
                }),
            )],
            5..=15 => vec![PadEvent::Axis(PAD, 0, 0.8)],
            16 => vec![PadEvent::Axis(PAD, 0, 0.0)],
            20 | 30 => vec![
                PadEvent::Button(PAD, south, 1.0),
                PadEvent::Button(PAD, south, 0.0),
            ],
            31 => vec![PadEvent::Disconnected(PAD)],
            _ => Vec::new(),
        };
        app.engine
            .resource::<GamepadState>()
            .borrow_mut()
            .feed(events);
        app.tick(DEFAULT_FIXED_DT);
        frames.push((replay::capture(&app.engine), digest::digest(&app.engine).0));
    }
    let [x, y, _] = position(&app);
    assert!(x > 0.0, "the stick moved the runner");
    assert!(
        (y - 1.0).abs() < 1e-6,
        "the first tap's rumble lifted the runner, and the unplugged one did not"
    );
    frames
}

#[test]
fn a_replayed_pad_session_reproduces_every_tick_digest() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), ACTIONS, PLAYER);
    let recorded = record(dir.path());

    let mut app = booted(dir.path());
    // As playback does: the recording, not the live poll, says what the pads are.
    *app.engine.resource::<replay::ReplayMode>().borrow_mut() = replay::ReplayMode::Playing;
    for (tick, (sources, digest)) in recorded.iter().enumerate() {
        replay::restore(&app.engine, sources);
        app.tick(DEFAULT_FIXED_DT);
        assert_eq!(
            digest::digest(&app.engine).0,
            *digest,
            "replay parted from the recording at tick {tick}"
        );
    }
    assert!(
        (position(&app)[1] - 1.0).abs() < 1e-6,
        "the rumble finished on replay too"
    );
}
