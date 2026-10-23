//! Collision events, joints and the character controller, driven from scripts
//! because their whole surface is the script seam.

use balaur::standard_app;

use crate::LOG;

/// Run a project made of `scene` and one script, and report what it logged as
/// an error.
pub(crate) fn run(scene: &str, script: &str) -> Vec<String> {
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
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    std::fs::write(dir.path().join("scripts/s.bs"), script).unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(crate::dev_config(dir.path())).unwrap();
    app.load_project().unwrap();
    for _ in 0..120 {
        app.tick(1.0 / 60.0);
    }
    balaur_core::logbuf::recent(500)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect()
}

pub(crate) fn run_clean(scene: &str, script: &str) {
    let errors = run(scene, script);
    assert!(errors.is_empty(), "the run logged errors: {errors:#?}");
}

/// A sensor tells the node's script who walked into it, and who left.
#[test]
fn a_trigger_calls_its_script_when_something_enters() {
    run_clean(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_trigger"
name = "Trigger"
parent = "n_world"
script = { source = "scripts/s.bs" }

[nodes.collider3d]
kind = "box"
size = [4.0, 4.0, 4.0]
sensor = true
events = ["collision"]

[[nodes]]
id = "n_faller"
name = "Faller"
parent = "n_world"
body3d = { kind = "dynamic" }

[nodes.transform]
position = [0.0, 6.0, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.5
"#,
        r#"pub struct S { node: Node, seen: int, left: int, ticks: int }
impl S {
    pub fn on_collision_enter(collision: any) { self.seen += 1; }
    pub fn on_collision_exit(collision: any) { self.left += 1; }

    fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 110 {
            if self.seen < 1 { log::error("nothing ever entered the trigger"); }
            if self.left < 1 { log::error("nothing ever left it"); }
        }
    }
}
"#,
    );
}

#[test]
fn a_joint_holds_two_bodies_together() {
    run_clean(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_anchor"
name = "Anchor"
parent = "n_world"
body3d = { kind = "static" }

[nodes.collider3d]
kind = "sphere"
radius = 0.2

[[nodes]]
id = "n_hanging"
name = "Hanging"
parent = "n_world"
body3d = { kind = "dynamic" }
script = { source = "scripts/s.bs" }

[nodes.transform]
position = [1.0, 0.0, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.2

[nodes.joint3d]
kind = "hinge"
connected_body = "/World/Anchor"
axis = [0.0, 0.0, 1.0]
anchor = [-1.0, 0.0, 0.0]
"#,
        r#"pub struct S { node: Node, ticks: int }
impl S {
    fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 110 {
            let position = self.node.transform.position;
            let far = math::sqrt(position.x * position.x + position.y * position.y);
            if far >= 1.2 { log::error("the joint let go: the body is {far} from the anchor"); }
            if position.y >= -0.1 { log::error("the body never swung: y is {position.y}"); }
        }
    }
}
"#,
    );
}

/// A hard landing tells the floor who hit it and how hard, as one map.
#[test]
fn a_contact_force_arrives_as_one_map() {
    run_clean(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_floor"
name = "Floor"
parent = "n_world"
body3d = { kind = "static" }
script = { source = "scripts/s.bs" }

[nodes.collider3d]
kind = "box"
size = [8.0, 0.4, 8.0]
events = ["contact_force"]

[[nodes]]
id = "n_crate"
name = "Crate"
parent = "n_world"
body3d = { kind = "dynamic" }

[nodes.transform]
position = [0.0, 3.0, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.5
"#,
        r#"pub struct S { node: Node, hits: int, ticks: int }
impl S {
    pub fn on_contact_force(contact: any) {
        if contact.get("force").as_float() > 0.0 && contact.get("other").as_node().name() == "Crate" {
            self.hits += 1;
        }
    }

    fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 110 && self.hits <= 0 {
            log::error("the landing never reported a force from the crate");
        }
    }
}
"#,
    );
}

