//! `softbody3d` and `softbody2d`: what each layout builds, what the material
//! rows change, and that the solver's positions reach the node.

use balaur::{AppConfig, standard_app};

use crate::LOG;

/// A project holding a closed tetrahedron mesh, a soft cuboid in 3D and a
/// soft grid in 2D, each with a script attached.
fn run(script: &str) -> Vec<String> {
    run_for(script, 4)
}

/// The same over `ticks` steps, for a body that has to be given time to fall,
/// settle or come apart.
fn run_for(script: &str, ticks: u32) -> Vec<String> {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (_dir, _app) = boot(script, ticks);
    balaur_core::logbuf::recent(120)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect()
}

/// The project above, loaded and ticked, for a test that reads the world.
fn boot(script: &str, ticks: u32) -> (tempfile::TempDir, balaur_core::App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        r#"[[assets]]
id = "wedge"
type = "mesh"
positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
indices = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]

[[assets]]
id = "square"
type = "mesh"
positions = [[-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.5, 0.5, 0.0], [-0.5, 0.5, 0.0]]
indices = [[0, 1, 2], [0, 2, 3]]

[[assets]]
id = "hub"
type = "mesh"
positions = [[-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.5, 0.5, 0.0], [-0.5, 0.5, 0.0], [0.0, 0.0, 0.0]]
indices = [[0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4]]

[[nodes]]
id = "n_blob"
name = "Blob"
script = { source = "scripts/s.rn" }

[nodes.softbody3d]
kind = "box"
cells = [2.0, 2.0, 2.0]

[[nodes]]
id = "n_blob2d"
name = "Blob2d"
parent = "n_blob"

[nodes.softbody2d]
kind = "grid"
cells = [2.0, 2.0]
"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/s.rn"), script).unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    for _ in 0..ticks {
        app.tick(1.0 / 60.0);
    }
    (dir, app)
}

fn run_clean(script: &str) {
    let errors = run(script);
    assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
}

/// How many soft bodies the 3D world holds.
fn bodies(app: &balaur_core::App) -> usize {
    let state = app.engine.resource::<balaur_physics::PhysicsState3d>();

    state.borrow().soft_bodies.len()
}

fn run_clean_for(script: &str, ticks: u32) {
    let errors = run_for(script, ticks);
    assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
}

#[test]
fn a_cuboid_soft_body_has_the_particles_its_cell_counts_ask_for() {
    run_clean(
        r#"pub fn init(this) {
    // Three particles an axis for two cells an axis.
    assert_eq!(this.node.softbody3d.softbody_particles(), 27, "the cuboid was not 3x3x3 particles");
}
"#,
    );
}

