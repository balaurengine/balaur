//! `scene.switch`: the scene left stays built and parked, and what stops, what
//! stays and what comes back.

use std::fmt::Write as _;

use balaur::hecs::Entity;
use balaur::{App, AppConfig, DEFAULT_FIXED_DT, digest, replay, standard_app};
use balaur_core::{parked, scene, scene_switch, snapshot};
use balaur_script::Value;

const SCREEN: &str = "pub class Screen {
    node: Node,
    pub saw_free: int,
    pub listed: int,
    pub inits: int,
    pub updates: int,
    pub fixed: int,
    pub parks: int,
    pub returns: int,
    pub keys: int,
    pub clicks: int,
    pub after_await: int,
}
impl Screen {
    async fn init() {
        self.inits = self.inits + 1;
        await task::frames(5);
        self.after_await = self.after_await + 1;
    }
    fn update(dt: float) {
        self.updates = self.updates + 1;
        if engine::stored(\"freed\") != nil { self.saw_free = 1; }
    }
    fn fixed_update(dt: float) {
        self.fixed = self.fixed + 1;
    }
    fn on_parked_changed(parked: bool) {
        if parked { self.parks = self.parks + 1; } else { self.returns = self.returns + 1; }
    }
    fn on_key_down(key: str) -> bool {
        self.keys = self.keys + 1;
        return false;
    }
    fn on_pointer_click(button: str) {
        self.clicks = self.clicks + 1;
    }
    fn on_free() {
        engine::store(\"freed\", true);
    }
    pub fn count_parked() {
        self.listed = scene::parked().len();
    }
    pub fn unload_it(path: str) {
        scene::unload(path);
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

fn project(dir: &std::path::Path, a_extra: &str, b_extra: &str) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"a.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("a.toml"), screen("A", a_extra)).unwrap();
    std::fs::write(dir.join("b.toml"), screen("B", b_extra)).unwrap();
    std::fs::write(dir.join("scripts/screen.bs"), SCREEN).unwrap();
}

fn booted(dir: &std::path::Path) -> App {
    let mut app = standard_app(AppConfig::dev(dir.to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app
}

fn app_of(a_extra: &str, b_extra: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), a_extra, b_extra);
    let app = booted(dir.path());
    (dir, app)
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

fn live(app: &App, name: &str) -> Option<Entity> {
    scene::find_node(&app.engine.world(), app.engine.root(), name)
}

/// A scene's root, wherever it is now.
fn anywhere(app: &App, name: &str) -> Entity {
    let world = app.engine.world();
    parked::every_node(&app.engine, &world)
        .into_iter()
        .find(|&at| world.get::<&scene::Name>(at).is_ok_and(|n| n.0 == name))
        .unwrap_or_else(|| panic!("no node named {name}"))
}

/// What the script on `node` holds in `name`, parked or not.
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

#[test]
fn a_kept_scene_leaves_the_tree_and_stops() {
    let (_dir, mut app) = app_of("", "");
    tick(&mut app, 3);
    let a = live(&app, "A").expect("A boots");
    switch(&mut app, "b.toml");

    assert!(live(&app, "A").is_none(), "A left the tree");
    assert!(live(&app, "B").is_some(), "B was built");
    assert_eq!(parked::scenes(&app.engine), ["a.toml"]);
    assert!(parked::is_parked(&app.engine, a));
    assert!(!balaur_core::process::ticking(&app.engine, a));
    {
        let world = app.engine.world();
        let child = scene::find_node(&world, a, "Child").unwrap();
        let shown = world.get::<&balaur_core::GlobalAppearance>(child).unwrap();
        assert!(
            !shown.visible,
            "whatever draws reads a parked node as hidden"
        );
    }

    let host = app.engine.script_host().unwrap();
    assert_eq!(host.instance_count(), 1, "only B's instance runs");
    let (updates, fixed) = (field(&app, a, "updates"), field(&app, a, "fixed"));
    assert!(updates > 0, "the control: A ran before it was parked");
    tick(&mut app, 10);
    assert_eq!(field(&app, a, "updates"), updates, "update stopped");
    assert_eq!(field(&app, a, "fixed"), fixed, "fixed_update stopped");
    let b = live(&app, "B").unwrap();
    assert!(field(&app, b, "updates") >= 10, "the control: B runs");
}

#[test]
fn a_return_runs_no_init_and_keeps_the_nodes_and_their_state() {
    let (_dir, mut app) = app_of("", "");
    tick(&mut app, 3);
    let a = live(&app, "A").unwrap();
    let updates = field(&app, a, "updates");
    switch(&mut app, "b.toml");
    tick(&mut app, 5);
    let b = live(&app, "B").unwrap();
    switch(&mut app, "a.toml");

    assert_eq!(live(&app, "A"), Some(a), "the same node came back");
    assert!(live(&app, "B").is_none());
    assert_eq!(parked::scenes(&app.engine), ["b.toml"]);
    assert!(parked::is_parked(&app.engine, b));
    assert_eq!(field(&app, a, "inits"), 1, "init ran once, at the build");
    assert_eq!(field(&app, a, "parks"), 1);
    assert_eq!(field(&app, a, "returns"), 1);
    assert_eq!(field(&app, b, "parks"), 1);
    tick(&mut app, 4);
    assert!(field(&app, a, "updates") > updates, "A runs again");
    assert!(!parked::is_parked(&app.engine, a));
    let world = app.engine.world();
    let child = scene::find_node(&world, a, "Child").unwrap();
    assert!(
        world
            .get::<&balaur_core::GlobalAppearance>(child)
            .unwrap()
            .visible
    );
    let at = world.get::<&balaur_core::GlobalTransform>(child).unwrap();
    assert!(
        (at.position.x - 1.0).abs() < f32::EPSILON,
        "its pose stands"
    );
}

#[test]
fn a_switch_to_the_file_already_there_resets_it() {
    let (_dir, mut app) = app_of("", "");
    let a = live(&app, "A").unwrap();
    switch(&mut app, "a.toml");
    let again = live(&app, "A").unwrap();
    assert_ne!(again, a, "a fresh build");
    assert!(!app.engine.world().contains(a), "the old one is freed");
    assert!(parked::scenes(&app.engine).is_empty());
    assert_eq!(field(&app, again, "inits"), 1);
    assert_eq!(field(&app, again, "parks"), 0);
}

#[test]
fn unloading_a_parked_file_makes_the_next_switch_build_it_again() {
    let (_dir, mut app) = app_of("", "");
    let a = live(&app, "A").unwrap();
    switch(&mut app, "b.toml");
    balaur_core::preload::unload(&app.engine, "a.toml");
    assert!(parked::scenes(&app.engine).is_empty());
    switch(&mut app, "a.toml");

    let again = live(&app, "A").unwrap();
    assert_ne!(again, a, "a fresh build, not the kept one");
    assert!(!app.engine.world().contains(a), "the kept copy is freed");
    assert_eq!(field(&app, again, "inits"), 1);
    assert!(live(&app, "B").is_none());
    assert_eq!(parked::scenes(&app.engine), ["b.toml"]);
}

#[test]
fn a_file_edited_while_parked_is_built_again_on_the_way_back() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), "", "");
    let mut app = booted(dir.path());
    let a = live(&app, "A").unwrap();
    switch(&mut app, "b.toml");
    std::fs::write(dir.path().join("a.toml"), screen("A2", "")).unwrap();
    let file = std::fs::File::options()
        .write(true)
        .open(dir.path().join("a.toml"))
        .unwrap();
    // A stamp of its own: two writes inside one clock tick would read as none.
    let edited = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000);
    file.set_modified(edited).unwrap();
    switch(&mut app, "a.toml");

