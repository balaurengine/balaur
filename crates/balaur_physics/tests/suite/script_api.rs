//! The physics bindings called the way a game calls them: from a script.
//!
//! The Rust-side tests next door drive the simulation directly. These check
//! the script surface — that every binding is registered, takes the arguments
//! it claims to, and returns what a script can use.

use balaur::standard_app;

use crate::LOG;

/// Run `body` inside a script's `init`, then report anything logged as an
/// error, or the compile error that stopped the load. A binding that is
/// missing or mistyped shows up there.
fn run(body: &str) -> Vec<String> {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"scenario\"\nname = \"Body\"\nscript = { source = \"scripts/scenario.bs\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/scenario.bs"),
        format!("pub class Scenario {{ node: Node }}\nimpl Scenario {{\n    fn init() {{\n{body}\n    }}\n}}\n"),
    )
    .unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(crate::dev_config(dir.path())).unwrap();
    // A script that does not compile fails the load, before any frame.
    if let Err(e) = app.load_project() {
        return vec![format!("{e:#}")];
    }
    app.tick(1.0 / 60.0);
    balaur_core::logbuf::recent(500)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect()
}

fn run_clean(body: &str) {
    let errors = run(body);
    assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
}

#[test]
fn colliders_can_be_added_in_every_shape_the_api_offers() {
    run_clean(
        r#"
        self.node.set_component("body3d", { "kind": physics3d::BODY_DYNAMIC });
        self.node.set_component("collider3d", { "shape": { "kind": physics3d::SHAPE_SPHERE, "radius": 0.5 } });
        self.node.set_component("collider3d", { "shape": { "kind": physics3d::SHAPE_BOX, "size": [1.0, 1.0, 1.0] } });
        "#,
    );
}

#[test]
fn linear_velocity_is_set_and_read_back() {
    run_clean(
        r#"
        self.node.set_component("body3d", { "kind": physics3d::BODY_DYNAMIC });
        self.node.set_component("collider3d", { "shape": { "kind": physics3d::SHAPE_SPHERE, "radius": 0.5 } });
        self.node.body3d.set_linear_velocity(vec3(1.0, 2.0, 3.0));
        let v = self.node.body3d.linear_velocity();
        if math::abs(v.x - 1.0) >= 0.0001 { log::error("x was not kept: {v.x}"); }
        if math::abs(v.y - 2.0) >= 0.0001 { log::error("y was not kept"); }
        if math::abs(v.z - 3.0) >= 0.0001 { log::error("z was not kept"); }
        "#,
    );
}

#[test]
fn an_impulse_starts_a_body_moving() {
    run_clean(
        r#"
        self.node.set_component("body3d", { "kind": physics3d::BODY_DYNAMIC });
        self.node.set_component("collider3d", { "shape": { "kind": physics3d::SHAPE_SPHERE, "radius": 0.5 } });
        self.node.body3d.apply_impulse(vec3(10.0, 0.0, 0.0));
        let x = self.node.body3d.linear_velocity().x;
        if x <= 0.0 { log::error("the impulse did nothing: {x}"); }
        "#,
    );
}

#[test]
fn pause_and_sleeping_are_readable_after_being_set() {
    run_clean(
        r#"
        physics::set_paused(true);
        if !physics::is_paused() { log::error("set_paused(true) did not stick"); }
        physics::set_paused(false);
        if physics::is_paused() { log::error("set_paused(false) did not stick"); }
        physics::set_sleeping_allowed(false);
        if physics::sleeping_allowed() { log::error("set_sleeping_allowed(false) did not stick"); }
        physics::set_sleeping_allowed(true);
        if !physics::sleeping_allowed() { log::error("set_sleeping_allowed(true) did not stick"); }
        "#,
    );
}

#[test]
fn gravity_and_clear_are_callable() {
    run_clean(
        r"
        physics3d::set_gravity(vec3(0.0, -1.0, 0.0));
        physics::clear();
        ",
    );
}

