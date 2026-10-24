//! `examples/hello`'s spinner in balaur_bs, and the rest of what a `.bs`
//! script asks of the engine.
//!
//! The same scene ticked the same frames must leave the node at the same
//! rotation, to the bit, whichever way the script was run: a dev build's
//! module, an export's pack or its native code.

#![cfg(any(feature = "bs", feature = "bs-jit"))]

use balaur::{App, AppConfig, standard_app};
use std::path::{Path, PathBuf};

const FRAMES: usize = 120;
const DT: f32 = 1.0 / 60.0;

fn hello() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello")
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// hello, with the spinner's script swapped for the file `script` names.
pub(super) fn project(script: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy(&hello(), dir.path());
    let scene = dir.path().join("scenes/main.toml");
    let text = std::fs::read_to_string(&scene).unwrap();
    let swapped = text.replace("\"scripts/spinner.bs\"", &format!("\"scripts/{script}\""));
    assert!(
        text != swapped || script == "spinner.bs",
        "the scene no longer names scripts/spinner.bs"
    );
    std::fs::write(scene, swapped).unwrap();
    dir
}

pub(super) fn spun(app: &App) -> [u32; 4] {
    let world = app.engine.world();
    let node = balaur_core::scene::find_node(&world, app.engine.root(), "Main/World/Spinner")
        .expect("hello has a Spinner");
    let t = world.get::<&balaur_core::Transform>(node).unwrap();
    t.rotation.to_array().map(f32::to_bits)
}

