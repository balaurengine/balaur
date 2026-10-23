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
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Body\"\nscript = { source = \"scripts/s.bs\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.bs"),
        format!("pub struct S {{ node: Node }}\nimpl S {{\n    fn init() {{\n{body}\n    }}\n}}\n"),
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
        self.node.set_component("body3d", map().with("kind", physics3d::BODY_DYNAMIC));
        self.node.set_component("collider3d", map().with("kind", physics3d::SHAPE_SPHERE).with("radius", 0.5));
        self.node.set_component("collider3d", map().with("kind", physics3d::SHAPE_BOX).with("size", [1.0, 1.0, 1.0]));
        "#,
    );
}

#[test]
fn linear_velocity_is_set_and_read_back() {
    run_clean(
        r#"
        self.node.set_component("body3d", map().with("kind", physics3d::BODY_DYNAMIC));
        self.node.set_component("collider3d", map().with("kind", physics3d::SHAPE_SPHERE).with("radius", 0.5));
        self.node.body3d.set_linear_velocity(1.0, 2.0, 3.0);
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
        self.node.set_component("body3d", map().with("kind", physics3d::BODY_DYNAMIC));
        self.node.set_component("collider3d", map().with("kind", physics3d::SHAPE_SPHERE).with("radius", 0.5));
        self.node.body3d.apply_impulse(10.0, 0.0, 0.0);
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
        physics3d::set_gravity(0.0, -1.0, 0.0);
        physics::clear();
        ",
    );
}

#[test]
fn the_2d_world_has_the_same_shape_of_api() {
    run_clean(
        r#"
        self.node.set_component("body2d", map().with("kind", physics2d::BODY_DYNAMIC));
        self.node.set_component("collider2d", map().with("kind", physics2d::SHAPE_CIRCLE).with("radius", 0.5));
        self.node.body2d.set_linear_velocity(1.0, 2.0);
        let v = self.node.body2d.linear_velocity();
        if !(math::abs(v.x - 1.0) < 0.0001 && math::abs(v.y - 2.0) < 0.0001) { log::error("the velocity was not kept"); }
        self.node.body2d.set_angular_velocity(1.5);
        if math::abs(self.node.body2d.angular_velocity() - 1.5) >= 0.0001 { log::error("the spin was not kept"); }
        self.node.body2d.apply_impulse(1.0, 0.0);
        physics2d::set_gravity(0.0, -9.81);
        "#,
    );
}

#[test]
fn overlaps_returns_an_empty_list_for_a_node_touching_nothing() {
    run_clean(
        r#"
        self.node.set_component("body3d", map().with("kind", physics3d::BODY_DYNAMIC));
        self.node.set_component("collider3d", map().with("kind", physics3d::SHAPE_SPHERE).with("radius", 0.5));
        if self.node.collider3d.overlaps().len() != 0 { log::error("3D overlaps should be empty"); }
        self.node.set_component("body2d", map().with("kind", physics2d::BODY_DYNAMIC));
        self.node.set_component("collider2d", map().with("kind", physics2d::SHAPE_CIRCLE).with("radius", 0.5).with("sensor", true));
        if self.node.collider2d.overlaps().len() != 0 { log::error("2D overlaps should be empty"); }
        "#,
    );
}

/// A script that passes a string where a binding takes a number does not
/// compile, and the error names the types.
#[test]
fn a_wrong_argument_is_reported_not_fatal() {
    let errors = run(
        "self.node.set_component(\"body3d\", map().with(\"kind\", physics3d::BODY_DYNAMIC));\n\
         self.node.body3d.apply_impulse(\"sideways\", 0.0, 0.0);",
    );
    assert!(!errors.is_empty(), "a string was accepted as an impulse");
    assert!(
        errors[0].contains("str") && errors[0].contains("float"),
        "unhelpful: {errors:#?}"
    );
}

#[test]
fn a_component_call_binds_the_node_for_the_module_driving_it() {
    run_clean(
        r#"
        self.node.set_component("body2d", map().with("kind", physics2d::BODY_DYNAMIC));
        self.node.body2d.apply_impulse(1.0, 0.0);
        self.node.body2d.set_linear_velocity(2.0, 0.0);
        if !self.node.has_component("body2d") {
            log::error("the node should carry the body2d it was given");
        }
        let table = self.node.get_component("body2d");
        if table.get("kind").as_str() != "dynamic" {
            log::error("get_component should hand back the component table");
        }
        "#,
    );
}

#[test]
fn a_component_call_no_driving_module_declares_is_refused() {
    let errors = run("self.node.sprite.apply_impulse(1.0, 0.0);");
    assert!(
        errors.iter().any(|e| e.contains("sprite")),
        "expected an error naming the component, got {errors:#?}"
    );
}

/// Every setter owes a reader (N8), and seven of the nineteen keys were
/// write-only.
#[test]
fn every_tuning_key_that_can_be_written_reads_back() {
    run_clean(
        r#"
        physics::set_tuning(map()
            .with("friction_in_bias_pass", true)
            .with("allowed_linear_error", 0.004)
            .with("max_corrective_velocity", 12.5)
            .with("prediction_distance", 0.006)
            .with("max_linear_velocity", 77.0)
            .with("static_contact_frequency_hz", 41.0)
            .with("static_contact_damping", 3.5));
        let back = physics::tuning().get("2d");
        if !back.get("friction_in_bias_pass").as_bool() { log::error("friction_in_bias_pass is write-only"); }
        if back.get("allowed_linear_error").as_float() - 0.004 >= 0.0001 { log::error("allowed_linear_error"); }
        if back.get("max_corrective_velocity").as_float() - 12.5 >= 0.0001 { log::error("max_corrective_velocity"); }
        if back.get("prediction_distance").as_float() - 0.006 >= 0.0001 { log::error("prediction_distance"); }
        if back.get("max_linear_velocity").as_float() - 77.0 >= 0.0001 { log::error("max_linear_velocity"); }
        if back.get("static_contact_frequency_hz").as_float() - 41.0 >= 0.0001 { log::error("static_contact_frequency"); }
        if back.get("static_contact_damping").as_float() - 3.5 >= 0.0001 { log::error("static_contact_damping"); }
        "#,
    );
}