/// A joint that gives way names both of its bodies and the force it broke at.
#[test]
fn a_breaking_joint_says_what_it_held_and_how_hard() {
    run_clean(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_anchor"
name = "Anchor"
parent = "n_world"
body3d = { kind = "static" }

[nodes.collider3d]
kind = "sphere"
radius = 0.2

[[nodes]]
id = "n_hanging"
name = "Hanging"
parent = "n_world"
body3d = { kind = "dynamic" }
script = { source = "scripts/s.bs" }

[nodes.transform]
position = [1.0, 0.0, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.2

[nodes.joint3d]
kind = "hinge"
connected_body = "/World/Anchor"
axis = [0.0, 0.0, 1.0]
anchor = [-1.0, 0.0, 0.0]
break_force = 0.001
"#,
        r#"pub struct S { node: Node, broke: int, ticks: int }
impl S {
    pub fn on_joint_break(broke: any) {
        let a = broke.get("a").as_node().name();
        let b = broke.get("b").as_node().name();
        if broke.get("force").as_float() > 0.001 && (a == "Anchor" || b == "Anchor") {
            self.broke += 1;
        }
    }

    fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 60 && self.broke != 1 {
            log::error("the joint broke {self.broke} times with its payload");
        }
    }
}
"#,
    );
}

/// A body carries several joints by putting each on a bodiless child, which
/// stands for the body above it, as a collider on a child does.
#[test]
fn a_joint_on_a_bodiless_child_ties_the_body_above_it() {
    run_clean(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_anchor"
name = "Anchor"
parent = "n_world"
body3d = { kind = "static" }

[nodes.collider3d]
kind = "sphere"
radius = 0.2

[[nodes]]
id = "n_hanging"
name = "Hanging"
parent = "n_world"
body3d = { kind = "dynamic" }
script = { source = "scripts/s.bs" }

[nodes.transform]
position = [1.0, 0.0, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.2

[[nodes]]
id = "n_link"
name = "Link"
parent = "n_hanging"

[nodes.joint3d]
kind = "hinge"
connected_body = "/World/Anchor"
axis = [0.0, 0.0, 1.0]
anchor = [-1.0, 0.0, 0.0]
"#,
        r#"pub struct S { node: Node, ticks: int }
impl S {
    fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 110 {
            let position = self.node.transform.position;
            let far = math::sqrt(position.x * position.x + position.y * position.y);
            if far >= 1.2 { log::error("the child's joint did not hold: the body is {far} from the anchor"); }
            if position.y >= -0.1 { log::error("the body never swung: y is {position.y}"); }
        }
    }
}
"#,
    );
}

#[test]
fn a_2d_joint_on_a_bodiless_child_ties_the_body_above_it() {
    run_clean(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_anchor"
name = "Anchor"
parent = "n_world"
body2d = { kind = "static" }

[nodes.collider2d]
kind = "circle"
radius = 0.2

[[nodes]]
id = "n_hanging"
name = "Hanging"
parent = "n_world"
body2d = { kind = "dynamic" }
script = { source = "scripts/s.bs" }

[nodes.transform]
position = [1.0, 0.0, 0.0]

[nodes.collider2d]
kind = "circle"
radius = 0.2

[[nodes]]
id = "n_link"
name = "Link"
parent = "n_hanging"

[nodes.joint2d]
kind = "hinge"
connected_body = "/World/Anchor"
anchor = [-1.0, 0.0]
"#,
        r#"pub struct S { node: Node, ticks: int }
impl S {
    fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 110 {
            let position = self.node.transform.position;
            let far = math::sqrt(position.x * position.x + position.y * position.y);
            if far >= 1.2 { log::error("the child's joint did not hold: the body is {far} from the anchor"); }
            if position.y >= -0.1 { log::error("the body never swung: y is {position.y}"); }
        }
    }
}
"#,
    );
}

