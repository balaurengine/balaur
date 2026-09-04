//! The physics bindings called the way a game calls them: from a script.
//!
//! The Rust-side tests next door drive the simulation directly. These check
//! the script surface — that every binding is registered, takes the arguments
//! it claims to, and returns what a script can use.

use balaur::{standard_app, AppConfig};

/// The log buffer is global and tests run in parallel, so one test's error
/// would surface in another's assertions.
static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Run `body` inside a script's `init`, then report anything logged as an
/// error. A binding that is missing or mistyped shows up there.
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
        "[[nodes]]\nid = \"n\"\nname = \"Body\"\nscript = \"scripts/s.rn\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.rn"),
        format!("pub fn init(this) {{\n{body}\n}}\n"),
    )
    .unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    balaur_core::logbuf::recent(50)
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
        r"
        physics3d::add_body(this.node, physics3d::BODY_DYNAMIC);
        physics3d::add_ball_collider(this.node, 0.5);
        physics3d::add_cuboid_collider(this.node, 0.5, 0.5, 0.5);
        ",
    );
}

#[test]
fn linear_velocity_is_set_and_read_back() {
    run_clean(
        r#"
        physics3d::add_body(this.node, physics3d::BODY_DYNAMIC);
        physics3d::add_ball_collider(this.node, 0.5);
        physics3d::set_linear_velocity(this.node, 1.0, 2.0, 3.0);
        let (x, y, z) = physics3d::linear_velocity(this.node);
        assert!(math::abs(x - 1.0) < 1e-4, "x was not kept: {}", x);
        assert!(math::abs(y - 2.0) < 1e-4, "y was not kept");
        assert!(math::abs(z - 3.0) < 1e-4, "z was not kept");
        "#,
    );
}

#[test]
fn an_impulse_starts_a_body_moving() {
    run_clean(
        r#"
        physics3d::add_body(this.node, physics3d::BODY_DYNAMIC);
        physics3d::add_ball_collider(this.node, 0.5);
        physics3d::apply_impulse(this.node, 10.0, 0.0, 0.0);
        let (x, _, _) = physics3d::linear_velocity(this.node);
        assert!(x > 0.0, "the impulse did nothing: {}", x);
        "#,
    );
}