#[test]
fn the_2d_world_has_the_same_shape_of_api() {
    run_clean(
        r#"
        self.node.set_component("body2d", { "kind": physics2d::BODY_DYNAMIC });
        self.node.set_component("collider2d", { "shape": { "kind": physics2d::SHAPE_CIRCLE, "radius": 0.5 } });
        self.node.body2d.set_linear_velocity(vec2(1.0, 2.0));
        let v = self.node.body2d.linear_velocity();
        if !(math::abs(v.x - 1.0) < 0.0001 && math::abs(v.y - 2.0) < 0.0001) { log::error("the velocity was not kept"); }
        self.node.body2d.set_angular_velocity(1.5);
        if math::abs(self.node.body2d.angular_velocity() - 1.5) >= 0.0001 { log::error("the spin was not kept"); }
        self.node.body2d.apply_impulse(vec2(1.0, 0.0));
        physics2d::set_gravity(vec2(0.0, -9.81));
        "#,
    );
}

#[test]
fn overlaps_returns_an_empty_list_for_a_node_touching_nothing() {
    run_clean(
        r#"
        self.node.set_component("body3d", { "kind": physics3d::BODY_DYNAMIC });
        self.node.set_component("collider3d", { "shape": { "kind": physics3d::SHAPE_SPHERE, "radius": 0.5 } });
        if self.node.collider3d.overlaps().len() != 0 { log::error("3D overlaps should be empty"); }
        self.node.set_component("body2d", { "kind": physics2d::BODY_DYNAMIC });
        self.node.set_component("collider2d", { "shape": { "kind": physics2d::SHAPE_CIRCLE, "radius": 0.5 }, "sensor": true });
        if self.node.collider2d.overlaps().len() != 0 { log::error("2D overlaps should be empty"); }
        "#,
    );
}

// A script that passes a string where a binding takes a vector does not
// compile, and the error names the types.
#[test]
fn a_wrong_argument_is_reported_not_fatal() {
    let errors = run(
        "self.node.set_component(\"body3d\", { \"kind\": physics3d::BODY_DYNAMIC });\n\
         self.node.body3d.apply_impulse(\"sideways\");",
    );
    assert!(!errors.is_empty(), "a string was accepted as an impulse");
    assert!(
        errors[0].contains("str") && errors[0].contains("Vec3"),
        "unhelpful: {errors:#?}"
    );
}

#[test]
fn a_component_call_binds_the_node_for_the_module_driving_it() {
    run_clean(
        r#"
        self.node.set_component("body2d", { "kind": physics2d::BODY_DYNAMIC });
        self.node.body2d.apply_impulse(vec2(1.0, 0.0));
        self.node.body2d.set_linear_velocity(vec2(2.0, 0.0));
        if !self.node.has_component("body2d") {
            log::error("the node should carry the body2d it was given");
        }
        let table = self.node.get_component("body2d");
        if table["kind"].as_str() != "dynamic" {
            log::error("get_component should hand back the component table");
        }
        "#,
    );
}

#[test]
fn a_component_call_no_driving_module_declares_is_refused() {
    let errors = run("self.node.sprite.apply_impulse(vec2(1.0, 0.0));");
    assert!(
        errors.iter().any(|e| e.contains("sprite")),
        "expected an error naming the component, got {errors:#?}"
    );
}

/// As `run_clean`, with proof the script reached its last line: a call that
/// errors ends the script, and nothing after it would log.
fn run_to_the_end(body: &str) {
    let errors = run(&format!("{body}\nlog::error(\"reached the end\");"));
    assert_eq!(errors, ["reached the end"], "the script did not run clean");
}

const BALL_3D: &str = r#"
        self.node.set_component("body3d", { "kind": physics3d::BODY_DYNAMIC, "gravity_scale": 0.0, "mass": 1.0 });
        self.node.set_component("collider3d", { "shape": { "kind": physics3d::SHAPE_SPHERE, "radius": 0.5 } });
"#;

const BALL_2D: &str = r#"
        self.node.set_component("body2d", { "kind": physics2d::BODY_DYNAMIC, "gravity_scale": 0.0, "mass": 1.0 });
        self.node.set_component("collider2d", { "shape": { "kind": physics2d::SHAPE_CIRCLE, "radius": 0.5 } });
"#;