pub(super) fn booted(dir: &Path) -> App {
    let mut app = standard_app(AppConfig::dev(dir.to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app
}

pub(super) fn ticked(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.tick(DT);
    }
}

/// Frames until `done`, for a file write the watcher reports from its own
/// thread; false if it has not after a few seconds.
#[allow(
    clippy::disallowed_methods,
    reason = "waits on a file watcher's thread, which runs on wall time; nothing simulated reads it"
)]
fn ticked_until(app: &mut App, done: impl Fn(&App) -> bool) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(5) {
        app.tick(DT);
        if done(app) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

fn run(script: &str) -> [u32; 4] {
    let dir = project(script);
    let mut app = booted(dir.path());
    ticked(&mut app, FRAMES);
    spun(&app)
}

// hello gives the spinner a speed of 3.5, so over its 2 s it turns 7 radians
// about y: the quaternion (0, sin 3.5, 0, cos 3.5).
#[test]
fn the_spinner_turns_about_y_by_its_speed() {
    const SIN: f32 = -0.350_783_23;
    const COS: f32 = -0.936_456_7;
    let [x, y, z, w] = run("spinner.bs").map(f32::from_bits);
    // q and -q are the same turn.
    let sign = if w * COS >= 0.0 { 1.0 } else { -1.0 };
    assert!(x.abs() < 1e-6 && z.abs() < 1e-6, "{:?}", [x, y, z, w]);
    assert!(
        (y - sign * SIN).abs() < 1e-4 && (w - sign * COS).abs() < 1e-4,
        "the spinner ended at {:?}",
        [x, y, z, w]
    );
}

// What an iOS export does with Xcode's compiler, here with this machine's:
// the spinner's C, built and loaded, turns it to the same bits as the module.
#[cfg(all(feature = "bs", unix))]
#[test]
fn an_exported_spinner_runs_as_native_code() {
    // Before the load: a loaded build runs every later copy of the module.
    let module = run("spinner.bs");
    let dir = project("spinner.bs");
    let pack = balaur::build_pack(dir.path()).unwrap();
    let scripts = balaur::native_scripts(dir.path(), &mut [], &pack)
        .unwrap()
        .expect("hello has a .bs script");
    // hello's five scripts: the spinner, the ball, the ground, the menu and
    // the platform.
    assert_eq!((scripts.modules, scripts.framework), (5, "BalaurScripts"));
    let work = tempfile::tempdir().unwrap();
    let (source, lib) = (work.path().join("s.c"), work.path().join("s.lib"));
    std::fs::write(&source, &scripts.c).unwrap();
    let built = std::process::Command::new("cc")
        .args(["-O2", "-ffp-contract=off", "-fPIC", "-shared", "-w", "-o"])
        .arg(&lib)
        .arg(&source)
        .status()
        .expect("a C compiler");
    assert!(built.success());
    assert_eq!(balaur::script_bs::load_native(&lib).unwrap(), 5);

    let mut app = booted(dir.path());
    let host = balaur::script_bs::BsHost::of(&app.engine).unwrap();
    assert_eq!(host.runs_natively("scripts/spinner.bs"), Some(true));
    ticked(&mut app, FRAMES);
    assert_eq!(spun(&app), module);
}

#[test]
fn a_saved_bs_script_reloads_under_a_running_scene() {
    let dir = project("spinner.bs");
    let mut app = booted(dir.path());
    ticked(&mut app, FRAMES);
    let [x, ..] = spun(&app).map(f32::from_bits);
    assert!(x.abs() < 1e-6, "the spinner turns about y, not x");

    // The same angle, now about x: only a reload can move the x component.
    let path = dir.path().join("scripts/spinner.bs");
    let text = std::fs::read_to_string(&path).unwrap();
    let edited = text.replace("vec3(0.0, self.angle, 0.0)", "vec3(self.angle, 0.0, 0.0)");
    assert_ne!(text, edited, "spinner.bs no longer sets the angle this way");
    std::fs::write(&path, edited).unwrap();
    let reached = ticked_until(&mut app, |app| {
        let [x, ..] = spun(app).map(f32::from_bits);
        x.abs() > 1e-3
    });
    assert!(reached, "the edit never reached the running script");
}

// A library is a `.bs` file with no struct: it exports, and saving it reloads
// the scripts that `use` it.
#[test]
fn a_script_uses_a_library_file_and_reloads_with_it() {
    use balaur_script::Value::Int;
    let dir = one_script(
        "Counter",
        "use steps;\npub struct C { count: int }\nimpl C {\n    fn update(dt: float) {\n        self.count = steps::next(self.count);\n    }\n}\n",
    );
    let lib = dir.path().join("scripts/steps.bs");
    std::fs::write(&lib, "pub fn next(n: int) -> int { n + 1 }\n").unwrap();
    balaur::build_pack(dir.path()).expect("a library exports beside its users");
    let mut app = booted(dir.path());
    ticked(&mut app, 2);
    assert_eq!(field(&app, "Counter", "count"), Some(Int(2)));
    std::fs::write(&lib, "pub fn next(n: int) -> int { n + 100 }\n").unwrap();
    let reached = ticked_until(
        &mut app,
        |app| matches!(field(app, "Counter", "count"), Some(Int(n)) if n > 100),
    );
    assert!(reached, "the library's edit never reached its user");
}

// A library's struct gaining a field moves the one its user reads: the
// user's own unit, built against the old layout, is built again.
#[test]
fn a_library_whose_struct_changes_rebuilds_the_script_reading_it() {
    use balaur_script::Value::Int;
    let dir = one_script(
        "Counter",
        "use steps;\npub struct C { count: int }\nimpl C {\n    fn update(dt: float) {\n        self.count += steps::step().by;\n    }\n}\n",
    );
    let lib = dir.path().join("scripts/steps.bs");
    std::fs::write(
        &lib,
        "pub struct Step { by: int }\npub fn step() -> Step { Step { by: 1 } }\n",
    )
    .unwrap();
    let mut app = booted(dir.path());
    ticked(&mut app, 2);
    assert_eq!(field(&app, "Counter", "count"), Some(Int(2)));
    std::fs::write(
        &lib,
        "pub struct Step { first: int, by: int }\npub fn step() -> Step { Step { first: 0, by: 100 } }\n",
    )
    .unwrap();
    let reached = ticked_until(
        &mut app,
        |app| matches!(field(app, "Counter", "count"), Some(Int(n)) if n > 100),
    );
    assert!(reached, "the user still reads the struct as it was");
}

#[test]
fn an_exported_bs_spinner_runs_from_the_pack_alone() {
    let dir = project("spinner.bs");
    let bytes = balaur::build_pack(dir.path()).unwrap().encode();
    drop(dir);

    let pack = balaur_core::Pack::decode(&bytes).unwrap();
    assert!(pack.scripts.contains_key("scripts/spinner.bs"));
    let mut app = standard_app(AppConfig::packed(pack)).unwrap();
    app.load_project().unwrap();
    ticked(&mut app, FRAMES);
    assert_eq!(
        spun(&app),
        run("spinner.bs"),
        "the packed spinner ended somewhere else"
    );
}

#[test]
fn an_export_names_a_bs_script_that_does_not_compile() {
    let dir = project("spinner.bs");
    let path = dir.path().join("scripts/spinner.bs");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace("self.angle = 0.0;", "self.angle = true;"),
    )
    .unwrap();
    let err = balaur::build_pack(dir.path()).unwrap_err();
    assert!(format!("{err:#}").contains("scripts/spinner.bs"), "{err:#}");
}

const WAITER: &str = "pub struct Waiter { node: Node, stage: int }

impl Waiter {
    async fn init() {
        self.stage = 1;
        await task::frames(3);
        self.stage = 2;
        await task::seconds(0.5);
        self.stage = 3;
        self.node.transform.position = vec3(1.0, 0.0, 0.0);
    }
}
";

fn waiter() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Waiter\"\nscript = { source = \"scripts/w.bs\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/w.bs"), WAITER).unwrap();
    dir
}

fn stage(app: &App) -> Option<balaur_script::Value> {
    let node = {
        let world = app.engine.world();
        balaur_core::scene::find_node(&world, app.engine.root(), "Waiter").unwrap()
    };
    let host = app.engine.script_host().unwrap();
    host.script_field(balaur_core::node_id_of(node), "stage")
}

#[test]
fn an_async_init_awaits_engine_frames_and_seconds() {
    use balaur_script::Value::Int;
    let dir = waiter();
    let mut app = booted(dir.path());
    assert_eq!(stage(&app), Some(Int(1)), "init runs to its first await");
    ticked(&mut app, 2);
    assert_eq!(stage(&app), Some(Int(1)), "three frames have not passed");
    ticked(&mut app, 2);
    assert_eq!(stage(&app), Some(Int(2)));
    ticked(&mut app, 20);
    assert_eq!(stage(&app), Some(Int(2)), "half a second has not passed");
    ticked(&mut app, 20);
    assert_eq!(stage(&app), Some(Int(3)));
}