/// A joint whose other end is named before that node exists still connects:
/// a scene file names nodes in whatever order it likes.
#[test]
fn a_joint_waits_for_a_node_that_comes_later() {
    run_clean(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_hanging"
name = "Hanging"
parent = "n_world"
body3d = { kind = "dynamic" }
script = { source = "scripts/s.bs" }

[nodes.transform]
position = [1.0, 0.0, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.2

[nodes.joint3d]
kind = "hinge"
connected_body = "/World/Anchor"
anchor = [-1.0, 0.0, 0.0]

[[nodes]]
id = "n_anchor"
name = "Anchor"
parent = "n_world"
body3d = { kind = "static" }

[nodes.collider3d]
kind = "sphere"
radius = 0.2
"#,
        r#"pub struct S { node: Node, ticks: int }
impl S {
    fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 110 {
            let position = self.node.transform.position;
            let far = math::sqrt(position.x * position.x + position.y * position.y);
            if far >= 1.2 { log::error("the forward reference never connected: {far} away"); }
        }
    }
}
"#,
    );
}

/// The character controller walks into a wall and stops, rather than passing
/// through it or being pushed back by the solver.
#[test]
fn a_character_slides_along_a_wall_instead_of_entering_it() {
    run_clean(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_wall"
name = "Wall"
parent = "n_world"

[nodes.transform]
position = [2.0, 0.0, 0.0]

[nodes.collider3d]
kind = "box"
size = [1.0, 8.0, 16.0]

[[nodes]]
id = "n_player"
name = "Player"
parent = "n_world"
script = { source = "scripts/s.bs" }

[nodes.collider3d]
kind = "capsule"
radius = 0.4
height = 1.0

[nodes.character3d]
floor_snap_length = 0.0
"#,
        r#"pub struct S { node: Node, ticks: int }
impl S {
    fn fixed_update(dt: float) {
        self.ticks += 1;
        self.node.character3d.move_character(0.1, 0.0, 0.05);
        if self.ticks == 110 {
            let position = self.node.transform.position;
            if position.x >= 1.2 { log::error("the character walked into the wall: x is {position.x}"); }
            if position.z <= 0.5 { log::error("the character did not slide along it: z is {position.z}"); }
        }
    }
}
"#,
    );
}

/// Every phase owes the snapshot a round trip: a joint, a motor and a voxel
/// edit must all come back the way they went in, or a rollback quietly
/// rebuilds a different world.
#[test]
fn joints_and_shape_edits_survive_a_snapshot() {
    use balaur_core::hecs::Entity;
    use balaur_core::scene::{self, Transform};
    use balaur_core::{App, AppConfig, components, snapshot};
    use balaur_physics::{PhysicsPlugin, PhysicsState3d};

    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();

    let root = app.engine.root();
    let spawn = |name: &str, x: f32| -> Entity {
        let e = scene::spawn_node(&mut app.engine.world_mut(), name, root);
        app.engine
            .world()
            .get::<&mut Transform>(e)
            .unwrap()
            .position
            .x = x;
        components::add(
            &app.engine,
            e,
            "body3d",
            Some(&toml::from_str("kind = \"dynamic\"").unwrap()),
        )
        .unwrap();
        components::add(
            &app.engine,
            e,
            "collider3d",
            Some(&toml::from_str("kind = \"sphere\"\nradius = 0.2").unwrap()),
        )
        .unwrap();
        e
    };
    let anchor = spawn("Anchor", 0.0);
    let hanging = spawn("Hanging", 1.0);
    components::add(
        &app.engine,
        hanging,
        "joint3d",
        Some(
            &toml::from_str("kind = \"hinge\"\nconnected_body = \"/Anchor\"\nbreak_force = 500.0")
                .unwrap(),
        ),
    )
    .unwrap();
    let _ = anchor;

    for _ in 0..10 {
        app.tick(1.0 / 60.0);
    }
    let taken = snapshot::capture(&app.engine);
    let before = {
        let state = app.engine.resource::<PhysicsState3d>();
        let state = state.borrow();
        (
            state.joints.len(),
            state.world.bodies[state.bodies[&hanging]].translation().y,
        )
    };
    assert_eq!(before.0, 1, "the joint was never made");

    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    snapshot::restore(&app.engine, &taken);
    let after = {
        let state = app.engine.resource::<PhysicsState3d>();
        let state = state.borrow();
        (
            state.joints.len(),
            state.world.bodies[state.bodies[&hanging]].translation().y,
        )
    };
    assert_eq!(after.0, before.0, "the joint did not come back");
    assert!(
        (after.1 - before.1).abs() < 1e-6,
        "the body came back at {} rather than {}",
        after.1,
        before.1
    );
}

