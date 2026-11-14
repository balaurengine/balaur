//! The 2D joints' readers and components, a follow's reset and the wheel
//! drives no game or editor script reaches yet, each run from a scene and a
//! script and checked by how the bodies moved.

use crate::boot;

fn run_checked(scene: &str, script: &str, ticks: usize, done: &str) {
    let mut booted = boot::project(scene, script);
    booted.tick(ticks);
    let errors = boot::errors();
    assert!(
        errors.len() == 1 && errors[0].contains(done),
        "the check did not run clean: {errors:#?}"
    );
}

const JOINTS_2D: &str = r#"[[nodes]]
id = "n_world"
name = "World"
script = { source = "scripts/scenario.bs" }

[[nodes]]
id = "n_anchor"
name = "Anchor"
parent = "n_world"
body2d = { kind = "static" }

[[nodes]]
id = "n_limited"
name = "Limited"
parent = "n_world"
body2d = { kind = "dynamic", mass = 1.0 }
collider2d = { shape = { kind = "circle", radius = 0.1 } }
transform = { position = [1.0, 0.0, 0.0] }

[nodes.joint2d]
kind = "hinge"
connected_body = "/World/Anchor"
anchor = [-1.0, 0.0]
axes = [{ axis = "rotation", limits = [-0.3, 0.3] }]

[[nodes]]
id = "n_roped"
name = "Roped"
parent = "n_world"
body2d = { kind = "dynamic", mass = 1.0 }
collider2d = { shape = { kind = "circle", radius = 0.1 } }
transform = { position = [10.0, -0.5, 0.0] }

[nodes.joint2d]
kind = "rope"
connected_body = "/World/Anchor"
connected_anchor = [10.0, 0.0]

[nodes.rope_joint2d]
max_length = 2.0

[[nodes]]
id = "n_sprung"
name = "Sprung"
parent = "n_world"
body2d = { kind = "dynamic", mass = 1.0, gravity_scale = 0.0 }
collider2d = { shape = { kind = "circle", radius = 0.1 } }
transform = { position = [20.0, -3.0, 0.0] }

[nodes.joint2d]
kind = "spring"
connected_body = "/World/Anchor"
connected_anchor = [20.0, 0.0]
axes = [{ axis = "x", stiffness = 40.0, damping = 4.0 }]

[nodes.spring_joint2d]
rest_length = 1.0

[[nodes]]
id = "n_snapped"
name = "Snapped"
parent = "n_world"
body2d = { kind = "dynamic", mass = 2.0 }
collider2d = { shape = { kind = "circle", radius = 0.1 } }
transform = { position = [30.0, -1.0, 0.0] }

[nodes.joint2d]
kind = "fixed"
connected_body = "/World/Anchor"
anchor = [0.0, 1.0]
connected_anchor = [30.0, 0.0]

[nodes.breakable_joint2d]
break_force = 5.0

[[nodes]]
id = "n_held"
name = "Held"
parent = "n_world"
body2d = { kind = "dynamic", mass = 2.0 }
collider2d = { shape = { kind = "circle", radius = 0.1 } }
transform = { position = [40.0, -1.0, 0.0] }

[nodes.joint2d]
kind = "fixed"
connected_body = "/World/Anchor"
anchor = [0.0, 1.0]
connected_anchor = [40.0, 0.0]

[nodes.breakable_joint2d]
break_force = 100.0
"#;

#[test]
fn a_2d_joint_reports_its_angle_limit_and_force() {
    run_checked(
        JOINTS_2D,
        r#"pub class Scenario { node: Node, ticks: int }
impl Scenario {
    fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 20 {
            let held = scene::root().get_node("World/Limited").joint2d.state();
            if held.status != physics2d::JOINT_ENABLED { log::error("status {held.status}"); }
            let angle = held.angle.unwrap();
            if math::abs(angle) <= 0.1 { log::error("the pendulum never swung: {angle}"); }
            if math::abs(angle) >= 0.35 { log::error("the limit did not hold: {angle}"); }
            if math::abs(held.limit_impulses.rotation.unwrap()) <= 0.0 { log::error("the limit carried no impulse"); }
            let pull = scene::root().get_node("World/Limited").joint2d.force();
            if pull.force <= 0.0 { log::error("the hinge held no force: {pull.force}"); }
            log::error("checked: 2d joint state");
        }
    }
}
"#,
        21,
        "checked: 2d joint state",
    );
}

#[test]
fn a_2d_rope_stops_at_its_length_and_a_spring_pulls_to_its_rest() {
    run_checked(
        JOINTS_2D,
        r#"pub class Scenario { node: Node, ticks: int }
impl Scenario {
    fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 120 {
            let roped = scene::root().get_node("World/Roped").transform.position;
            let hang = math::sqrt((roped.x - 10.0) * (roped.x - 10.0) + roped.y * roped.y);
            if hang >= 2.05 { log::error("the rope let the ball fall to {hang}"); }
            if hang <= 1.9 { log::error("the ball never fell to the rope's length: {hang}"); }
            let sprung = scene::root().get_node("World/Sprung").transform.position;
            let stretch = math::sqrt((sprung.x - 20.0) * (sprung.x - 20.0) + sprung.y * sprung.y);
            if math::abs(stretch - 1.0) >= 0.2 { log::error("the spring settled {stretch} from its anchor, not 1"); }
            log::error("checked: 2d rope and spring");
        }
    }
}
"#,
        121,
        "checked: 2d rope and spring",
    );
}