#[test]
fn pause_and_sleeping_are_readable_after_being_set() {
    run_clean(
        r"
        physics::set_paused(true);
        assert!(physics::is_paused());
        physics::set_paused(false);
        assert!(!physics::is_paused());

        physics::set_sleeping_allowed(false);
        assert!(!physics::sleeping_allowed());
        physics::set_sleeping_allowed(true);
        assert!(physics::sleeping_allowed());
        ",
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
        r"
        physics2d::add_body(this.node, physics2d::BODY_DYNAMIC);
        physics2d::add_collider(this.node, #{ kind: physics2d::SHAPE_CIRCLE, radius: 0.5 });
        physics2d::set_linear_velocity(this.node, 1.0, 2.0);
        let (x, y) = physics2d::linear_velocity(this.node);
        assert!(math::abs(x - 1.0) < 1e-4 && math::abs(y - 2.0) < 1e-4);

        physics2d::set_angular_velocity(this.node, 1.5);
        assert!(math::abs(physics2d::angular_velocity(this.node) - 1.5) < 1e-4);

        physics2d::apply_impulse(this.node, 1.0, 0.0);
        physics2d::set_gravity(0.0, -9.81);
        ",
    );
}

#[test]
fn overlaps_returns_an_empty_list_for_a_node_touching_nothing() {
    run_clean(
        r#"
        physics3d::add_body(this.node, physics3d::BODY_DYNAMIC);
        physics3d::add_ball_collider(this.node, 0.5);
        let hits = physics3d::overlaps(this.node);
        assert!(hits is Vec && hits.len() == 0, "3D overlaps should be empty");

        physics2d::add_body(this.node, physics2d::BODY_DYNAMIC);
        physics2d::add_collider(this.node, #{ kind: physics2d::SHAPE_CIRCLE, radius: 0.5, sensor: true });
        let hits2 = physics2d::overlaps(this.node);
        assert!(hits2 is Vec && hits2.len() == 0, "2D overlaps should be empty");
        "#,
    );
}

#[test]
fn a_wrong_argument_is_reported_not_fatal() {
    let errors = run(r"physics3d::add_body(this.node, 42);");
    assert!(!errors.is_empty(), "a number was accepted as a body kind");
    assert!(
        errors[0].contains("string") || errors[0].contains("expected"),
        "unhelpful: {errors:#?}"
    );
}

#[test]
fn a_component_handle_binds_the_node_for_the_module_driving_it() {
    run_clean(
        r#"
        physics2d::add_body(this.node, physics2d::BODY_DYNAMIC);
        this.node.body2d.apply_impulse(1.0, 0.0);
        this.node.body2d.set_linear_velocity(2.0, 0.0);
        if !this.node.body2d.has() {
            log::error("the handle should see the body2d it was made for");
        }
        let table = this.node.body2d.get();
        if table.kind != "dynamic" {
            log::error("get() should hand back the component table");
        }
        "#,
    );
}

#[test]
fn a_component_handle_refuses_a_function_no_driving_module_declares() {
    let errors = run("this.node.sprite.apply_impulse(1.0, 0.0);");
    assert!(
        errors.iter().any(|e| e.contains("apply_impulse")),
        "expected an error naming the missing function, got {errors:#?}"
    );
}

/// Every setter owes a reader (N8), and seven of the nineteen keys were
/// write-only.
#[test]
fn every_tuning_key_that_can_be_written_reads_back() {
    run_clean(
        r#"
        physics::set_tuning(#{
            friction_in_bias_pass: true,
            allowed_linear_error: 0.004,
            max_corrective_velocity: 12.5,
            prediction_distance: 0.006,
            max_linear_velocity: 77.0,
            static_contact_frequency: 41.0,
            static_contact_damping: 3.5,
        });
        let back = physics::tuning();
        assert!(back.friction_in_bias_pass, "friction_in_bias_pass is write-only");
        assert!((back.allowed_linear_error - 0.004) < 0.0001, "allowed_linear_error");
        assert!((back.max_corrective_velocity - 12.5) < 0.0001, "max_corrective_velocity");
        assert!((back.prediction_distance - 0.006) < 0.0001, "prediction_distance");
        assert!((back.max_linear_velocity - 77.0) < 0.0001, "max_linear_velocity");
        assert!((back.static_contact_frequency - 41.0) < 0.0001, "static_contact_frequency");
        assert!((back.static_contact_damping - 3.5) < 0.0001, "static_contact_damping");
        "#,
    );
}

/// `voxelize` walks resolution cubed, so a number a script got wrong used to
/// hang the game rather than fail.
#[test]
fn voxelize_refuses_a_resolution_it_would_never_finish() {
    run_clean(
        r#"
        let mesh = #{
            points: [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            indices: [0, 1, 2, 0, 1, 3, 0, 2, 3, 1, 2, 3],
        };
        let (ok, why) = script::attempt(|| geometry3d::voxelize(mesh, #{ resolution: 100000.0 }));
        assert!(!ok, "a resolution of 100000 was accepted");
        let (fine, _) = script::attempt(|| geometry3d::voxelize(mesh, #{ resolution: 8.0 }));
        assert!(fine, "a sane resolution stopped working");
        "#,
    );
}

/// The chassis's own `forward_axis`, not z: a car built along x used to read
/// the speed it was sliding sideways at.
#[test]
fn vehicle_speed_measures_along_the_chassis_forward_axis() {
    run_clean(
        r#"
        this.node.set_component("body3d", #{ kind: "dynamic" });
        this.node.set_component("collider3d", #{ kind: "cuboid" });
        this.node.set_component("vehicle3d", #{ forward_axis: 0.0 });
        physics3d::set_linear_velocity(this.node, 5.0, 0.0, 0.0);
        let along_x = physics3d::vehicle_speed(this.node);
        assert!(along_x > 4.9, "a car built on x reads {} along its own forward", along_x);
        "#,
    );
}
