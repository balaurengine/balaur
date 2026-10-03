//! Voxels and the shapes built from a mesh, the solver knobs, and the
//! geometry toolkit.

use balaur::{AppConfig, standard_app};

use crate::LOG;

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
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
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
script = { source = "scripts/s.rn" }

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
    assert!(this.node.collider3d.voxel(0, 1, 0), "the middle cell should be filled");
    this.node.collider3d.set_voxel(0, 1, 0, false);
    assert!(!this.node.collider3d.voxel(0, 1, 0), "digging left the cell filled");
    this.node.collider3d.set_voxel(5, 5, 5, true);
    assert!(this.node.collider3d.voxel(5, 5, 5), "a new cell was not added");
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
    let mesh = this.node.collider3d.collider_mesh();
    assert!(mesh.points.len() > 0, "the grid tessellated to nothing");
    assert!(mesh.indices.len() % 3 == 0, "the triangles are not triples");
}
"#,
    );
}

#[test]
fn the_mesh_backed_shapes_build() {
    run_clean(
        r##"pub fn init(this) {
    for kind in ["convex_hull", "convex_decomposition", "triangle_mesh"] {
        this.node.collider3d.set_collider(#{ kind: kind, mesh: "#wedge" });
    }
    for fit in ["aabb", "obb", "convex_hull"] {
        this.node.collider3d.set_collider(#{ kind: "fit", fit: fit, mesh: "#wedge" });
    }
    this.node.collider3d.set_collider(#{ kind: "voxelized_mesh", mesh: "#wedge", voxel_size: 0.25 });
}
"##,
    );
}

#[test]
fn the_solver_knobs_are_set_and_read_back() {
    run_clean(
        r#"pub fn init(this) {
    physics::set_tuning(#{ solver_iterations: 8, length_unit: 64.0, ccd_substeps: 2 });
    let tuning = physics::tuning()["3d"];
    assert!(tuning.solver_iterations == 8.0, "iterations read back as {}", tuning.solver_iterations);
    assert!(tuning.length_unit == 64.0, "the length unit read back as {}", tuning.length_unit);
    assert!(tuning.ccd_substeps == 2.0, "substeps read back as {}", tuning.ccd_substeps);
    let quarantined = physics::quarantined();
    for world in [quarantined.physics3d, quarantined.physics2d] {
        assert!(world.bodies.len() + world.colliders.len() + world.soft_bodies.len() == 0, "something was quarantined in a still world");
    }
    assert!(physics::threads() >= 1, "a build always has at least one thread");
}
"#,
    );
}

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

#[test]
fn debug_draw_is_set_and_read_back() {
    run_clean(
        r#"pub fn init(this) {
    physics::set_debug_draw(true);
    assert!(physics::debug_draw().enabled, "the switch did not stay on");
    physics::set_debug_draw(#{ colliders: true, impulse_joints: true, contacts: false });
    let modes = physics::debug_draw();
    assert!(modes.colliders && modes.impulse_joints, "the named modes were not kept");
    assert!(!modes.contacts, "a mode that was not named came on");
    physics::set_debug_draw(false);
    assert!(!physics::debug_draw().enabled, "the switch did not go off");
}
"#,
    );
}

/// Rapier hands the fitted pose back beside the shape, off the node's origin.
#[test]
fn a_fitted_box_keeps_its_pose() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        r##"[[assets]]
id = "aside"
type = "mesh"
positions = [[2.0, 0.0, 0.0], [3.0, 0.0, 0.0], [2.0, 1.0, 0.0], [2.0, 0.0, 1.0]]
indices = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]

[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_aabb"
name = "Aabb"
parent = "n_world"
collider3d = { kind = "fit", fit = "aabb", mesh = "#aside" }

[[nodes]]
id = "n_obb"
name = "Obb"
parent = "n_world"
collider3d = { kind = "fit", fit = "obb", mesh = "#aside" }
"##,
    )
    .unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    let world = app.engine.world();
    let root = app.engine.root();
    let aabb_node = balaur_core::scene::find_node(&world, root, "World/Aabb").unwrap();
    let obb_node = balaur_core::scene::find_node(&world, root, "World/Obb").unwrap();
    drop(world);
    let bounds = |node| {
        let state = app.engine.resource::<balaur_physics::PhysicsState3d>();
        let state = state.borrow();
        let aabb = state.world.colliders[state.colliders[&node][0]].compute_aabb();
        (
            [aabb.mins.x, aabb.mins.y, aabb.mins.z],
            [aabb.maxs.x, aabb.maxs.y, aabb.maxs.z],
        )
    };
    let (mins, maxs) = bounds(aabb_node);
    for (axis, (low, high)) in [(2.0, 3.0), (0.0, 1.0), (0.0, 1.0)].into_iter().enumerate() {
        assert!(
            (mins[axis] - low).abs() < 1e-4 && (maxs[axis] - high).abs() < 1e-4,
            "the fitted box spans {mins:?}..{maxs:?}, not the mesh's own bounds"
        );
    }
    let (mins, maxs) = bounds(obb_node);
    let centroid = [2.25, 0.25, 0.25];
    assert!(
        (0..3).all(|axis| mins[axis] <= centroid[axis] && centroid[axis] <= maxs[axis]),
        "the oriented box {mins:?}..{maxs:?} does not hold the mesh's centroid"
    );

    let back = balaur_core::components::get(&app.engine, aabb_node, "collider3d").unwrap();
    assert_eq!(
        back.get("kind").and_then(toml::Value::as_str),
        Some("fit"),
        "a fitted box reads back as the fit that made it"
    );
}