#[test]
fn every_3d_layout_builds() {
    run_clean(
        r##"pub fn init(this) {
    let body = this.node.softbody3d;
    body.set_softbody(#{ kind: physics3d::SOFT_SPHERE, radius: 0.5, subdivisions: 1 });
    assert!(body.softbody_particles() > 0, "the sphere has no particles");
    body.set_softbody(#{ kind: physics3d::SOFT_CLOTH, cells: [3.0, 3.0, 1.0], size: [1.0, 0.0, 1.0] });
    assert_eq!(body.softbody_particles(), 16, "a 3x3 cloth is 4x4 particles");
    body.set_softbody(#{ kind: physics3d::SOFT_CLOTH_TUBE, radius: 0.3, cells: [6.0, 4.0, 1.0] });
    assert!(body.softbody_particles() > 0, "the tube has no particles");
    body.set_softbody(#{ kind: physics3d::SOFT_ROPE, particle_count: 8 });
    assert_eq!(body.softbody_particles(), 8, "the rope has the wrong particle count");
    body.set_softbody(#{ kind: physics3d::SOFT_TRIANGLE_MESH, mesh: "#wedge" });
    assert_eq!(body.softbody_particles(), 4, "the wedge has four corners");
}
"##,
    );
}

/// The approximate tetrahedrization: a closed mesh is covered with cells, and
/// what comes out is a body with more particles than the mesh had corners.
#[test]
fn a_volumetric_body_fills_a_closed_mesh_with_cells() {
    run_clean(
        r##"pub fn init(this) {
    let body = this.node.softbody3d;
    body.set_softbody(#{ kind: physics3d::SOFT_VOLUMETRIC, mesh: "#wedge", cell_size: 0.2 });
    assert!(body.softbody_particles() > 4, "the tetrahedrization added no particles");
    assert!(body.softbody_volume() > 0.0, "the filled body encloses nothing");
}
"##,
    );
}

/// The cell size is the knob that matters: a smaller one is a finer body.
#[test]
fn a_smaller_cell_size_makes_a_finer_volumetric_body() {
    run_clean(
        r##"pub fn init(this) {
    let body = this.node.softbody3d;
    body.set_softbody(#{ kind: physics3d::SOFT_VOLUMETRIC, mesh: "#wedge", cell_size: 0.4 });
    let coarse = body.softbody_particles();
    body.set_softbody(#{ kind: physics3d::SOFT_VOLUMETRIC, mesh: "#wedge", cell_size: 0.15 });
    assert!(body.softbody_particles() > coarse, "halving the cell size added no particles");
}
"##,
    );
}

/// A generator that would run the machine out of memory is an error, not a
/// hang: the numbers come from a text field.
#[test]
fn a_layout_past_the_particle_cap_is_refused() {
    let errors = run(r"pub fn init(this) {
    this.node.softbody3d.set_softbody(#{ kind: physics3d::SOFT_BOX, cells: [400.0, 400.0, 400.0] });
}
");
    assert!(
        errors.iter().any(|e| e.contains("particles")),
        "the cap did not report the particle count: {errors:#?}"
    );
}

#[test]
fn a_cell_count_no_integer_holds_is_refused_rather_than_built() {
    let errors = run(r"pub fn init(this) {
    this.node.softbody3d.set_softbody(#{ kind: physics3d::SOFT_CLOTH, cells: [1.0e30, 1.0e30, 1.0] });
}
");
    assert!(
        errors.iter().any(|e| e.contains("particles")),
        "the cap did not report the particle count: {errors:#?}"
    );
}

/// The point of a soft body: the solver owns the positions, and the node is
/// drawn from them rather than from what was authored.
#[test]
fn a_falling_soft_body_moves_its_particles() {
    run_clean(
        r#"pub fn init(this) {
    this.first = this.node.softbody3d.softbody_position(0);
    this.ticks = 0;
}

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    if this.ticks == 3 {
        let now = this.node.softbody3d.softbody_position(0);
        assert!(now.y < this.first.y, "gravity did not move the body's particles");
    }
}
"#,
    );
}

/// A pinned particle is the hook a cloth hangs from, so it must not fall with
/// the rest of the body.
#[test]
fn a_pinned_particle_stays_where_it_was_put() {
    run_clean(
        r#"pub fn init(this) {
    this.node.softbody3d.set_softbody(#{ kind: physics3d::SOFT_ROPE, particle_count: 8, pinned_particles: [0] });
    this.first = this.node.softbody3d.softbody_position(0);
    this.ticks = 0;
}

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    if this.ticks == 3 {
        let body = this.node.softbody3d;
        let held = body.softbody_position(0);
        assert!((held.y - this.first.y).abs() < 0.001, "the pinned particle fell");
        assert!(body.softbody_position(7).y < this.first.y, "the free end did not fall");
    }
}
"#,
    );
}

/// Every mechanical row round-trips: what is written is what the component
/// reads back, so an inspector edit is not silently dropped.
#[test]
fn the_material_rows_are_set_and_read_back() {
    run_clean(
        r#"pub fn init(this) {
    let node = this.node;
    node.softbody3d.edge_frequency = 120.0;
    node.softbody3d.bend_damping = 0.4;
    node.softbody3d.cell_model = physics3d::CELL_COROTATIONAL;
    node.softbody3d.young_modulus = 50000.0;
    node.softbody3d.poisson_ratio = 0.45;
    node.softbody3d.plastic_yield = 0.2;
    node.softbody3d.edge_plastic_flow = physics3d::FLOW_COMPRESSION;
    node.softbody3d.tear_strain = 0.6;
    node.softbody3d.shape_matching = true;
    assert_eq!(node.softbody3d.edge_frequency, 120.0, "edge_frequency did not stick");
    assert_eq!(node.softbody3d.bend_damping, 0.4, "bend_damping did not stick");
    assert_eq!(node.softbody3d.cell_model, physics3d::CELL_COROTATIONAL, "cell_model did not stick");
    assert_eq!(node.softbody3d.young_modulus, 50000.0, "young_modulus did not stick");
    assert_eq!(node.softbody3d.poisson_ratio, 0.45, "poisson_ratio did not stick");
    assert_eq!(node.softbody3d.plastic_yield, 0.2, "plastic_yield did not stick");
    assert_eq!(node.softbody3d.edge_plastic_flow, physics3d::FLOW_COMPRESSION, "edge_plastic_flow did not stick");
    assert_eq!(node.softbody3d.tear_strain, 0.6, "tear_strain did not stick");
    assert_eq!(node.softbody3d.shape_matching, true, "shape_matching did not stick");
}
"#,
    );
}

/// Volume preservation is what keeps a jelly from collapsing, and the read
/// side has to report it from the solver rather than from what was authored.
#[test]
fn volume_preservation_reads_back_from_the_solver() {
    run_clean(
        r#"pub fn init(this) {
    let node = this.node;
    node.softbody3d.volume_preservation = false;
    node.softbody3d.volume_factor = 1.5;
    assert_eq!(node.softbody3d.volume_preservation, false, "volume_preservation did not stick");
    assert_eq!(node.softbody3d.volume_factor, 1.5, "volume_factor did not stick");
    assert!(node.softbody3d.softbody_rest_volume() > 0.0, "the block encloses nothing at rest");
}
"#,
    );
}

#[test]
fn the_2d_world_has_the_same_shape_of_api() {
    run_clean(
        r##"pub fn init(this) {
    let blob = this.node.get_node("Blob2d");
    assert_eq!(blob.softbody2d.softbody_particles(), 9, "the grid was not 3x3 particles");
    assert!(blob.softbody2d.softbody_rest_area() > 0.0, "the grid encloses nothing");
    blob.softbody2d.set_softbody(#{ kind: physics2d::SOFT_CIRCLE, radius: 0.5, particle_count: 12 });
    assert!(blob.softbody2d.softbody_particles() > 0, "the disk has no particles");
    blob.softbody2d.set_softbody(#{ kind: physics2d::SOFT_ROPE, particle_count: 6 });
    assert_eq!(blob.softbody2d.softbody_particles(), 6, "the 2D rope has the wrong count");
    blob.softbody2d.set_softbody(#{ kind: physics2d::SOFT_TRIANGLE_MESH, mesh: "#square" });
    assert_eq!(blob.softbody2d.softbody_particles(), 4, "the square has four corners");
}
"##,
    );
}

/// The 2D half of the approximate meshing: an outline is triangulated rather
/// than tetrahedrized, and the same `cell_size` knob decides how finely.
#[test]
fn a_2d_volumetric_body_fills_an_outline_with_triangles() {
    run_clean(
        r##"pub fn init(this) {
    let blob = this.node.get_node("Blob2d");
    blob.softbody2d.set_softbody(#{ kind: physics2d::SOFT_VOLUMETRIC, mesh: "#square", cell_size: 0.2 });
    assert!(blob.softbody2d.softbody_particles() > 4, "the triangulation added no particles");
    assert!(blob.softbody2d.softbody_area() > 0.0, "the filled body encloses nothing");
}
"##,
    );
}

/// A tear threshold is a mechanical property like any other, and what it
/// does is change the body's topology mid-step: a rope stretched past its
/// strain comes apart, and the node's `on_tear` hears about it.
#[test]
fn a_rope_past_its_tear_strain_comes_apart() {
    run_clean_for(
        r#"pub fn init(this) {
    // One end pinned, a heavy free end, and edges that break at a tenth of
    // their rest length: the rope cannot hold itself up.
    this.node.softbody3d.set_softbody(#{
        kind: physics3d::SOFT_ROPE, a: [0.0, 0.0, 0.0], b: [0.0, -2.0, 0.0], particle_count: 12,
        pinned_particles: [0], tear_strain: 0.05, tear_force: 2.0,
        edge_frequency: 4.0, mass: 400.0,
    });
    this.torn = 0;
    this.ticks = 0;
    this.before = this.node.softbody3d.softbody_particles();
}

pub fn on_tear(this, tear) {
    this.torn = this.torn + tear["pieces"] - tear["pieces"] + 1;
}

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    if this.ticks == 40 {
        // A control first: without it a body that never simulated would
        // pass this test by never tearing and never being asked to.
        assert!(this.before == 12, "the rope was not built with twelve particles");
        assert!(this.torn > 0, "the rope never tore, after 40 steps under its own weight");
    }
}
"#,
        45,
    );
}

/// A cloth is looked at from above, so its triangles have to face up.
///
/// Rapier winds a sheet spanned along +x then +z with its front underneath
/// (`du x dv` is -y), which drew the sheet unlit and back-face culled: it was
/// there, and all you could see was its shadow.
#[test]
fn a_cloth_faces_up() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = balaur_core::App::new(balaur_core::AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut balaur_physics::PhysicsPlugin::default()).unwrap();
    let root = app.engine.root();
    let node = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Sheet", root);
    balaur_core::components::add(
        &app.engine,
        node,
        "softbody3d",
        Some(
            &toml::from_str("kind = \"cloth\"\ncells = [3.0, 3.0, 1.0]\nsize = [2.0, 0.0, 2.0]")
                .unwrap(),
        ),
    )
    .unwrap();

    let solved = app.engine.world();
    let solved = solved
        .get::<&balaur_core::mesh::SolvedMesh>(node)
        .expect("a soft body hands its geometry to whatever draws it");
    assert!(!solved.indices.is_empty(), "the sheet has no triangles");
    let up = solved
        .indices
        .iter()
        .map(|[a, b, c]| {
            let at = |i: &u32| glamx::Vec3::from_array(solved.positions[*i as usize]);
            (at(b) - at(a)).cross(at(c) - at(a)).y
        })
        .filter(|y| *y > 0.0)
        .count();
    assert_eq!(
        up,
        solved.indices.len(),
        "{} of {} triangles face down",
        solved.indices.len() - up,
        solved.indices.len()
    );
}

