//! The soft-body rows past the generators' defaults: custom layouts, seams,
//! listed tension-only edges, the surface collider's rows and their live
//! edits, and what a script reads off particles, edges and cells.

use balaur::standard_app;
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{App, components};
use balaur_physics::{PhysicsPlugin, PhysicsState2d, PhysicsState3d};

use crate::LOG;

fn app() -> App {
    let mut app = App::new(balaur_core::AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    app
}

fn node(app: &App, name: &str, at: [f32; 3]) -> Entity {
    let root = app.engine.root();
    let e = scene::spawn_node(&mut app.engine.world_mut(), name, root);
    app.engine
        .world()
        .get::<&mut Transform>(e)
        .unwrap()
        .position = at.into();
    e
}

fn add(app: &App, e: Entity, component: &str, text: &str) -> anyhow::Result<()> {
    components::add(
        &app.engine,
        e,
        component,
        Some(&toml::from_str(text).unwrap()),
    )
}

/// The 3D soft body a table builds, read with `f`.
fn built3<T>(text: &str, f: impl FnOnce(&balaur_physics::rapier3d::prelude::SoftBody) -> T) -> T {
    let app = app();
    let e = node(&app, "Soft", [0.0; 3]);
    add(&app, e, "softbody3d", text).unwrap();
    let state = app.engine.resource::<PhysicsState3d>();
    let state = state.borrow();
    f(&state.world.soft_bodies[state.soft_bodies[&e]])
}

fn built2<T>(text: &str, f: impl FnOnce(&balaur_physics::rapier2d::prelude::SoftBody) -> T) -> T {
    let app = app();
    let e = node(&app, "Soft", [0.0; 3]);
    add(&app, e, "softbody2d", text).unwrap();
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    f(&state.world.soft_bodies[state.soft_bodies[&e]])
}

fn joins(edge: [u32; 2], a: u32, b: u32) -> bool {
    edge == [a, b] || edge == [b, a]
}

const TETRA: &str = "kind = \"custom\"\npoints = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]";

#[test]
fn a_custom_body_is_built_from_the_points_and_elements_it_lists() {
    let (particles, cells, edges) =
        built3(&format!("{TETRA}\ncell_indices = [[0, 1, 2, 3]]"), |b| {
            (b.num_particles(), b.cells().len(), b.edges().len())
        });
    assert_eq!(
        (particles, cells, edges),
        (4, 1, 6),
        "the cell's six edges come from rapier"
    );
    let wired = built3(
        &format!("{TETRA}\nwire_indices = [[0, 1], [1, 2]]\nedge_indices = [[0, 1], [1, 2]]"),
        |b| {
            (
                b.edges().len(),
                b.meshes()
                    .any(balaur_physics::rapier3d::dynamics::SoftCollisionMesh::is_wire),
            )
        },
    );
    assert_eq!(
        wired,
        (2, true),
        "the listed edges and the wire were not taken"
    );
    let flat = built2(
        "kind = \"custom\"\npoints = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]\ncell_indices = [[0, 1, 2]]",
        |b| (b.num_particles(), b.cells().len()),
    );
    assert_eq!(flat, (3, 1));
}

#[test]
fn a_custom_index_past_its_points_is_refused() {
    let app = app();
    let e = node(&app, "Soft", [0.0; 3]);
    let why = add(
        &app,
        e,
        "softbody3d",
        &format!("{TETRA}\ncell_indices = [[0, 1, 2, 9]]"),
    )
    .expect_err("an index past the points was taken");
    assert!(format!("{why:#}").contains("cell_indices"), "{why:#}");
    let why = add(
        &app,
        e,
        "softbody3d",
        "kind = \"custom\"\npoints = [[0.0, 0.0, 0.0]]",
    )
    .expect_err("a custom body with nothing joining its points was taken");
    assert!(format!("{why:#}").contains("points"), "{why:#}");
}

const ROPE: &str = "kind = \"rope\"\nparticle_count = 4";

#[test]
fn a_seam_joins_particles_the_layout_left_apart() {
    let plain = built3(ROPE, |b| b.edges().iter().any(|e| joins(e.vertices, 0, 3)));
    assert!(!plain, "the rope already joined its two ends");
    let sewn = built3(&format!("{ROPE}\nseams = [{{ a = 0, b = 3 }}]"), |b| {
        b.edges().iter().any(|e| joins(e.vertices, 0, 3))
    });
    assert!(sewn, "the seam made no edge between the rope's ends");
}

#[test]
fn listed_tension_only_edges_are_the_only_slack_ones() {
    let slack = |text: &str| {
        built3(text, |b| {
            b.edges()
                .iter()
                .filter(|e| e.tension_only)
                .map(|e| e.vertices)
                .collect::<Vec<_>>()
        })
    };
    let listed = slack(&format!(
        "{ROPE}\ntension_only = \"listed\"\ntension_only_edges = [{{ a = 1, b = 0 }}]"
    ));
    assert!(listed.len() == 1 && joins(listed[0], 0, 1), "{listed:?}");
    let all = built3(&format!("{ROPE}\ntension_only = \"all\""), |b| {
        b.edges().iter().all(|e| e.tension_only)
    });
    assert!(all, "tension_only = all left an edge resisting compression");
    let none = slack(&format!(
        "{ROPE}\ntension_only_edges = [{{ a = 0, b = 1 }}]"
    ));
    assert!(
        none.is_empty(),
        "edges were listed without `listed`: {none:?}"
    );
}

#[test]
fn an_edge_spring_reaches_a_bending_edge() {
    let spring = built3(
        &format!("{ROPE}\nedge_springs = [{{ a = 0, b = 2, hz = 7.0, damping = 0.5 }}]"),
        |b| {
            b.edges()
                .iter()
                .find(|e| joins(e.vertices, 0, 2))
                .and_then(|e| e.softness)
                .map(|s| (s.natural_frequency, s.damping_ratio))
        },
    );
    assert_eq!(
        spring,
        Some((7.0, 0.5)),
        "the bending edge kept the material's spring"
    );
}

#[test]
fn a_tube_narrows_to_its_end_radius_and_cloth_takes_its_woven_damping() {
    let ends = built3(
        "kind = \"cloth_tube\"\nradius = 0.5\nend_radius = 0.25\ncells = [8.0, 4.0, 1.0]",
        |b| {
            let radial = |p: balaur_physics::rapier3d::math::Vector| (p.x * p.x + p.z * p.z).sqrt();
            let rings: Vec<f32> = b.particle_positions().map(radial).collect();
            let (first, last) = (rings[0], rings[rings.len() - 1]);
            (first, last)
        },
    );
    assert!(
        (ends.0 - 0.5).abs() < 1e-3 && (ends.1 - 0.25).abs() < 1e-3,
        "{ends:?}"
    );
    let damped = built3(
        "kind = \"cloth\"\ncells = [3.0, 3.0, 1.0]\nwarp_damping = 0.3",
        |b| {
            b.edges()
                .iter()
                .filter_map(|e| e.softness)
                .any(|s| (s.damping_ratio - 0.3).abs() < 1e-6)
        },
    );
    assert!(damped, "no warp spring took the warp damping");
}

#[test]
fn a_2d_outline_is_rapiers_polygon_of_its_points() {
    let (particles, pieces) = built2(
        "kind = \"outline\"\npoints = [[0.0, 0.0], [1.0, 0.0], [1.5, 1.0], [0.5, 1.5], [-0.5, 1.0]]",
        |b| (b.num_particles(), b.volume_pieces().len()),
    );
    assert_eq!(
        (particles, pieces),
        (5, 1),
        "the outline is not one held area of five corners"
    );
}

#[test]
fn switching_a_soft_body_off_and_editing_its_surface_keep_it_built() {
    let mut app = app();
    let e = node(&app, "Soft", [0.0, 2.0, 0.0]);
    add(
        &app,
        e,
        "softbody3d",
        "kind = \"box\"\ncells = [1.0, 1.0, 1.0]",
    )
    .unwrap();
    let handle = |app: &App| {
        let state = app.engine.resource::<PhysicsState3d>();
        state.borrow().soft_bodies[&e]
    };
    let before = handle(&app);
    let patch = |app: &App, text: &str| {
        components::patch(&app.engine, e, "softbody3d", &toml::from_str(text).unwrap()).unwrap();
    };
    patch(&app, "enabled = false\nfriction = 0.9\nsensor = true");
    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    assert!(handle(&app) == before, "the edits built the body again");
    let state = app.engine.resource::<PhysicsState3d>();
    let state = state.borrow();
    let body = &state.world.soft_bodies[before];
    assert!(!body.is_enabled(), "the body was not switched off");
    let y = body.center_of_mass().y;
    assert!((y - 2.0).abs() < 0.01, "a switched-off body fell to {y}");
    let collider = body
        .meshes()
        .next()
        .map(balaur_physics::rapier3d::dynamics::SoftCollisionMesh::collider)
        .unwrap();
    let collider = &state.world.colliders[collider];
    assert!(
        (collider.friction() - 0.9).abs() < 1e-6,
        "friction {}",
        collider.friction()
    );
    assert!(collider.is_sensor(), "the collider is not a sensor");
}

/// Run a project of the floor and the scripted soft body below, and pass
/// when the only error it logged is the script's `done` line.
fn run_checked(body: &str, script: &str, ticks: u32, done: &str) {
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
        format!(
            r#"[[assets]]
id = "cube"
type = "mesh"
positions = [[-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, 0.5, -0.5], [-0.5, 0.5, -0.5], [-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.5, 0.5, 0.5], [-0.5, 0.5, 0.5]]
indices = [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [2, 3, 7], [2, 7, 6], [1, 2, 6], [1, 6, 5], [0, 4, 7], [0, 7, 3]]

[[assets]]
id = "square"
type = "mesh"
positions = [[-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.5, 0.5, 0.0], [-0.5, 0.5, 0.0]]
indices = [[0, 1, 2], [0, 2, 3]]

[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_floor"
name = "Floor"
parent = "n_world"

[nodes.transform]
position = [0.0, -1.0, 0.0]

[nodes.collider3d]
kind = "box"
size = [20.0, 1.0, 20.0]

[[nodes]]
id = "n_post"
name = "Post"
parent = "n_world"
body3d = {{ kind = "static" }}

[nodes.transform]
position = [3.0, 0.0, 0.0]

[[nodes]]
id = "n_flat"
name = "Flat"
parent = "n_world"

[nodes.softbody2d]
kind = "grid"
cells = [1.0, 1.0]

[[nodes]]
id = "n_soft"
name = "Soft"
parent = "n_world"
script = {{ source = "scripts/s.bs" }}

[nodes.softbody3d]
{body}
"#
        ),
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/s.bs"), script).unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(crate::dev_config(dir.path())).unwrap();
    app.load_project().unwrap();
    for _ in 0..ticks {
        app.tick(1.0 / 60.0);
    }
    let errors: Vec<String> = balaur_core::logbuf::recent(500)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect();
    assert!(
        errors.len() == 1 && errors[0].contains(done),
        "the check did not run clean: {errors:#?}"
    );
}

#[test]
fn a_sensor_soft_body_passes_through_the_floor() {
    run_checked(
        "kind = \"box\"\ncells = [1.0, 1.0, 1.0]\nsensor = true",
        r#"pub struct S { node: Node, ticks: int }
impl S {
    fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 90 {
            let y = self.node.softbody3d.softbody_center().at(1).as_float();
            if y >= -1.0 { log::error("the sensor body stopped on the floor at {y}"); }
            log::error("checked: sensor");
        }
    }
}
"#,
        92,
        "checked: sensor",
    );
}

#[test]
fn a_volumetric_crust_holds_fewer_particles_than_a_solid_fill() {
    run_checked(
        "kind = \"box\"\ncells = [1.0, 1.0, 1.0]",
        r##"pub struct S { node: Node }
impl S {
    fn init() {
        let n = self.node;
        n.softbody3d.set_softbody(map().with("kind", physics3d::SOFT_VOLUMETRIC).with("mesh", "#cube").with("cell_size", 0.2));
        let solid = n.softbody3d.softbody_particles();
        n.softbody3d.set_softbody(map().with("kind", physics3d::SOFT_VOLUMETRIC).with("mesh", "#cube").with("cell_size", 0.2).with("fill", physics3d::FILL_SURFACE));
        let crust = n.softbody3d.softbody_particles();
        if crust >= solid { log::error("the crust held {crust} particles against the fill's {solid}"); }
        n.softbody3d.set_softbody(map().with("kind", physics3d::SOFT_BOX).with("cells", [1.0, 1.0, 1.0]).with("mesh", "#cube").with("skin", true));
        if n.softbody3d.softbody_particles() != 8 { log::error("the skin changed the box's particles"); }
        log::error("checked: fills");
    }
}
"##,
        1,
        "checked: fills",
    );
}

#[test]
fn a_script_reads_every_particle_edge_and_cell_row() {
    run_checked(
        "kind = \"box\"\ncells = [1.0, 1.0, 1.0]\ntear_strain = 10.0",
        r#"pub struct S { node: Node }
impl S {
    fn init() {
        let n = self.node;
        let p = n.softbody3d.softbody_particle(0);
        let mass = p.get("mass").as_float();
        let inverse = p.get("inverse_mass").as_float();
        if !(mass > 0.0 && inverse > 0.0) { log::error("particle mass {mass} {inverse}"); }
        if !(!p.get("pinned").as_bool() && p.get("on_surface").as_bool() && !p.get("damaged").as_bool()) { log::error("particle flags"); }
        if !p.get("target").is_nil() { log::error("a free particle has a target"); }
        n.softbody3d.pin_particle(0);
        n.softbody3d.set_particle_target(0, [0.0, 0.5, 0.0]);
        if n.softbody3d.softbody_particle(0).get("inverse_mass").as_float() != 0.0 { log::error("a held particle still has an inverse mass"); }
        if n.softbody3d.softbody_particle(0).get("target").at(1).as_float() != 0.5 { log::error("the target was not kept"); }
        n.softbody3d.add_particle_force(1, [0.0, 0.0, 3.0]);
        if n.softbody3d.softbody_particle(1).get("force").at(2).as_float() != 3.0 { log::error("the particle force was not kept"); }
        let edge = n.softbody3d.softbody_edge(0);
        let rest = edge.get("rest_length").as_float();
        if !(rest > 0.0 && edge.get("initial_rest_length").as_float() == rest) { log::error("edge lengths"); }
        let kind = edge.get("kind").as_str();
        if kind != "structural" && kind != "bending" { log::error("edge kind {kind}"); }
        if !(edge.get("softness_hz").as_float() > 0.0 && edge.get("tear_resistance").as_float() == 1.0) { log::error("edge spring"); }
        let cells = n.softbody3d.softbody_cells();
        if !(cells.len() > 0 && cells[0].len() == 4) { log::error("cells"); }
        let cell = n.softbody3d.softbody_cell(0);
        if !(cell.get("rest_volume").as_float() > 0.0 && cell.get("stiffness_scale").as_float() == 1.0) { log::error("cell rows"); }
        let stretch = cell.get("plastic_stretch");
        if !(stretch.len() == 3 && stretch.at(0).at(0).as_float() == 1.0) { log::error("plastic stretch"); }
        if n.softbody3d.softbody_boundary().len() <= 0 { log::error("no boundary"); }
        let pieces = n.softbody3d.softbody_volume_pieces();
        if !(pieces.len() == 1 && pieces[0].get("rest_volume").as_float() > 0.0) { log::error("volume pieces"); }
        if !(n.softbody3d.softbody_particle_radius() > 0.0 && n.softbody3d.softbody_mass() > 0.0) { log::error("radius and mass"); }
        let post = scene::get_node("World/Post");
        n.softbody3d.attach_particle(2, post);
        let ties = n.softbody3d.softbody_attachments();
        if !(ties.len() == 1 && ties.at(0).get("particle").as_int() == 2 && ties.at(0).get("body").as_node() == post) { log::error("attachments"); }
        n.softbody3d.set_edge_tear_resistance(0, 3.0);
        if n.softbody3d.softbody_edge(0).get("tear_resistance").as_float() != 3.0 { log::error("tear resistance"); }
        n.softbody3d.set_particle_damaged(3, true);
        if !n.softbody3d.softbody_particle(3).get("damaged").as_bool() { log::error("damaged"); }
        if n.softbody3d.has_pending_tears() { log::error("tears pending before any"); }
        n.softbody3d.tear_edge(1);
        if !n.softbody3d.has_pending_tears() { log::error("tear_edge marked nothing"); }
        let contacts = n.softbody3d.softbody_contacts();
        if !(contacts.get("edges").len() == 0 && contacts.get("volumes").len() == 0) { log::error("contacts before a step"); }
        if n.softbody3d.softbody_dihedrals().len() != 0 { log::error("a box has no hinges"); }
        n.softbody3d.reset_plasticity();
        log::error("checked: rows");
    }
}
"#,
        1,
        "checked: rows",
    );
}

#[test]
fn a_cut_splits_the_body_and_its_pieces_stay_reachable() {
    run_checked(
        "kind = \"rope\"\nparticle_count = 6\ngravity_scale = 0.0",
        r#"pub struct S { node: Node }
impl S {
    fn init() {
        let n = self.node;
        let before = n.softbody3d.softbody_particles();
        let blade = [[-1.0, -0.5, -1.0], [1.0, -0.5, -1.0], [0.0, -0.5, 2.0]];
        let crossing = n.softbody3d.softbody_crossing(blade);
        if crossing.get("edges").len() <= 0 { log::error("the blade meets no edge"); }
        let records = n.softbody3d.cut_softbody(blade);
        let changed = records.len();
        if changed != 1 { log::error("the cut changed {changed} pieces"); }
        let pieces = records.at(0).get("pieces").as_int();
        if pieces != 2 { log::error("the cut left {pieces} pieces"); }
        if records.at(0).get("piece_particles").len() != 2 { log::error("piece particles"); }
        // Two particles go in where the blade crosses the rope, one a side.
        let after = n.softbody3d.softbody_particles();
        if after != before + 2 { log::error("the cut went from {before} particles to {after}"); }
        let last = n.softbody3d.softbody_position(after - 1);
        if !(last.y < 0.01 && last.y > -1.01) { log::error("the last particle is off the rope: {last.y}"); }
        log::error("checked: cut");
    }
}
"#,
        1,
        "checked: cut",
    );
}

#[test]
fn a_2d_polyline_takes_its_meshs_triangle_edges_when_asked() {
    run_checked(
        "kind = \"box\"\ncells = [1.0, 1.0, 1.0]",
        r##"pub struct S { node: Node }
impl S {
    fn init() {
        let flat = scene::get_node("World/Flat");
        flat.softbody2d.set_softbody(map().with("kind", physics2d::SOFT_POLYLINE).with("mesh", "#square"));
        let chain = flat.softbody2d.softbody_edges().len();
        flat.softbody2d.set_softbody(map().with("kind", physics2d::SOFT_POLYLINE).with("mesh", "#square").with("edges", physics2d::EDGES_MESH));
        let mesh = flat.softbody2d.softbody_edges().len();
        if mesh <= chain { log::error("the mesh's edges gave {mesh} against the chain's {chain}"); }
        flat.softbody2d.set_softbody(map().with("kind", physics2d::SOFT_VOLUMETRIC).with("mesh", "#square").with("cell_size", 0.2).with("min_angle", 0.3));
        if flat.softbody2d.softbody_particles() <= 4 { log::error("the fill added no particles"); }
        log::error("checked: polyline");
    }
}
"##,
        1,
        "checked: polyline",
    );
}
