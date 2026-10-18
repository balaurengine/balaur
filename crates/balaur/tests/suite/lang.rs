//! `examples/hello`'s spinner in balaur_lang, beside its Rune original.
//!
//! The same scene ticked the same frames must leave the node at the same
//! rotation, to the bit, whichever language turned it.

#![cfg(any(feature = "lang", feature = "lang-jit"))]

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
fn project(script: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy(&hello(), dir.path());
    let scene = dir.path().join("scenes/main.toml");
    let text = std::fs::read_to_string(&scene).unwrap();
    let swapped = text.replace("\"scripts/spinner.rn\"", &format!("\"scripts/{script}\""));
    assert!(
        text != swapped || script == "spinner.rn",
        "the scene no longer names scripts/spinner.rn"
    );
    std::fs::write(scene, swapped).unwrap();
    dir
}

fn spun(app: &App) -> [u32; 4] {
    let world = app.engine.world();
    let node = balaur_core::scene::find_node(&world, app.engine.root(), "Main/World/Spinner")
        .expect("hello has a Spinner");
    let t = world.get::<&balaur_core::Transform>(node).unwrap();
    t.rotation.to_array().map(f32::to_bits)
}

fn booted(dir: &Path) -> App {
    let mut app = standard_app(AppConfig::dev(dir.to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app
}

fn ticked(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.tick(DT);
    }
}

fn run(script: &str) -> [u32; 4] {
    let dir = project(script);
    let mut app = booted(dir.path());
    ticked(&mut app, FRAMES);
    spun(&app)
}

#[test]
fn the_spinner_turns_the_same_in_either_language() {
    let rune = run("spinner.rn");
    let lang = run("spinner.bl");
    assert_ne!(
        rune,
        [0, 0, 0, 1f32.to_bits()],
        "the Rune spinner never turned"
    );
    assert_eq!(lang, rune, "the .bl spinner ended somewhere else");
}

#[test]
fn a_saved_bl_script_reloads_under_a_running_scene() {
    let dir = project("spinner.bl");
    let mut app = booted(dir.path());
    ticked(&mut app, FRAMES);
    let [x, ..] = spun(&app).map(f32::from_bits);
    assert!(x.abs() < 1e-6, "the spinner turns about y, not x");

    // The same angle, now about x: only a reload can move the x component.
    let path = dir.path().join("scripts/spinner.bl");
    let text = std::fs::read_to_string(&path).unwrap();
    let edited = text.replace("vec3(0.0, self.angle, 0.0)", "vec3(self.angle, 0.0, 0.0)");
    assert_ne!(text, edited, "spinner.bl no longer sets the angle this way");
    std::fs::write(&path, edited).unwrap();
    // The watcher looks every few frames, so this many cover one look.
    ticked(&mut app, balaur::script_lang::POLL_FRAMES as usize);
    let [x, ..] = spun(&app).map(f32::from_bits);
    assert!(x.abs() > 1e-3, "the edit never reached the running script");
}

#[test]
fn an_exported_bl_spinner_runs_from_the_pack_alone() {
    let dir = project("spinner.bl");
    let bytes = balaur::build_pack(dir.path()).unwrap().encode();
    drop(dir);

    let pack = balaur_core::Pack::decode(&bytes).unwrap();
    assert!(pack.scripts.contains_key("scripts/spinner.bl"));
    let mut app = standard_app(AppConfig::packed(pack)).unwrap();
    app.load_project().unwrap();
    ticked(&mut app, FRAMES);
    assert_eq!(
        spun(&app),
        run("spinner.rn"),
        "the packed spinner ended somewhere else"
    );
}

#[test]
fn an_export_names_a_bl_script_that_does_not_compile() {
    let dir = project("spinner.bl");
    let path = dir.path().join("scripts/spinner.bl");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace("self.angle = 0.0;", "self.angle = true;"),
    )
    .unwrap();
    let err = balaur::build_pack(dir.path()).unwrap_err();
    assert!(format!("{err:#}").contains("scripts/spinner.bl"), "{err:#}");
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
        "[[nodes]]\nid = \"n\"\nname = \"Waiter\"\nscript = { source = \"scripts/w.bl\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/w.bl"), WAITER).unwrap();
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
            "[[nodes]]\nid = \"n\"\nname = \"{name}\"\nscript = {{ source = \"scripts/s.bl\" }}\n"
        ),
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/s.bl"), source).unwrap();
    dir
}

fn field(app: &App, node: &str, name: &str) -> Option<balaur_script::Value> {
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
    assert_eq!(host.set_breakpoints("scripts/s.bl", &[4]).unwrap(), [4]);
    assert_eq!(host.breakpoints("scripts/s.bl"), [4]);
    ticked(&mut app, 1);
    let stop = host.paused().expect("the breakpoint stopped the update");
    assert_eq!((stop.path.as_str(), stop.line), ("scripts/s.bl", 4));
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

    host.set_breakpoints("scripts/s.bl", &[]).unwrap();
    host.resume(StepMode::Continue);
    assert!(host.paused().is_none());
    assert!(!app.engine.is_frozen());
    assert_eq!(field(&app, "Counter", "count"), Some(Int(2)));
    ticked(&mut app, 1);
    assert_eq!(field(&app, "Counter", "count"), Some(Int(3)));
}

#[test]
fn balaur_check_names_the_line_a_bl_script_breaks_on() {
    let broken = COUNTER.replace(
        "let next = self.count + 1;",
        "let next = self.count + true;",
    );
    let dir = one_script("Counter", &broken);
    let found = balaur::check_project(dir.path()).unwrap();
    let bl: Vec<_> = found.iter().filter(|f| f.file == "scripts/s.bl").collect();
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

const MOVER_RN: &str = "pub fn init(this) {
    this.arm = this.node.get_node(\"Arm\");
    this.arm.set_name(\"Hand\");
    this.node.add_tag(\"mover\");
    this.seen = this.node.children().len();
    this.label = this.node.path();
}

pub fn update(this, dt) {
    this.node.transform.translate(dt, 0.0, 0.0);
    this.arm.transform.translate(0.0, dt, 0.0);
}
";

const MOVER_BL: &str = "pub struct Mover { node: Node, arm: Node, seen: int, label: str }

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
fn node_operations_do_in_bl_what_they_do_in_rune() {
    let rune = moved("rn", MOVER_RN);
    let lang = moved("bl", MOVER_BL);
    assert_ne!(rune.0, 0, "the Rune Mover never moved");
    assert_eq!(rune.2, ["mover"]);
    assert_eq!(lang, rune);
}