// A write that leaves a file as it was is no edit: a reload would drop the
// init parked on its await. macOS reports the writes from just before the
// watch began, so this is every script a test writes and then boots.
#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "gives the file watcher's thread wall time to report the write"
)]
fn a_file_written_as_it_was_keeps_its_parked_init() {
    use balaur_script::Value::Int;
    let dir = waiter();
    let mut app = booted(dir.path());
    assert_eq!(stage(&app), Some(Int(1)));
    std::fs::write(dir.path().join("scripts/w.bs"), WAITER).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    ticked(&mut app, 4);
    assert_eq!(stage(&app), Some(Int(2)), "the parked init went on");
}

const COUNTER: &str = "pub struct Counter { count: int }
impl Counter {
    fn update(dt: float) {
        let next = self.count + 1;
        self.count = next;
    }
}
";

fn one_script(name: &str, source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        format!(
            "[[nodes]]\nid = \"n\"\nname = \"{name}\"\nscript = {{ source = \"scripts/s.bs\" }}\n"
        ),
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/s.bs"), source).unwrap();
    dir
}

pub(super) fn field(app: &App, node: &str, name: &str) -> Option<balaur_script::Value> {
    let node = {
        let world = app.engine.world();
        balaur_core::scene::find_node(&world, app.engine.root(), node).unwrap()
    };
    let host = app.engine.script_host().unwrap();
    host.script_field(balaur_core::node_id_of(node), name)
}

#[test]
fn a_breakpoint_freezes_the_game_until_the_editor_steps_on() {
    use balaur_script::{PauseReason, StepMode, Value::Int};
    let dir = one_script("Counter", COUNTER);
    let mut app = booted(dir.path());
    ticked(&mut app, 1);
    assert_eq!(field(&app, "Counter", "count"), Some(Int(1)));

    let host = app.engine.script_host().unwrap();
    assert_eq!(host.set_breakpoints("scripts/s.bs", &[4]).unwrap(), [4]);
    assert_eq!(host.breakpoints("scripts/s.bs"), [4]);
    ticked(&mut app, 1);
    let stop = host.paused().expect("the breakpoint stopped the update");
    assert_eq!((stop.path.as_str(), stop.line), ("scripts/s.bs", 4));
    assert_eq!(stop.reason, PauseReason::Breakpoint);
    assert_eq!(stop.frames[0].function, "Counter.update");
    assert!(app.engine.is_frozen());
    ticked(&mut app, 3);
    assert!(host.paused().is_some(), "a frozen frame ran the script on");

    host.resume(StepMode::Over);
    let stop = host.paused().expect("stepping over stops on the next line");
    assert_eq!((stop.line, stop.reason), (5, PauseReason::Step));
    let next = stop.frames[0].locals.iter().find(|(n, _)| n == "next");
    assert_eq!(next.map(|(_, v)| v.clone()), Some(Int(2)));

    host.set_breakpoints("scripts/s.bs", &[]).unwrap();
    host.resume(StepMode::Continue);
    assert!(host.paused().is_none());
    assert!(!app.engine.is_frozen());
    assert_eq!(field(&app, "Counter", "count"), Some(Int(2)));
    ticked(&mut app, 1);
    assert_eq!(field(&app, "Counter", "count"), Some(Int(3)));
}

const TICKER: &str = "pub struct Ticker { ticks: int }
impl Ticker {
    async fn go() {
        await task::frames(1);
        let seen = self.ticks;
        self.ticks = seen + 1;
        await task::frames(1);
        self.ticks += 10;
    }
}
";

#[test]
fn a_breakpoint_in_an_async_method_holds_its_task() {
    use balaur_script::{PauseReason, StepMode, Value::Int};
    let dir = one_script("Ticker", TICKER);
    let mut app = booted(dir.path());
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("scripts/s.bs", &[5]).unwrap();
    host.call_all("go");
    assert!(host.paused().is_none(), "it waits a frame first");
    ticked(&mut app, 1);
    let stop = host.paused().expect("the breakpoint stopped the task");
    assert_eq!((stop.line, stop.reason), (5, PauseReason::Breakpoint));
    assert_eq!(stop.frames[0].function, "Ticker.go");
    assert!(app.engine.is_frozen());
    ticked(&mut app, 3);

    host.resume(StepMode::Over);
    let stop = host.paused().expect("stepping over stops on the next line");
    assert_eq!(stop.line, 6);
    let seen = stop.frames[0].locals.iter().find(|(n, _)| n == "seen");
    assert_eq!(seen.map(|(_, v)| v.clone()), Some(Int(0)));
    host.set_breakpoints("scripts/s.bs", &[]).unwrap();
    host.resume(StepMode::Continue);
    assert!(host.paused().is_none());
    assert!(!app.engine.is_frozen());
    assert_eq!(field(&app, "Ticker", "ticks"), Some(Int(1)));
    ticked(&mut app, 2);
    assert_eq!(
        field(&app, "Ticker", "ticks"),
        Some(Int(11)),
        "the task ran on"
    );
}

