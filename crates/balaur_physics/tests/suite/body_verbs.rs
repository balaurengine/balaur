//! The `body2d` and `body3d` calls, and the world's `wake_all`, that no game
//! or editor script makes yet, each run from a script and checked by what it
//! changed.

use crate::boot;

/// Boot `scene` with `script` on its `Body` node, tick `ticks` frames, and
/// pass when the only error logged is the script's `done` line.
fn run_checked(scene: &str, script: &str, ticks: usize, done: &str) {
    let mut booted = boot::project(scene, script);
    booted.tick(ticks);
    let errors = boot::errors();
    assert!(
        errors.len() == 1 && errors[0].contains(done),
        "the check did not run clean: {errors:#?}"
    );
}

const BODY_2D: &str = r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_body"
name = "Body"
parent = "n_world"
script = { source = "scripts/scenario.bs" }
body2d = { kind = "dynamic", gravity_scale = 0.0, mass = 2.0, inertia = 1.0, dominance = -3 }
collider2d = { shape = { kind = "circle", radius = 0.5 } }
transform = { position = [0.0, 2.0, 0.0] }

[[nodes]]
id = "n_wall"
name = "Wall"
parent = "n_world"
body2d = { kind = "static" }
collider2d = { shape = { kind = "circle", radius = 0.5 } }
transform = { position = [10.0, 0.0, 0.0] }

[[nodes]]
id = "n_mover"
name = "Mover"
parent = "n_world"
body2d = { kind = "kinematic" }
collider2d = { shape = { kind = "circle", radius = 0.5 } }
character2d = {}
transform = { position = [-10.0, 0.0, 0.0] }
"#;

const BODY_3D: &str = r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_body"
name = "Body"
parent = "n_world"
script = { source = "scripts/scenario.bs" }
body3d = { kind = "dynamic", gravity_scale = 0.0, mass = 2.0, dominance = -3 }
collider3d = { shape = { kind = "sphere", radius = 0.5 } }
transform = { position = [0.0, 2.0, 0.0] }

[[nodes]]
id = "n_wall"
name = "Wall"
parent = "n_world"
body3d = { kind = "static" }
collider3d = { shape = { kind = "sphere", radius = 0.5 } }
transform = { position = [10.0, 0.0, 0.0] }

[[nodes]]
id = "n_floor"
name = "Floor"
parent = "n_world"
collider3d = { shape = { kind = "box", size = [20.0, 1.0, 20.0] } }
transform = { position = [0.0, -20.5, 0.0] }

[[nodes]]
id = "n_dropped"
name = "Dropped"
parent = "n_world"
body3d = { kind = "dynamic" }
transform = { position = [0.0, -19.5, 0.0] }

[[nodes]]
id = "n_shape"
name = "Shape"
parent = "n_dropped"
collider3d = { shape = { kind = "sphere", radius = 0.5 } }
"#;

#[test]
fn a_2d_body_reports_its_mass_energy_and_dominance() {
    run_checked(
        BODY_2D,
        r#"pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        let n = self.node;
        if n.body2d.total_mass() != 2.0 { log::error("total_mass is {n.body2d.total_mass()}, not the stated 2"); }
        n.body2d.set_linear_velocity(vec2(3.0, 0.0));
        let moving = n.body2d.kinetic_energy();
        if math::abs(moving - 9.0) >= 0.001 { log::error("kinetic_energy of 2 kg at 3 m/s is {moving}"); }
        n.body2d.set_linear_velocity(vec2(0.0, 0.0));
        n.body2d.set_angular_velocity(2.0);
        if math::abs(n.body2d.kinetic_energy() - 2.0) >= 0.001 { log::error("a spin of 2 rad/s with inertia 1 holds {n.body2d.kinetic_energy()}"); }
        n.body2d.set_angular_velocity(0.0);
        physics2d::set_gravity(vec2(0.0, -10.0));
        n.body2d.gravity_scale = 1.0;
        let height = n.body2d.potential_energy();
        if math::abs(height - 40.0) >= 0.01 { log::error("potential_energy of 2 kg at 2 m is {height}"); }
        if n.body2d.effective_dominance() != -3 { log::error("a dynamic body's effective_dominance is {n.body2d.effective_dominance()}"); }
        let wall = scene::root().get_node("World/Wall").body2d.effective_dominance();
        if wall != 128 { log::error("a static body outranks with {wall}, not 128"); }
        log::error("checked: 2d readers");
    }
}
"#,
        1,
        "checked: 2d readers",
    );
}

#[test]
fn a_2d_body_takes_a_held_torque_and_a_torque_impulse() {
    run_checked(
        BODY_2D,
        r#"pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        let n = self.node;
        n.body2d.add_constant_torque(2.0);
        n.body2d.add_constant_torque(1.0);
        if n.body2d.constant_torque() != 3.0 { log::error("add_constant_torque summed to {n.body2d.constant_torque()}"); }
        n.body2d.set_constant_torque(0.5);
        if n.body2d.constant_torque() != 0.5 { log::error("set_constant_torque left {n.body2d.constant_torque()}"); }
        n.body2d.set_constant_torque(0.0);
        n.body2d.apply_torque_impulse(2.0);
        if math::abs(n.body2d.angular_velocity() - 2.0) >= 0.0001 { log::error("a torque impulse of 2 on inertia 1 spins at {n.body2d.angular_velocity()}"); }
        log::error("checked: 2d torques");
    }
}
"#,
        1,
        "checked: 2d torques",
    );
}

