//! A subtree tagged `local` is this machine's alone: no snapshot holds it, so
//! a restore neither frees it nor writes its scripts' fields.

use balaur::{App, AppConfig, DEFAULT_FIXED_DT, digest, standard_app};
use balaur_core::{scene, snapshot};
use balaur_script::Value;

const KEEPER: &str = "pub class Keeper {
    node: Node,
    pub signed_in: int,
}
impl Keeper {
    pub fn init() {
        self.node.add_tag(\"local\");
    }
    pub fn sign_in() {
        self.signed_in += 1;
    }
}
";

const MOVER: &str = "pub class Mover { node: Node, pub steps: int }
impl Mover {
    pub fn fixed_update(dt: float) { self.steps += 1; }
}
";

fn booted() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in [
        (
            "project.toml",
            "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
        ),
        (
            "main.toml",
            "[[nodes]]\nid = \"mover\"\nname = \"Mover\"\nscript = { source = \"scripts/mover.bs\" }\n",
        ),
        ("scripts/mover.bs", MOVER),
        ("scripts/keeper.bs", KEEPER),
    ] {
        let at = dir.path().join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

fn keeper(app: &App) -> balaur::hecs::Entity {
    let root = app.engine.root();
    let made = scene::spawn_node(&mut app.engine.world_mut(), "Keeper", root);
    app.engine
        .script_host()
        .unwrap()
        .attach_with_props(balaur::node_id_of(made), "scripts/keeper.bs", &[])
        .unwrap();
    made
}

fn field(app: &App, node: balaur::hecs::Entity, name: &str) -> Option<Value> {
    app.engine
        .script_host()
        .unwrap()
        .script_field(balaur::node_id_of(node), name)
}

fn sign_in(app: &App, node: balaur::hecs::Entity) {
    app.engine
        .script_host()
        .unwrap()
        .call_on(balaur::node_id_of(node), "sign_in", &[]);
}

#[test]
fn a_restore_leaves_a_local_node_made_after_the_snapshot() {
    let (_dir, mut app) = booted();
    let taken = snapshot::capture(&app.engine);
    let wanted = digest::digest(&app.engine);
    let kept = keeper(&app);
    sign_in(&app, kept);
    app.tick(DEFAULT_FIXED_DT);
    snapshot::restore(&app.engine, &taken);

    assert!(app.engine.world().contains(kept), "not freed as new");
    assert_eq!(field(&app, kept, "signed_in"), Some(Value::Int(1)));
    assert_eq!(
        digest::digest(&app.engine),
        wanted,
        "the shared world is back"
    );
}

#[test]
fn a_restore_leaves_a_local_nodes_fields_as_they_are_now() {
    let (_dir, mut app) = booted();
    let kept = keeper(&app);
    let mover = scene::find_node(&app.engine.world(), app.engine.root(), "Mover").unwrap();
    let taken = snapshot::capture(&app.engine);
    sign_in(&app, kept);
    for _ in 0..3 {
        app.tick(DEFAULT_FIXED_DT);
    }
    let steps = field(&app, mover, "steps");
    snapshot::restore(&app.engine, &taken);

    assert_ne!(
        field(&app, mover, "steps"),
        steps,
        "the control: the mover went back"
    );
    assert_eq!(
        field(&app, kept, "signed_in"),
        Some(Value::Int(1)),
        "the local script kept what it did since"
    );
}

const ARENA: &str = "pub class Arena {
    node: Node,
    view: Node,
}
impl Arena {
    pub fn init() {
        self.view = self.node.get_node(\"View\");
    }
    pub fn fixed_update(dt: float) {
        let marker = self.node.get_node(\"P0\");
        self.view.transform.position = marker.transform.position;
    }
    pub fn reaches() -> bool {
        self.view.is_valid() && self.node.get_node(\"P0\").is_valid()
    }
}
";

/// The scripted node, a sibling it looks up each step, and its local view.
fn arena() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let scene = "[[nodes]]\nid = \"n_arena\"\nname = \"Arena\"\nscript = { source = \"scripts/arena.bs\" }\n\n\
                 [[nodes]]\nid = \"n_p0\"\nname = \"P0\"\nparent = \"n_arena\"\n\n\
                 [[nodes]]\nid = \"n_view\"\nname = \"View\"\nparent = \"n_arena\"\ntags = [\"local\"]\n";
    for (path, text) in [
        (
            "project.toml",
            "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
        ),
        ("main.toml", scene),
        ("scripts/arena.bs", ARENA),
    ] {
        let at = dir.path().join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

#[test]
fn a_restore_leaves_a_shared_script_holding_its_local_view() {
    balaur_core::logbuf::capture_for_test();
    let (_dir, mut app) = arena();
    let arena_node = scene::find_node(&app.engine.world(), app.engine.root(), "Arena").unwrap();
    let view = scene::find_node(&app.engine.world(), app.engine.root(), "Arena/View").unwrap();
    app.tick(DEFAULT_FIXED_DT);
    let taken = snapshot::capture(&app.engine);
    for _ in 0..3 {
        app.tick(DEFAULT_FIXED_DT);
    }
    snapshot::restore(&app.engine, &taken);
    for _ in 0..3 {
        app.tick(DEFAULT_FIXED_DT);
    }

    assert_eq!(
        field(&app, arena_node, "view"),
        Some(Value::Node(balaur::node_id_of(view).0)),
        "the field still names the local view"
    );
    assert_eq!(
        app.engine
            .script_host()
            .unwrap()
            .call_on(balaur::node_id_of(arena_node), "reaches", &[]),
        Some(Value::Bool(true))
    );
    let errors: Vec<_> = balaur_core::logbuf::heard()
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}
