//! `scene.switch`: the scene left is freed, the one asked for is built new,
//! and what sits beside it stays.

use balaur::hecs::Entity;
use balaur::{App, AppConfig, DEFAULT_FIXED_DT, digest, replay, standard_app};
use balaur_core::{scene, scene_switch, snapshot};
use balaur_script::Value;

const SCREEN: &str = "pub class Screen {
    node: Node,
    pub inits: int,
    pub updates: int,
}
impl Screen {
    pub fn init() {
        self.inits += 1;
        let keeper = scene::root().get_node(\"Keeper\");
        if keeper.is_valid() { keeper.call(\"saw_init\"); }
    }
    pub fn update(dt: float) {
        self.updates += 1;
    }
    pub fn on_free() {
        let keeper = scene::root().get_node(\"Keeper\");
        if keeper.is_valid() { keeper.call(\"saw_free\"); }
    }
    pub fn go_under(path: str) -> int {
        scene::switch(path, scene::SwitchOptions { under: self.node.get_node(\"Child\") });
        1
    }
    pub fn current_under() -> str {
        scene::current(self.node.get_node(\"Child\")).name()
    }
    pub fn current_at_root() -> str {
        scene::current(nil).name()
    }
    pub fn preload_it(path: str) -> int {
        scene::preload(path);
        1
    }
}
";

/// A node beside the scenes, under the tree's root: what a game's services
/// sit on.
const KEEPER: &str = "pub class Keeper {
    node: Node,
    pub inits: int,
    pub frees: int,
    pub updates: int,
}
impl Keeper {
    pub fn update(dt: float) {
        self.updates += 1;
    }
    pub fn saw_init() {
        self.inits += 1;
    }
    pub fn saw_free() {
        self.frees += 1;
    }
}
";

/// A scene of one script and a child, with the same ids in every file: what
/// two screens' files commonly hold.
fn screen(name: &str, extra: &str) -> String {
    format!(
        "[[nodes]]\nid = \"screen\"\nname = \"{name}\"\nscript = {{ source = \"scripts/screen.bs\" }}\n\n[nodes.transform]\nposition = [0, 0, 0]\n{extra}\n[[nodes]]\nid = \"child\"\nname = \"Child\"\nparent = \"screen\"\n\n[nodes.transform]\nposition = [1, 0, 0]\n"
    )
}

fn project(dir: &std::path::Path, b_extra: &str) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"a.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("a.toml"), screen("A", "")).unwrap();
    std::fs::write(dir.join("b.toml"), screen("B", b_extra)).unwrap();
    std::fs::write(dir.join("c.toml"), screen("C", "")).unwrap();
    std::fs::write(dir.join("scripts/screen.bs"), SCREEN).unwrap();
    std::fs::write(dir.join("scripts/keeper.bs"), KEEPER).unwrap();
}

fn booted(dir: &std::path::Path) -> App {
    let mut app = standard_app(AppConfig {
        watch: false,
        ..AppConfig::dev(dir.to_string_lossy().as_ref())
    })
    .unwrap();
    app.load_project().unwrap();
    app
}

fn app_of(b_extra: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), b_extra);
    let app = booted(dir.path());
    (dir, app)
}

/// The keeper, made under the tree's root once the main scene stands.
fn keeper(app: &App) -> Entity {
    let root = app.engine.root();
    let made = scene::spawn_node(&mut app.engine.world_mut(), "Keeper", root);
    app.engine
        .script_host()
        .unwrap()
        .attach_with_props(balaur::node_id_of(made), "scripts/keeper.bs", &[])
        .unwrap();
    made
}

fn tick(app: &mut App, times: usize) {
    for _ in 0..times {
        app.tick(DEFAULT_FIXED_DT);
    }
}

fn switch(app: &mut App, path: &str) {
    scene_switch::request(&app.engine, path);
    tick(app, 1);
}

fn live(app: &App, path: &str) -> Option<Entity> {
    scene::find_node(&app.engine.world(), app.engine.root(), path)
}

/// What the script on `node` holds in `name`.
fn field(app: &App, node: Entity, name: &str) -> i64 {
    let host = app.engine.script_host().expect("a script host");
    let id = balaur::node_id_of(node);
    let (_, state) = host
        .save_state()
        .into_iter()
        .find(|(at, _)| *at == id)
        .expect("the node keeps an instance");
    let Value::Map(fields) = state else {
        panic!("a script's state is a map");
    };
    fields
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, value)| match value {
            Value::Int(n) => Some(*n),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no field {name}"))
}

fn call(app: &App, node: Entity, method: &str, args: &[Value]) -> Option<Value> {
    let host = app.engine.script_host().unwrap();
    host.call_on(balaur::node_id_of(node), method, args)
}

fn alpha(app: &App, node: Entity) -> f32 {
    app.engine
        .world()
        .get::<&balaur_core::Appearance>(node)
        .unwrap()
        .tint
        .w
}

#[test]
fn a_switch_frees_the_scene_it_leaves_and_runs_its_on_free() {
    let (_dir, mut app) = app_of("");
    let a = live(&app, "A").expect("A boots");
    let keeper = keeper(&app);
    switch(&mut app, "b.toml");

    assert!(!app.engine.world().contains(a), "A is freed, not kept");
    assert!(live(&app, "B").is_some(), "B was built");
    assert_eq!(field(&app, keeper, "frees"), 1, "A's on_free ran");
    assert_eq!(
        app.engine.script_host().unwrap().instance_count(),
        2,
        "B's instance and the keeper's, nothing of A's"
    );
}

#[test]
fn a_switch_back_builds_the_scene_new_and_runs_its_init_again() {
    let (_dir, mut app) = app_of("");
    tick(&mut app, 3);
    let a = live(&app, "A").unwrap();
    let keeper = keeper(&app);
    switch(&mut app, "b.toml");
    switch(&mut app, "a.toml");

    let again = live(&app, "A").unwrap();
    assert_ne!(again, a, "a fresh build");
    assert_eq!(
        field(&app, again, "inits"),
        1,
        "a new object, its first init"
    );
    assert_eq!(
        field(&app, again, "updates"),
        0,
        "nothing carried from before"
    );
    assert_eq!(field(&app, keeper, "inits"), 2, "B's init, then A's");
    assert_eq!(field(&app, keeper, "frees"), 2, "A's on_free, then B's");
}

#[test]
fn a_switch_to_the_file_already_there_resets_it() {
    let (_dir, mut app) = app_of("");
    let a = live(&app, "A").unwrap();
    switch(&mut app, "a.toml");
    let again = live(&app, "A").unwrap();
    assert_ne!(again, a, "a fresh build");
    assert!(!app.engine.world().contains(a), "the old one is freed");
    assert_eq!(field(&app, again, "inits"), 1);
}

#[test]
fn a_node_beside_the_scene_stays_through_a_switch_and_keeps_running() {
    let (_dir, mut app) = app_of("");
    let keeper = keeper(&app);
    tick(&mut app, 2);
    let before = field(&app, keeper, "updates");
    assert!(before > 0, "the control: the keeper runs");
    switch(&mut app, "b.toml");
    tick(&mut app, 3);
    switch(&mut app, "c.toml");

    assert_eq!(live(&app, "Keeper"), Some(keeper), "the same node");
    assert!(
        field(&app, keeper, "updates") >= before + 5,
        "it ran every tick"
    );
    let current = scene_switch::current(&app.engine, None).unwrap();
    assert_eq!(current.path, "c.toml");
    assert_eq!(Some(current.root), live(&app, "C"));
}

#[test]
fn a_switch_under_a_node_replaces_only_the_scene_it_built_there() {
    let (_dir, mut app) = app_of("");
    let a = live(&app, "A").unwrap();
    let slot = live(&app, "A/Child").unwrap();
    let other = scene::spawn_node(&mut app.engine.world_mut(), "Other", slot);
    let under = |scene: &str| scene_switch::Request {
        scene: scene.to_string(),
        under: Some(slot),
        fade: 0.0,
    };
    scene_switch::ask(&app.engine, under("b.toml"));
    tick(&mut app, 1);
    let b = live(&app, "A/Child/B").expect("B is built under the slot");
    scene_switch::ask(&app.engine, under("c.toml"));
    tick(&mut app, 1);

    assert!(!app.engine.world().contains(b), "B is freed");
    assert!(live(&app, "A/Child/C").is_some(), "C stands in its place");
    assert_eq!(live(&app, "A/Child/Other"), Some(other), "a sibling stays");
    assert_eq!(live(&app, "A"), Some(a), "the scene above is untouched");
    let at_root = scene_switch::current(&app.engine, None).unwrap();
    assert_eq!((at_root.path.as_str(), at_root.root), ("a.toml", a));
}

#[test]
fn a_script_switches_under_a_node_and_reads_what_stands_there() {
    let (_dir, mut app) = app_of("");
    let a = live(&app, "A").unwrap();
    let asked = call(&app, a, "go_under", &[Value::Str("b.toml".into())]);
    assert_eq!(asked, Some(Value::Int(1)), "the control: the call ran");
    tick(&mut app, 1);

    assert!(live(&app, "A/Child/B").is_some());
    assert_eq!(
        call(&app, a, "current_under", &[]),
        Some(Value::Str("B".into()))
    );
    assert_eq!(
        call(&app, a, "current_at_root", &[]),
        Some(Value::Str("A".into()))
    );
}

#[test]
fn a_fade_tints_the_scene_it_switches_and_nothing_beside_it() {
    let (_dir, mut app) = app_of("");
    let a = live(&app, "A").unwrap();
    let keeper = keeper(&app);
    scene_switch::ask(
        &app.engine,
        scene_switch::Request {
            scene: "b.toml".to_string(),
            under: None,
            fade: 0.2,
        },
    );
    tick(&mut app, 3);
    assert!(alpha(&app, a) < 1.0, "A fades out");
    assert!(
        (alpha(&app, keeper) - 1.0).abs() < f32::EPSILON,
        "the keeper does not"
    );
    tick(&mut app, 30);
    let b = live(&app, "B").unwrap();
    assert!(!app.engine.world().contains(a));
    assert!((alpha(&app, b) - 1.0).abs() < f32::EPSILON, "B ends whole");
    assert!((alpha(&app, keeper) - 1.0).abs() < f32::EPSILON);
}

/// Go to B and back at fixed ticks: what a script's switch does on a tick.
fn recorded_run(dir: &std::path::Path) -> Vec<(serde_json::Map<String, serde_json::Value>, u64)> {
    let mut app = booted(dir);
    let mut frames = Vec::new();
    for at in 0..40 {
        match at {
            10 | 30 => scene_switch::request(&app.engine, "b.toml"),
            20 => scene_switch::request(&app.engine, "a.toml"),
            _ => {}
        }
        app.tick(DEFAULT_FIXED_DT);
        frames.push((replay::capture(&app.engine), digest::digest(&app.engine).0));
    }
    frames
}

#[test]
fn a_replay_of_a_session_that_switches_reproduces_every_tick_digest() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), "");
    let recorded = recorded_run(dir.path());
    let mut app = booted(dir.path());
    for (at, (sources, wanted)) in recorded.iter().enumerate() {
        match at {
            10 | 30 => scene_switch::request(&app.engine, "b.toml"),
            20 => scene_switch::request(&app.engine, "a.toml"),
            _ => {}
        }
        replay::restore(&app.engine, sources);
        app.tick(DEFAULT_FIXED_DT);
        assert_eq!(
            digest::digest(&app.engine).0,
            *wanted,
            "parted at tick {at}"
        );
    }
}

#[test]
fn a_restore_across_a_switch_brings_back_the_scene_it_left() {
    let (_dir, mut app) = app_of("");
    tick(&mut app, 2);
    let taken = snapshot::capture(&app.engine);
    let wanted = digest::digest(&app.engine);
    switch(&mut app, "b.toml");
    tick(&mut app, 2);
    snapshot::restore(&app.engine, &taken);

    let a = live(&app, "A").expect("A is back");
    assert!(live(&app, "B").is_none(), "B is gone");
    assert_eq!(digest::digest(&app.engine), wanted, "the very state");
    let current = scene_switch::current(&app.engine, None).unwrap();
    assert_eq!((current.path.as_str(), current.root), ("a.toml", a));
    switch(&mut app, "c.toml");
    assert!(!app.engine.world().contains(a), "the next switch frees it");
}

#[test]
fn preloading_builds_no_node_and_a_missing_file_ends_the_call() {
    let (_dir, mut app) = app_of("");
    let a = live(&app, "A").unwrap();
    let nodes = scene::collect_subtree(&app.engine.world(), app.engine.root()).len();
    assert_eq!(
        call(&app, a, "preload_it", &[Value::Str("b.toml".into())]),
        Some(Value::Int(1))
    );
    let after = scene::collect_subtree(&app.engine.world(), app.engine.root()).len();
    assert_eq!(after, nodes, "no node built");
    assert_eq!(
        call(&app, a, "preload_it", &[Value::Str("nope.toml".into())]),
        None
    );
    switch(&mut app, "b.toml");
    assert!(live(&app, "B").is_some(), "the switch builds it");
}

thread_local! {
    static WARMED: std::cell::RefCell<Vec<(&'static str, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn heard(doing: &'static str, value: &toml::Value) {
    let named = value["named"].as_str().unwrap_or_default().to_string();
    WARMED.with_borrow_mut(|heard| heard.push((doing, named)));
}

#[test]
fn a_pack_warms_the_scenes_it_ships_once_its_first_frame_is_out() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), "\n[nodes.warmed]\nnamed = \"shipped\"\n");
    let pack = balaur::build_pack(dir.path()).unwrap();
    let mut app = standard_app(AppConfig::packed(pack)).unwrap();
    balaur_core::preload::on_component(
        &app.engine,
        "warmed",
        balaur_core::preload::Warming {
            preload: |_, value| heard("preload", value),
            unload: |_, value| heard("unload", value),
        },
    );
    app.load_project().unwrap();
    assert_eq!(WARMED.with_borrow(Vec::len), 0, "nothing before a frame");
    tick(&mut app, 5);
    assert_eq!(
        WARMED.with_borrow(Clone::clone),
        [("preload", "shipped".to_string())]
    );
}

#[test]
fn unload_hands_what_preload_handed_to_the_same_plugin() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), "\n[nodes.warmed]\nnamed = \"mine\"\n");
    let app = booted(dir.path());
    balaur_core::preload::on_component(
        &app.engine,
        "warmed",
        balaur_core::preload::Warming {
            preload: |_, value| heard("preload", value),
            unload: |_, value| heard("unload", value),
        },
    );
    balaur_core::preload::scene(&app.engine, "b.toml").unwrap();
    balaur_core::preload::unload(&app.engine, "b.toml");
    balaur_core::preload::unload(&app.engine, "b.toml");
    let told = WARMED.with_borrow(Clone::clone);
    assert_eq!(
        told,
        [
            ("preload", "mine".to_string()),
            ("unload", "mine".to_string())
        ],
        "the second unload finds nothing cached to let go of"
    );
}