/// What play-in-editor does on stop: the world is replaced by a fresh one,
/// so every handle naming the old one's arena has to go with it.
///
/// Left behind, a stale handle names whatever lands in that slot next — the
/// editor rebuilds the scene right after the clear — and removing the stale
/// node takes the live body with it. The sheet then draws where it was built
/// and never moves again, which is what this caught.
#[test]
fn clearing_the_world_forgets_the_soft_bodies_it_held() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = balaur_core::App::new(balaur_core::AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut balaur_physics::PhysicsPlugin::default()).unwrap();
    let root = app.engine.root();
    let cuboid = toml::from_str("kind = \"box\"\ncells = [2.0, 2.0, 2.0]").unwrap();
    let spawn = |app: &balaur_core::App, name: &str| {
        let node = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), name, root);
        balaur_core::components::add(&app.engine, node, "softbody3d", Some(&cuboid)).unwrap();
        node
    };

    let first = spawn(&app, "First");
    balaur_physics::clear(&app.engine);
    assert_eq!(bodies(&app), 0, "the clear left a soft body behind");

    // The rebuild the editor does next: its bodies take the slots the cleared
    // ones had, so a handle left over from before would name one of them.
    let second = spawn(&app, "Second");
    assert_eq!(bodies(&app), 1, "the rebuilt node has no soft body");
    balaur_core::scene::free_subtree(&mut app.engine.world_mut(), first);
    app.tick(1.0 / 60.0);

    assert_eq!(
        bodies(&app),
        1,
        "pruning the cleared node took the live one too"
    );
    let state = app.engine.resource::<balaur_physics::PhysicsState3d>();
    let state = state.borrow();
    let handle = state.soft_bodies[&second];
    assert!(
        state.world.soft_bodies.get(handle).is_some(),
        "the surviving node's handle names nothing in the world"
    );
}

