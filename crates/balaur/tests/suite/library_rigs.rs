//! The library's camera rigs as a game runs them: `scripts/follow.bs` trails a
//! target by the share of the gap its `lag` closes in a frame, and settles at
//! its offset.

use balaur::{App, AppConfig, standard_app};
use balaur_core::glamx::Vec3;
use balaur_core::scene::{GlobalTransform, Transform};

const FOLLOW: &str = include_str!("../../../../editor/library/scripts/follow.bs");
const ORBIT: &str = include_str!("../../../../editor/library/scripts/orbit_camera.bs");
const PLATFORMER: &str = include_str!("../../../../editor/library/scripts/platformer_rig.bs");
const THIRD_PERSON_RIG: &str = include_str!("../../../../editor/library/rigs/third_person.toml");
const ORBIT_RIG: &str = include_str!("../../../../editor/library/rigs/orbit_camera.toml");
const PLATFORMER_RIG: &str = include_str!("../../../../editor/library/rigs/platformer.toml");

const DT: f32 = 1.0 / 60.0;

/// The third person rig's `lag`, which the expected paths below are worked from.
const THIRD_PERSON_LAG: f32 = 0.2;

const PIVOT: &str = "Third person rig";

fn subject(name: &str, at: Vec3) -> String {
    format!(
        "[[nodes]]\nid = \"subject\"\nname = \"{name}\"\n\n[nodes.transform]\nposition = [{}, {}, {}]\n\n",
        at.x, at.y, at.z
    )
}

/// `scene` under one `World` root, which every node it leaves without a
/// parent joins, as a rig dropped into a level does.
fn rooted(scene: &str) -> String {
    let mut out = String::from("[[nodes]]\nid = \"world\"\nname = \"World\"\n");
    for block in scene.split("[[nodes]]").skip(1) {
        let own_keys = block.split("\n[").next().unwrap_or_default();
        out.push_str("[[nodes]]");
        if !own_keys.contains("\nparent = ") {
            out.push_str("\nparent = \"world\"");
        }
        out.push_str(block);
    }
    out
}

