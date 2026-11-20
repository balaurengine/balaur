//! `node.transform.set_global_position` from a script, Godot's
//! `global_position = point`, under a parent that moved, turned and scaled.

use balaur::{App, AppConfig, standard_app};
use balaur_core::glamx::Vec3;
use balaur_core::scene::GlobalTransform;
use balaur_script::Value;

const POINT: Vec3 = Vec3::new(4.0, -1.5, 7.25);

/// Reads where it stands on its first frame and sends itself to `POINT`, then
/// reads where that put it.
const PLACER: &str = r"
pub class Placer { node: Node, frames: int, first: Vec3, read: Vec3 }
impl Placer {
    pub fn update(dt: float) {
        self.frames += 1;
        if self.frames == 1 {
            self.first = self.node.transform.global_position();
            self.node.transform.set_global_position(vec3(4.0, -1.5, 7.25));
        } else {
            self.read = self.node.transform.global_position();
        }
    }
}
";

const SCENE: &str = r#"
[[nodes]]
id = "parent"
name = "Parent"

[nodes.transform]
position = [3, -2, 5]
rotation_euler = [0.4, 1.1, -0.7]
scale = [2, 0.5, 1.5]

[[nodes]]
id = "child"
name = "Child"
parent = "parent"
script = { source = "scripts/placer.bs" }

[nodes.transform]
position = [1, 1, 1]
rotation_euler = [0.2, 0, 0.3]
"#;

fn booted(dir: &std::path::Path) -> App {
    for (path, text) in [
        (
            "project.toml",
            "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
        ),
        ("main.toml", SCENE),
        ("scripts/placer.bs", PLACER),
    ] {
        let at = dir.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    let mut config = AppConfig::dev(dir.to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    app
}

fn field(app: &App, name: &str) -> Value {
    let world = app.engine.world();
    let child = balaur_core::scene::find_node(&world, app.engine.root(), "Parent/Child")
        .expect("the scene has Parent/Child");
    app.engine
        .script_host()
        .unwrap()
        .script_field(balaur::node_id_of(child), name)
        .unwrap_or_else(|| panic!("the placer holds no `{name}`"))
}

fn vector(app: &App, name: &str) -> Vec3 {
    match field(app, name) {
        Value::Vec3(v) => Vec3::from_array(v),
        other => panic!("`{name}` is {other:?}, not a vector"),
    }
}

#[test]
fn a_script_sets_a_world_point_under_a_moved_turned_scaled_parent_and_reads_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = booted(dir.path());
    app.tick(1.0 / 60.0);
    app.tick(1.0 / 60.0);
    assert_eq!(
        field(&app, "frames"),
        Value::Int(2),
        "the placer ran both frames"
    );
    let first = vector(&app, "first");
    assert!(
        (first - POINT).length() > 1.0,
        "the child started away from the point, at {first:?}"
    );
    let read = vector(&app, "read");
    assert!(
        (read - POINT).length() < 1e-4,
        "set to {POINT:?}, it reads back {read:?}"
    );
}

#[test]
fn a_script_reads_the_composed_world_position_on_its_first_frame() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = booted(dir.path());
    app.tick(1.0 / 60.0);
    let parent = {
        let world = app.engine.world();
        let at = balaur_core::scene::find_node(&world, app.engine.root(), "Parent").unwrap();
        *world.get::<&GlobalTransform>(at).unwrap()
    };
    let composed = parent.position + parent.rotation * (Vec3::ONE * parent.scale);
    let first = vector(&app, "first");
    assert!(
        (first - composed).length() < 1e-4,
        "the first frame read {first:?}, where the scene puts the child at {composed:?}"
    );
}

/// Sends itself one unit further along x each frame, its rider one unit above.
const LEADER: &str = r"
pub class Leader { node: Node, frames: int }
impl Leader {
    pub fn update(dt: float) {
        self.frames += 1;
        self.node.transform.set_global_position(vec3(self.frames as float, 0.0, 0.0));
    }
}
";

/// Reads where the leader and its rider stand, after the leader wrote.
const WATCHER: &str = r#"
pub class Watcher { node: Node, leader_at: Vec3, rider_at: Vec3 }
impl Watcher {
    pub fn update(dt: float) {
        self.leader_at = self.node.get_node("../Leader").transform.global_position();
        self.rider_at = self.node.get_node("../Leader/Rider").transform.global_position();
    }
}
"#;

const FOLLOWING: &str = r#"
[[nodes]]
id = "world"
name = "World"

[[nodes]]
id = "leader"
name = "Leader"
parent = "world"
script = { source = "scripts/leader.bs" }

[nodes.transform]
position = [0, 0, 0]

[[nodes]]
id = "rider"
name = "Rider"
parent = "leader"

[nodes.transform]
position = [0, 1, 0]

[[nodes]]
id = "watcher"
name = "Watcher"
parent = "world"
script = { source = "scripts/watcher.bs" }
"#;

#[test]
fn a_world_point_one_script_writes_is_read_by_another_from_the_next_frame() {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in [
        (
            "project.toml",
            "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
        ),
        ("main.toml", FOLLOWING),
        ("scripts/leader.bs", LEADER),
        ("scripts/watcher.bs", WATCHER),
    ] {
        let at = dir.path().join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    let read = |app: &App, name: &str| {
        let world = app.engine.world();
        let watcher = balaur_core::scene::find_node(&world, app.engine.root(), "World/Watcher")
            .expect("the scene has World/Watcher");
        match app
            .engine
            .script_host()
            .unwrap()
            .script_field(balaur::node_id_of(watcher), name)
        {
            Some(Value::Vec3(v)) => Vec3::from_array(v),
            other => panic!("`{name}` is {other:?}"),
        }
    };
    for frame in 1..=5 {
        app.tick(1.0 / 60.0);
        // Within a frame a world position is the one the last sync placed.
        let synced = (frame - 1) as f32;
        let leader = read(&app, "leader_at");
        let rider = read(&app, "rider_at");
        assert_eq!(
            leader,
            Vec3::new(synced, 0.0, 0.0),
            "frame {frame}: the leader"
        );
        assert_eq!(
            rider,
            Vec3::new(synced, 1.0, 0.0),
            "frame {frame}: the rider"
        );
    }
}