    assert!(!app.engine.world().contains(a), "the kept copy is freed");
    assert!(live(&app, "A2").is_some(), "the edit shows");
}

#[test]
fn unloading_a_parked_scene_frees_its_nodes_and_runs_on_free() {
    let (_dir, mut app) = app_of("", "");
    let a = live(&app, "A").unwrap();
    let child = scene::find_node(&app.engine.world(), a, "Child").unwrap();
    switch(&mut app, "b.toml");
    balaur_core::preload::unload(&app.engine, "a.toml");

    assert!(parked::scenes(&app.engine).is_empty());
    assert!(!app.engine.world().contains(a));
    assert!(!app.engine.world().contains(child));
    balaur_core::preload::unload(&app.engine, "a.toml");
    // The holder went with it: only the tree's own nodes are left.
    let held = app.engine.world().query::<&parked::Held>().iter().count();
    assert_eq!(held, 0);
    let host = app.engine.script_host().unwrap();
    assert_eq!(host.instance_count(), 1);
    tick(&mut app, 1);
    let b = live(&app, "B").unwrap();
    assert_eq!(field(&app, b, "saw_free"), 1, "on_free ran");
}

#[test]
fn the_script_api_lists_and_unloads_parked_scenes() {
    let (_dir, mut app) = app_of("", "");
    switch(&mut app, "b.toml");
    switch(&mut app, "a.toml");
    let a = live(&app, "A").unwrap();
    let host = app.engine.script_host().unwrap();
    let id = balaur::node_id_of(a);
    host.call_on(id, "count_parked", &[]);
    assert_eq!(field(&app, a, "listed"), 1);
    host.call_on(id, "unload_it", &[Value::text("b.toml")]);
    assert!(parked::scenes(&app.engine).is_empty());
    host.call_on(id, "count_parked", &[]);
    assert_eq!(field(&app, a, "listed"), 0);
    assert!(live(&app, "A").is_some(), "what runs stays");
}

