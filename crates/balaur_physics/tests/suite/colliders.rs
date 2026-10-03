//! What a collider carries besides its shape, where it sits, and which body
//! it belongs to.

use balaur_core::hecs::Entity;
use balaur_core::scene;
use balaur_core::{App, AppConfig, components};
use balaur_physics::{PhysicsPlugin, PhysicsState3d};

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
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
        r#"kind = "box"
friction = 0.9
restitution = 0.25
friction_combine = "max"
restitution_combine = "min"
collision_margin = 0.02
mass = 4.0
collision_layer = ["1", "3"]
collision_mask = ["2"]
events = ["collision"]
contact_pairs = ["dynamic_dynamic", "static_static"]"#,
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
    assert_eq!(flags("collision_layer"), ["1", "3"]);
    assert_eq!(flags("collision_mask"), ["2"]);
    assert_eq!(flags("events"), ["collision"]);
    assert_eq!(flags("contact_pairs"), ["dynamic_dynamic", "static_static"]);
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

/// `mass` and `density` are each derived from the other, so reporting both
/// would let the first patch pin the mass and freeze the density for good.
#[test]
fn a_density_a_patch_writes_reaches_the_collider() {
    let app = app();
    let root = app.engine.root();
    let e = child_of(&app, root, "Box");
    let params: toml::Value = toml::from_str("kind = \"box\"\ndensity = 1.0").unwrap();
    components::add(&app.engine, e, "collider3d", Some(&params)).unwrap();
    let read = |key: &str| {
        components::get(&app.engine, e, "collider3d")
            .and_then(|c| c.get(key).and_then(balaur_core::components::as_f64))
            .unwrap_or_default()
    };
    #[allow(clippy::float_cmp, reason = "no mass is exactly none, not nearly")]
    {
        assert_eq!(
            read("mass"),
            0.0,
            "a collider on its density reports no mass"
        );
    }

    let patch: toml::Value = toml::from_str("density = 15.0").unwrap();
    components::patch(&app.engine, e, "collider3d", &patch).unwrap();
    assert!(
        (read("density") - 15.0).abs() < 1e-5,
        "density came back as {}",
        read("density")
    );

    let state = app.engine.resource::<balaur_physics::PhysicsState3d>();
    let state = state.borrow();
    let handle = state.colliders[&e][0];
    let mass = state.world.colliders[handle].mass();
    assert!((mass - 15.0).abs() < 1e-4, "rapier weighs it {mass}");
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
        Some(&toml::from_str("kind = \"sphere\"\nradius = 0.5").unwrap()),
    )
    .unwrap();
    let state = app.engine.resource::<PhysicsState3d>();
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
        Some(&toml::from_str("kind = \"sphere\"\noffset = [0.0, 1.0, 0.0]").unwrap()),
    )
    .unwrap();
    let state = app.engine.resource::<PhysicsState3d>();
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
        "sphere",
        "box",
        "capsule",
        "cylinder",
        "cone",
        "triangle",
        "segment",
        "world_boundary",
    ] {
        let e = child_of(&app, root, kind);
        let params: toml::Value = toml::from_str(&format!("kind = \"{kind}\"")).unwrap();
        components::add(&app.engine, e, "collider3d", Some(&params))
            .unwrap_or_else(|e| panic!("collider3d kind '{kind}' did not build: {e:#}"));
        let state = app.engine.resource::<PhysicsState3d>();
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
        Some(&toml::from_str("kind = \"box\"\nedge_radius = 0.1").unwrap()),
    )
    .unwrap();
    let back = components::get(&app.engine, e, "collider3d").unwrap();
    assert_eq!(back.get("kind").unwrap().as_str(), Some("box"));
    assert!(
        (back
            .get("edge_radius")
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
        "rectangle",
        "capsule",
        "triangle",
        "segment",
        "world_boundary",
    ] {
        let e = child_of(&app, root, kind);
        let params: toml::Value = toml::from_str(&format!("kind = \"{kind}\"")).unwrap();
        components::add(&app.engine, e, "collider2d", Some(&params))
            .unwrap_or_else(|e| panic!("collider2d kind '{kind}' did not build: {e:#}"));
    }
}