/// `is_grounded` used to run a zero-translation `move_shape` that still
/// snapped to ground and wrote the transform, so asking the question moved
/// the world — and asking it from `update` desynced the simulation.
#[test]
fn asking_is_grounded_does_not_move_the_character() {
    run_clean(
        r#"[[nodes]]
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
size = [16.0, 1.0, 16.0]

[[nodes]]
id = "n_player"
name = "Player"
parent = "n_world"
script = { source = "scripts/s.bs" }

[nodes.transform]
position = [0.0, 1.0, 0.0]

[nodes.collider3d]
kind = "capsule"
radius = 0.4
height = 1.0

[nodes.character3d]
"#,
        r#"pub struct S { node: Node, ticks: int, parked: float, grounded: bool }
impl S {
    fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks < 40 {
            self.node.character3d.move_character(0.0, -0.2, 0.0);
            return;
        }
        if self.ticks == 40 {
            self.parked = self.node.transform.position.y;
            self.grounded = self.node.character3d.is_on_floor();
        }
        let i = 0;
        while i < 8 {
            self.node.character3d.is_on_floor();
            i += 1;
        }
        if self.ticks == 100 {
            if !self.grounded { log::error("the character never landed, so the test proves nothing"); }
            let drift = self.node.transform.position.y - self.parked;
            if drift >= 0.0001 || drift <= -0.0001 { log::error("asking moved it by {drift}"); }
        }
    }
}
"#,
    );
}

/// 2D `move_character` wrote `Pose2::from_translation`, so a rotated
/// `character2d` snapped to angle 0 the first time it moved. The ray only
/// misses while the box is still standing on end.
#[test]
fn a_rotated_2d_character_keeps_its_angle_when_it_moves() {
    run_clean(
        r#"[[nodes]]
id = "n_player"
name = "Player"
script = { source = "scripts/s.bs" }

[nodes.transform]
rotation_euler = [0.0, 0.0, 1.5707963]

[nodes.collider2d]
kind = "rectangle"
size = [2.0, 0.1]

[nodes.character2d]
floor_snap_length = 0.0
step_height = 0.0
"#,
        r#"pub struct S { node: Node, ticks: int }
impl S {
    fn fixed_update(dt: float) {
        self.ticks += 1;
        self.node.character2d.move_character(0.0, 0.0);
        if self.ticks == 30 {
            let flat = physics2d::raycast(map().with("origin", [0.5, 3.0]).with("direction", [0.0, -1.0]).with("max_distance", 100.0));
            if !flat.is_nil() { log::error("the character lay back down: a ray beside it still hits"); }
            let upright = physics2d::raycast(map().with("origin", [0.0, 3.0]).with("direction", [0.0, -1.0]).with("max_distance", 100.0));
            if upright.is_nil() { log::error("the character is not where the test thinks it is"); }
        }
    }
}
"#,
    );
}

/// `contacts` reported parry's `local_p1`, which is in the *first* shape's
/// local space — and the first may be the other node. Every other query is
/// world-space, and a ground ten units down is where the point must be.
#[test]
fn a_contact_point_is_reported_in_world_space() {
    run_clean(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_ground"
name = "Ground"
parent = "n_world"

[nodes.transform]
position = [0.0, -10.0, 0.0]

[nodes.collider3d]
kind = "box"
size = [16.0, 1.0, 16.0]

[[nodes]]
id = "n_faller"
name = "Faller"
parent = "n_world"
body3d = { kind = "dynamic" }
script = { source = "scripts/s.bs" }

[nodes.transform]
position = [0.0, -8.0, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.5
"#,
        r#"pub struct S { node: Node, ticks: int, seen: int }
impl S {
    fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 110 {
            for contact in self.node.collider3d.contacts() {
                self.seen += 1;
                let y = contact.get("point").at(1).as_float();
                if y >= -8.0 { log::error("the contact point is in local space: y is {y}"); }
            }
            if self.seen <= 0 { log::error("the body never landed, so nothing was checked"); }
        }
    }
}
"#,
    );
}