#[test]
fn a_3d_body_is_struck_and_pushed_by_a_vec3() {
    run_to_the_end(&format!(
        r#"{BALL_3D}
        self.node.body3d.set_linear_velocity(vec3(1.0, 2.0, 3.0));
        self.node.body3d.apply_impulse(vec3(0.0, 0.0, 2.0));
        let struck = self.node.body3d.linear_velocity().z;
        if math::abs(struck - 5.0) >= 0.0001 {{ log::error("2 Ns on 1 kg at 3 m/s left z at {{struck}}"); }}
        self.node.body3d.apply_force(vec3(0.0, 60.0, 0.0));
        let pushed = self.node.body3d.linear_velocity().y;
        if math::abs(pushed - 3.0) >= 0.01 {{ log::error("one step of 60 N on 1 kg left y at {{pushed}}"); }}
        self.node.body3d.set_constant_force(vec3(3.0, 0.0, 0.0));
        self.node.body3d.add_constant_force(vec3(1.0, 0.0, 0.0));
        let held = self.node.body3d.constant_force().x;
        if math::abs(held - 4.0) >= 0.0001 {{ log::error("3 N and 1 N more hold {{held}}"); }}
        "#
    ));
}

#[test]
fn a_3d_body_is_turned_by_a_vec3() {
    run_to_the_end(&format!(
        r#"{BALL_3D}
        self.node.body3d.set_angular_velocity(vec3(0.0, 4.0, 0.0));
        let spin = self.node.body3d.angular_velocity().y;
        if math::abs(spin - 4.0) >= 0.0001 {{ log::error("the spin came back as {{spin}}"); }}
        self.node.body3d.apply_torque_impulse(vec3(1.0, 0.0, 0.0));
        let kicked = self.node.body3d.angular_velocity().x;
        if kicked <= 0.0 {{ log::error("a torque impulse about x left x at {{kicked}}"); }}
        self.node.body3d.apply_torque(vec3(0.0, 0.0, 60.0));
        let turned = self.node.body3d.angular_velocity().z;
        if turned <= 0.0 {{ log::error("a torque about z left z at {{turned}}"); }}
        self.node.body3d.set_constant_torque(vec3(0.0, 0.0, 2.0));
        self.node.body3d.add_constant_torque(vec3(0.0, 0.0, 1.0));
        let held = self.node.body3d.constant_torque().z;
        if math::abs(held - 3.0) >= 0.0001 {{ log::error("2 Nm and 1 Nm more hold {{held}}"); }}
        "#
    ));
}

// A push off the centre of mass turns the body as well as moving it, and a
// point away from the axis moves faster than the centre does.
#[test]
fn a_3d_push_at_a_point_takes_the_push_and_the_point_as_two_vectors() {
    run_to_the_end(&format!(
        r#"{BALL_3D}
        self.node.body3d.apply_impulse_at_point(vec3(0.0, 0.0, 1.0), vec3(1.0, 0.0, 0.0));
        let moved = self.node.body3d.linear_velocity().z;
        if math::abs(moved - 1.0) >= 0.0001 {{ log::error("1 Ns on 1 kg left z at {{moved}}"); }}
        let spun = self.node.body3d.angular_velocity().y;
        if spun >= 0.0 {{ log::error("a strike off the centre did not turn the body: {{spun}}"); }}
        let centre = self.node.body3d.velocity_at_point(vec3(0.0, 0.0, 0.0)).z;
        let rim = self.node.body3d.velocity_at_point(vec3(1.0, 0.0, 0.0)).z;
        if rim <= centre {{ log::error("the struck side moves at {{rim}}, the centre at {{centre}}"); }}
        self.node.body3d.apply_force_at_point(vec3(0.0, 60.0, 0.0), vec3(0.0, 0.0, 0.0));
        let pushed = self.node.body3d.linear_velocity().y;
        if math::abs(pushed - 1.0) >= 0.01 {{ log::error("one step of 60 N through the centre left y at {{pushed}}"); }}
        self.node.body3d.add_constant_force_at_point(vec3(0.0, 5.0, 0.0), vec3(1.0, 0.0, 0.0));
        if math::abs(self.node.body3d.constant_force().y - 5.0) >= 0.0001 {{ log::error("the constant force was not kept"); }}
        if self.node.body3d.constant_torque().z <= 0.0 {{ log::error("a constant force off the centre holds no torque"); }}
        "#
    ));
}

