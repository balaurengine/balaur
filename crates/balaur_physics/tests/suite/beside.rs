//! The components beside a collider, a soft body and a joint: the rows only
//! some of the main component's kinds read, each a component of its own, read
//! by the main one's build and at their defaults when the node has none.

use balaur_core::components::StableId;
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{App, components, snapshot};
use balaur_physics::{PhysicsPlugin, PhysicsState2d, PhysicsState3d};

fn app() -> App {
    let mut app = App::new(balaur_core::AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin).unwrap();
    app
}

fn node(app: &App, name: &str, at: [f32; 3]) -> Entity {
    let root = app.engine.root();
    let e = scene::spawn_node(&mut app.engine.world_mut(), name, root);
    app.engine
        .world_mut()
        .insert_one(e, StableId(format!("n_{name}")))
        .unwrap();
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

fn particles(app: &App, e: Entity) -> usize {
    let state = app.engine.resource::<PhysicsState3d>();
    let state = state.borrow();
    state.world.soft_bodies[state.soft_bodies[&e]].num_particles()
}

#[test]
fn a_rope_without_its_beside_component_is_built_at_the_defaults() {
    let app = app();
    let e = node(&app, "Rope", [0.0; 3]);
    add(&app, e, "softbody3d", "kind = \"rope\"").unwrap();
    assert_eq!(particles(&app, e), 16, "a bare rope is not 16 particles");
}

#[test]
fn writing_a_beside_component_builds_the_main_one_again() {
    let app = app();
    let e = node(&app, "Rope", [0.0; 3]);
    add(&app, e, "softbody3d", "kind = \"rope\"").unwrap();
    add(&app, e, "soft_rope3d", "particle_count = 8").unwrap();
    assert_eq!(
        particles(&app, e),
        8,
        "adding soft_rope3d left the rope as it was"
    );
    components::patch(
        &app.engine,
        e,
        "soft_rope3d",
        &toml::from_str("particle_count = 5").unwrap(),
    )
    .unwrap();
    assert_eq!(
        particles(&app, e),
        5,
        "a patch of soft_rope3d left the rope as it was"
    );
}

#[test]
fn removing_a_beside_component_builds_the_main_one_at_its_defaults() {
    let app = app();
    let e = node(&app, "Rope", [0.0; 3]);
    add(&app, e, "soft_rope3d", "particle_count = 8").unwrap();
    add(&app, e, "softbody3d", "kind = \"rope\"").unwrap();
    assert_eq!(
        particles(&app, e),
        8,
        "the rope was not built with its beside rows"
    );
    components::remove(&app.engine, e, "soft_rope3d").unwrap();
    assert_eq!(
        particles(&app, e),
        16,
        "the rope kept rows that were removed"
    );
}

#[test]
fn a_beside_component_the_kind_does_not_read_changes_nothing_and_says_so() {
    let app = app();
    let e = node(&app, "Block", [0.0; 3]);
    add(&app, e, "softbody3d", "kind = \"box\"").unwrap();
    let before = particles(&app, e);
    add(&app, e, "soft_rope3d", "particle_count = 3").unwrap();
    assert_eq!(particles(&app, e), before, "a box read a rope's rows");
    let found = balaur_core::warnings::warnings(&app.engine, e);
    assert!(
        found
            .iter()
            .any(|w| w.component == "soft_rope3d" && w.warning.message.contains("reads none")),
        "{found:?}"
    );
}

#[test]
fn a_moved_row_on_the_main_component_is_refused_by_name() {
    let app = app();
    let e = node(&app, "Shape", [0.0; 3]);
    for (component, row) in [
        ("collider3d", "fit = \"aabb\""),
        ("collider2d", "overlap = 0.5"),
        ("softbody3d", "cells = [2.0, 2.0, 2.0]"),
        ("softbody2d", "particle_count = 8"),
        ("joint3d", "max_length = 1.0"),
        ("joint2d", "articulation = true"),
    ] {
        let why = add(&app, e, component, row).expect_err(row);
        let name = row.split(' ').next().unwrap();
        assert!(format!("{why:#}").contains(name), "{component}: {why:#}");
    }
}

/// A scene names the beside component in any order: it is applied first, so
/// a rope pinned at a particle only its own count reaches is built once,
/// whole.
#[test]
fn a_scene_builds_the_main_component_with_its_beside_rows() {
    let booted = crate::boot::project(
        "[[nodes]]\nid = \"n_bridge\"\nname = \"Bridge\"\nsoftbody2d = { kind = \"rope\", pinned_particles = [0, 23] }\nsoft_rope2d = { particle_count = 24 }\n",
        "",
    );
    let errors = crate::boot::errors();
    assert!(errors.is_empty(), "the scene logged errors: {errors:#?}");
    let e = booted.node("Bridge");
    let state = booted.app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    assert_eq!(
        state.world.soft_bodies[state.soft_bodies[&e]].num_particles(),
        24
    );
}

#[test]
fn an_articulation_is_the_component_beside_the_joint() {
    let app = app();
    let anchor = node(&app, "Anchor", [0.0; 3]);
    add(&app, anchor, "body3d", "kind = \"static\"").unwrap();
    let ball = node(&app, "Ball", [1.0, 0.0, 0.0]);
    add(&app, ball, "body3d", "kind = \"dynamic\"").unwrap();
    add(
        &app,
        ball,
        "collider3d",
        "shape = { kind = \"sphere\", radius = 0.1 }",
    )
    .unwrap();
    add(
        &app,
        ball,
        "joint3d",
        "kind = \"hinge\"\nconnected_body = \"/Anchor\"\nanchor = [-1.0, 0.0, 0.0]",
    )
    .unwrap();
    let reduced = |app: &App| {
        let state = app.engine.resource::<PhysicsState3d>();
        matches!(
            state.borrow().joints[&ball].handle,
            balaur_physics::joint::JointHandle3d::Multibody(_)
        )
    };
    assert!(!reduced(&app), "a joint with no articulation3d is reduced");
    add(&app, ball, "articulation3d", "").unwrap();
    assert!(
        reduced(&app),
        "articulation3d left the joint an impulse one"
    );
    components::remove(&app.engine, ball, "articulation3d").unwrap();
    assert!(
        !reduced(&app),
        "the joint stayed in its chain without articulation3d"
    );
}

/// The coupled rows are a generic joint's: a ball socket names no
/// `generic_joint3d`, and one beside it is not read.
#[test]
fn a_generic_joints_rows_are_read_by_a_generic_joint_alone() {
    let app = app();
    let anchor = node(&app, "Anchor", [0.0; 3]);
    add(&app, anchor, "body3d", "kind = \"static\"").unwrap();
    let ball = node(&app, "Ball", [1.0, 0.0, 0.0]);
    add(&app, ball, "body3d", "kind = \"dynamic\"").unwrap();
    add(
        &app,
        ball,
        "generic_joint3d",
        "coupled_rotation = [\"y\", \"z\"]",
    )
    .unwrap();
    let coupled = |app: &App| {
        let state = app.engine.resource::<PhysicsState3d>();
        let state = state.borrow();
        let balaur_physics::joint::JointHandle3d::Impulse(handle) = state.joints[&ball].handle
        else {
            panic!("an impulse joint");
        };
        state
            .world
            .impulse_joints
            .get(handle)
            .unwrap()
            .data
            .coupled_axes
    };
    add(
        &app,
        ball,
        "joint3d",
        "kind = \"ball_socket\"\nconnected_body = \"/Anchor\"\nanchor = [-1.0, 0.0, 0.0]",
    )
    .unwrap();
    assert!(
        coupled(&app).is_empty(),
        "a ball socket read a generic joint's rows"
    );
    components::patch(
        &app.engine,
        ball,
        "joint3d",
        &toml::from_str("kind = \"generic\"").unwrap(),
    )
    .unwrap();
    assert!(
        !coupled(&app).is_empty(),
        "a generic joint left its coupled rows unread"
    );
}

#[test]
fn a_snapshot_carries_the_beside_components() {
    let mut app = app();
    let e = node(&app, "Rope", [0.0; 3]);
    add(&app, e, "soft_rope3d", "particle_count = 8").unwrap();
    add(&app, e, "softbody3d", "kind = \"rope\"").unwrap();
    add(&app, e, "collider3d", "shape = { kind = \"sphere\" }").unwrap();
    app.tick(1.0 / 60.0);
    let taken = snapshot::capture(&app.engine);
    components::patch(
        &app.engine,
        e,
        "soft_rope3d",
        &toml::from_str("particle_count = 5").unwrap(),
    )
    .unwrap();
    add(&app, e, "convex_decomposition3d", "max_convex_hulls = 3").unwrap();
    snapshot::restore(&app.engine, &taken);
    let rope = components::get(&app.engine, e, "soft_rope3d").unwrap();
    assert_eq!(
        rope.get("particle_count").and_then(toml::Value::as_integer),
        Some(8),
        "the restore left the rope's later count"
    );
    assert!(
        components::get(&app.engine, e, "convex_decomposition3d").is_none(),
        "a component added after the snapshot outlived its restore"
    );
    assert_eq!(particles(&app, e), 8);
}