/// A body whose node is freed leaves nothing behind: the handles here index
/// rapier's arena, and a stale one is a panic a script call away.
#[test]
fn freeing_a_node_frees_its_soft_body() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = balaur_core::App::new(balaur_core::AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut balaur_physics::PhysicsPlugin::default()).unwrap();
    let root = app.engine.root();
    let node = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Blob", root);
    balaur_core::components::add(
        &app.engine,
        node,
        "softbody3d",
        Some(&toml::from_str("kind = \"box\"\ncells = [2.0, 2.0, 2.0]").unwrap()),
    )
    .unwrap();
    assert_eq!(bodies(&app), 1, "the soft body was not made");
    balaur_core::scene::free_subtree(&mut app.engine.world_mut(), node);
    app.tick(1.0 / 60.0);
    assert_eq!(bodies(&app), 0, "the freed node left its soft body behind");
}

/// What the solver drew goes with the component, or the renderer keeps
/// drawing the last pose of a body that no longer exists.
#[test]
fn removing_a_soft_body_takes_its_solved_mesh_with_it() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = balaur_core::App::new(balaur_core::AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut balaur_physics::PhysicsPlugin::default()).unwrap();
    let root = app.engine.root();
    let node = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Blob", root);
    let cuboid = toml::from_str("kind = \"box\"\ncells = [2.0, 2.0, 2.0]").unwrap();
    balaur_core::components::add(&app.engine, node, "softbody3d", Some(&cuboid)).unwrap();
    app.tick(1.0 / 60.0);
    let solved = |app: &balaur_core::App| {
        app.engine
            .world()
            .get::<&balaur_core::mesh::SolvedMesh>(node)
            .is_ok()
    };
    assert!(solved(&app), "the body drew nothing to begin with");
    balaur_core::components::remove(&app.engine, node, "softbody3d").unwrap();
    app.tick(1.0 / 60.0);
    assert_eq!(bodies(&app), 0, "the removed component left its soft body");
    assert!(!solved(&app), "the removed component left its solved mesh");
}

/// A radius of 0 is worked out from the layout, and reading the component
/// back has to say 0 still: a saved number would stay put when `cells` moves.
#[test]
fn an_automatic_particle_radius_reads_back_as_automatic() {
    run_clean(
        r#"pub fn init(this) {
    this.node.softbody3d.set_softbody(#{ kind: physics3d::SOFT_BOX, cells: [2.0, 2.0, 2.0], particle_radius: 0.0 });
    let read = this.node.get_component("softbody3d");
    assert!(read.particle_radius == 0.0, "the automatic radius read back as a number");
}
"#,
    );
}