/// `voxelize` walks resolution cubed, so a number a script got wrong used to
/// hang the game rather than fail. The failing call ends the script's call,
/// so the sane one goes first.
#[test]
fn voxelize_refuses_a_resolution_it_would_never_finish() {
    let errors = run(r#"
        let mesh = map()
            .with("points", [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
            .with("indices", [0, 1, 2, 0, 1, 3, 0, 2, 3, 1, 2, 3]);
        let fine = geometry3d::voxelize(mesh, map().with("resolution", 8.0));
        if fine.kind() != "map" { log::error("a sane resolution stopped working"); }
        geometry3d::voxelize(mesh, map().with("resolution", 100000.0));
        log::error("a resolution of 100000 was accepted");
    "#);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0].contains("resolution") && !errors[0].contains("was accepted"),
        "the refusal does not name the resolution: {errors:#?}"
    );
}

/// The chassis's own `forward_axis`, not z: a car built along x used to read
/// the speed it was sliding sideways at.
#[test]
fn vehicle_speed_measures_along_the_chassis_forward_axis() {
    run_clean(
        r#"
        self.node.set_component("body3d", map().with("kind", "dynamic"));
        self.node.set_component("collider3d", map().with("kind", "box"));
        self.node.set_component("vehicle3d", map().with("forward_axis", physics3d::AXIS_X));
        self.node.body3d.set_linear_velocity(5.0, 0.0, 0.0);
        let along_x = self.node.vehicle3d.vehicle_speed();
        if along_x <= 4.9 { log::error("a car built on x reads {along_x} along its own forward"); }
        "#,
    );
}

/// A schema property is a field on the component, so one number moves
/// without building a table for it.
#[test]
fn a_collider_property_is_read_and_written_as_a_field() {
    run_clean(
        r#"
        self.node.set_component("body3d", map().with("kind", "dynamic"));
        self.node.set_component("collider3d", map().with("kind", "box").with("density", 1.0));
        self.node.collider3d.density = 15.0;
        let back = self.node.collider3d.density;
        if math::abs(back - 15.0) >= 0.0001 { log::error("density came back as {back}"); }
        let heavy = self.node.collider3d.collider_mass();
        self.node.collider3d.density = 1.0;
        let light = self.node.collider3d.collider_mass();
        if heavy <= light * 10.0 { log::error("{heavy} was not ten times {light}"); }
        "#,
    );
}

/// Writing a field leaves the rest of the component alone, as `patch` does.
#[test]
fn writing_one_property_field_leaves_the_others() {
    run_clean(
        r#"
        self.node.set_component("collider3d", map().with("kind", "sphere").with("radius", 0.75).with("friction", 0.25));
        self.node.collider3d.friction = 0.9;
        let radius = self.node.collider3d.radius;
        if math::abs(radius - 0.75) >= 0.0001 { log::error("radius became {radius}"); }
        let friction = self.node.collider3d.friction;
        if math::abs(friction - 0.9) >= 0.0001 { log::error("friction became {friction}"); }
        "#,
    );
}

/// A property one component declares is not a field on another, so a typo or
/// the wrong component is an error rather than a silent no-op.
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
        self.node.set_component("body3d", map().with("kind", physics3d::BODY_DYNAMIC).with("gravity_scale", 0.0).with("mass", 1.0));
        self.node.set_component("collider3d", map().with("kind", physics3d::SHAPE_SPHERE).with("radius", 0.5));
        self.node.body3d.apply_force(60.0, 0.0, 0.0);
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
        physics::set_tuning(map()
            .with("length_unit", 2.0)
            .with("soft_resweep_strain", 0.5)
            .with("soft_recovery", map().with("overlap_split", 5).with("overlap_patch_constraints", "keep"))
            .with("2d", map().with("length_unit", 64.0).with("soft_recovery", map().with("overlap_split", 2))));
        let both = physics::tuning();
        let unit3 = both.get("3d").get("length_unit").as_float();
        if unit3 != 2.0 { log::error("the shared key missed the 3D world: {unit3}"); }
        let unit2 = both.get("2d").get("length_unit").as_float();
        if unit2 != 64.0 { log::error("the 2D table lost to the shared key: {unit2}"); }
        if both.get("2d").get("soft_resweep_strain").as_float() != 0.5 { log::error("a shared soft row missed the 2D world"); }
        if both.get("3d").get("soft_recovery").get("overlap_split").as_int() != 5 { log::error("the shared recovery row missed 3D"); }
        if both.get("2d").get("soft_recovery").get("overlap_split").as_int() != 2 { log::error("the 2D recovery row lost"); }
        if both.get("2d").get("soft_recovery").get("overlap_patch_constraints").as_str() != "keep" { log::error("a recovery word was dropped"); }
        if both.get("3d").get("friction_model").as_str() != "simplified" { log::error("the 3D world lost its friction model"); }
        let counted = physics::counters();
        if !(counted.get("physics3d").has("solver_ms") && counted.get("physics2d").has("ccd_substep_count")) { log::error("a counter is missing"); }
        log::error("checked: tuning");
    "#);
    assert!(
        errors.len() == 1 && errors[0].contains("checked: tuning"),
        "the check did not run clean: {errors:#?}"
    );
}