/// One entry from a script's `#{}` map.
fn entry<'a>(value: &'a balaur_script::Value, key: &str) -> &'a balaur_script::Value {
    let balaur_script::Value::Map(pairs) = value else {
        panic!("{key}: not a map, but {value:?}");
    };
    pairs
        .iter()
        .find_map(|(k, v)| (k == key).then_some(v))
        .unwrap_or_else(|| panic!("no `{key}` in {value:?}"))
}

#[test]
fn quarantine_and_counters_report_both_worlds() {
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
    std::fs::write(
        dir.path().join("main.toml"),
        r#"[[nodes]]
id = "n_world"
name = "World"
script = { source = "scripts/s.rn" }

[[nodes]]
id = "n_ball3"
name = "Ball3"
parent = "n_world"
body3d = { kind = "dynamic" }
collider3d = { kind = "sphere", radius = 0.5 }

[[nodes]]
id = "n_ball2"
name = "Ball2"
parent = "n_world"
body2d = { kind = "dynamic" }
collider2d = { kind = "circle", radius = 0.5 }
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.rn"),
        "pub fn report(this) {\n    [physics::quarantined(), physics::counters()]\n}\n",
    )
    .unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    let root = app.engine.root();
    let (world_node, ball2) = {
        let world = app.engine.world();
        (
            balaur_core::scene::find_node(&world, root, "World").unwrap(),
            balaur_core::scene::find_node(&world, root, "World/Ball2").unwrap(),
        )
    };
    {
        let state = app.engine.resource::<balaur_physics::PhysicsState2d>();
        let mut state = state.borrow_mut();
        let handle = state.bodies[&ball2];
        let broken = balaur_physics::rapier2d::math::Vector::new(f32::NAN, 0.0);
        state.world.bodies[handle].set_linvel(broken, true);
    }
    app.tick(1.0 / 60.0);
    let host = app.engine.script_host().unwrap();
    let Some(balaur_script::Value::List(report)) =
        host.call_on(balaur_core::node_id_of(world_node), "report", &[])
    else {
        panic!("the probe did not answer a list");
    };
    let bodies = |world: &str| match entry(entry(&report[0], world), "bodies") {
        balaur_script::Value::List(nodes) => nodes.clone(),
        other => panic!("{world} bodies: {other:?}"),
    };
    assert_eq!(
        bodies("physics2d"),
        [balaur_script::Value::Node(ball2.to_bits().get())],
        "the 2D world's quarantine names the body it disabled"
    );
    assert!(
        bodies("physics3d").is_empty(),
        "the 3D world quarantined nothing"
    );
    for world in ["physics3d", "physics2d"] {
        let counters = entry(&report[1], world);
        for key in [
            "step_ms",
            "broad_phase_ms",
            "narrow_phase_ms",
            "contact_pair_count",
        ] {
            assert!(
                matches!(entry(counters, key), balaur_script::Value::Num(n) if *n >= 0.0),
                "{world}.{key} is not a count"
            );
        }
    }
}
