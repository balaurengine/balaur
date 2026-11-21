//! `scene.repeat`: one instance of an item scene per key under a host, named
//! for its key, kept from call to call and hidden when no item asks for it.

use balaur::hecs::Entity;
use balaur::{App, AppConfig, standard_app};
use balaur_core::scene;
use balaur_script::Value;

const HOST: &str = "pub class Host {
    node: Node,
}
impl Host {
    pub fn fill(keys: [str], file: str) -> [str] {
        let items: [scene::RepeatItem] = [];
        for key in keys {
            items.push(scene::RepeatItem { key: key, scene: file });
        }
        let out: [str] = [];
        for row in scene::repeat(self.node, items) {
            out.push(row.name());
        }
        out
    }
}
";

/// Counts its own `init`, so a reused row is told from a new one.
const ROW: &str = "pub class Row {
    node: Node,
    pub inits: int,
}
impl Row {
    pub fn init() {
        self.inits += 1;
    }
}
";

const MAIN: &str = "[[nodes]]\nid = \"host\"\nname = \"Host\"\nscript = { source = \"scripts/host.bs\" }\n\n[[nodes]]\nid = \"head\"\nname = \"Head\"\nparent = \"host\"\n";

const ROW_SCENE: &str = "[[nodes]]\nid = \"row\"\nname = \"Row\"\nscript = { source = \"scripts/row.bs\" }\n\n[[nodes]]\nid = \"label\"\nname = \"Label\"\nparent = \"row\"\n";

const PAIR_SCENE: &str =
    "[[nodes]]\nid = \"a\"\nname = \"A\"\n\n[[nodes]]\nid = \"b\"\nname = \"B\"\n";

fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    std::fs::create_dir_all(at.join("scripts")).unwrap();
    std::fs::write(
        at.join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(at.join("main.toml"), MAIN).unwrap();
    std::fs::write(at.join("row.toml"), ROW_SCENE).unwrap();
    std::fs::write(at.join("other.toml"), ROW_SCENE).unwrap();
    std::fs::write(at.join("pair.toml"), PAIR_SCENE).unwrap();
    std::fs::write(at.join("scripts/host.bs"), HOST).unwrap();
    std::fs::write(at.join("scripts/row.bs"), ROW).unwrap();
    let mut app = standard_app(AppConfig {
        watch: false,
        ..AppConfig::dev(at.to_string_lossy().as_ref())
    })
    .unwrap();
    app.load_project().unwrap();
    (dir, app)
}

fn host(app: &App) -> Entity {
    scene::find_node(&app.engine.world(), app.engine.root(), "Host").unwrap()
}

fn fill(app: &App, keys: &[&str], file: &str) -> Option<Value> {
    let list = keys.iter().map(|k| Value::Str((*k).into())).collect();
    app.engine.script_host().unwrap().call_on(
        balaur::node_id_of(host(app)),
        "fill",
        &[Value::List(list), Value::Str(file.into())],
    )
}

fn keys(value: Option<Value>) -> Vec<String> {
    match value {
        Some(Value::List(items)) => items
            .into_iter()
            .map(|item| match item {
                Value::Str(s) => s,
                other => panic!("a key is a string, got {other:?}"),
            })
            .collect(),
        other => panic!("fill answers the keys, got {other:?}"),
    }
}

/// The host's children after `Head`, as `(name, visible)`.
fn rows(app: &App) -> Vec<(Entity, bool)> {
    let world = app.engine.world();
    let children = world.get::<&scene::Children>(host(app)).unwrap().0.clone();
    children
        .into_iter()
        .skip(1)
        .map(|child| {
            let visible = world
                .get::<&balaur_core::Appearance>(child)
                .is_ok_and(|a| a.visible);
            (child, visible)
        })
        .collect()
}

fn inits(app: &App, node: Entity) -> i64 {
    let id = balaur::node_id_of(node);
    let (_, state) = app
        .engine
        .script_host()
        .unwrap()
        .save_state()
        .into_iter()
        .find(|(at, _)| *at == id)
        .expect("the row keeps an instance");
    let Value::Map(fields) = state else {
        panic!("a script's state is a map");
    };
    match fields.iter().find(|(key, _)| key == "inits") {
        Some((_, Value::Int(n))) => *n,
        other => panic!("no inits, got {other:?}"),
    }
}