/// A 2D body a generator laid out has no polygon to deform, so its cells are
/// drawn as one, and that polygon is not a component the author added.
#[test]
fn a_generated_2d_soft_body_is_drawn_from_its_cells() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (_dir, app) = boot("pub fn init(this) {}\n", 2);
    let world = app.engine.world();
    let node = balaur_core::ids::find(&world, app.engine.root(), "n_blob2d").expect("the 2D blob");
    let drawn = world
        .get::<&balaur::render::Renderable2d>(node)
        .expect("the generated body draws nothing");
    let polygon = drawn.polygon.as_ref().expect("and not as a polygon");
    assert_eq!(polygon.positions.len(), 9, "a 2x2 grid is 3x3 particles");
    assert!(!polygon.indices.is_empty(), "with its cells as triangles");
    drop(drawn);
    drop(world);
    assert!(
        balaur_core::components::get(&app.engine, node, "polygon").is_none(),
        "the drawn cells read back as an authored polygon"
    );
}

/// `solver = "fem"` runs the elasticity as one implicit step over the body,
/// and a body on it still falls. The script logs once it has checked, so a
/// run that never reached the check fails too.
#[test]
fn a_body_on_the_implicit_solver_falls() {
    let errors = run_for(
        r#"pub fn init(this) {
    this.node.softbody3d.set_softbody(#{ kind: physics3d::SOFT_BOX, cells: [2.0, 2.0, 2.0], solver: physics3d::SOFT_SOLVER_FEM, cell_model: physics3d::CELL_COROTATIONAL });
    let read = this.node.get_component("softbody3d");
    assert!(read.solver == physics3d::SOFT_SOLVER_FEM, "the body runs on the constraint solver");
    this.first = this.node.softbody3d.softbody_position(0);
    this.ticks = 0;
}

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    if this.ticks == 3 {
        let now = this.node.softbody3d.softbody_position(0);
        assert!(now.y < this.first.y, "the implicit solver held the body still");
        log::error("checked: the body fell");
    }
}
"#,
        8,
    );
    assert!(
        errors.len() == 1 && errors[0].contains("checked: the body fell"),
        "the check did not run clean: {errors:#?}"
    );
}

/// The skin as the collision mesh: a skinned volumetric body builds with it.
#[test]
fn a_skinned_body_can_collide_through_its_skin() {
    run_clean(
        r##"pub fn init(this) {
    let body = this.node.softbody3d;
    body.set_softbody(#{ kind: physics3d::SOFT_VOLUMETRIC, mesh: "#wedge", cell_size: 0.2, skin: true, skin_collision: true });
    assert!(body.softbody_particles() > 0, "the skinned body has no particles");
}
"##,
    );
}

