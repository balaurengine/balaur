//! Picking down to the triangle: a node's box narrows the field, and the
//! geometry decides, so a ray through a gap in a model misses it.

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use balaur_core::scene::{Transform, find_node};

/// The log buffer is global and tests run in parallel.
static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// One triangle filling the lower-left half of a two-by-two box, so the
/// upper-right quadrant is inside the node's bounds and outside its geometry.
const SCENE: &str = r##"[[assets]]
id = "wedge"
type = "mesh"
positions = [[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [-1.0, 1.0, 0.0]]
indices = [[0, 1, 2]]

[[nodes]]
id = "n_wedge"
name = "Wedge"
script = "scripts/s.rn"

[nodes.mesh]
source = "#wedge"
"##;

/// The move at the end is the control: a script that never ran leaves the
/// node where the scene put it, and the assertions below would hold vacuously.
const SCRIPT: &str = r#"pub fn init(this) {
    let through = render::pick_ray(-0.5, -0.5, 5.0, 0.0, 0.0, -1.0);
    if !through.is_some() {
        log::error("the ray through the triangle picked nothing");
    }
    let gap = render::pick_ray(0.6, 0.6, 5.0, 0.0, 0.0, -1.0);
    if gap.is_some() {
        log::error("the ray through the gap picked the node's box, not its triangles");
    }
    this.node.set_position(0.0, 3.0, 0.0);
}
"#;

fn run() -> (App, Vec<String>) {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"r\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), SCENE).unwrap();
    std::fs::write(dir.path().join("scripts/s.rn"), SCRIPT).unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    let errors = balaur_core::logbuf::recent(50)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect();
    (app, errors)
}

#[test]
fn a_ray_through_a_gap_in_the_geometry_misses_what_the_box_would_have_hit() {
    let (app, errors) = run();
    assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
    let world = app.engine.world();
    let node = find_node(&world, app.engine.root(), "Wedge").expect("the scene's node");
    let moved = world.get::<&Transform>(node).unwrap().position.y;
    assert!(
        (moved - 3.0).abs() < 1e-4,
        "the script never ran, so the picks above proved nothing"
    );
}
