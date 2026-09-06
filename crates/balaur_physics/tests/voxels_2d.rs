//! The 2D voxel collider: a grid of filled cells a script may dig into, the
//! shape tile collision is built on.

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use balaur_physics::PhysicsState2d;
use balaur_physics::rapier2d::math::IVector;

/// The log buffer is global and tests run in parallel.
static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

const SCENE: &str = r#"[[assets]]
id = "wall"
type = "voxels"
size = [1.0, 1.0, 1.0]
cells = [[0, 0, 0], [0, 1, 0], [1, 0, 0]]

[[nodes]]
id = "n_wall"
name = "Wall"
script = "scripts/s.rn"

[nodes.collider2d]
kind = "voxels"
voxels = "#wall"
"#;

/// The script's last write is the control: a cell nothing else fills, which
/// the assertion below reads back off the shape.
const SCRIPT: &str = r#"pub fn init(this) {
    assert!(physics2d::voxel(this.node, 0, 1), "the cell above the corner should be filled");
    assert!(!physics2d::voxel(this.node, 4, 4), "a cell nobody wrote should be empty");
    physics2d::set_voxel(this.node, 0, 1, false);
    assert!(!physics2d::voxel(this.node, 0, 1), "digging left the cell filled");
    let (x, y) = physics2d::voxel_at(this.node, 0.5, 0.5);
    assert!(x == 0 && y == 0, "a world point landed in the wrong cell");
    physics2d::set_voxel(this.node, 7, 7, true);
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
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), SCENE).unwrap();
    std::fs::write(dir.path().join("scripts/s.rn"), SCRIPT).unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    let errors = balaur_core::logbuf::recent(80)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect();
    (app, errors)
}

#[test]
fn a_2d_voxel_grid_can_be_read_and_dug_into() {
    let (app, errors) = run();
    assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    let handles = state
        .colliders
        .values()
        .next()
        .expect("the scene declares a collider");
    let handle = *handles.first().expect("the collider reached rapier");
    let voxels = state.world.colliders[handle]
        .shape()
        .as_voxels()
        .expect("a voxel grid, not some other shape");
    assert!(
        voxels
            .voxel_state(IVector::new(7, 7))
            .is_some_and(|cell| !cell.is_empty()),
        "the script's last write is missing, so the script never ran"
    );
    assert!(
        voxels
            .voxel_state(IVector::new(0, 1))
            .is_none_or(|cell| cell.is_empty()),
        "the cell the script dug is filled again"
    );
    assert!(
        state.shape_revision > 0,
        "a dug grid has to reach the digest, or a peer never learns of the hole"
    );
}
