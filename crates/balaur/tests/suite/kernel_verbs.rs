//! The kernel calls no game or editor script makes yet, each run from a
//! script and checked by what it answered or changed.

use balaur::{App, AppConfig, DEFAULT_FIXED_DT, standard_app};
use balaur_script::Value;

const SCENE: &str = r#"[[nodes]]
id = "n"
name = "Probe"
script = { source = "scripts/probe.bs" }
"#;

/// A game whose one node runs `script`, with `settings` appended to its
/// `project.toml` and `files` beside it.
fn booted(script: &str, settings: &str, files: &[(&str, &[u8])]) -> (tempfile::TempDir, App) {
    booted_with(SCENE, script, settings, files, |_| {})
}

/// [`booted`] with its own scene, and `before` run on the app before the
/// project loads.
fn booted_with(
    scene: &str,
    script: &str,
    settings: &str,
    files: &[(&str, &[u8])],
    before: impl FnOnce(&App),
) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!("[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n\n{settings}"),
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    std::fs::write(dir.path().join("scripts").join("probe.bs"), script).unwrap();
    for (path, bytes) in files {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    before(&app);
    app.load_project().unwrap();
    (dir, app)
}

fn node(app: &App, path: &str) -> balaur::hecs::Entity {
    let world = app.engine.world();
    balaur_core::scene::find_node(&world, app.engine.root(), path)
        .unwrap_or_else(|| panic!("the scene has no {path}"))
}

/// What the script on the node at `path` holds under `name`.
fn field_of(app: &App, path: &str, name: &str) -> Option<Value> {
    let at = node(app, path);
    app.engine
        .script_host()?
        .script_field(balaur::node_id_of(at), name)
}

fn field(app: &App, name: &str) -> Option<Value> {
    field_of(app, "Probe", name)
}

fn number(app: &App, name: &str) -> f64 {
    match field(app, name) {
        Some(Value::Num(n)) => n,
        other => panic!("{name} is {other:?}"),
    }
}

#[test]
fn the_math_a_script_has_answers_as_libm_does() {
    let (_dir, app) = booted(
        r"pub class Probe { node: Node, atan: float, exp: float, log: float, log10: float, sinh: float, cosh: float, tanh: float }
impl Probe {
    pub fn init() {
        self.atan = math::atan(1.0);
        self.exp = math::exp(1.0);
        self.log = math::log(10.0);
        self.log10 = math::log10(1000.0);
        self.sinh = math::sinh(1.0);
        self.cosh = math::cosh(1.0);
        self.tanh = math::tanh(0.5);
    }
}
",
        "",
        &[],
    );
    for (name, want) in [
        ("atan", std::f64::consts::FRAC_PI_4),
        ("exp", std::f64::consts::E),
        ("log", std::f64::consts::LN_10),
        ("log10", 3.0),
        ("sinh", 1.175_201_193_643_801_4),
        ("cosh", 1.543_080_634_815_243_7),
        ("tanh", 0.462_117_157_260_009_74),
    ] {
        let got = number(&app, name);
        assert!(
            (got - want).abs() < 1e-12,
            "math::{name} answered {got}, not {want}"
        );
    }
}

#[test]
fn a_script_reads_the_tick_it_runs_in_and_the_rate_the_project_set() {
    let (_dir, mut app) = booted(
        r"pub class Probe { node: Node, rate: int, seen: int, steps: int }
impl Probe {
    pub fn init() {
        self.rate = engine::tick_hz();
    }
    pub fn fixed_update(dt: float) {
        self.seen = engine::tick();
        self.steps += 1;
    }
}
",
        "[time]\ntick_hz = 30\n",
        &[],
    );
    assert_eq!(field(&app, "rate"), Some(Value::Int(30)));
    let step = 1.0 / 30.0;
    for _ in 0..5 {
        app.tick(step);
    }
    let Some(Value::Int(steps)) = field(&app, "steps") else {
        panic!("fixed_update never ran");
    };
    assert!(steps > 0, "fixed_update never ran");
    let ticked = i64::try_from(app.engine.tick()).unwrap();
    assert_eq!(field(&app, "seen"), Some(Value::Int(ticked)));
}

