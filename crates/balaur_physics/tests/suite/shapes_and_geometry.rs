//! Voxels and the shapes built from a mesh, the solver knobs, and the
//! geometry toolkit.

use balaur::standard_app;

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
script = { source = "scripts/s.bs" }

[nodes.collider3d]
kind = "voxels"
voxels = "#pillar"
"##,
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/s.bs"), script).unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(crate::dev_config(dir.path())).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    balaur_core::logbuf::recent(500)
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
        r#"pub struct S { node: Node }
impl S {
    fn init() {
        if !self.node.collider3d.voxel(0, 1, 0) { log::error("the middle cell should be filled"); }
        self.node.collider3d.set_voxel(0, 1, 0, false);
        if self.node.collider3d.voxel(0, 1, 0) { log::error("digging left the cell filled"); }
        self.node.collider3d.set_voxel(5, 5, 5, true);
        if !self.node.collider3d.voxel(5, 5, 5) { log::error("a new cell was not added"); }
    }
}
"#,
    );
}

/// A voxel collider draws itself: parry tessellates every shape, and this is
/// how a voxel terrain gets on screen at all.
#[test]
fn a_voxel_collider_can_be_turned_into_a_mesh() {
    run_clean(
        r#"pub struct S { node: Node }
impl S {
    fn init() {
        let mesh = self.node.collider3d.collider_mesh();
        if mesh.get("points").len() <= 0 { log::error("the grid tessellated to nothing"); }
        if mesh.get("indices").len() % 3 != 0 { log::error("the triangles are not triples"); }
    }
}
"#,
    );
}

#[test]
fn the_mesh_backed_shapes_build() {
    run_clean(
        r##"pub struct S { node: Node }
impl S {
    fn init() {
        for kind in ["convex_hull", "convex_decomposition", "triangle_mesh"] {
            self.node.collider3d.set_collider(map().with("kind", kind).with("mesh", "#wedge"));
        }
        for fit in ["aabb", "obb", "convex_hull"] {
            self.node.collider3d.set_collider(map().with("kind", "fit").with("fit", fit).with("mesh", "#wedge"));
        }
        self.node.collider3d.set_collider(map().with("kind", "voxelized_mesh").with("mesh", "#wedge").with("voxel_size", 0.25));
    }
}
"##,
    );
}

#[test]
fn the_solver_knobs_are_set_and_read_back() {
    run_clean(
        r#"pub struct S { node: Node }
impl S {
    fn init() {
        physics::set_tuning(map().with("solver_iterations", 8).with("length_unit", 64.0).with("ccd_substeps", 2));
        let tuning = physics::tuning().get("3d");
        let iterations = tuning.get("solver_iterations").as_float();
        if iterations != 8.0 { log::error("iterations read back as {iterations}"); }
        let unit = tuning.get("length_unit").as_float();
        if unit != 64.0 { log::error("the length unit read back as {unit}"); }
        let substeps = tuning.get("ccd_substeps").as_float();
        if substeps != 2.0 { log::error("substeps read back as {substeps}"); }
        let quarantined = physics::quarantined();
        for world in [quarantined.get("physics3d"), quarantined.get("physics2d")] {
            if world.get("bodies").len() + world.get("colliders").len() + world.get("soft_bodies").len() != 0 {
                log::error("something was quarantined in a still world");
            }
        }
        if physics::threads() < 1 { log::error("a build always has at least one thread"); }
    }
}
"#,
    );
}

#[test]
fn the_geometry_toolkit_works_on_a_mesh() {
    run_clean(
        r##"pub struct S { node: Node }
impl S {
    fn init() {
        let hull = geometry3d::convex_hull("#wedge");
        let points = hull.get("points").len();
        if points < 4 { log::error("the hull of a tetrahedron has four points, not {points}"); }

        let pieces = geometry3d::convex_decomposition("#wedge", map().with("resolution", 16.0));
        if pieces.len() < 1 { log::error("the decomposition found no pieces"); }

        let grid = geometry3d::voxelize("#wedge", map().with("resolution", 8.0));
        if grid.get("cells").len() <= 0 { log::error("voxelising found no cells"); }

        let halves = geometry3d::split("#wedge", map().with("point", [0.25, 0.0, 0.0]).with("normal", [1.0, 0.0, 0.0]));
        if halves.len() != 2 { log::error("a cut gives two halves, not {halves.len()}"); }
    }
}
"##,
    );
}

#[test]
fn debug_draw_is_set_and_read_back() {
    run_clean(
        r#"pub struct S { node: Node }
impl S {
    fn init() {
        physics::set_debug_draw(true);
        if !physics::debug_draw().get("enabled").as_bool() { log::error("the switch did not stay on"); }
        physics::set_debug_draw(map().with("colliders", true).with("impulse_joints", true).with("contacts", false));
        let modes = physics::debug_draw();
        if !(modes.get("colliders").as_bool() && modes.get("impulse_joints").as_bool()) { log::error("the named modes were not kept"); }
        if modes.get("contacts").as_bool() { log::error("a mode that was not named came on"); }
        physics::set_debug_draw(false);
        if physics::debug_draw().get("enabled").as_bool() { log::error("the switch did not go off"); }
    }
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
    let mut app = standard_app(crate::dev_config(dir.path())).unwrap();
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

/// One entry from a script's map.
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
script = { source = "scripts/s.bs" }

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
        dir.path().join("scripts/s.bs"),
        "pub struct S { node: Node }\n\
         impl S {\n    pub fn report() -> [any] { [physics::quarantined(), physics::counters()] }\n}\n",
    )
    .unwrap();
    let mut app = standard_app(crate::dev_config(dir.path())).unwrap();
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