const CRASHER: &str = "pub struct Crasher { at: int }
impl Crasher {
    fn update(dt: float) {
        let items = [1, 2, 3];
        self.at += 1;
        self.at = items[self.at + 5];
    }
}
";

#[test]
fn break_on_error_holds_the_line_that_trapped() {
    use balaur_script::{PauseReason, StepMode, Value};
    let dir = one_script("Crasher", CRASHER);
    let mut app = booted(dir.path());
    let host = app.engine.script_host().unwrap();
    host.set_break_on_error(true);
    assert!(host.break_on_error());
    ticked(&mut app, 1);
    let stop = host.paused().expect("the trap stopped the game");
    assert_eq!((stop.line, stop.reason), (6, PauseReason::Error));
    assert!(stop.message.contains("update"), "{}", stop.message);
    let items = stop.frames[0].locals.iter().find(|(n, _)| n == "items");
    assert_eq!(
        items.map(|(_, v)| v.clone()),
        Some(Value::List(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3)
        ]))
    );
    assert!(app.engine.is_frozen());

    host.resume(StepMode::Continue);
    assert!(host.paused().is_none(), "a trap's stop is dismissed");
    assert!(!app.engine.is_frozen());
    host.set_break_on_error(false);
    ticked(&mut app, 1);
    assert!(host.paused().is_none(), "off, a trap is only logged");
}

#[test]
fn balaur_check_names_the_line_a_bs_script_breaks_on() {
    let broken = COUNTER.replace(
        "let next = self.count + 1;",
        "let next = self.count + true;",
    );
    let dir = one_script("Counter", &broken);
    let found = balaur::check_project(dir.path()).unwrap();
    let bl: Vec<_> = found.iter().filter(|f| f.file == "scripts/s.bs").collect();
    assert_eq!(bl.len(), 1, "{found:?}");
    assert_eq!(
        (bl[0].line, bl[0].severity),
        (4, "error"),
        "{}",
        bl[0].message
    );

    let dir = one_script("Counter", COUNTER);
    assert!(balaur::check_project(dir.path()).unwrap().is_empty());
}

const MOVER_BS: &str = "pub struct Mover { node: Node, arm: Node, seen: int, label: str }