#[test]
fn a_2d_joint_snaps_past_its_break_force_and_holds_below_it() {
    run_checked(
        JOINTS_2D,
        r#"pub class Scenario { node: Node, ticks: int }
impl Scenario {
    fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 60 {
            let snapped = scene::root().get_node("World/Snapped").transform.position.y;
            if snapped >= -2.0 { log::error("a joint pulled past its break_force held the ball at {snapped}"); }
            let held = scene::root().get_node("World/Held").transform.position.y;
            if math::abs(held + 1.0) >= 0.05 { log::error("a joint under its break_force let go: {held}"); }
            log::error("checked: 2d break");
        }
    }
}
"#,
        61,
        "checked: 2d break",
    );
}

/// A PID-followed body holding against gravity, its script resetting the
/// summed error at tick 300 when `reset` says so.
fn followed(dim: &str, reset: bool) -> String {
    let (shape, gains, at) = if dim == "3d" {
        ("sphere", "[8.0, 8.0, 8.0]", "[20.0, 20.0, 20.0]")
    } else {
        ("circle", "[8.0, 8.0]", "[20.0, 20.0]")
    };
    let scene = format!(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_goal"
name = "Goal"
parent = "n_world"
transform = {{ position = [0.0, 1.0, 0.0] }}

[[nodes]]
id = "n_body"
name = "Body"
parent = "n_world"
script = {{ source = "scripts/scenario.bs" }}
body{dim} = {{ kind = "dynamic" }}
collider{dim} = {{ shape = {{ kind = "{shape}" }} }}
follow{dim} = {{ target = "/World/Goal", kind = "pid", position_gain = {gains}, integral_gain = {at} }}
"#
    );
    let call = if reset {
        format!("self.node.follow{dim}.reset();")
    } else {
        String::new()
    };
    let script = format!(
        r#"pub class Scenario {{ node: Node, ticks: int }}
impl Scenario {{
    fn fixed_update(dt: float) {{
        self.ticks += 1;
        if self.ticks == 300 {{ {call} }}
        if self.ticks == 320 {{ log::error("height {{self.node.transform.position.y}}"); }}
    }}
}}
"#
    );
    let mut booted = boot::project(&scene, &script);
    booted.tick(321);
    let errors = boot::errors();
    errors
        .iter()
        .find_map(|e| e.strip_prefix("height ").map(str::to_owned))
        .unwrap_or_else(|| panic!("{dim}: the body never reported: {errors:#?}"))
}

#[test]
fn resetting_a_pid_follow_drops_what_it_summed_in_both_dimensions() {
    for dim in ["3d", "2d"] {
        let height = |reset| {
            followed(dim, reset)
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<f32>().ok())
                .unwrap()
        };
        let (kept, dropped) = (height(false), height(true));
        assert!(
            dropped < kept - 0.001,
            "{dim}: after a reset the body sat at {dropped}, against {kept} without one"
        );
    }
}

const CAR: &str = r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_ground"
name = "Ground"
parent = "n_world"
collider3d = { shape = { kind = "box", size = [200.0, 1.0, 200.0] } }
transform = { position = [0.0, -0.5, 0.0] }

[[nodes]]
id = "n_car"
name = "Car"
parent = "n_world"
script = { source = "scripts/scenario.bs" }
body3d = { kind = "dynamic", mass = 10.0, gravity_scale = 0.0, lock_translation = ["y"], lock_rotation = ["x", "y", "z"] }
collider3d = { shape = { kind = "sphere", radius = 0.1 } }
vehicle3d = {}
transform = { position = [0.0, 0.6, 0.0] }

[[nodes]]
id = "n_wheel"
name = "Wheel"
parent = "n_car"
wheel3d = { radius = 0.4 }
"#;

#[test]
fn a_wheel_is_driven_braked_and_steered_from_a_script() {
    run_checked(
        CAR,
        r#"pub class Scenario { node: Node, ticks: int, driven: float }
impl Scenario {
    fn fixed_update(dt: float) {
        self.ticks += 1;
        let wheel = self.node.get_node("Wheel");
        if self.ticks == 2 {
            wheel.wheel3d.set_engine_force(200.0);
            wheel.wheel3d.set_steering(0.3);
        }
        if self.ticks == 40 {
            let state = wheel.wheel3d.state();
            if state.engine_force != 200.0 { log::error("set_engine_force reads back {state.engine_force}"); }
            if math::abs(state.steering - 0.3) >= 0.0001 { log::error("set_steering reads back {state.steering}"); }
            if math::abs(state.axle.z) <= 0.1 { log::error("a steered wheel's axle still points along x"); }
            self.driven = math::abs(self.node.vehicle3d.speed());
            if self.driven <= 0.1 { log::error("200 N of engine force left the car at {self.driven}"); }
            wheel.wheel3d.set_engine_force(0.0);
            wheel.wheel3d.set_brake(5.0);
        }
        if self.ticks == 80 {
            if wheel.wheel3d.state().brake != 5.0 { log::error("set_brake reads back {wheel.wheel3d.state().brake}"); }
            let braked = math::abs(self.node.vehicle3d.speed());
            if braked >= self.driven * 0.5 { log::error("the brake slowed the car only from {self.driven} to {braked}"); }
            log::error("checked: wheel");
        }
    }
}
"#,
        81,
        "checked: wheel",
    );
}
