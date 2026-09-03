//! What a collider carries besides its shape, where it sits, and which body
//! it belongs to.

use balaur_core::hecs::Entity;
use balaur_core::scene;
use balaur_core::{components, App, AppConfig};
use balaur_physics::{PhysicsPlugin, PhysicsState};

fn app() -> App {
    let mut app = App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    app.add_plugin(PhysicsPlugin).unwrap();
    app
}

fn child_of(app: &App, parent: Entity, name: &str) -> Entity {
    scene::spawn_node(&mut app.engine.world_mut(), name, parent)
}

#[test]
fn collider_material_round_trips() {
    let app = app();
    let root = app.engine.root();
    let e = child_of(&app, root, "Box");
    let params: toml::Value = toml::from_str(
        r#"kind = "cuboid"
friction = 0.9
restitution = 0.25
friction_combine = "max"
restitution_combine = "min"
contact_skin = 0.02
mass = 4.0
layers = ["1", "3"]
mask = ["2"]
events = ["collision"]
active_collisions = ["dynamic_dynamic", "static_static"]"#,
    )
    .unwrap();
    components::add(&app.engine, e, "collider3d", Some(&params)).unwrap();
    let back = components::get(&app.engine, e, "collider3d").unwrap();
    let text = |key: &str| {
        back.get(key)
            .and_then(toml::Value::as_str)
            .unwrap()
            .to_string()
    };
    let flags = |key: &str| {
        balaur_core::components::as_flags(back.get(key))
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    assert_eq!(text("friction_combine"), "max");
    assert_eq!(text("restitution_combine"), "min");
    assert_eq!(flags("layers"), ["1", "3"]);
    assert_eq!(flags("mask"), ["2"]);
    assert_eq!(flags("events"), ["collision"]);
    assert_eq!(
        flags("active_collisions"),
        ["dynamic_dynamic", "static_static"]
    );
    assert!(
        (back
            .get("mass")
            .and_then(balaur_core::components::as_f64)
            .unwrap()
            - 4.0)
            .abs()
            < 1e-5
    );
}

/// The compound-shape story: a collider on a child node belongs to the body
/// above it, at the child's own offset from that body.
#[test]
fn a_child_collider_joins_the_body_above_it() {
    let app = app();
    let root = app.engine.root();
    let body = child_of(&app, root, "Body");
    components::add(
        &app.engine,
        body,
        "body3d",
        Some(&toml::from_str("kind = \"dynamic\"").unwrap()),
    )
    .unwrap();
    let feet = child_of(&app, body, "Feet");
    {
        let world = app.engine.world();
        let mut transform = world
            .get::<&mut balaur_core::scene::Transform>(feet)
            .unwrap();
        transform.position.y = -2.0;
    }
    components::add(
        &app.engine,
        feet,
        "collider3d",
        Some(&toml::from_str("kind = \"ball\"\nradius = 0.5").unwrap()),
    )
    .unwrap();
    let state = app.engine.resource::<PhysicsState>();
    let state = state.borrow();
    let handle = state.colliders[&feet][0];
    let collider = &state.world.colliders[handle];
    assert_eq!(
        collider.parent(),
        Some(state.bodies[&body]),
        "the child's collider did not join its parent's body"
    );
    let offset = collider.position_wrt_parent().unwrap().translation;
    assert!(
        (offset.y + 2.0).abs() < 1e-5,
        "the child's collider sits at {offset:?}, not two units below the body"
    );
}

/// `offset` moves the shape without moving the node, which is what a capsule
/// standing on a node's origin needs.
#[test]
fn an_offset_moves_the_shape_and_not_the_node() {
    let app = app();
    let root = app.engine.root();
    let e = child_of(&app, root, "Body");
    components::add(
        &app.engine,
        e,
        "body3d",
        Some(&toml::from_str("kind = \"static\"").unwrap()),
    )
    .unwrap();
    components::add(
        &app.engine,
        e,
        "collider3d",
        Some(&toml::from_str("kind = \"ball\"\noffset = [0.0, 1.0, 0.0]").unwrap()),
    )
    .unwrap();
    let state = app.engine.resource::<PhysicsState>();
    let state = state.borrow();
    let collider = &state.world.colliders[state.colliders[&e][0]];
    assert!((collider.position().translation.y - 1.0).abs() < 1e-5);
}

/// Every shape the schema offers must build, including the ones this phase
/// added. A kind that only parses is not a kind.
#[test]
fn every_declared_shape_builds() {
    let app = app();
    let root = app.engine.root();
    for kind in [
        "ball",
        "cuboid",
        "capsule",
        "cylinder",
        "cone",
        "triangle",
        "segment",
        "halfspace",
    ] {
        let e = child_of(&app, root, kind);
        let params: toml::Value = toml::from_str(&format!("kind = \"{kind}\"")).unwrap();
        components::add(&app.engine, e, "collider3d", Some(&params))
            .unwrap_or_else(|e| panic!("collider3d kind '{kind}' did not build: {e:#}"));
        let state = app.engine.resource::<PhysicsState>();
        assert!(
            state.borrow().colliders.contains_key(&e),
            "collider3d kind '{kind}' built nothing"
        );
    }
}

/// A border rounds a shape and reads back as one, so the inspector shows what
/// the author wrote rather than a shape they never named.
#[test]
fn a_border_rounds_a_cuboid() {
    let app = app();
    let root = app.engine.root();
    let e = child_of(&app, root, "Rounded");
    components::add(
        &app.engine,
        e,
        "collider3d",
        Some(&toml::from_str("kind = \"cuboid\"\nborder = 0.1").unwrap()),
    )
    .unwrap();
    let back = components::get(&app.engine, e, "collider3d").unwrap();
    assert_eq!(back.get("kind").unwrap().as_str(), Some("cuboid"));
    assert!(
        (back
            .get("border")
            .and_then(balaur_core::components::as_f64)
            .unwrap()
            - 0.1)
            .abs()
            < 1e-6
    );
}

/// 2D grew from three shapes to ten; the same test, one dimension down.
#[test]
fn every_declared_2d_shape_builds() {
    let app = app();
    let root = app.engine.root();
    for kind in [
        "circle",
        "rect",
        "capsule",
        "triangle",
        "segment",
        "halfspace",
    ] {
        let e = child_of(&app, root, kind);
        let params: toml::Value = toml::from_str(&format!("kind = \"{kind}\"")).unwrap();
        components::add(&app.engine, e, "collider2d", Some(&params))
            .unwrap_or_else(|e| panic!("collider2d kind '{kind}' did not build: {e:#}"));
    }
}