/// The inspector reads a collider back through `get`; without the authored
/// params under it, a 2D re-save loses the offset, the one-way flag and the
/// asset a mesh-backed shape was built from.
#[test]
fn a_2d_collider_round_trips_through_get() {
    let app = app();
    let root = app.engine.root();
    let e = child_of(&app, root, "Platform");
    let params: toml::Value = toml::from_str(
        r#"kind = "rectangle"
size = [4.0, 0.5]
offset = [0.5, -1.0]
offset_rotation = 0.75
one_way = true
one_way_axis = [0.0, 1.0]
friction = 0.9"#,
    )
    .unwrap();
    components::add(&app.engine, e, "collider2d", Some(&params)).unwrap();
    let back = components::get(&app.engine, e, "collider2d").expect("collider2d reports itself");
    let f = |key: &str| {
        back.get(key)
            .and_then(balaur_core::components::as_f64)
            .unwrap_or_default()
    };
    assert_eq!(back.get("kind").unwrap().as_str(), Some("rectangle"));
    assert_eq!(back.get("one_way").unwrap().as_bool(), Some(true));
    assert!((f("offset_rotation") - 0.75).abs() < 1e-6);
    assert!((f("friction") - 0.9).abs() < 1e-6);
    let offset = back.get("offset").unwrap().as_array().unwrap();
    assert!((offset[0].as_float().unwrap() - 0.5).abs() < 1e-6);
    assert!((offset[1].as_float().unwrap() + 1.0).abs() < 1e-6);
}

/// `one_way` is a no-op unless the axis reaches the hook, which runs while
/// the world is borrowed and reads it from a side table.
#[test]
fn a_2d_one_way_collider_lets_a_body_up_and_lands_it_coming_down() {
    let mut app = app();
    let root = app.engine.root();
    let platform = child_of(&app, root, "Platform");
    components::add(
        &app.engine,
        platform,
        "collider2d",
        Some(
            &toml::from_str(
                "kind = \"rectangle\"\nsize = [4.0, 0.2]\none_way = true\none_way_axis = [0.0, 1.0]",
            )
            .unwrap(),
        ),
    )
    .unwrap();
    let ball = child_of(&app, root, "Ball");
    app.engine
        .world()
        .get::<&mut balaur_core::Transform>(ball)
        .unwrap()
        .position
        .y = -1.0;
    for (name, params) in [
        ("body2d", "kind = \"dynamic\""),
        ("collider2d", "kind = \"circle\"\nradius = 0.25"),
    ] {
        components::add(
            &app.engine,
            ball,
            name,
            Some(&toml::from_str(params).unwrap()),
        )
        .unwrap();
    }
    {
        let state = app.engine.resource::<balaur_physics::PhysicsState2d>();
        let mut state = state.borrow_mut();
        let handle = state.bodies[&ball];
        let up = balaur_physics::rapier2d::math::Vector::new(0.0, 7.0);
        state.world.bodies[handle].set_linvel(up, true);
    }
    let mut highest = f32::MIN;
    for _ in 0..120 {
        app.tick(1.0 / 60.0);
        let y = app
            .engine
            .world()
            .get::<&balaur_core::Transform>(ball)
            .unwrap()
            .position
            .y;
        highest = highest.max(y);
    }
    let y = app
        .engine
        .world()
        .get::<&balaur_core::Transform>(ball)
        .unwrap()
        .position
        .y;
    assert!(
        highest > 0.5,
        "the ball stopped under the platform at {highest}"
    );
    assert!(
        (y - 0.35).abs() < 0.05,
        "the ball came back down and did not land on the platform: y = {y}"
    );
}