#[test]
fn repeat_builds_one_row_per_item_after_the_hosts_own_children() {
    let (_dir, app) = app();
    assert_eq!(
        keys(fill(&app, &["a", "b", "c"], "row.toml")),
        ["a", "b", "c"]
    );
    let rows = rows(&app);
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|(_, visible)| *visible));
    let world = app.engine.world();
    let head = world.get::<&scene::Children>(host(&app)).unwrap().0[0];
    assert_eq!(world.get::<&scene::Name>(head).unwrap().0.as_str(), "Head");
    for (row, _) in &rows {
        assert!(
            scene::find_node(&world, *row, "Label").is_some(),
            "each row is the whole item scene"
        );
    }
}

#[test]
fn each_row_and_its_children_carry_ids_of_their_own() {
    let (_dir, app) = app();
    fill(&app, &["a", "b"], "row.toml");
    let world = app.engine.world();
    let id = |node: Entity| {
        world
            .get::<&balaur_core::components::StableId>(node)
            .unwrap()
            .0
            .clone()
    };
    let rows = rows(&app);
    let labels: Vec<Entity> = rows
        .iter()
        .map(|(row, _)| scene::find_node(&world, *row, "Label").unwrap())
        .collect();
    assert_ne!(
        id(rows[0].0),
        id(rows[1].0),
        "a binding finds its own row by id"
    );
    assert_ne!(id(labels[0]), id(labels[1]));
}

#[test]
fn a_key_asked_again_keeps_its_node_and_an_order_change_moves_it() {
    let (_dir, app) = app();
    fill(&app, &["a", "b"], "row.toml");
    let first = rows(&app);
    assert_eq!(keys(fill(&app, &["b", "a"], "row.toml")), ["b", "a"]);
    let second = rows(&app);
    assert_eq!(second[0].0, first[1].0, "b kept its node and went first");
    assert_eq!(second[1].0, first[0].0);
    for (row, _) in &second {
        assert_eq!(inits(&app, *row), 1, "a kept row is not built again");
    }
}

#[test]
fn a_shorter_list_hides_the_spares_and_a_new_key_takes_one() {
    let (_dir, app) = app();
    fill(&app, &["a", "b", "c"], "row.toml");
    let built: Vec<Entity> = rows(&app).into_iter().map(|(row, _)| row).collect();
    fill(&app, &["a"], "row.toml");
    let shown: Vec<bool> = rows(&app).into_iter().map(|(_, v)| v).collect();
    assert_eq!(shown, [true, false, false], "spares are hidden, not freed");
    let world = app.engine.world();
    let names: Vec<String> = rows(&app)
        .into_iter()
        .map(|(row, _)| world.get::<&scene::Name>(row).unwrap().0.clone())
        .collect();
    assert_eq!(names, ["a", "spare0", "spare1"], "a spare gives up its key");
    drop(world);
    assert_eq!(keys(fill(&app, &["a", "z"], "row.toml")), ["a", "z"]);
    let after = rows(&app);
    assert_eq!(after.len(), 3, "no row was built for z");
    assert!(built.contains(&after[1].0));
    assert_eq!(inits(&app, after[1].0), 1);
    assert!(after[1].1, "z's row is shown again");
}

#[test]
fn an_item_of_another_scene_gets_a_node_of_its_own() {
    let (_dir, app) = app();
    fill(&app, &["a"], "row.toml");
    let first = rows(&app)[0].0;
    fill(&app, &["a"], "other.toml");
    let after = rows(&app);
    assert_eq!(after.len(), 2);
    assert_ne!(
        after[0].0, first,
        "a row of row.toml does not stand for other.toml"
    );
    assert!(!after[1].1, "the row.toml row is spare");
}

#[test]
fn a_scene_with_two_roots_or_two_items_sharing_a_key_end_the_call() {
    let (_dir, app) = app();
    fill(&app, &["a", "b"], "row.toml");
    let before = rows(&app);
    assert_eq!(fill(&app, &["a", "c"], "pair.toml"), None);
    assert_eq!(
        rows(&app),
        before,
        "a refused build leaves the host's rows as they were"
    );
    assert_eq!(fill(&app, &["a", "a"], "row.toml"), None);
    assert_eq!(fill(&app, &["a/b"], "row.toml"), None, "a key names a node");
    assert_eq!(
        keys(fill(&app, &["a"], "row.toml")),
        ["a"],
        "the host still repeats"
    );
    assert_eq!(rows(&app).len(), 2, "on the rows it had built");
}
