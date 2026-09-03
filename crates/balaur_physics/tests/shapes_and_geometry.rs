//! Phases 8, 9 and 11 of `docs/PLAN-rapier.md`: voxels and the shapes built
//! from a mesh, the solver knobs, and the geometry toolkit.

use balaur::{standard_app, AppConfig};

static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A project whose scene declares a cube mesh and a small voxel grid, so the
/// asset-backed shapes have something to be built from.
fn run(script: &str) -> Vec<String> {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        r##"[[assets]]
id = "pillar"
type = "voxels"
size = [1.0, 1.0, 1.0]
cells = [[0, 0, 0], [0, 1, 0], [0, 2, 0]]

[[assets]]
id = "wedge"
type = "mesh"
positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
indices = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]

[[nodes]]
id = "n_terrain"
name = "Terrain"
script = "scripts/s.rn"

[nodes.collider3d]
kind = "voxels"
voxels = "#pillar"
"##,
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/s.rn"), script).unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    balaur_core::logbuf::recent(80)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect()
}

fn run_clean(script: &str) {
    let errors = run(script);
    assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
}

/// The point of voxels over a mesh: a game may dig into them.
#[test]
fn a_voxel_grid_can_be_dug_into() {
    run_clean(
        r#"pub fn init(this) {
    assert!(physics3d::voxel(this.node, 0, 1, 0), "the middle cell should be filled");
    physics3d::set_voxel(this.node, 0, 1, 0, false);
    assert!(!physics3d::voxel(this.node, 0, 1, 0), "digging left the cell filled");
    physics3d::set_voxel(this.node, 5, 5, 5, true);
    assert!(physics3d::voxel(this.node, 5, 5, 5), "a new cell was not added");
}
"#,
    );
}

/// A voxel collider draws itself: parry tessellates every shape, and this is
/// how a voxel terrain gets on screen at all.
#[test]
fn a_voxel_collider_can_be_turned_into_a_mesh() {
    run_clean(
        r#"pub fn init(this) {
    let mesh = physics3d::collider_mesh(this.node);
    assert!(mesh.points.len() > 0, "the grid tessellated to nothing");
    assert!(mesh.indices.len() % 3 == 0, "the triangles are not triples");
}
"#,
    );
}

/// The mesh-backed shapes this phase added, each built from the same asset.
#[test]
fn the_mesh_backed_shapes_build() {
    run_clean(
        r##"pub fn init(this) {
    for kind in ["convex_hull", "convex_decomposition", "trimesh"] {
        physics3d::set_collider(this.node, #{ kind: kind, mesh: "#wedge" });
    }
    for fit in ["aabb", "obb", "convex_hull"] {
        physics3d::set_collider(this.node, #{ kind: "fit", fit: fit, mesh: "#wedge" });
    }
    physics3d::set_collider(this.node, #{ kind: "voxelized_mesh", mesh: "#wedge", voxel_size: 0.25 });
}
"##,
    );
}

/// Every solver knob a game may set, read back.
#[test]
fn the_solver_knobs_are_set_and_read_back() {
    run_clean(
        r#"pub fn init(this) {
    physics::set_tuning(#{ solver_iterations: 8.0, length_unit: 64.0, ccd_substeps: 2.0 });
    let tuning = physics::tuning();
    assert!(tuning.solver_iterations == 8.0, "iterations read back as {}", tuning.solver_iterations);
    assert!(tuning.length_unit == 64.0, "the length unit read back as {}", tuning.length_unit);
    assert!(tuning.ccd_substeps == 2.0, "substeps read back as {}", tuning.ccd_substeps);
    assert!(physics::quarantined().len() == 0, "something was quarantined in a still world");
    assert!(physics::threads() >= 1, "a build always has at least one thread");
}
"#,
    );
}

/// The geometry toolkit: what a game reaches for when it breaks something.
#[test]
fn the_geometry_toolkit_works_on_a_mesh() {
    run_clean(
        r##"pub fn init(this) {
    let hull = geometry3d::convex_hull("#wedge");
    assert!(hull.points.len() >= 4, "the hull of a tetrahedron has four points, not {}", hull.points.len());

    let pieces = geometry3d::convex_decomposition("#wedge", #{ resolution: 16.0 });
    assert!(pieces.len() >= 1, "the decomposition found no pieces");

    let grid = geometry3d::voxelize("#wedge", #{ resolution: 8.0 });
    assert!(grid.cells.len() > 0, "voxelising found no cells");

    let halves = geometry3d::split("#wedge", #{ point: [0.25, 0.0, 0.0], normal: [1.0, 0.0, 0.0] });
    assert!(halves.len() == 2, "a cut gives two halves, not {}", halves.len());
}
"##,
    );
}

/// The debug draw is on a switch, and the switch reads back.
#[test]
fn debug_draw_is_set_and_read_back() {
    run_clean(
        r#"pub fn init(this) {
    physics::set_debug_draw(true);
    assert!(physics::debug_draw().enabled, "the switch did not stay on");
    physics::set_debug_draw(#{ colliders: true, joints: true, contacts: false });
    let modes = physics::debug_draw();
    assert!(modes.colliders && modes.joints, "the named modes were not kept");
    assert!(!modes.contacts, "a mode that was not named came on");
    physics::set_debug_draw(false);
    assert!(!physics::debug_draw().enabled, "the switch did not go off");
}
"#,
    );
}
