//! `follow3d` and `follow2d` pull a body to a node's pose with rapier's PD and
//! PID controllers, and a snapshot keeps what a PID summed.

use balaur_core::App;
use balaur_core::hecs::Entity;
use balaur_core::scene::find_node;

fn boot(scene: &str) -> (App, tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
    let guard = crate::LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = balaur::standard_app(balaur::AppConfig::dev(
        dir.path().to_string_lossy().as_ref(),
    ))
    .unwrap();
    app.load_project().unwrap();
    (app, dir, guard)
}

fn node(app: &App, path: &str) -> Entity {
    find_node(&app.engine.world(), app.engine.root(), path).expect("the scene's node")
}

fn path(app: &mut App, body: Entity, steps: usize) -> Vec<[f32; 3]> {
    (0..steps)
        .map(|_| {
            app.tick(1.0 / 60.0);
            let world = app.engine.world();
            let t = world.get::<&balaur_core::Transform>(body).unwrap();
            [t.position.x, t.position.y, t.position.z]
        })
        .collect()
}

fn scene(dim: &str, follow: &str, gravity_scale: f32) -> String {
    let shape = if dim == "3d" { "sphere" } else { "circle" };
    format!(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_goal"
name = "Goal"
parent = "n_world"
transform = {{ position = [3.0, 1.0, 0.0] }}

[[nodes]]
id = "n_body"
name = "Body"
parent = "n_world"
body{dim} = {{ kind = "dynamic", gravity_scale = {gravity_scale:?} }}
collider{dim} = {{ kind = "{shape}" }}
follow{dim} = {{ target = "/World/Goal", {follow} }}
"#
    )
}

fn distance(at: [f32; 3]) -> f32 {
    ((at[0] - 3.0).powi(2) + (at[1] - 1.0).powi(2) + at[2].powi(2)).sqrt()
}

#[test]
fn a_followed_body_reaches_its_target_in_both_dimensions() {
    for dim in ["3d", "2d"] {
        let gains = if dim == "3d" {
            "[2.0, 2.0, 2.0]"
        } else {
            "[2.0, 2.0]"
        };
        let (mut app, _dir, _log) = boot(&scene(dim, &format!("position_gain = {gains}"), 0.0));
        let body = node(&app, "World/Body");
        let first = path(&mut app, body, 1)[0];
        let last = *path(&mut app, body, 300).last().unwrap();
        assert!(
            distance(first) > 0.5,
            "{dim}: the body started at the goal: {first:?}"
        );
        assert!(
            distance(last) < 0.05,
            "{dim}: the body stopped short: {last:?}"
        );
    }
}

#[test]
fn a_body_with_no_axes_named_stays_put() {
    let (mut app, _dir, _log) = boot(&scene("3d", "translation_axes = []", 0.0));
    let body = node(&app, "World/Body");
    let last = *path(&mut app, body, 60).last().unwrap();
    assert!(
        distance(last) > 3.0,
        "the body moved with no axes: {last:?}"
    );
}

#[test]
fn a_pid_holds_against_gravity_where_a_pd_sags() {
    let settle = |kind: &str| {
        let gains = "position_gain = [8.0, 8.0, 8.0], integral_gain = [20.0, 20.0, 20.0]";
        let (mut app, _dir, _log) =
            boot(&scene("3d", &format!(r#"kind = "{kind}", {gains}"#), 1.0));
        let body = node(&app, "World/Body");
        *path(&mut app, body, 600).last().unwrap()
    };
    let (pd, pid) = (settle("pd"), settle("pid"));
    assert!(1.0 - pd[1] > 0.01, "the pd held against gravity: {pd:?}");
    assert!(
        distance(pid) < 0.5 * distance(pd),
        "the pid sagged as far: {pid:?} vs {pd:?}"
    );
}

#[test]
fn a_restored_pid_steps_as_if_it_never_stopped() {
    for dim in ["3d", "2d"] {
        let (mut app, _dir, _log) = boot(&scene(dim, r#"kind = "pid""#, 1.0));
        let body = node(&app, "World/Body");
        path(&mut app, body, 30);
        let taken = balaur_core::snapshot::capture(&app.engine);
        let rows = taken.0[if dim == "3d" { "physics" } else { "physics2d" }]["follows"]
            .as_array()
            .map_or(0, Vec::len);
        assert_eq!(rows, 1, "{dim}: the frame holds no follow row");
        let straight = path(&mut app, body, 30);
        balaur_core::snapshot::restore(&app.engine, &taken);
        assert_eq!(
            straight,
            path(&mut app, body, 30),
            "{dim}: a restored pid stepped differently"
        );
    }
}