impl Mover {
    fn init() {
        self.arm = self.node.get_node(\"Arm\");
        self.arm.set_name(\"Hand\");
        self.node.add_tag(\"mover\");
        self.seen = self.node.children().len();
        self.label = self.node.path();
    }

    fn update(dt: float) {
        self.node.transform.translate(dt, 0.0, 0.0);
        self.arm.transform.translate(0.0, dt, 0.0);
    }
}
";

/// A Mover with an Arm under it, the Mover running `source` as `scripts/m.<ext>`.
fn movers(ext: &str, source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    let scene = format!(
        "[[nodes]]\nid = \"m\"\nname = \"Mover\"\nscript = {{ source = \"scripts/m.{ext}\" }}\n\
         [nodes.transform]\nposition = [0, 0, 0]\n\n\
         [[nodes]]\nid = \"a\"\nname = \"Arm\"\nparent = \"m\"\n\
         [nodes.transform]\nposition = [0, 0, 0]\n"
    );
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    std::fs::write(dir.path().join(format!("scripts/m.{ext}")), source).unwrap();
    dir
}

/// What the script did: where both nodes went, its tags and what it read.
fn moved(ext: &str, source: &str) -> (u32, u32, Vec<String>, Vec<Option<balaur_script::Value>>) {
    let dir = movers(ext, source);
    let mut app = booted(dir.path());
    ticked(&mut app, 30);
    let (x, y, tags) = {
        let world = app.engine.world();
        let root = app.engine.root();
        let mover = balaur_core::scene::find_node(&world, root, "Mover").unwrap();
        let hand =
            balaur_core::scene::find_node(&world, root, "Mover/Hand").expect("the Arm was renamed");
        let x = world
            .get::<&balaur_core::Transform>(mover)
            .unwrap()
            .position
            .x;
        let y = world
            .get::<&balaur_core::Transform>(hand)
            .unwrap()
            .position
            .y;
        let tags = world
            .get::<&balaur_core::scene::Tags>(mover)
            .map(|t| t.0.clone())
            .unwrap_or_default();
        (x.to_bits(), y.to_bits(), tags)
    };
    let read = ["seen", "label"].map(|f| field(&app, "Mover", f)).to_vec();
    (x, y, tags, read)
}

#[test]
fn node_operations_reach_the_node_its_children_and_its_tags() {
    use balaur_script::Value::{Int, Str};
    let (x, y, tags, read) = moved("bs", MOVER_BS);
    // Thirty frames of a sixtieth each, translated by dt.
    let walked = 30.0 * DT;
    assert!(
        (f32::from_bits(x) - walked).abs() < 1e-5,
        "the Mover is at {}",
        f32::from_bits(x)
    );
    assert!(
        (f32::from_bits(y) - walked).abs() < 1e-5,
        "the Hand is at {}",
        f32::from_bits(y)
    );
    assert_eq!(tags, ["mover"]);
    assert_eq!(read[0], Some(Int(1)), "the Mover has one child");
    assert!(
        matches!(&read[1], Some(Str(path)) if path.ends_with("Mover")),
        "{:?}",
        read[1]
    );
}

const HITTER_BS: &str = "pub struct Hitter { node: Node, done: bool }

impl Hitter {
    fn update(dt: float) {
        if !self.done {
            self.done = true;
            let hit = map().with(\"damage\", 3).with(\"tags\", [\"fire\", \"ice\"]);
            self.node.get_node(\"Target\").call(\"take\", hit);
        }
    }
}
";

const LISTENER_BS: &str = "pub struct Listener { node: Node, heard: int, who: str }

impl Listener {
    pub fn hear(msg: any) -> int {
        self.heard = msg[\"got\"].as_int();
        self.who = msg[\"who\"].as_str();
        self.heard * 2
    }
}
";

const TARGET_BS: &str = "pub struct Target { node: Node, damage: int, first: str, reply: int }

impl Target {
    pub fn take(hit: any) {
        self.damage = hit[\"damage\"].as_int();
        self.first = hit[\"tags\"][0].as_str();
        let msg = map().with(\"got\", self.damage).with(\"who\", \"target\");
        self.reply = self.node.get_node(\"../Listener\").call(\"hear\", msg).as_int();
    }
}
";

#[test]
fn payloads_cross_between_scripts_as_any() {
    use balaur_script::Value::{Int, Str};
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    // Hitter is the root; Target and Listener sit under it.
    let node = |id: &str, name: &str, script: &str, parent: &str| {
        format!(
            "[[nodes]]\nid = \"{id}\"\nname = \"{name}\"\n{parent}script = {{ source = \"scripts/{script}\" }}\n\n"
        )
    };
    let under = "parent = \"h\"\n";
    let scene = node("h", "Hitter", "h.bs", "")
        + &node("t", "Target", "t.bs", under)
        + &node("l", "Listener", "l.bs", under);
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    for (file, source) in [
        ("h.bs", HITTER_BS),
        ("t.bs", TARGET_BS),
        ("l.bs", LISTENER_BS),
    ] {
        std::fs::write(dir.path().join("scripts").join(file), source).unwrap();
    }
    let mut app = booted(dir.path());
    ticked(&mut app, 2);
    assert_eq!(field(&app, "Hitter/Target", "damage"), Some(Int(3)));
    assert_eq!(
        field(&app, "Hitter/Target", "first"),
        Some(Str("fire".into()))
    );
    assert_eq!(field(&app, "Hitter/Listener", "heard"), Some(Int(3)));
    assert_eq!(
        field(&app, "Hitter/Listener", "who"),
        Some(Str("target".into()))
    );
    assert_eq!(field(&app, "Hitter/Target", "reply"), Some(Int(6)));
}

#[test]
fn a_binding_that_takes_or_gives_a_value_is_typed_any() {
    let source = "pub struct Fetcher { request: any }
impl Fetcher {
    fn init() {
        self.request = http::request(\"https://example.com\", nil(), nil());
    }
}
";
    let dir = one_script("Fetcher", source);
    let found = balaur::check_project(dir.path()).unwrap();
    assert!(found.is_empty(), "{found:?}");
}

const LOGGER_BS: &str = "pub struct Logger { node: Node, matched: bool, scale: float }
impl Logger {
    fn init() {
        log::info(\"bs says info\");
        log::warn(\"bs says warn\");
        log::error(\"bs says error\");
        self.matched = regex::matches(\"a+\", \"caaat\");
        self.scale = engine::time_scale();
    }
}
";

/// `log::info` and the other ops registered without Rust types are typed
/// from their docs: `(message: string)` takes a `str`, `-> float` gives a
/// `float`, and docs with no `->` give nothing back.
#[test]
fn a_bs_script_calls_the_engine_ops_its_docs_type() {
    use balaur_script::Value::{Bool, Num};
    balaur_core::logbuf::capture_for_test();
    let dir = one_script("Logger", LOGGER_BS);
    let found = balaur::check_project(dir.path()).unwrap();
    assert!(found.is_empty(), "{found:?}");
    let app = booted(dir.path());
    let lines = balaur_core::logbuf::recent(500);
    for level in ["info", "warn", "error"] {
        let message = format!("bs says {level}");
        assert!(
            lines
                .iter()
                .any(|e| e.tag == "script" && e.level == level && e.message == message),
            "no `{message}` at {level}: {lines:#?}"
        );
    }
    assert_eq!(field(&app, "Logger", "matched"), Some(Bool(true)));
    assert_eq!(field(&app, "Logger", "scale"), Some(Num(1.0)));

    let host = app.engine.script_host().unwrap();
    let tools = host
        .tools_for("scripts/s.bs")
        .expect("a .bs file has tools");
    let typing = LOGGER_BS.replace("log::info(\"bs says info\");", "log::");
    let offered: Vec<String> = tools
        .complete("scripts/s.bs", &typing, 4, 14)
        .into_iter()
        .map(|i| format!("{}: {}", i.label, i.detail))
        .collect();
    for want in [
        "info: fn info(str)",
        "recent: fn recent(int?) -> [log::Entry]",
        "Field: struct log::Field { name: str, value: str }",
    ] {
        assert!(offered.contains(&want.into()), "{offered:?}");
    }
    assert_eq!(
        tools.hover("scripts/s.bs", LOGGER_BS, 4, 15).as_deref(),
        Some("fn log::info(str)")
    );
}

const REACH_BS: &str = "pub struct Reach {
    node: Node,
    button: int,
    turn: float,
    missing: bool,
    position: float,
    pointer: float,
}
impl Reach {
    fn init() {
        self.button = input::MOUSE_BUTTON_LEFT;
        self.turn = math::PI * 2.0;
        events::listen(self.node, \"ping\");
        render::draw_line_2d(0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0);
        self.missing = !scene::get_node(\"Nowhere\").is_valid();
        self.position = self.node.get_component(\"transform\", \"position\")[0].as_float();
        self.pointer = input::mouse_position().x;
    }
}
";

/// What the editor's calls need from the engine, each typed: its constants,
/// a call leaving optional arguments out, a node that is not there, one
/// property of a component, and a tuple as a vector.
#[test]
fn a_bs_script_reaches_constants_optional_arguments_and_tuples() {
    use balaur_script::Value::{Bool, Int, Num};
    let dir = one_script("Reach", REACH_BS);
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Reach\"\nscript = { source = \"scripts/s.bs\" }\n\
         [nodes.transform]\nposition = [3, 4, 5]\n",
    )
    .unwrap();
    let found = balaur::check_project(dir.path()).unwrap();
    assert!(found.is_empty(), "{found:?}");
    let app = booted(dir.path());
    assert_eq!(field(&app, "Reach", "button"), Some(Int(0)));
    assert_eq!(
        field(&app, "Reach", "turn"),
        Some(Num(std::f64::consts::TAU))
    );
    assert_eq!(field(&app, "Reach", "missing"), Some(Bool(true)));
    assert_eq!(field(&app, "Reach", "position"), Some(Num(3.0)));
    assert_eq!(field(&app, "Reach", "pointer"), Some(Num(0.0)));
}

const CHECKER_BS: &str = "pub struct Checker { node: Node, count: int, message: str, line: int, first: str }
impl Checker {
    fn init() {
        let found = script::check(\"scripts/other.bs\", \"pub struct A {{ x: int }}\\nfn f() -> int {{ nope }}\");
        self.count = found.len();
        self.message = found[0].message;
        self.line = found[0].line;
        let outline = script::symbols(\"scripts/other.bs\", \"pub struct A {{ x: int }}\");
        self.first = outline[0].name;
    }
}
";

/// An engine call that hands back tables a record declares gives a script
/// structs: `found[0].message`, not `found[0][\"message\"].as_str()`.
#[test]
fn a_record_result_is_a_struct_a_script_reads_by_field() {
    use balaur_script::Value::{Int, Str};
    let dir = one_script("Checker", CHECKER_BS);
    let found = balaur::check_project(dir.path()).unwrap();
    assert!(found.is_empty(), "{found:?}");
    let app = booted(dir.path());
    assert_eq!(field(&app, "Checker", "count"), Some(Int(1)));
    assert_eq!(field(&app, "Checker", "line"), Some(Int(2)));
    let Some(Str(message)) = field(&app, "Checker", "message") else {
        panic!("no message");
    };
    assert!(message.contains("nope"), "{message}");
    assert_eq!(field(&app, "Checker", "first"), Some(Str("A".into())));
}

const ORC_BS: &str = "pub struct Orc { node: Node }

impl Orc {
    pub fn info() -> any {
        map().with(\"hp\", 7).with(\"name\", \"orc\")
    }
}
";

const ASKER_BS: &str = "pub struct Asker { node: Node, hp: int, name: str }

impl Asker {
    async fn init() {
        let asked = self.node.get_node(\"Orc\").call_async(\"info\", nil());
        let info = await task::wait(asked);
        self.hp = info[\"hp\"].as_int();
        self.name = info[\"name\"].as_str();
    }
}
";

#[test]
fn task_wait_hands_an_async_init_what_the_engine_answered() {
    use balaur_script::Value::{Int, Str};
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    let scene = "[[nodes]]\nid = \"a\"\nname = \"Asker\"\nscript = { source = \"scripts/a.bs\" }\n\n\
                 [[nodes]]\nid = \"o\"\nname = \"Orc\"\nparent = \"a\"\nscript = { source = \"scripts/o.bs\" }\n";
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    std::fs::write(dir.path().join("scripts/a.bs"), ASKER_BS).unwrap();
    std::fs::write(dir.path().join("scripts/o.bs"), ORC_BS).unwrap();
    let mut app = booted(dir.path());
    assert_eq!(
        field(&app, "Asker", "hp"),
        Some(Int(0)),
        "the answer comes a step later"
    );
    ticked(&mut app, 3);
    assert_eq!(field(&app, "Asker", "hp"), Some(Int(7)));
    assert_eq!(field(&app, "Asker", "name"), Some(Str("orc".into())));
}

const TOOLED_BS: &str = "pub struct Tooled { node: Node, speed: float }
impl Tooled {
    fn update(dt: float) {
        self.speed += dt;
    }
}
";

#[test]
fn a_bs_file_answers_the_editor_s_tooling() {
    let dir = one_script("Tooled", TOOLED_BS);
    let app = booted(dir.path());
    let host = app.engine.script_host().unwrap();
    let tools = host
        .tools_for("scripts/s.bs")
        .expect("a .bs file has tools");
    let typing = TOOLED_BS.replace("self.speed += dt;", "self.");
    let labels: Vec<String> = tools
        .complete("scripts/s.bs", &typing, 4, 14)
        .into_iter()
        .map(|i| i.label)
        .collect();
    assert!(
        labels.contains(&"speed".into()) && labels.contains(&"node".into()),
        "{labels:?}"
    );
    assert_eq!(
        tools.hover("scripts/s.bs", TOOLED_BS, 4, 15).as_deref(),
        Some("speed: float")
    );
    let def = tools
        .definition("scripts/s.bs", TOOLED_BS, 4, 15)
        .expect("a field");
    assert_eq!((def.file.as_str(), def.line), ("scripts/s.bs", 1));
    let found = tools.check("scripts/s.bs", "fn f() -> int { true }");
    assert_eq!((found.len(), found[0].line), (1, 1));
}

const ASKS_TOOLS_BS: &str = r#"pub struct Editor {
    node: Node,
    completed: int,
    first: str,
    errors: int,
    hover: str,
    symbols: int,
    hook: str,
    renamed: str,
    formatted: str,
}

impl Editor {
    fn init() {
        let src = "pub struct T {{ speed: float }}\nimpl T {{\n    fn update(dt: float) {{\n        self.\n    }}\n}}\n";
        let rows = script::complete("scripts/t.bs", src, 4, 14);
        self.completed = rows.len();
        self.first = rows[0].label;
        self.errors = script::check("scripts/t.bs", "fn f() -> int {{ true }}").len();
        let hover = script::hover("scripts/t.bs", "pub struct T {{ speed: float }}\nfn g(t: T) -> float {{ t.speed }}\n", 2, 26);
        self.hover = hover.unwrap().title;
        let syms = script::symbols("scripts/t.bs", "pub struct T {{ pub hp: int = 3 }}\nimpl T {{\n    fn update(dt: float) {{\n    }}\n}}\n");
        self.symbols = syms.len();
        self.hook = syms[2].name + ":" + syms[2].kind;
        self.renamed = script::rename("scripts/t.bs", "pub struct T {{ hp: int }}\n", "hp", "health")[0].source;
        self.formatted = script::format("scripts/t.bs", "fn f() {{\nlet x = 1;\n}}\n");
    }
}
"#;

#[test]
fn the_editor_s_script_module_answers_for_a_bs_file() {
    use balaur_script::Value::{Int, Str};
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"e\"\nname = \"Editor\"\nscript = { source = \"scripts/e.bs\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/e.bs"), ASKS_TOOLS_BS).unwrap();
    let app = booted(dir.path());
    // The field and the method.
    assert_eq!(field(&app, "Editor", "completed"), Some(Int(2)));
    assert_eq!(field(&app, "Editor", "first"), Some(Str("speed".into())));
    assert_eq!(field(&app, "Editor", "errors"), Some(Int(1)));
    assert_eq!(
        field(&app, "Editor", "hover"),
        Some(Str("speed: float".into()))
    );
    assert_eq!(field(&app, "Editor", "symbols"), Some(Int(3)));
    assert_eq!(
        field(&app, "Editor", "hook"),
        Some(Str("update:function".into()))
    );
    assert_eq!(
        field(&app, "Editor", "renamed"),
        Some(Str("pub struct T { health: int }\n".into()))
    );
    assert_eq!(
        field(&app, "Editor", "formatted"),
        Some(Str("fn f() {\n    let x = 1;\n}\n".into()))
    );
}

/// One script, as `scripts/s.bs` on a node named `B`.
fn bench_project(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"B\"\nscript = { source = \"scripts/s.bs\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/s.bs"), source).unwrap();
    dir
}

// Nanoseconds per engine call: a frame whose update makes `CALLS` of one
// kind, timed past a first frame that compiles and warms up.
#[test]
#[ignore = "a measurement: run with --release -- --ignored --nocapture"]
#[allow(
    clippy::disallowed_methods,
    reason = "a measurement reads the wall clock; it ticks no simulation it could make unreproducible"
)]
fn engine_calls_cost() {
    const CALLS: u32 = 200_000;
    let time = |source: String| {
        let dir = bench_project(&source);
        let mut app = booted(dir.path());
        ticked(&mut app, 2);
        (0..5)
            .map(|_| {
                let t = std::time::Instant::now();
                ticked(&mut app, 1);
                t.elapsed().as_secs_f64() * 1e9 / f64::from(CALLS)
            })
            .fold(f64::MAX, f64::min)
    };
    let bs = |body: &str| {
        format!(
            "pub struct B {{ node: Node, out: float }}\nimpl B {{\n    fn update(dt: float) {{\n        let t = 0.0;\n        for i in 0..{CALLS} {{ {body} }}\n        self.out = t;\n    }}\n}}\n"
        )
    };
    let rows = [
        (
            "binding, one string argument",
            bs("t += input::action_value(\"spin\");"),
        ),
        (
            "node operation",
            bs("if self.node.is_valid() { t += 1.0; }"),
        ),
        (
            "component property read",
            bs("t += self.node.transform.position.x;"),
        ),
    ];
    eprintln!("{:<32}{:>14}", "ns per engine call", "Balaur Script");
    for (label, source) in rows {
        eprintln!("{label:<32}{:>14.0}", time(source));
    }
}

const LIBRARY_CHECK_BS: &str = "use util;
use defs;
use icons;
pub struct Check { said: str }
impl Check {
    fn init() {
        let c = util::hex_to_rgb(\"#ff8000\");
        let back = util::rotate_euler([0.1, 0.2, 0.3], 2, 0.0);
        let cells = util::cells([\"a\", \"b\"]);
        let m = map().with(\"b\", 1).with(\"a\", 2);
        let copy = util::deep(m);
        copy.set(\"c\", 3);
        self.said = \"{util::prop_label(\\\"edge_radius\\\")}|{util::prop_label(\\\"uv_scale\\\")}|{util::match_case(\\\"Arm_L\\\", \\\"arm_r\\\")}|{util::to_hex([1.0, 0.5, 0.0])}|{c.r} {c.b}|{util::index_of(\\\"a/b\\\", \\\"/\\\").or(-1)}|{cells.len()}|{util::join(util::keys_sorted(m), \\\",\\\")}|{m.has(\\\"c\\\")}|{util::dist([0.0, 0.0, 0.0], [3.0, 4.0, 0.0])}|{util::engine_path(\\\"/g\\\", \\\"a.toml\\\")}|{back.len()}|{defs::workspace_name(\\\"ui\\\")}|{defs::chip_on(map(), \\\"scene\\\", \\\"proj\\\")}|{icons::component(\\\"widget\\\").token}\";
    }
}
";

/// The editor's `util`, `defs` and `icons` libraries, one call each, read
/// back from the script's field.
#[test]
fn the_editor_libraries_answer_a_script_that_uses_them() {
    use balaur_script::Value::Str;
    let dir = one_script("Check", LIBRARY_CHECK_BS);
    let editor = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editor/scripts");
    for lib in ["util", "defs", "icons"] {
        let from = editor.join(format!("{lib}.bs"));
        std::fs::copy(&from, dir.path().join(format!("scripts/{lib}.bs"))).unwrap();
    }
    let found = balaur::check_project(dir.path()).unwrap();
    assert!(found.is_empty(), "{found:?}");
    let app = booted(dir.path());
    assert_eq!(
        field(&app, "Check", "said"),
        Some(Str(
            "Edge radius|UV scale|Arm_r|#ff8000|1.0 0.0|1|3|a,b|false|5.0|/g/a.toml|3|UI|true|node_ui"
                .into()
        ))
    );
}

const NAMED_HANDLERS: &str = r#"pub struct Panel { frame: int, said: str, clicks: int }
impl Panel {
    pub fn update(dt: float) {
        self.frame += 1;
        let controls: [any] = [
            map().with("kind", "text_field").with("text", "").with("on", "typed").with("data", "query"),
            map().with("kind", "button").with("text", "Go").with("on", "pressed").with("data", 7),
        ];
        ui::fill_strip("Panel/Strip", controls);
        if self.frame == 2 {
            let made = scene::get_node("Panel/Strip").children();
            ui::edit(made[0], "ball");
            ui::click(made[1]);
        }
    }
    pub fn typed(data: any, text: any) {
        self.said = "{data}={text}";
    }
    pub fn pressed(data: any, on: any) {
        if on.as_bool() {
            self.clicks += data.as_int();
        }
    }
}
"#;

// A script that hands the engine no functions names its handler: the pool
// calls it on the script above the control, with the spec's data first.
#[test]
fn a_pooled_control_calls_the_method_it_names_with_its_data() {
    use balaur_script::Value::{Int, Str};
    let dir = one_script("Panel", NAMED_HANDLERS);
    let scene = dir.path().join("main.toml");
    let mut text = std::fs::read_to_string(&scene).unwrap();
    text.push_str("\n[[nodes]]\nid = \"strip\"\nname = \"Strip\"\nparent = \"n\"\n[nodes.widget]\nkind = \"row\"\n");
    std::fs::write(&scene, text).unwrap();
    let mut app = booted(dir.path());
    ticked(&mut app, 4);
    assert_eq!(field(&app, "Panel", "said"), Some(Str("query=ball".into())));
    assert_eq!(field(&app, "Panel", "clicks"), Some(Int(7)));
}

const EXPORTER_BS: &str =
    "pub struct Tuned { pub speed: float = 2.5, pub label: str = \"go\", seen: str }
impl Tuned {
    fn init() {
        let rows = script::exports(\"scripts/s.bs\");
        self.seen = \"{rows.len()} {rows[0].name}={rows[0].default} {rows[1].name}:{rows[1].type}\";
    }
}
";

/// What a script declares tunable, as the Inspector reads it.
#[test]
fn a_script_reads_what_another_declares_tunable() {
    use balaur_script::Value::Str;
    let dir = one_script("Tuned", EXPORTER_BS);
    let app = booted(dir.path());
    assert_eq!(
        field(&app, "Tuned", "seen"),
        Some(Str("2 label=go speed:float".into()))
    );
}