#[test]
fn a_parked_script_hears_no_input_hook_or_event() {
    let (_dir, mut app) = app_of("", "");
    let a = live(&app, "A").unwrap();
    let host = app.engine.script_host().unwrap();
    host.call_all_with("on_key_down", &[Value::text("x")]);
    assert_eq!(
        field(&app, a, "keys"),
        1,
        "the control: a live script hears"
    );

    switch(&mut app, "b.toml");
    host.call_all_with("on_key_down", &[Value::text("x")]);
    assert_eq!(field(&app, a, "keys"), 1, "nor every script's hook");
    balaur_core::events::announce(&app.engine, a, "pointer_click", Value::text("left"));
    assert_eq!(field(&app, a, "clicks"), 0, "nor an event from its node");
    assert_eq!(
        host.call_on(balaur::node_id_of(a), "update", &[Value::Num(0.0)]),
        None,
        "nor a call",
    );

    switch(&mut app, "a.toml");
    balaur_core::events::announce(&app.engine, a, "pointer_click", Value::text("left"));
    assert_eq!(field(&app, a, "clicks"), 1, "back, it hears again");
}

#[test]
fn a_task_a_parked_script_awaits_holds_until_it_returns() {
    let (_dir, mut app) = app_of("", "");
    let a = live(&app, "A").unwrap();
    switch(&mut app, "b.toml");
    tick(&mut app, 20);
    assert_eq!(field(&app, a, "after_await"), 0, "no line of it ran");
    let b = live(&app, "B").unwrap();
    assert_eq!(
        field(&app, b, "after_await"),
        1,
        "the control: B's finished"
    );

    switch(&mut app, "a.toml");
    tick(&mut app, 3);
    assert_eq!(
        field(&app, a, "after_await"),
        1,
        "it goes on from the await"
    );
}

#[test]
fn a_parked_timer_holds_its_count() {
    let timer = "\n[nodes.timer]\nwait_time = 100.0\nautostart = true\n";
    let (_dir, mut app) = app_of(timer, "");
    tick(&mut app, 2);
    let a = live(&app, "A").unwrap();
    let left = |app: &App| {
        let held = balaur_core::components::get(&app.engine, a, "timer").unwrap();
        balaur_core::components::prop_f32(&held, "time_left")
    };
    assert!(left(&app) < 100.0, "the control: it counts");
    switch(&mut app, "b.toml");
    let held = left(&app);
    tick(&mut app, 30);
    assert!((left(&app) - held).abs() < f32::EPSILON, "it holds");
    switch(&mut app, "a.toml");
    tick(&mut app, 3);
    assert!(left(&app) < held, "it goes on");
}

#[test]
fn a_parked_scene_takes_no_new_node_or_script() {
    let (_dir, mut app) = app_of("", "");
    let a = live(&app, "A").unwrap();
    assert!(parked::refuse_if_parked(&app.engine, a, "spawning").is_ok());
    switch(&mut app, "b.toml");
    let refused = parked::refuse_if_parked(&app.engine, a, "spawning").unwrap_err();
    assert!(refused.to_string().contains("parked"), "{refused}");
}

