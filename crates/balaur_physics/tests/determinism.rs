//! Determinism is a core engine feature: identical inputs must produce
//! bit-for-bit identical simulations. This test guards the same-platform half.
//!
//! The cross-platform half is `scripts/determinism_trace.sh`, which writes the
//! same per-tick digest these tests compare and hands it to CI as an artifact
//! to diff across the matrix. What is still unproven is aarch64 against
//! x86_64, where FMA contraction is the thing to expect.

use balaur_core::digest::{self, Digest};
use balaur_core::{App, AppConfig, Transform};
use balaur_physics::PhysicsPlugin;

fn write_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("scenes")).unwrap();
    std::fs::write(
        root.join("project.toml"),
        "name = \"det\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("scenes/main.toml"),
        r#"
[[nodes]]
id = "n_ground"
name = "Ground"
position = [0.0, -1.0, 0.0]
body = "static"
collider = { kind = "cuboid", half_extents = [10.0, 0.5, 10.0] }

[[nodes]]
id = "n_balla"
name = "BallA"
position = [0.1, 5.0, 0.0]
body = "dynamic"
collider = { kind = "ball", radius = 0.5 }

[[nodes]]
id = "n_ballb"
name = "BallB"
position = [-0.1, 7.0, 0.05]
body = "dynamic"
collider = { kind = "ball", radius = 0.5 }
"#,
    )
    .unwrap();
}

fn boot(root: &std::path::Path) -> App {
    let mut app = App::new(AppConfig {
        project_root: root.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    app.add_plugin(PhysicsPlugin).unwrap();
    app.load_project().unwrap();
    app
}

/// One digest per tick. Comparing the whole chain rather than the end state
/// is what names *when* two runs parted, not just that they did.
fn trace(root: &std::path::Path, frames: u32) -> Vec<Digest> {
    let mut app = boot(root);
    (0..frames)
        .map(|_| {
            app.tick(balaur_core::FIXED_DT);
            digest::digest(&app.engine)
        })
        .collect()
}

fn simulate(root: &std::path::Path, frames: u32) -> Vec<[u32; 3]> {
    let mut app = boot(root);
    for _ in 0..frames {
        app.tick(balaur_core::FIXED_DT);
    }
    // Collect exact float bits of every node position, in tree order.
    let engine = app.engine.clone();
    let world = engine.world();
    let root_entity = engine.root();
    let mut out = Vec::new();
    for entity in balaur_core::scene::collect_subtree(&world, root_entity) {
        if let Ok(t) = world.get::<&Transform>(entity) {
            out.push([
                t.position.x.to_bits(),
                t.position.y.to_bits(),
                t.position.z.to_bits(),
            ]);
        }
    }
    out
}

#[test]
fn simulation_is_bitwise_reproducible() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let a = simulate(dir.path(), 300);
    let b = simulate(dir.path(), 300);
    assert!(!a.is_empty());
    assert_eq!(a, b, "two identical runs must match bit for bit");
}

/// The 2D world holds itself to the same standard. Integer literals in the
/// scene (`half_extents = [10, 1]`) must parse as floats too.
fn write_project_2d(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("scenes")).unwrap();
    std::fs::write(
        root.join("project.toml"),
        "name = \"det2d\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("scenes/main.toml"),
        r#"
[[nodes]]
id = "n_ground"
name = "Ground"
position = [0.0, -1.0, 0.0]
body2d = "static"
collider2d = { kind = "rect", half_extents = [10, 1] }

[[nodes]]
id = "n_balla"
name = "BallA"
position = [0.1, 5.0, 0.0]
body2d = "dynamic"
collider2d = { kind = "circle", radius = 0.5, restitution = 0.4 }

[[nodes]]
id = "n_boxb"
name = "BoxB"
position = [-0.1, 7.0, 0.0]
rotation_euler = [0.0, 0.0, 0.4]
body2d = "dynamic"
collider2d = { kind = "rect", half_extents = [0.5, 0.3] }
"#,
    )
    .unwrap();
}

#[test]
fn simulation_2d_is_bitwise_reproducible() {
    let dir = tempfile::tempdir().unwrap();
    write_project_2d(dir.path());
    let a = simulate(dir.path(), 300);
    let b = simulate(dir.path(), 300);
    assert!(!a.is_empty());
    assert_eq!(a, b, "two identical 2D runs must match bit for bit");
    // The integer-literal ground must actually be 10 wide: the ball dropped
    // at x = 0.1 has to come to rest on it instead of free-falling past it.
    let resting = a.iter().any(|p| {
        let y = f32::from_bits(p[1]);
        (y - 0.5).abs() < 0.2
    });
    assert!(resting, "ball should rest on the ground: {a:?}");
}

/// The end state agreeing is weaker than every tick agreeing: two runs can
/// part in the middle and land on the same rest position.
#[test]
fn two_runs_agree_on_every_tick_not_just_the_last() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let a = trace(dir.path(), 300);
    let b = trace(dir.path(), 300);
    assert_eq!(a.len(), 300);
    let parted = a.iter().zip(&b).position(|(x, y)| x != y);
    assert_eq!(parted, None, "the two runs parted at tick {parted:?}");
}

#[test]
fn the_digest_moves_while_the_simulation_does() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let a = trace(dir.path(), 60);
    assert_ne!(
        a[0], a[30],
        "a digest that never changes is hashing nothing that moves"
    );
}

#[test]
fn a_divergence_report_names_the_node_and_the_slice() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let mut app = boot(dir.path());
    for _ in 0..30 {
        app.tick(balaur_core::FIXED_DT);
    }
    let before = digest::entries(&app.engine);

    let ball = balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), "BallA")
        .expect("the scene has a BallA");
    app.engine
        .world_mut()
        .get::<&mut Transform>(ball)
        .unwrap()
        .position
        .x += 1.0;

    let report = digest::first_divergence(&before, &digest::entries(&app.engine))
        .expect("moving a ball is a divergence");
    assert!(
        report.starts_with("n_balla/transform:"),
        "the report has to name the stable id, got {report}"
    );
}