#[test]
fn a_2d_body_is_struck_and_pushed_by_a_vec2() {
    run_to_the_end(&format!(
        r#"{BALL_2D}
        self.node.body2d.set_linear_velocity(vec2(1.0, 2.0));
        self.node.body2d.apply_impulse(vec2(2.0, 0.0));
        let struck = self.node.body2d.linear_velocity().x;
        if math::abs(struck - 3.0) >= 0.0001 {{ log::error("2 Ns on 1 kg at 1 m/s left x at {{struck}}"); }}
        self.node.body2d.apply_force(vec2(0.0, 60.0));
        let pushed = self.node.body2d.linear_velocity().y;
        if math::abs(pushed - 3.0) >= 0.01 {{ log::error("one step of 60 N on 1 kg left y at {{pushed}}"); }}
        self.node.body2d.set_constant_force(vec2(3.0, 0.0));
        self.node.body2d.add_constant_force(vec2(1.0, 0.0));
        let held = self.node.body2d.constant_force().x;
        if math::abs(held - 4.0) >= 0.0001 {{ log::error("3 N and 1 N more hold {{held}}"); }}
        "#
    ));
}

#[test]
fn a_2d_push_at_a_point_takes_the_push_and_the_point_as_two_vectors() {
    run_to_the_end(&format!(
        r#"{BALL_2D}
        self.node.body2d.apply_impulse_at_point(vec2(0.0, 1.0), vec2(1.0, 0.0));
        let moved = self.node.body2d.linear_velocity().y;
        if math::abs(moved - 1.0) >= 0.0001 {{ log::error("1 Ns on 1 kg left y at {{moved}}"); }}
        let spun = self.node.body2d.angular_velocity();
        if spun <= 0.0 {{ log::error("a strike off the centre did not turn the body: {{spun}}"); }}
        let centre = self.node.body2d.velocity_at_point(vec2(0.0, 0.0)).y;
        let rim = self.node.body2d.velocity_at_point(vec2(1.0, 0.0)).y;
        if rim <= centre {{ log::error("the struck side moves at {{rim}}, the centre at {{centre}}"); }}
        self.node.body2d.apply_force_at_point(vec2(60.0, 0.0), vec2(0.0, 0.0));
        let pushed = self.node.body2d.linear_velocity().x;
        if math::abs(pushed - 1.0) >= 0.01 {{ log::error("one step of 60 N through the centre left x at {{pushed}}"); }}
        self.node.body2d.add_constant_force_at_point(vec2(0.0, 5.0), vec2(1.0, 0.0));
        if math::abs(self.node.body2d.constant_force().y - 5.0) >= 0.0001 {{ log::error("the constant force was not kept"); }}
        if self.node.body2d.constant_torque() <= 0.0 {{ log::error("a constant force off the centre holds no torque"); }}
        "#
    ));
}

#[test]
fn each_world_takes_its_gravity_as_a_vector_and_reads_it_back() {
    run_to_the_end(
        r#"
        physics3d::set_gravity(vec3(0.0, -1.5, 0.25));
        let down = physics3d::gravity();
        if !(down.y == -1.5 && down.z == 0.25) { log::error("3D gravity came back as {down.y}, {down.z}"); }
        physics2d::set_gravity(vec2(0.5, -980.0));
        let flat = physics2d::gravity();
        if !(flat.x == 0.5 && flat.y == -980.0) { log::error("2D gravity came back as {flat.x}, {flat.y}"); }
        "#,
    );
}

#[test]
fn teleport_puts_a_body_at_the_vector_it_is_given() {
    run_to_the_end(&format!(
        r#"{BALL_3D}
        self.node.body3d.set_linear_velocity(vec3(9.0, 0.0, 0.0));
        self.node.body3d.teleport(vec3(1.0, 2.0, 3.0));
        let at = self.node.body3d.predict_position(0.0).position;
        if !(at.x == 1.0 && at.y == 2.0 && at.z == 3.0) {{ log::error("the 3D body is at {{at.x}}, {{at.y}}, {{at.z}}"); }}
        if self.node.body3d.linear_velocity().x != 0.0 {{ log::error("a teleport kept the velocity"); }}
        {BALL_2D}
        self.node.body2d.teleport(vec2(-4.0, 6.0));
        let flat = self.node.body2d.predict_position(0.0).position;
        if !(flat.x == -4.0 && flat.y == 6.0) {{ log::error("the 2D body is at {{flat.x}}, {{flat.y}}"); }}
        "#
    ));
}

