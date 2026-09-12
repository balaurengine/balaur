//! The flags that stop a body catching on a seam between two faces, and the
//! one-way platform that has to hold whichever side of a contact pair it is.

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use balaur_core::scene::{Transform, find_node};
use balaur_physics::rapier2d::parry::shape::PolylineFlags;
use balaur_physics::rapier2d::prelude::TriMeshFlags as TriMeshFlags2;
use balaur_physics::rapier3d::parry::shape::HeightFieldFlags;
use balaur_physics::{PhysicsState, PhysicsState2d};

/// The log buffer is global and tests run in parallel.
use crate::LOG;

/// A project of one scene and one script, stepped `frames` times.
fn run(scene: &str, script: &str, frames: u32) -> App {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    std::fs::write(dir.path().join("scripts/s.rn"), script).unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
    }
    app
}

const FLAT_MESH: &str = r#"[[assets]]
id = "floor"
type = "mesh"
positions = [[-2.0, 0.0], [2.0, 0.0], [2.0, -1.0], [-2.0, -1.0]]
indices = [[0, 1, 2], [0, 2, 3]]
"#;

fn heightfield_scene(extra: &str) -> String {
    format!(
        r##"[[assets]]
id = "ground"
type = "heightfield"
rows = 3
columns = 3
heights = [0, 0, 0, 0, 0, 0, 0, 0, 0]

[[nodes]]
id = "n_ground"
name = "Ground"

[nodes.collider3d]
kind = "heightfield"
heightfield = "#ground"
{extra}
"##
    )
}

fn mesh_scene_2d(kind: &str, extra: &str) -> String {
    format!(
        r##"{FLAT_MESH}
[[nodes]]
id = "n_floor"
name = "Floor"

[nodes.collider2d]
kind = "{kind}"
mesh = "#floor"
{extra}
"##
    )
}

/// The first collider's handle, for a scene that declares exactly one.
fn only_collider_3d(app: &App) -> balaur_physics::rapier3d::prelude::ColliderHandle {
    let state = app.engine.resource::<PhysicsState>();
    let state = state.borrow();
    let handles = state
        .colliders
        .values()
        .next()
        .expect("the scene declares a collider");
    *handles.first().expect("the collider reached rapier")
}

fn only_collider_2d(app: &App) -> balaur_physics::rapier2d::prelude::ColliderHandle {
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    let handles = state
        .colliders
        .values()
        .next()
        .expect("the scene declares a collider");
    *handles.first().expect("the collider reached rapier")
}

#[test]
fn a_heightfield_fixes_its_internal_edges_unless_it_is_told_not_to() {
    let flags = |scene: String| {
        let app = run(&scene, "", 1);
        let handle = only_collider_3d(&app);
        let state = app.engine.resource::<PhysicsState>();
        let state = state.borrow();
        state.world.colliders[handle]
            .shape()
            .as_heightfield()
            .expect("a heightfield shape")
            .flags()
    };
    assert!(
        flags(heightfield_scene("")).contains(HeightFieldFlags::FIX_INTERNAL_EDGES),
        "without the flag a body catches on the seam between two cells"
    );
    assert!(
        !flags(heightfield_scene("fix_internal_edges = false"))
            .contains(HeightFieldFlags::FIX_INTERNAL_EDGES),
        "the key has to be able to turn it off"
    );
}

#[test]
fn a_2d_trimesh_fixes_its_internal_edges_as_the_3d_one_does() {
    let app = run(&mesh_scene_2d("trimesh", ""), "", 1);
    let handle = only_collider_2d(&app);
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    let flags = state.world.colliders[handle]
        .shape()
        .as_trimesh()
        .expect("a trimesh shape")
        .flags();
    assert!(
        flags.contains(TriMeshFlags2::FIX_INTERNAL_EDGES),
        "a body would catch on the seam between the two triangles"
    );
}

#[test]
fn a_polyline_is_two_sided_until_it_is_asked_to_be_oriented() {
    let plain = run(&mesh_scene_2d("polyline", ""), "", 1);
    let handle = only_collider_2d(&plain);
    {
        let state = plain.engine.resource::<PhysicsState2d>();
        let state = state.borrow();
        let flags = state.world.colliders[handle]
            .shape()
            .as_polyline()
            .expect("a polyline shape")
            .flags();
        assert!(
            !flags.contains(PolylineFlags::ORIENTED),
            "the winding decides which side is solid, so this one is opt-in"
        );
    }
    let oriented = run(&mesh_scene_2d("polyline", "oriented = true"), "", 1);
    let handle = only_collider_2d(&oriented);
    let state = oriented.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    let flags = state.world.colliders[handle]
        .shape()
        .as_polyline()
        .expect("a polyline shape")
        .flags();
    assert!(flags.contains(PolylineFlags::ORIENTED));
}

/// Two pairs, declared in opposite orders, so one platform is `collider1` of
/// its contact pair and the other is `collider2`. Both have to let a body
/// through from below: rapier reads the axis in the first collider's frame,
/// and testing only that collider left half the platforms solid.
const ONE_WAY_SCENE: &str = r#"[[nodes]]
id = "n_level"
name = "Level"

[[nodes]]
id = "n_platform_a"
name = "PlatformA"
parent = "n_level"

[nodes.transform]
position = [0.0, 0.0, 0.0]

[nodes.collider2d]
kind = "rect"
half_extents = [4.0, 0.1]
one_way = true

[[nodes]]
id = "n_body_a"
name = "BodyA"
parent = "n_level"
script = "scripts/s.rn"

[nodes.transform]
position = [0.0, -2.0, 0.0]

[nodes.body2d]
kind = "dynamic"

[nodes.collider2d]
kind = "circle"
radius = 0.3

[[nodes]]
id = "n_body_b"
name = "BodyB"
parent = "n_level"
script = "scripts/s.rn"

[nodes.transform]
position = [10.0, -2.0, 0.0]

[nodes.body2d]
kind = "dynamic"

[nodes.collider2d]
kind = "circle"
radius = 0.3

[[nodes]]
id = "n_platform_b"
name = "PlatformB"
parent = "n_level"

[nodes.transform]
position = [10.0, 0.0, 0.0]

[nodes.collider2d]
kind = "rect"
half_extents = [4.0, 0.1]
one_way = true
"#;

const RISE: &str = r"pub fn init(this) {
    this.node.body2d.set_linear_velocity(0.0, 15.0);
}
";

#[test]
fn a_one_way_platform_lets_a_body_through_whichever_side_of_the_pair_it_is() {
    let app = run(ONE_WAY_SCENE, RISE, 20);
    let world = app.engine.world();
    let root = app.engine.root();
    let height = |name: &str| {
        let node = find_node(&world, root, &format!("Level/{name}"))
            .unwrap_or_else(|| panic!("no node named {name}"));
        world.get::<&Transform>(node).unwrap().position.y
    };
    assert!(
        height("BodyA") > 0.5,
        "the body under the first platform did not pass through it ({})",
        height("BodyA")
    );
    assert!(
        height("BodyB") > 0.5,
        "the body under the platform declared after it did not pass through ({})",
        height("BodyB")
    );
}