/// A disk is a hoop of particles around nothing, so the area it holds is all
/// that keeps it round when it lands. The script logs once it has checked.
#[test]
fn a_disk_keeps_its_area_when_it_lands() {
    let errors = run_for(
        r#"pub fn init(this) {
    let floor = this.node.add_child("Floor");
    floor.set_component("transform", #{ position: [0.0, -0.8, 0.0] });
    floor.set_component("collider2d", #{ kind: physics2d::SHAPE_RECTANGLE, size: [10.0, 0.4] });
    let blob = this.node.get_node("Blob2d").softbody2d;
    blob.set_softbody(#{ kind: physics2d::SOFT_CIRCLE, radius: 0.5, particle_count: 24 });
    this.before = blob.softbody_area();
    this.ticks = 0;
}

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    if this.ticks == 90 {
        let now = this.node.get_node("Blob2d").softbody2d.softbody_area();
        assert!(now > this.before * 0.8, "the disk caved in when it landed");
        log::error("checked: the disk held");
    }
}
"#,
        95,
    );
    assert!(
        errors.len() == 1 && errors[0].contains("checked: the disk held"),
        "the check did not run clean: {errors:#?}"
    );
}

/// What the 2D blob hands the renderer after `script` ran and `ticks` passed.
fn solved_2d(script: &str, ticks: u32) -> balaur_core::mesh::SolvedPolygon {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (_dir, app) = boot(script, ticks);
    let world = app.engine.world();
    let node = balaur_core::ids::find(&world, app.engine.root(), "n_blob2d").expect("the 2D blob");
    let solved = world
        .get::<&balaur_core::mesh::SolvedPolygon>(node)
        .expect("the body handed nothing to the renderer");
    (*solved).clone()
}

/// A volumetric body carries the mesh it filled as a skin, so what it hands
/// over is that mesh's own vertices and triangles, which a `polygon` drawing
/// the same mesh deforms by, rather than the cells it simulates.
#[test]
fn a_2d_volumetric_body_hands_over_the_mesh_it_filled() {
    let solved = solved_2d(
        r##"pub fn init(this) {
    this.node.get_node("Blob2d").softbody2d.set_softbody(#{ kind: physics2d::SOFT_VOLUMETRIC, mesh: "#square", cell_size: 0.2 });
}
"##,
        20,
    );
    assert_eq!(solved.positions.len(), 4, "not the square's four corners");
    assert_eq!(
        solved.indices,
        vec![[0, 1, 2], [0, 2, 3]],
        "not the square's own triangles"
    );
    assert!(
        solved.positions.iter().all(|p| p[1] < 0.3),
        "the corners, from y = 0.5 at the top, did not fall with the cells: {:?}",
        solved.positions
    );
}

/// A rope has segments and no inside, so it is drawn as a strip along them.
#[test]
fn a_2d_rope_hands_over_a_ribbon() {
    let solved = solved_2d(
        r#"pub fn init(this) {
    this.node.get_node("Blob2d").softbody2d.set_softbody(#{ kind: physics2d::SOFT_ROPE, a: [0.0, 0.0], b: [1.0, 0.0], particle_count: 5 });
}
"#,
        2,
    );
    assert_eq!(solved.positions.len(), 10, "two vertices a particle");
    assert_eq!(solved.indices.len(), 8, "two triangles a segment");
}

/// A `polygon` body is its outline, and a vertex inside it is held by the
/// mesh's own triangle edges: left free it fell through the floor alone.
#[test]
fn a_2d_polygon_body_keeps_a_vertex_inside_its_outline() {
    let solved = solved_2d(
        r##"pub fn init(this) {
    let floor = this.node.add_child("Floor");
    floor.set_component("transform", #{ position: [0.0, -0.8, 0.0] });
    floor.set_component("collider2d", #{ kind: physics2d::SHAPE_RECTANGLE, size: [10.0, 0.4] });
    this.node.get_node("Blob2d").softbody2d.set_softbody(#{ kind: physics2d::SOFT_POLYGON, mesh: "#hub", particle_radius: 0.05 });
}
"##,
        90,
    );
    let [x, y] = solved.positions[4];
    let low = solved.positions[..4]
        .iter()
        .map(|p| p[1])
        .fold(f32::MAX, f32::min);
    let high = solved.positions[..4]
        .iter()
        .map(|p| p[1])
        .fold(f32::MIN, f32::max);
    assert!(
        x.abs() < 0.3 && y > low && y < high,
        "the hub left its outline: {:?}",
        solved.positions
    );
}

/// A body with nothing of its own to deform is drawn in its `color`.
#[test]
fn a_generated_body_is_drawn_in_its_color() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (_dir, app) = boot(
        r#"pub fn init(this) {
    this.node.get_node("Blob2d").softbody2d.color = [0.9, 0.3, 0.2, 1.0];
}
"#,
        3,
    );
    let world = app.engine.world();
    let node = balaur_core::ids::find(&world, app.engine.root(), "n_blob2d").expect("the 2D blob");
    let drawn = world
        .get::<&balaur::render::Renderable2d>(node)
        .expect("the generated body draws nothing");
    let want = [0.9, 0.3, 0.2, 1.0];
    assert!(
        drawn
            .color
            .iter()
            .zip(want)
            .all(|(a, b)| (a - b).abs() < 1e-6),
        "drawn in {:?}",
        drawn.color
    );
}

/// A ray reads the node out of the collider it met, and a soft body's
/// colliders are made by rapier, not by the node's own collider component.
#[test]
fn a_ray_that_meets_a_soft_body_names_its_node() {
    let errors = run_for(
        r#"pub fn init(this) { this.ticks = 0; }

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    if this.ticks == 2 {
        let hit = physics3d::raycast(#{ origin: [0.0, 5.0, 0.0], direction: [0.0, -1.0, 0.0], max_distance: 20.0 });
        assert!(hit is Object, "the ray passed through the soft body");
        assert!(hit.node == this.node, "the ray met a collider that names no node");
        log::error("checked: the ray named the body");
    }
}
"#,
        4,
    );
    assert!(
        errors.len() == 1 && errors[0].contains("checked: the ray named the body"),
        "the check did not run clean: {errors:#?}"
    );
}

/// A tear makes each piece a soft body of its own: it draws with the node
/// and goes when the node does, rather than falling on unseen.
#[test]
fn a_torn_off_piece_is_drawn_and_freed_with_its_node() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (_dir, mut app) = boot(
        r"pub fn init(this) {
    this.node.softbody3d.set_softbody(#{
        kind: physics3d::SOFT_ROPE, a: [0.0, 0.0, 0.0], b: [0.0, -2.0, 0.0], particle_count: 12,
        pinned_particles: [0], tear_strain: 0.05, tear_force: 2.0, edge_frequency: 4.0, mass: 400.0,
    });
}
",
        45,
    );
    let node = {
        let world = app.engine.world();
        balaur_core::ids::find(&world, app.engine.root(), "n_blob").expect("the blob")
    };
    {
        let state = app.engine.resource::<balaur_physics::PhysicsState3d>();
        let state = state.borrow();
        let handle = state.soft_bodies[&node];
        let kept = state.world.soft_bodies.get(handle).unwrap().num_particles();
        let everything: usize = state
            .world
            .soft_bodies
            .iter()
            .map(|(_, b)| b.num_particles())
            .sum();
        assert!(everything > kept, "the rope never came apart into pieces");
        let world = app.engine.world();
        let solved = world.get::<&balaur_core::mesh::SolvedMesh>(node).unwrap();
        assert!(
            solved.positions.len() > kept,
            "only the piece the node kept is drawn: {} of {everything}",
            solved.positions.len()
        );
    }
    balaur_core::scene::free_subtree(&mut app.engine.world_mut(), node);
    app.tick(1.0 / 60.0);
    let state = app.engine.resource::<balaur_physics::PhysicsState3d>();
    assert_eq!(
        state.borrow().world.soft_bodies.len(),
        0,
        "a piece outlived its node"
    );
}