#[test]
fn a_character_moves_by_a_vector_and_answers_the_offset_it_applied() {
    run_to_the_end(
        r#"
        self.node.set_component("collider3d", { "shape": { "kind": physics3d::SHAPE_CAPSULE, "radius": 0.4, "height": 1.0 } });
        self.node.set_component("character3d", { "floor_snap_length": 0.0 });
        let moved = self.node.character3d.move_by(vec3(0.25, 0.0, -0.5));
        if !(math::abs(moved.applied.x - 0.25) < 0.001 && math::abs(moved.applied.z + 0.5) < 0.001) {
            log::error("nothing was in the way, yet it applied {moved.applied.x}, {moved.applied.z}");
        }
        let at = self.node.transform.position;
        if !(math::abs(at.x - 0.25) < 0.001 && math::abs(at.z + 0.5) < 0.001) { log::error("the node is at {at.x}, {at.z}"); }
        "#,
    );
    run_to_the_end(
        r#"
        self.node.set_component("collider2d", { "shape": { "kind": physics2d::SHAPE_CAPSULE, "radius": 0.4, "height": 1.0 } });
        self.node.set_component("character2d", { "floor_snap_length": 0.0 });
        let moved = self.node.character2d.move_by(vec2(0.25, 0.5));
        if !(math::abs(moved.applied.x - 0.25) < 0.001 && math::abs(moved.applied.y - 0.5) < 0.001) {
            log::error("nothing was in the way, yet it applied {moved.applied.x}, {moved.applied.y}");
        }
        let at = self.node.transform.position;
        if !(math::abs(at.x - 0.25) < 0.001 && math::abs(at.y - 0.5) < 0.001) { log::error("the node is at {at.x}, {at.y}"); }
        "#,
    );
}

#[test]
fn a_colliders_boxes_answer_their_two_corners_as_vectors() {
    run_to_the_end(&format!(
        r#"{BALL_3D}
        self.node.body3d.set_linear_velocity(vec3(6.0, 0.0, 0.0));
        let shape = self.node.collider3d.aabb();
        if !(math::abs(shape.min.x + 0.5) < 0.001 && math::abs(shape.max.y - 0.5) < 0.001 && math::abs(shape.max.z - 0.5) < 0.001) {{
            log::error("a ball of radius 0.5 spans {{shape.min.x}} to {{shape.max.y}}");
        }}
        if self.node.collider3d.swept_aabb().max.x <= shape.max.x {{ log::error("the swept box does not reach ahead"); }}
        if self.node.collider3d.collision_aabb().max.x < shape.max.x {{ log::error("the collision box is inside the shape's"); }}
        if self.node.collider3d.broad_phase_aabb().min.x > shape.min.x {{ log::error("the broad-phase box is inside the shape's"); }}
        {BALL_2D}
        self.node.body2d.set_linear_velocity(vec2(0.0, 6.0));
        let flat = self.node.collider2d.aabb();
        if !(math::abs(flat.min.x + 0.5) < 0.001 && math::abs(flat.max.y - 0.5) < 0.001) {{
            log::error("a disc of radius 0.5 spans {{flat.min.x}} to {{flat.max.y}}");
        }}
        if self.node.collider2d.swept_aabb().max.y <= flat.max.y {{ log::error("the 2D swept box does not reach ahead"); }}
        if self.node.collider2d.collision_aabb().max.y < flat.max.y {{ log::error("the 2D collision box is inside the shape's"); }}
        if self.node.collider2d.broad_phase_aabb().min.y > flat.min.y {{ log::error("the 2D broad-phase box is inside the shape's"); }}
        "#
    ));
}

#[test]
fn a_body_refuses_loose_floats_and_the_other_dimensions_vector() {
    for (body, named) in [
        (
            "self.node.body3d.apply_impulse(1.0, 0.0, 0.0);",
            "takes 1 argument",
        ),
        (
            "self.node.body3d.set_linear_velocity(vec2(1.0, 0.0));",
            "Vec3",
        ),
        (
            "self.node.body2d.set_linear_velocity(1.0, 0.0);",
            "takes 1 argument",
        ),
        ("physics2d::set_gravity(vec3(0.0, -9.81, 0.0));", "Vec2"),
    ] {
        let errors = run(body);
        assert!(
            errors.iter().any(|e| e.contains(named)),
            "`{body}` was accepted: {errors:#?}"
        );
    }
}

