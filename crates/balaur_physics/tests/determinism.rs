//! Determinism is a core engine feature: identical inputs must produce
//! bit-for-bit identical simulations. This test guards the same-platform
//! half; cross-platform runs compare the same digest in CI.

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
name = "Ground"
position = [0.0, -1.0, 0.0]
body = "fixed"
collider = { shape = "cuboid", half_extents = [10.0, 0.5, 10.0] }

[[nodes]]
name = "BallA"
position = [0.1, 5.0, 0.0]
body = "dynamic"
collider = { shape = "ball", radius = 0.5 }

[[nodes]]
name = "BallB"
position = [-0.1, 7.0, 0.05]
body = "dynamic"
collider = { shape = "ball", radius = 0.5 }
"#,
    )
    .unwrap();
}

fn simulate(root: &std::path::Path, frames: u32) -> Vec<[u32; 3]> {
    let mut app = App::new(AppConfig {
        project_root: root.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
    })
    .unwrap();
    app.add_plugin(PhysicsPlugin).unwrap();
    app.load_project().unwrap();
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
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