/// A material row tuned on a live body takes effect where the body is: a
/// stiffness changed mid-fall does not snap it back to where it was built.
#[test]
fn a_material_change_does_not_rebuild_the_body_from_rest() {
    let errors = run_for(
        r#"pub fn init(this) { this.ticks = 0; this.before = 0.0; }

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    let body = this.node.softbody3d;
    if this.ticks == 6 {
        this.before = body.softbody_position(0).y;
        body.edge_frequency = 20.0;
    }
    if this.ticks == 7 {
        assert!(body.edge_frequency == 20.0, "the stiffness did not take");
        assert!(body.softbody_position(0).y <= this.before + 0.0001, "the change snapped the body back to rest");
        log::error("checked: tuned in place");
    }
}
"#,
        9,
    );
    assert!(
        errors.len() == 1 && errors[0].contains("checked: tuned in place"),
        "the check did not run clean: {errors:#?}"
    );
}

/// The calls a script uses to hold, drag, tie and push a body, checked over
/// one script that logs once it reached its end.
fn run_checked(script: &str, ticks: u32, done: &str) {
    let errors = run_for(script, ticks);
    assert!(
        errors.len() == 1 && errors[0].contains(done),
        "the check did not run clean: {errors:#?}"
    );
}

#[test]
fn pinning_a_free_particle_holds_it_and_unpinning_lets_it_fall() {
    run_checked(
        r#"pub fn init(this) {
    let body = this.node.softbody3d;
    body.set_softbody(#{ kind: physics3d::SOFT_ROPE, particle_count: 8 });
    body.pin_particle(0);
    this.first = body.softbody_position(0).y;
    this.ticks = 0;
}

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    let body = this.node.softbody3d;
    if this.ticks == 4 {
        assert!((body.softbody_position(0).y - this.first).abs() < 0.001, "the pinned particle fell");
        assert!(body.softbody_position(7).y < this.first, "the free end did not fall");
        body.unpin_particle(0);
    }
    if this.ticks == 10 {
        assert!(body.softbody_position(0).y < this.first - 0.001, "the released particle stayed put");
        log::error("checked: held and let go");
    }
}
"#,
        12,
        "checked: held and let go",
    );
}

#[test]
fn a_held_particle_is_dragged_to_its_target() {
    run_checked(
        r#"pub fn init(this) {
    let body = this.node.softbody3d;
    body.set_softbody(#{ kind: physics3d::SOFT_ROPE, particle_count: 8, pinned_particles: [0] });
    body.set_particle_target(0, [2.0, 1.0, 0.0]);
    this.ticks = 0;
}

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    if this.ticks == 3 {
        let at = this.node.softbody3d.softbody_position(0);
        assert!((at.x - 2.0).abs() < 0.01 && (at.y - 1.0).abs() < 0.01, "the held particle did not move to its target");
        log::error("checked: dragged");
    }
}
"#,
        5,
        "checked: dragged",
    );
}

#[test]
fn an_impulse_kicks_the_body_and_the_edges_report_their_stress() {
    run_checked(
        r#"pub fn init(this) {
    let body = this.node.softbody3d;
    body.set_softbody(#{ kind: physics3d::SOFT_ROPE, particle_count: 8, pinned_particles: [0] });
    body.apply_softbody_impulse([0.0, 20.0, 0.0]);
    this.first = body.softbody_position(7).y;
    this.ticks = 0;
}

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    let body = this.node.softbody3d;
    if this.ticks == 2 {
        assert!(body.softbody_position(7).y > this.first, "the kick did not lift the free end");
        assert!(body.softbody_velocity(7).y > 0.0, "and it is not moving up");
        let edges = body.softbody_edges();
        assert!(edges.len() > 0 && edges.len() == body.softbody_stress().len(), "the edges and their stress do not line up");
        assert!(edges[0].len() == 2, "an edge is two particle indices");
        log::error("checked: kicked");
    }
}
"#,
        4,
        "checked: kicked",
    );
}