#[test]
fn a_handle_method_drops_its_components_name() {
    run_clean(
        r#"
        self.node.set_component("body3d", { "kind": physics3d::BODY_DYNAMIC });
        self.node.set_component("collider3d", { "shape": { "kind": physics3d::SHAPE_BOX } });
        if self.node.collider3d.volume() <= 0.0 { log::error("a box encloses nothing"); }
        if self.node.collider3d.total_mass() <= 0.0 { log::error("a box weighs nothing"); }
        if self.node.collider3d.mass_properties().mass <= 0.0 { log::error("its mass properties are empty"); }
        "#,
    );
}

#[test]
fn a_handle_method_spelled_with_its_components_name_is_refused() {
    for (body, named) in [
        ("self.node.collider3d.collider_volume();", "collider3d"),
        (
            "self.node.character3d.move_character_by(vec3(0.0, 0.0, 0.0));",
            "character3d",
        ),
        ("physics3d::collider_volume(self.node);", "collider_volume"),
    ] {
        let errors = run(body);
        assert!(
            errors.iter().any(|e| e.contains(named)),
            "`{body}` was accepted: {errors:#?}"
        );
    }
}

// Every setter owes a reader (N8).
#[test]
fn every_tuning_key_that_can_be_written_reads_back() {
    run_clean(
        r#"
        physics::set_tuning(physics::TuningOptions {
            friction_in_bias_pass: true,
            allowed_linear_error: 0.004,
            max_corrective_velocity: 12.5,
            prediction_distance: 0.006,
            max_linear_velocity: 77.0,
            static_contact_frequency_hz: 41.0,
            static_contact_damping: 3.5,
        });
        let back = physics::tuning().physics2d;
        if !back.friction_in_bias_pass.unwrap() { log::error("friction_in_bias_pass is write-only"); }
        if back.allowed_linear_error.unwrap() - 0.004 >= 0.0001 { log::error("allowed_linear_error"); }
        if back.max_corrective_velocity.unwrap() - 12.5 >= 0.0001 { log::error("max_corrective_velocity"); }
        if back.prediction_distance.unwrap() - 0.006 >= 0.0001 { log::error("prediction_distance"); }
        if back.max_linear_velocity.unwrap() - 77.0 >= 0.0001 { log::error("max_linear_velocity"); }
        if back.static_contact_frequency_hz.unwrap() - 41.0 >= 0.0001 { log::error("static_contact_frequency"); }
        if back.static_contact_damping.unwrap() - 3.5 >= 0.0001 { log::error("static_contact_damping"); }
        "#,
    );
}

// `voxelize` walks resolution cubed. The failing call ends the script's call, so the sane one goes
// first.
#[test]
fn voxelize_refuses_a_resolution_it_would_never_finish() {
    let errors = run(r#"
        let mesh = geometry3d::Mesh {
            points: [vec3(0.0, 0.0, 0.0), vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 1.0)],
            indices: [0, 1, 2, 0, 1, 3, 0, 2, 3, 1, 2, 3],
        };
        let fine = geometry3d::voxelize(mesh, geometry3d::VoxelizeOptions { resolution: 8 });
        if fine.cells.len() == 0 { log::error("a sane resolution stopped working"); }
        geometry3d::voxelize(mesh, geometry3d::VoxelizeOptions { resolution: 100000 });
        log::error("a resolution of 100000 was accepted");
    "#);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0].contains("resolution") && !errors[0].contains("was accepted"),
        "the refusal does not name the resolution: {errors:#?}"
    );
}

// The chassis's own `forward_axis`, not z: a car built along x would read its sideways slide.
#[test]
fn vehicle_speed_measures_along_the_chassis_forward_axis() {
    run_clean(
        r#"
        self.node.set_component("body3d", { "kind": "dynamic" });
        self.node.set_component("collider3d", { "shape": { "kind": "box" } });
        self.node.set_component("vehicle3d", { "forward_axis": physics3d::AXIS_X });
        self.node.body3d.set_linear_velocity(vec3(5.0, 0.0, 0.0));
        let along_x = self.node.vehicle3d.speed();
        if along_x <= 4.9 { log::error("a car built on x reads {along_x} along its own forward"); }
        "#,
    );
}