#[test]
fn a_fade_returns_a_scene_seen_whole() {
    let (_dir, mut app) = app_of("", "");
    let a = live(&app, "A").unwrap();
    let alpha = |app: &App, node: Entity| {
        app.engine
            .world()
            .get::<&balaur_core::Appearance>(node)
            .unwrap()
            .tint
            .w
    };
    scene_switch::request_with_fade(&app.engine, "b.toml", 0.1);
    tick(&mut app, 20);
    assert!(parked::is_parked(&app.engine, a));
    assert!(
        (alpha(&app, a) - 1.0).abs() < f32::EPSILON,
        "not parked faded out"
    );
    switch(&mut app, "a.toml");
    assert!(
        (alpha(&app, a) - 1.0).abs() < f32::EPSILON,
        "a cut shows it whole"
    );
}

#[test]
fn a_digest_covers_a_parked_scene_under_its_path() {
    let (_dir, mut app) = app_of("", "");
    let a = live(&app, "A").unwrap();
    switch(&mut app, "b.toml");
    let entries = digest::entries(&app.engine);
    let label = |wanted: &str| entries.iter().any(|entry| entry.label == wanted);
    assert!(label("a.toml|screen/transform"), "the parked scene's root");
    assert!(label("screen/transform"), "the control: B's root, same id");

    let before = digest::digest(&app.engine);
    app.engine
        .world()
        .get::<&mut balaur_core::Transform>(a)
        .unwrap()
        .position
        .y = 3.0;
    assert_ne!(digest::digest(&app.engine), before, "it is world state");
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
fn a_replay_of_a_session_that_parks_reproduces_every_tick_digest() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), "", "");
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
fn a_snapshot_keeps_a_parked_scene_and_puts_it_back() {
    let (_dir, mut app) = app_of("", "");
    tick(&mut app, 2);
    let a = live(&app, "A").unwrap();
    switch(&mut app, "b.toml");
    tick(&mut app, 2);
    let taken = snapshot::capture(&app.engine);
    let parked_updates = field(&app, a, "updates");
    let wanted = digest::digest(&app.engine);

    // A write to the parked node, then the same state: it comes back parked.
    app.engine
        .world()
        .get::<&mut balaur_core::Transform>(a)
        .unwrap()
        .position
        .y = 9.0;
    snapshot::restore(&app.engine, &taken);
    assert_eq!(parked::scenes(&app.engine), ["a.toml"]);
    assert_eq!(digest::digest(&app.engine), wanted, "the very state");
    assert_eq!(field(&app, a, "updates"), parked_updates);
    tick(&mut app, 3);
    assert_eq!(
        field(&app, a, "updates"),
        parked_updates,
        "and it stays held"
    );
}

#[test]
fn a_restore_across_a_return_parks_the_scene_again() {
    let (_dir, mut app) = app_of("", "");
    tick(&mut app, 2);
    switch(&mut app, "b.toml");
    tick(&mut app, 2);
    let taken = snapshot::capture(&app.engine);
    let wanted = digest::digest(&app.engine);

    switch(&mut app, "a.toml");
    tick(&mut app, 4);
    assert_eq!(parked::scenes(&app.engine), ["b.toml"]);
    snapshot::restore(&app.engine, &taken);

    assert_eq!(parked::scenes(&app.engine), ["a.toml"]);
    assert_eq!(parked::current(&app.engine).as_deref(), Some("b.toml"));
    assert!(live(&app, "B").is_some());
    assert!(live(&app, "A").is_none());
    assert_eq!(digest::digest(&app.engine), wanted, "the very state");
    let a = anywhere(&app, "A");
    assert!(parked::is_parked(&app.engine, a));
    let updates = field(&app, a, "updates");
    tick(&mut app, 3);
    assert_eq!(
        field(&app, a, "updates"),
        updates,
        "its script is held again"
    );
    let b = live(&app, "B").unwrap();
    assert!(field(&app, b, "updates") > 0);
}

/// A scene of `nodes` nodes under one script's root.
fn big_scene(name: &str, nodes: usize) -> String {
    let mut text = screen(name, "");
    for at in 0..nodes - 2 {
        writeln!(
            text,
            "\n[[nodes]]\nid = \"n{at}\"\nname = \"N{at}\"\nparent = \"screen\""
        )
        .unwrap();
    }
    text
}