#[test]
fn a_particle_tied_to_a_body_hangs_from_it() {
    run_checked(
        r#"pub fn init(this) {
    let hook = this.node.add_child("Hook");
    hook.set_component("transform", #{ position: [0.0, 0.0, 0.0] });
    hook.set_component("body3d", #{ kind: physics3d::BODY_STATIC });
    let body = this.node.softbody3d;
    body.set_softbody(#{ kind: physics3d::SOFT_ROPE, particle_count: 8 });
    this.hook = hook;
    this.ticks = 0;
    this.first = body.softbody_position(0).y;
}

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    let body = this.node.softbody3d;
    if this.ticks == 1 {
        body.attach_particle(0, this.hook);
    }
    if this.ticks == 20 {
        assert!(body.softbody_position(0).y > this.first - 0.2, "the tied particle fell away from the body");
        assert!(body.detach_particle(0), "the particle was not attached");
        log::error("checked: tied");
    }
}
"#,
        22,
        "checked: tied",
    );
}

#[test]
fn the_2d_body_takes_the_same_calls() {
    run_checked(
        r#"pub fn init(this) {
    let body = this.node.get_node("Blob2d").softbody2d;
    body.set_softbody(#{ kind: physics2d::SOFT_ROPE, particle_count: 6 });
    body.pin_particle(0);
    body.apply_softbody_radial_impulse([0.0, 0.0], 5.0, 0.0);
    this.ticks = 0;
}

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    if this.ticks == 2 {
        let body = this.node.get_node("Blob2d").softbody2d;
        assert!(body.softbody_edges().len() == body.softbody_stress().len(), "the edges and their stress do not line up");
        assert!(!body.softbody_sleeping(), "a body just struck is asleep");
        log::error("checked: 2d");
    }
}
"#,
        4,
        "checked: 2d",
    );
}

#[test]
fn per_particle_and_per_edge_rows_build_and_name_what_they_cannot() {
    run_checked(
        r#"pub fn init(this) {
    let body = this.node.softbody3d;
    body.set_softbody(#{
        kind: physics3d::SOFT_ROPE, particle_count: 4,
        masses: [1.0, 1.0, 1.0, 50.0],
        tear_resistance: [#{ a: 1, b: 2, resistance: 0.2 }],
        edge_springs: [#{ a: 0, b: 1, frequency: 90.0, damping: 1.0 }],
    });
    assert!(body.softbody_particles() == 4, "the rows changed the layout");
    log::error("checked: rows built");
}
"#,
        1,
        "checked: rows built",
    );
    let short = run(r"pub fn init(this) {
    this.node.softbody3d.set_softbody(#{ kind: physics3d::SOFT_ROPE, particle_count: 4, masses: [1.0, 2.0] });
}
");
    assert!(short.iter().any(|e| e.contains("masses")), "{short:#?}");
    let stray = run(r"pub fn init(this) {
    this.node.softbody3d.set_softbody(#{ kind: physics3d::SOFT_ROPE, particle_count: 4, tear_resistance: [#{ a: 0, b: 3, resistance: 0.5 }] });
}
");
    assert!(
        stray
            .iter()
            .any(|e| e.contains("no edge joins particles 0 and 3")),
        "{stray:#?}"
    );
}

#[test]
fn a_body_that_does_not_collide_lets_a_ray_through() {
    run_checked(
        r#"pub fn init(this) {
    this.node.softbody3d.set_softbody(#{ kind: physics3d::SOFT_BOX, cells: [2.0, 2.0, 2.0], collides: false });
    this.ticks = 0;
}

pub fn fixed_update(this, dt) {
    this.ticks = this.ticks + 1;
    if this.ticks == 2 {
        let hit = physics3d::raycast(#{ origin: [0.0, 5.0, 0.0], direction: [0.0, -1.0, 0.0], max_distance: 20.0 });
        assert!(!(hit is Object), "the ray met a body that does not collide");
        log::error("checked: passed through");
    }
}
"#,
        4,
        "checked: passed through",
    );
}

#[test]
fn a_woven_cloth_and_a_2d_skin_collision_build() {
    run_checked(
        r##"pub fn init(this) {
    let cloth = this.node.softbody3d;
    cloth.set_softbody(#{ kind: physics3d::SOFT_CLOTH, cells: [4.0, 4.0, 1.0], warp_frequency: 80.0, weft_frequency: 10.0 });
    assert!(cloth.softbody_particles() == 25, "a woven 4x4 cloth is not 5x5 particles");
    let flat = this.node.get_node("Blob2d").softbody2d;
    flat.set_softbody(#{ kind: physics2d::SOFT_VOLUMETRIC, mesh: "#square", cell_size: 0.2, skin_collision: true });
    assert!(flat.softbody_particles() > 4, "the skinned 2D body has no cells");
    log::error("checked: woven");
}
"##,
        1,
        "checked: woven",
    );
}