/// A project whose main scene is `scene` under a root, with every library
/// script a rig names.
fn booted(dir: &std::path::Path, scene: &str) -> App {
    let scene = rooted(scene);
    for (path, text) in [
        (
            "project.toml",
            "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
        ),
        ("main.toml", scene.as_str()),
        ("scripts/follow.bs", FOLLOW),
        ("scripts/orbit_camera.bs", ORBIT),
        ("scripts/platformer_rig.bs", PLATFORMER),
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

fn node(world: &balaur_core::hecs::World, app: &App, path: &str) -> balaur_core::hecs::Entity {
    balaur_core::scene::find_node(world, app.engine.root(), &format!("World/{path}"))
        .unwrap_or_else(|| panic!("the scene has no World/{path}"))
}

fn global(app: &App, path: &str) -> Vec3 {
    let world = app.engine.world();
    world
        .get::<&GlobalTransform>(node(&world, app, path))
        .unwrap()
        .position
}

/// Move a node in place, as a physics step moves a body, stamping the write.
fn put(app: &App, path: &str, at: Vec3) {
    let world = app.engine.world();
    let moved = node(&world, app, path);
    world.get::<&mut Transform>(moved).unwrap().position = at;
    balaur_core::transform::written(&app.engine, moved);
}

fn ticks(app: &mut App, n: usize) {
    for _ in 0..n {
        app.tick(DT);
    }
}

#[test]
fn the_third_person_pivot_closes_the_share_of_the_gap_its_lag_asks_each_frame() {
    let dir = tempfile::tempdir().unwrap();
    let goal = Vec3::new(6.0, 0.0, -4.0);
    let mut app = booted(dir.path(), &(subject("Player", goal) + THIRD_PERSON_RIG));
    for frame in 1..=5 {
        app.tick(DT);
        let share = 1.0 - balaur_core::libm::expf(-(frame as f32) * DT / THIRD_PERSON_LAG);
        let at = global(&app, PIVOT);
        assert!(
            (at - goal * share).length() < 1e-4,
            "frame {frame}: the pivot is at {at:?}, the lag puts it at {:?}",
            goal * share
        );
    }
}

#[test]
fn the_third_person_camera_trails_a_moving_player_and_settles_at_its_framing() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = booted(
        dir.path(),
        &(subject("Player", Vec3::ZERO) + THIRD_PERSON_RIG),
    );
    let mut player = Vec3::ZERO;
    for _ in 0..60 {
        player.x += 0.1;
        put(&app, "Player", player);
        app.tick(DT);
    }
    let pivot = global(&app, PIVOT);
    assert!(
        pivot.x > 1.0 && pivot.x < player.x - 0.5,
        "moving, the pivot trails the player at {player:?}: it is at {pivot:?}"
    );
    ticks(&mut app, 150);
    let pivot = global(&app, PIVOT);
    assert!(
        (pivot - player).length() < 1e-3,
        "stopped, the pivot settles on the player at {player:?}: it is at {pivot:?}"
    );
    let camera = global(&app, &format!("{PIVOT}/Camera"));
    let framing = Vec3::new(0.0, 3.5, 8.0);
    assert!(
        (camera - (player + framing)).length() < 1e-3,
        "the camera keeps the rig's framing: {camera:?}"
    );
}

#[test]
fn a_follow_takes_the_same_path_at_any_frame_rate() {
    let scene = subject("Player", Vec3::new(10.0, 0.0, 0.0)) + THIRD_PERSON_RIG;
    let (one_dir, many_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut one = booted(one_dir.path(), &scene);
    one.tick(4.0 * DT);
    let mut many = booted(many_dir.path(), &scene);
    ticks(&mut many, 4);
    let (one, many) = (global(&one, PIVOT).x, global(&many, PIVOT).x);
    assert!(one > 0.0, "the pivot moved at all, to {one}");
    assert!(
        (one - many).abs() < 1e-4,
        "one long frame landed at {one} and four short ones at {many}"
    );
}

#[test]
fn a_follow_with_no_lag_lands_on_its_target_and_its_offset() {
    let dir = tempfile::tempdir().unwrap();
    let scene = subject("Target", Vec3::new(3.0, 1.0, 0.0))
        + "[[nodes]]\nid = \"follower\"\nname = \"Follower\"\n\n[nodes.script]\nsource = \"scripts/follow.bs\"\n\n[nodes.script.props]\ntarget = \"../Target\"\noffset = [0, 2, 0]\n";
    let mut app = booted(dir.path(), &scene);
    app.tick(DT);
    let at = global(&app, "Follower");
    assert!(
        (at - Vec3::new(3.0, 3.0, 0.0)).length() < 1e-4,
        "the follower landed at {at:?}"
    );
}

#[test]
fn the_orbit_rig_pivot_settles_on_its_subject_with_the_camera_around_it() {
    let dir = tempfile::tempdir().unwrap();
    let watched = Vec3::new(2.0, 1.0, -3.0);
    let mut app = booted(dir.path(), &(subject("Subject", watched) + ORBIT_RIG));
    ticks(&mut app, 120);
    let pivot = global(&app, "Orbit rig");
    assert!(
        (pivot - watched).length() < 1e-3,
        "the pivot settled at {pivot:?}"
    );
    let range = (global(&app, "Orbit rig/Camera") - pivot).length();
    assert!(
        (range - 8.0).abs() < 1e-3,
        "the camera orbits at the script's distance, got {range}"
    );
}

#[test]
fn the_platformer_camera_trails_the_falling_player_and_settles_above_it() {
    let dir = tempfile::tempdir().unwrap();
    let ground = "[[nodes]]\nid = \"ground\"\nname = \"Ground\"\nbody2d = { kind = \"static\" }\n\n[nodes.transform]\nposition = [0, -3, 0]\n\n[nodes.collider2d]\nshape = { kind = \"rectangle\", size = [24, 1.0] }\n\n";
    let mut app = booted(dir.path(), &(ground.to_string() + PLATFORMER_RIG));
    let (player, camera) = ("Platformer rig/Player", "Platformer rig/Camera");
    ticks(&mut app, 15);
    let (falling, above) = (global(&app, player), global(&app, camera));
    assert!(
        falling.y < -0.1 && above.y > falling.y + 1.05,
        "falling, the camera trails above the player at {falling:?}: it is at {above:?}"
    );
    assert!(
        (above.z - 10.0).abs() < 1e-5,
        "the camera keeps its depth: {above:?}"
    );
    ticks(&mut app, 225);
    let (landed, settled) = (global(&app, player), global(&app, camera));
    assert!(
        (settled - (landed + Vec3::new(0.0, 1.0, 10.0))).length() < 1e-3,
        "landed at {landed:?}, the camera settles one above it at its depth: {settled:?}"
    );
}