#[test]
fn past_the_budget_the_scene_left_longest_ago_is_freed() {
    let half = parked::MAX_PARKED_NODES / 2 + 1;
    let (dir, mut app) = app_of("", "");
    for name in ["c", "d", "e"] {
        let text = big_scene(&name.to_uppercase(), half);
        std::fs::write(dir.path().join(format!("{name}.toml")), text).unwrap();
    }
    let a = live(&app, "A").unwrap();
    switch(&mut app, "c.toml");
    switch(&mut app, "d.toml");
    assert_eq!(parked::scenes(&app.engine), ["a.toml", "c.toml"]);
    switch(&mut app, "e.toml");
    assert_eq!(
        parked::scenes(&app.engine),
        ["d.toml"],
        "the oldest go until it fits"
    );
    assert!(!app.engine.world().contains(a));
    switch(&mut app, "a.toml");
    assert_eq!(
        field(&app, live(&app, "A").unwrap(), "inits"),
        1,
        "a scene freed is built again"
    );
}

#[test]
fn a_scene_past_the_budget_alone_is_freed_as_it_is_left() {
    let (dir, mut app) = app_of("", "");
    let text = big_scene("C", parked::MAX_PARKED_NODES + 1);
    std::fs::write(dir.path().join("c.toml"), text).unwrap();
    switch(&mut app, "c.toml");
    let c = live(&app, "C").unwrap();
    switch(&mut app, "a.toml");
    assert!(!app.engine.world().contains(c));
    assert!(parked::scenes(&app.engine).is_empty());
}

#[test]
fn a_memory_warning_frees_every_parked_scene_and_not_the_one_running() {
    let (_dir, mut app) = app_of("", "");
    let a = live(&app, "A").unwrap();
    switch(&mut app, "b.toml");
    assert!(app.engine.world().contains(a), "the control: A is parked");
    balaur_core::facts::update_device(&app.engine, |facts| facts.memory_warnings += 1);
    tick(&mut app, 2);
    assert!(parked::scenes(&app.engine).is_empty());
    assert!(!app.engine.world().contains(a));
    assert!(live(&app, "B").is_some());
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
    project(dir.path(), "", "\n[nodes.warmed]\nnamed = \"shipped\"\n");
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
    let warmed = "\n[nodes.warmed]\nnamed = \"mine\"\n";
    project(dir.path(), "", warmed);
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

const BODY: &str = "\n[nodes.body3d]\nkind = \"dynamic\"\n\n[nodes.collider3d]\nshape = { kind = \"sphere\", radius = 0.5 }\n";

#[test]
fn a_collider_authored_off_stays_off_when_its_scene_returns() {
    let off = "\n[nodes.body3d]\nkind = \"static\"\n\n[nodes.collider3d]\nshape = { kind = \"sphere\", radius = 0.5 }\nenabled = false\n";
    let (_dir, mut app) = app_of(off, "");
    let a = live(&app, "A").unwrap();
    let enabled = |app: &App| {
        let state = app.engine.resource::<balaur::physics::PhysicsState3d>();
        let state = state.borrow();
        state.colliders[&a]
            .iter()
            .all(|&handle| state.world.colliders[handle].is_enabled())
    };
    assert!(!enabled(&app), "the control: authored off");
    switch(&mut app, "b.toml");
    switch(&mut app, "a.toml");
    assert!(!enabled(&app), "a return gives back what it asks for");
}

#[test]
fn a_parked_body_holds_still_and_goes_on_falling_when_it_returns() {
    let (_dir, mut app) = app_of(BODY, "");
    tick(&mut app, 5);
    let a = live(&app, "A").unwrap();
    let height = |app: &App| {
        app.engine
            .world()
            .get::<&balaur_core::Transform>(a)
            .unwrap()
            .position
            .y
    };
    assert!(height(&app) < 0.0, "the control: it falls");
    switch(&mut app, "b.toml");
    let held = height(&app);
    tick(&mut app, 20);
    assert!((height(&app) - held).abs() < f32::EPSILON, "it holds");
    switch(&mut app, "a.toml");
    tick(&mut app, 5);
    assert!(height(&app) < held, "it goes on");
}