#[test]
fn a_node_that_stops_listening_hears_no_more() {
    let (_dir, mut app) = booted(
        r#"pub class Probe { node: Node, heard: int, frame: int }
impl Probe {
    pub fn init() {
        events::listen(self.node, "ping");
    }
    pub fn update(dt: float) {
        self.frame += 1;
        if self.frame == 1 {
            self.node.emit("ping", nil);
        }
        if self.frame == 3 {
            events::stop_listening(self.node, "ping");
            self.node.emit("ping", nil);
        }
    }
    pub fn on_ping(payload: any, source: any) {
        self.heard += 1;
    }
}
"#,
        "",
        &[],
    );
    for _ in 0..6 {
        app.tick(DEFAULT_FIXED_DT);
    }
    assert_eq!(field(&app, "frame"), Some(Value::Int(6)));
    assert_eq!(
        field(&app, "heard"),
        Some(Value::Int(1)),
        "the first ping is heard, the one after stop_listening is not"
    );
}

#[test]
fn a_script_reads_the_locale_the_system_reports() {
    let script = r#"pub class Probe { node: Node, locale: str }
impl Probe {
    pub fn init() {
        let said = strings::system_locale();
        self.locale = if said == nil { "nil" } else { said.unwrap() };
    }
}
"#;
    let told = |locale: Option<&str>| {
        let (_dir, app) = booted_with(SCENE, script, "", &[], |app| {
            let facts = balaur::facts::PlatformFacts {
                system_locale: locale.map(String::from),
                ..balaur::facts::PlatformFacts::default()
            };
            app.engine.resource::<balaur::facts::Facts>().borrow_mut().0 = Some(facts);
        });
        field(&app, "locale")
    };
    assert_eq!(told(Some("ro-RO")), Some(Value::Str("ro-RO".into())));
    assert_eq!(told(None), Some(Value::Str("nil".into())));
}

// The two nodes take the same jump in the same step; only the one that resets
// is drawn from where it landed rather than from where it left.
#[test]
fn a_reset_drops_the_pose_a_node_was_blending_from() {
    let scene = r#"[[nodes]]
id = "w"
name = "World"

[[nodes]]
id = "t"
name = "Teleported"
parent = "w"
script = { source = "scripts/probe.bs" }

[[nodes]]
id = "m"
name = "Moved"
parent = "w"
script = { source = "scripts/probe.bs" }
"#;
    let (_dir, mut app) = booted_with(
        scene,
        r#"pub class Probe { node: Node, steps: int }
impl Probe {
    pub fn fixed_update(dt: float) {
        self.steps += 1;
        if self.steps == 2 {
            self.node.transform.position = vec3(10.0, 0.0, 0.0);
            if self.node.name() == "Teleported" {
                self.node.reset_interpolation();
            }
        }
    }
}
"#,
        "",
        &[],
        |_| {},
    );
    for _ in 0..20 {
        if field_of(&app, "World/Teleported", "steps") == Some(Value::Int(2)) {
            break;
        }
        app.tick(DEFAULT_FIXED_DT);
    }
    assert_eq!(
        field_of(&app, "World/Teleported", "steps"),
        Some(Value::Int(2))
    );
    let from = |path: &str| {
        let at = node(&app, path);
        let world = app.engine.world();
        let kept = world
            .get::<&balaur::interpolate::Interpolation>(at)
            .unwrap_or_else(|_| panic!("{path} keeps no poses"));
        (kept.previous.position.x, kept.current.position.x)
    };
    assert_eq!(
        from("World/Moved"),
        (0.0, 10.0),
        "the control blends from 0 to 10"
    );
    assert_eq!(from("World/Teleported"), (10.0, 10.0));
}

#[test]
fn every_node_carrying_a_component_is_found_in_tree_order() {
    let scene = r#"[[nodes]]
id = "p"
name = "Probe"
script = { source = "scripts/probe.bs" }

[[nodes]]
id = "a"
name = "A"
parent = "p"
timer = { wait_time = 1.0 }

[[nodes]]
id = "b"
name = "B"
parent = "a"
timer = { wait_time = 1.0 }

[[nodes]]
id = "c"
name = "C"
parent = "p"

[[nodes]]
id = "d"
name = "D"
parent = "c"
timer = { wait_time = 1.0 }
"#;
    let (_dir, app) = booted_with(
        scene,
        r#"pub class Probe { node: Node, found: str, none: int }
impl Probe {
    pub fn init() {
        for at in scene::with_component("timer") {
            self.found = self.found + at.name() + ";";
        }
        self.none = scene::with_component("states").len();
    }
}
"#,
        "",
        &[],
        |_| {},
    );
    assert_eq!(field(&app, "found"), Some(Value::Str("A;B;D;".into())));
    assert_eq!(field(&app, "none"), Some(Value::Int(0)));
}