#[test]
fn a_2d_body_is_put_to_sleep_and_woken_one_by_one_and_all_at_once() {
    run_checked(
        BODY_2D,
        r#"pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        let n = self.node;
        if n.body2d.is_sleeping() { log::error("a new body is asleep"); }
        n.body2d.sleep();
        if !n.body2d.is_sleeping() { log::error("sleep left the body awake"); }
        n.body2d.wake_up();
        if n.body2d.is_sleeping() { log::error("wake_up left the body asleep"); }
        n.body2d.sleep();
        physics2d::wake_all();
        if n.body2d.is_sleeping() { log::error("wake_all left the body asleep"); }
        log::error("checked: 2d sleep");
    }
}
"#,
        1,
        "checked: 2d sleep",
    );
}

#[test]
fn a_2d_kinematic_body_reports_where_it_was_told_to_go() {
    run_checked(
        BODY_2D,
        r#"pub class Scenario { node: Node, ticks: int }
impl Scenario {
    pub fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 2 {
            let mover = scene::root().get_node("World/Mover");
            mover.character2d.move_by(vec2(1.5, 0.0));
            let next = mover.body2d.next_position();
            if math::abs(next.position.x + 8.5) >= 0.001 { log::error("next_position is {next.position.x}, not -8.5"); }
            log::error("checked: 2d next");
        }
    }
}
"#,
        3,
        "checked: 2d next",
    );
}

#[test]
fn a_3d_body_reports_its_mass_energy_and_dominance() {
    run_checked(
        BODY_3D,
        r#"pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        let n = self.node;
        if n.body3d.total_mass() != 2.0 { log::error("total_mass is {n.body3d.total_mass()}, not the stated 2"); }
        n.body3d.set_linear_velocity(vec3(3.0, 0.0, 0.0));
        let moving = n.body3d.kinetic_energy();
        if math::abs(moving - 9.0) >= 0.001 { log::error("kinetic_energy of 2 kg at 3 m/s is {moving}"); }
        if !n.body3d.is_moving() { log::error("a body at 3 m/s is not moving"); }
        n.body3d.set_linear_velocity(vec3(0.0, 0.0, 0.0));
        if n.body3d.is_moving() { log::error("a still body is moving"); }
        physics3d::set_gravity(vec3(0.0, -10.0, 0.0));
        n.body3d.gravity_scale = 1.0;
        let height = n.body3d.potential_energy();
        if math::abs(height - 40.0) >= 0.01 { log::error("potential_energy of 2 kg at 2 m is {height}"); }
        if n.body3d.effective_dominance() != -3 { log::error("a dynamic body's effective_dominance is {n.body3d.effective_dominance()}"); }
        let wall = scene::root().get_node("World/Wall").body3d.effective_dominance();
        if wall != 128 { log::error("a static body outranks with {wall}, not 128"); }
        log::error("checked: 3d readers");
    }
}
"#,
        1,
        "checked: 3d readers",
    );
}

#[test]
fn a_3d_body_predicts_the_pull_of_its_forces() {
    run_checked(
        BODY_3D,
        r#"pub class Scenario { node: Node, ticks: int }
impl Scenario {
    pub fn init() {
        self.node.body3d.add_constant_force(vec3(4.0, 0.0, 0.0));
    }

    pub fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 2 {
            let n = self.node;
            let coasting = n.body3d.predict_position(1.0).position.x;
            let pulled = n.body3d.predict_position_with_forces(1.0).position.x;
            if pulled <= coasting + 0.5 { log::error("4 N on 2 kg for a second reaches {pulled}, coasting {coasting}"); }
            log::error("checked: 3d forces");
        }
    }
}
"#,
        3,
        "checked: 3d forces",
    );
}

#[test]
fn a_3d_body_is_put_to_sleep_and_woken_one_by_one_and_all_at_once() {
    run_checked(
        BODY_3D,
        r#"pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        let n = self.node;
        if n.body3d.is_sleeping() { log::error("a new body is asleep"); }
        n.body3d.sleep();
        if !n.body3d.is_sleeping() { log::error("sleep left the body awake"); }
        n.body3d.wake_up();
        if n.body3d.is_sleeping() { log::error("wake_up left the body asleep"); }
        n.body3d.sleep();
        physics3d::wake_all();
        if n.body3d.is_sleeping() { log::error("wake_all left the body asleep"); }
        log::error("checked: 3d sleep");
    }
}
"#,
        1,
        "checked: 3d sleep",
    );
}

// The 2D twin is `colliders::a_bodys_hardest_contact_counts_its_child_colliders`.
#[test]
fn a_3d_bodys_hardest_contact_counts_its_child_colliders() {
    run_checked(
        BODY_3D,
        r#"pub class Scenario { node: Node, ticks: int }
impl Scenario {
    pub fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 20 {
            let held = scene::root().get_node("World/Dropped").body3d.max_contact_impulse();
            if held <= 0.0 { log::error("a ball resting on the floor by its child collider took no contact"); }
            let floating = self.node.body3d.max_contact_impulse();
            if floating != 0.0 { log::error("a ball touching nothing took {floating}"); }
            log::error("checked: 3d contact");
        }
    }
}
"#,
        21,
        "checked: 3d contact",
    );
}