// A schema property is a field on the component, so one number moves
// without building a table for it.
#[test]
fn a_collider_property_is_read_and_written_as_a_field() {
    run_clean(
        r#"
        self.node.set_component("body3d", { "kind": "dynamic" });
        self.node.set_component("collider3d", { "shape": { "kind": "box" }, "density": 1.0 });
        self.node.collider3d.density = 15.0;
        let back = self.node.collider3d.density;
        if math::abs(back - 15.0) >= 0.0001 { log::error("density came back as {back}"); }
        let heavy = self.node.collider3d.total_mass();
        self.node.collider3d.density = 1.0;
        let light = self.node.collider3d.total_mass();
        if heavy <= light * 10.0 { log::error("{heavy} was not ten times {light}"); }
        "#,
    );
}

#[test]
fn writing_one_property_field_leaves_the_others() {
    run_clean(
        r#"
        self.node.set_component("collider3d", { "shape": { "kind": "sphere", "radius": 0.75 }, "density": 2.5 });
        self.node.collider3d.density = 0.9;
        let radius = self.node.collider3d.shape["radius"].as_float();
        if math::abs(radius - 0.75) >= 0.0001 { log::error("radius became {radius}"); }
        let density = self.node.collider3d.density;
        if math::abs(density - 0.9) >= 0.0001 { log::error("density became {density}"); }
        "#,
    );
}

// A property one component declares is not a field on another, so a typo or
// the wrong component is an error rather than a silent no-op.
#[test]
fn a_property_of_another_component_is_refused() {
    for body in [
        "let d = self.node.body3d.density;",
        "self.node.body3d.density = 4.0;",
    ] {
        let errors = run(body);
        assert!(
            errors.iter().any(|e| e.contains("body3d")),
            "`{body}` was accepted on a body3d: {errors:#?}"
        );
    }
}

#[test]
fn apply_force_pushes_for_one_step_and_leaves_no_constant_force() {
    run_clean(
        r#"
        self.node.set_component("body3d", { "kind": physics3d::BODY_DYNAMIC, "gravity_scale": 0.0, "mass": 1.0 });
        self.node.set_component("collider3d", { "shape": { "kind": physics3d::SHAPE_SPHERE, "radius": 0.5 } });
        self.node.body3d.apply_force(vec3(60.0, 0.0, 0.0));
        let vx = self.node.body3d.linear_velocity().x;
        if !(vx > 0.9 && vx < 1.1) { log::error("one step of 60 N on 1 kg is 1 m/s at 60 Hz, got {vx}"); }
        let fx = self.node.body3d.constant_force().x;
        if fx != 0.0 { log::error("apply_force left a constant force of {fx}"); }
        "#,
    );
}

#[test]
fn each_world_takes_its_own_tuning_over_the_shared_keys() {
    let errors = run(r#"
        physics::set_tuning(physics::TuningOptions {
            length_unit: 2.0,
            soft_resweep_strain: 0.5,
            soft_recovery: physics::SoftRecovery { overlap_split: 5, overlap_patch_constraints: "keep" },
            physics2d: physics::Tuning { length_unit: 64.0, soft_recovery: physics::SoftRecovery { overlap_split: 2 } },
        });
        let both = physics::tuning();
        let unit3 = both.physics3d.length_unit.unwrap();
        if unit3 != 2.0 { log::error("the shared key missed the 3D world: {unit3}"); }
        let unit2 = both.physics2d.length_unit.unwrap();
        if unit2 != 64.0 { log::error("the 2D table lost to the shared key: {unit2}"); }
        if both.physics2d.soft_resweep_strain.unwrap() != 0.5 { log::error("a shared soft row missed the 2D world"); }
        if both.physics3d.soft_recovery.unwrap().overlap_split.unwrap() != 5 { log::error("the shared recovery row missed 3D"); }
        if both.physics2d.soft_recovery.unwrap().overlap_split.unwrap() != 2 { log::error("the 2D recovery row lost"); }
        if both.physics2d.soft_recovery.unwrap().overlap_patch_constraints.unwrap() != "keep" { log::error("a recovery word was dropped"); }
        if both.physics3d.friction_model.unwrap() != "simplified" { log::error("the 3D world lost its friction model"); }
        let counted = physics::counters();
        if !(counted.physics3d.solver_ms >= 0.0 && counted.physics2d.ccd_substep_count >= 0) { log::error("a counter is missing"); }
        log::error("checked: tuning");
    "#);
    assert!(
        errors.len() == 1 && errors[0].contains("checked: tuning"),
        "the check did not run clean: {errors:#?}"
    );
}