/// The numbers a probe function on a script returned, in order.
fn numbers(value: Option<balaur_script::Value>) -> Vec<f64> {
    use balaur_script::Value;
    let Some(Value::List(items) | Value::Many(items)) = value else {
        panic!("the probe answered {value:?}");
    };
    items
        .iter()
        .map(|item| match item {
            Value::Num(n) => *n,
            Value::Int(n) => *n as f64,
            other => panic!("the probe answered {other:?} in its list"),
        })
        .collect()
}

/// A project whose one node runs `script`, booted and loaded. Holds [`LOG`]
/// for as long as the caller keeps the guard, so a script's error lands in no
/// other test's log.
fn scripted(
    scene: &str,
    script: &str,
) -> (App, tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
    let guard = crate::LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    std::fs::write(dir.path().join("scripts/s.rn"), script).unwrap();
    let mut app = balaur::standard_app(balaur::AppConfig::dev(
        dir.path().to_string_lossy().as_ref(),
    ))
    .unwrap();
    app.load_project().unwrap();
    (app, dir, guard)
}

#[test]
fn a_swept_box_reaches_where_the_body_is_going() {
    let (app, _dir, _log) = scripted(
        r#"[[nodes]]
id = "n_ball"
name = "Ball"
script = { source = "scripts/s.rn" }
body3d = { kind = "dynamic", gravity_scale = 0.0 }
collider3d = { kind = "sphere", radius = 0.5 }
"#,
        r"pub fn boxes(this) {
    let (ax, ay, az, bx, by, bz) = this.node.collider3d.aabb();
    let (sx, sy, sz, tx, ty, tz) = this.node.collider3d.swept_aabb();
    [ax, bx, sx, tx]
}
",
    );
    let ball = balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), "Ball")
        .expect("the scene's ball");
    {
        let state = app.engine.resource::<PhysicsState3d>();
        let mut state = state.borrow_mut();
        let handle = state.bodies[&ball];
        let velocity = balaur_physics::rapier3d::math::Vector::new(6.0, 0.0, 0.0);
        state.world.bodies[handle].set_linvel(velocity, true);
    }
    let host = app.engine.script_host().unwrap();
    let [from, to, swept_from, swept_to] =
        numbers(host.call_on(balaur_core::node_id_of(ball), "boxes", &[]))[..]
    else {
        panic!("the probe answered four numbers");
    };
    let step = 6.0 * f64::from(balaur_core::fixed_dt());
    assert!(
        (swept_from - from).abs() < 1e-5,
        "the sweep starts where the ball is"
    );
    assert!(
        (swept_to - (to + step)).abs() < 1e-4,
        "the sweep ends at {swept_to}, not one step on at {}",
        to + step
    );
}

/// Child nodes are where a compound body keeps its colliders.
#[test]
fn a_bodys_hardest_contact_counts_its_child_colliders() {
    let (mut app, _dir, _log) = scripted(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_ground"
name = "Ground"
parent = "n_world"
body2d = { kind = "static" }
collider2d = { kind = "rectangle", size = [20.0, 1.0] }

[[nodes]]
id = "n_crate"
name = "Crate"
parent = "n_world"
script = { source = "scripts/s.rn" }
body2d = { kind = "dynamic" }

[nodes.transform]
position = [0.0, 1.0, 0.0]

[[nodes]]
id = "n_shape"
name = "Shape"
parent = "n_crate"
collider2d = { kind = "rectangle", size = [1.0, 1.0] }
"#,
        r"pub fn hardest(this) {
    [this.node.body2d.max_contact_impulse()]
}
",
    );
    for _ in 0..60 {
        app.tick(1.0 / 60.0);
    }
    let crate_node =
        balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), "World/Crate")
            .expect("the scene's crate");
    let host = app.engine.script_host().unwrap();
    let hardest = numbers(host.call_on(balaur_core::node_id_of(crate_node), "hardest", &[]));
    assert!(
        hardest[0] > 0.0,
        "a crate resting on the ground through its child's collider took no contact"
    );
}
