//! The soft-body rows past the generators' defaults: custom layouts, seams,
//! listed tension-only edges, the surface collider's rows and their live
//! edits, and what a script reads off particles, edges and cells.

use balaur::{AppConfig, standard_app};
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
script = {{ source = "scripts/s.rn" }}

[nodes.softbody3d]
{body}
"#
        ),
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
    let errors: Vec<String> = balaur_core::logbuf::recent(80)
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
        r#"pub fn init(this) { this.ticks = 0; }

pub fn fixed_update(this, dt) {
    this.ticks += 1;
    if this.ticks == 90 {
        let y = this.node.softbody3d.softbody_center().y;
        assert!(y < -1.0, "the sensor body stopped on the floor at {}", y);
        log::error("checked: sensor");
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
        r##"pub fn init(this) {
    let body = this.node.softbody3d;
    body.set_softbody(#{ kind: physics3d::SOFT_VOLUMETRIC, mesh: "#cube", cell_size: 0.2 });
    let solid = body.softbody_particles();
    body.set_softbody(#{ kind: physics3d::SOFT_VOLUMETRIC, mesh: "#cube", cell_size: 0.2, fill: physics3d::FILL_SURFACE });
    let crust = body.softbody_particles();
    assert!(crust < solid, "the crust held {} particles against the fill's {}", crust, solid);
    body.set_softbody(#{ kind: physics3d::SOFT_BOX, cells: [1.0, 1.0, 1.0], mesh: "#cube", skin: true });
    assert!(body.softbody_particles() == 8, "the skin changed the box's particles");
    log::error("checked: fills");
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
        r#"pub fn init(this) {
    let body = this.node.softbody3d;
    let p = body.softbody_particle(0);
    assert!(p.mass > 0.0 && p.inverse_mass > 0.0, "particle mass {} {}", p.mass, p.inverse_mass);
    assert!(!p.pinned && p.on_surface && !p.damaged, "particle flags");
    assert!(p.target == (), "a free particle has a target");
    body.pin_particle(0);
    body.set_particle_target(0, [0.0, 0.5, 0.0]);
    assert!(body.softbody_particle(0).inverse_mass == 0.0, "a held particle still has an inverse mass");
    assert!(body.softbody_particle(0).target.y == 0.5, "the target was not kept");
    body.add_particle_force(1, [0.0, 0.0, 3.0]);
    assert!(body.softbody_particle(1).force.z == 3.0, "the particle force was not kept");
    let edge = body.softbody_edge(0);
    assert!(edge.rest_length > 0.0 && edge.initial_rest_length == edge.rest_length, "edge lengths");
    assert!(edge.kind == "structural" || edge.kind == "bending", "edge kind {}", edge.kind);
    assert!(edge.softness_hz > 0.0 && edge.tear_resistance == 1.0, "edge spring");
    let cells = body.softbody_cells();
    assert!(cells.len() > 0 && cells[0].len() == 4, "cells");
    let cell = body.softbody_cell(0);
    assert!(cell.rest_volume > 0.0 && cell.stiffness_scale == 1.0, "cell rows");
    assert!(cell.plastic_stretch.len() == 3 && cell.plastic_stretch[0].x == 1.0, "plastic stretch");
    assert!(body.softbody_boundary().len() > 0, "no boundary");
    let pieces = body.softbody_volume_pieces();
    assert!(pieces.len() == 1 && pieces[0].rest_volume > 0.0, "volume pieces");
    assert!(body.softbody_particle_radius() > 0.0 && body.softbody_mass() > 0.0, "radius and mass");
    body.attach_particle(2, scene::get_node("World/Post"));
    let ties = body.softbody_attachments();
    assert!(ties.len() == 1 && ties[0].particle == 2 && ties[0].body == scene::get_node("World/Post"), "attachments");
    body.set_edge_tear_resistance(0, 3.0);
    assert!(body.softbody_edge(0).tear_resistance == 3.0, "tear resistance");
    body.set_particle_damaged(3, true);
    assert!(body.softbody_particle(3).damaged, "damaged");
    assert!(!body.has_pending_tears(), "tears pending before any");
    body.tear_edge(1);
    assert!(body.has_pending_tears(), "tear_edge marked nothing");
    let contacts = body.softbody_contacts();
    assert!(contacts.edges.len() == 0 && contacts.volumes.len() == 0, "contacts before a step");
    assert!(body.softbody_dihedrals().len() == 0, "a box has no hinges");
    body.reset_plasticity();
    log::error("checked: rows");
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
        r#"pub fn init(this) {
    let body = this.node.softbody3d;
    let before = body.softbody_particles();
    let blade = [[-1.0, -0.5, -1.0], [1.0, -0.5, -1.0], [0.0, -0.5, 2.0]];
    let crossing = body.softbody_crossing(blade);
    assert!(crossing.edges.len() > 0, "the blade meets no edge");
    let records = body.cut_softbody(blade);
    assert!(records.len() == 1, "the cut changed {} pieces", records.len());
    assert!(records[0].pieces == 2, "the cut left {} pieces", records[0].pieces);
    assert!(records[0].piece_particles.len() == 2, "piece particles");
    // Two particles go in where the blade crosses the rope, one a side.
    let after = body.softbody_particles();
    assert!(after == before + 2, "the cut went from {} particles to {}", before, after);
    let last = body.softbody_position(after - 1);
    assert!(last.y < 0.01 && last.y > -1.01, "the last particle is off the rope: {}", last.y);
    log::error("checked: cut");
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
        r##"pub fn init(this) {
    let flat = scene::get_node("World/Flat").softbody2d;
    flat.set_softbody(#{ kind: physics2d::SOFT_POLYLINE, mesh: "#square" });
    let chain = flat.softbody_edges().len();
    flat.set_softbody(#{ kind: physics2d::SOFT_POLYLINE, mesh: "#square", edges: physics2d::EDGES_MESH });
    let mesh = flat.softbody_edges().len();
    assert!(mesh > chain, "the mesh's edges gave {} against the chain's {}", mesh, chain);
    flat.set_softbody(#{ kind: physics2d::SOFT_VOLUMETRIC, mesh: "#square", cell_size: 0.2, min_angle: 0.3 });
    assert!(flat.softbody_particles() > 4, "the fill added no particles");
    log::error("checked: polyline");
}
"##,
        1,
        "checked: polyline",
    );
}