#[test]
fn a_reference_that_resolves_to_nothing_is_absent_rather_than_an_error() {
    let (_dir, app) = booted(
        r#"pub class Probe { node: Node, seen: str }
impl Probe {
    pub fn init() {
        self.seen = "{assets::exists("art/dot.png")} {assets::exists("art/none.png")} {assets::exists("id://nowhere")}";
    }
}
"#,
        "",
        &[("art/dot.png", b"\x89PNG")],
    );
    assert_eq!(
        field(&app, "seen"),
        Some(Value::Str("true false false".into()))
    );
}

// Every whole-step wait up to 10 seconds, each beside a `task::frames` wait of
// its step count asked on the same tick, through a pause that holds both.
#[test]
fn a_seconds_wait_wakes_on_the_step_a_frames_wait_of_its_length_does() {
    let (_dir, mut app) = booted(
        r#"pub class Probe { node: Node, by_seconds: [int], by_frames: [int], woken: int }
impl Probe {
    pub fn init() {
        self.by_seconds = [0; 600];
        self.by_frames = [0; 600];
        for k in 1..600 {
            self.node.call_async("seconds_wait", k);
            self.node.call_async("frames_wait", k);
        }
    }
    pub async fn seconds_wait(k: int) {
        await task::seconds(k as float / 60.0);
        self.by_seconds[k] = engine::tick();
        self.woken += 1;
    }
    pub async fn frames_wait(k: int) {
        await task::frames(k);
        self.by_frames[k] = engine::tick();
        self.woken += 1;
    }
}
"#,
        "",
        &[],
    );
    for frame in 0..700 {
        app.engine.set_paused((100..130).contains(&frame));
        app.tick(DEFAULT_FIXED_DT);
    }
    assert_eq!(field(&app, "woken"), Some(Value::Int(1198)));
    let (Some(Value::List(by_seconds)), Some(Value::List(by_frames))) =
        (field(&app, "by_seconds"), field(&app, "by_frames"))
    else {
        panic!("the probe holds no lists");
    };
    let late: Vec<usize> = (1..600)
        .filter(|&k| by_seconds[k] != by_frames[k])
        .collect();
    assert!(
        late.is_empty(),
        "{} waits woke on another step: {late:?}",
        late.len()
    );
    let tick = |k: usize| match by_seconds[k] {
        Value::Int(tick) => tick,
        ref other => panic!("wait {k} woke on {other:?}"),
    };
    assert_eq!(tick(2) - tick(1), 1, "one step apart");
    assert_eq!(tick(599) - tick(1), 598 + 30, "the paused span holds them");
}

#[test]
fn a_script_reads_the_time_zone_a_test_holds_by_its_name() {
    let (_dir, app) = booted_with(
        SCENE,
        r#"pub class Probe { node: Node, zone: str }
impl Probe {
    pub fn init() {
        let zone = engine::time_zone();
        self.zone = if zone == nil { "none" } else { zone.unwrap() };
    }
}
"#,
        "",
        &[],
        |app| balaur_core::facts::hold_time_zone(&app.engine, Some("Europe/Madrid".into())),
    );
    assert_eq!(field(&app, "zone"), Some(Value::text("Europe/Madrid")));
}

#[test]
fn a_script_reads_the_machines_time_zone_at_an_instant() {
    // A July and a January noon of 2026, UTC: the two sides of daylight saving.
    let (_dir, app) = booted(
        r"pub class Probe { node: Node, summer: int, winter: int, whole: int, now: int }
impl Probe {
    pub fn init() {
        self.summer = engine::utc_offset(1784894400.0);
        self.winter = engine::utc_offset(1768478400.0);
        self.whole = engine::utc_offset(1784894400);
        self.now = engine::utc_offset(engine::unix_time());
    }
}
",
        "",
        &[],
    );
    for (name, at) in [("summer", 1_784_894_400.0), ("winter", 1_768_478_400.0)] {
        let Some(Value::Int(seconds)) = field(&app, name) else {
            panic!("{name} is {:?}", field(&app, name));
        };
        assert_eq!(seconds, balaur_core::facts::utc_offset(at), "{name}");
        assert!(seconds.abs() <= 14 * 3600, "{name}: {seconds} s from UTC");
        assert_eq!(seconds % 60, 0, "{name}: {seconds} s is no whole minute");
    }
    assert_eq!(field(&app, "whole"), field(&app, "summer"));
    assert!(matches!(field(&app, "now"), Some(Value::Int(_))));
}
