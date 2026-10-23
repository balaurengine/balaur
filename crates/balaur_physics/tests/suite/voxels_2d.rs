//! The 2D voxel collider: a grid of filled cells a script may dig into, the
//! shape tile collision is built on.

use balaur::standard_app;
use balaur_core::App;
use balaur_physics::PhysicsState2d;
use balaur_physics::rapier2d::math::IVector;

/// The log buffer is global and tests run in parallel.
use crate::LOG;

const SCENE: &str = r##"[[assets]]
id = "wall"
type = "voxels"
size = [1.0, 1.0, 1.0]
cells = [[0, 0, 0], [0, 1, 0], [1, 0, 0]]

[[nodes]]
id = "n_wall"
name = "Wall"
script = { source = "scripts/s.bs" }

[nodes.collider2d]
kind = "voxels"
voxels = "#wall"
"##;

/// The script's last write is the control: a cell nothing else fills, which
/// the assertion below reads back off the shape.
const SCRIPT: &str = r#"pub struct S { node: Node }
impl S {
    fn init() {
        if !self.node.collider2d.voxel(0, 1) { log::error("the cell above the corner should be filled"); }
        if self.node.collider2d.voxel(4, 4) { log::error("a cell nobody wrote should be empty"); }
        self.node.collider2d.set_voxel(0, 1, false);
        if self.node.collider2d.voxel(0, 1) { log::error("digging left the cell filled"); }
        let cell = self.node.collider2d.voxel_at(0.5, 0.5);
        if !(cell[0] == 0 && cell[1] == 0) { log::error("a world point landed in the wrong cell"); }
        self.node.collider2d.set_voxel(7, 7, true);
    }
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
    std::fs::write(dir.path().join("scripts/s.bs"), SCRIPT).unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(crate::dev_config(dir.path())).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    let errors = balaur_core::logbuf::recent(500)
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
            .is_none_or(balaur_physics::rapier2d::geometry::VoxelState::is_empty),
        "the cell the script dug is filled again"
    );
    assert!(
        state.shape_revision > 0,
        "a dug grid has to reach the digest, or a peer never learns of the hole"
    );
}
