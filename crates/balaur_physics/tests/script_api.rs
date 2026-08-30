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
        "name = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Body\"\nscript = \"scripts/s.luau\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.luau"),
        format!("local S = {{}}\nfunction S:init()\n{body}\nend\nreturn S\n"),
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
        physics.add_body(self.node, physics.BODY_DYNAMIC)
        physics.add_ball_collider(self.node, 0.5)
        physics.add_cuboid_collider(self.node, 0.5, 0.5, 0.5)
        ",
    );
}

/// Velocity round-trips: what is set is what comes back, and a body that was
/// pushed is moving.
#[test]
fn linear_velocity_is_set_and_read_back() {
    run_clean(
        r#"
        physics.add_body(self.node, physics.BODY_DYNAMIC)
        physics.add_ball_collider(self.node, 0.5)
        physics.set_linear_velocity(self.node, 1.0, 2.0, 3.0)
        local x, y, z = physics.linear_velocity(self.node)
        assert(math.abs(x - 1.0) < 1e-4, "x was not kept: " .. tostring(x))
        assert(math.abs(y - 2.0) < 1e-4, "y was not kept")
        assert(math.abs(z - 3.0) < 1e-4, "z was not kept")
        "#,
    );
}

#[test]
fn an_impulse_starts_a_body_moving() {
    run_clean(
        r#"
        physics.add_body(self.node, physics.BODY_DYNAMIC)
        physics.add_ball_collider(self.node, 0.5)
        physics.apply_impulse(self.node, 10.0, 0.0, 0.0)
        local x = physics.linear_velocity(self.node)
        assert(x > 0.0, "the impulse did nothing: " .. tostring(x))
        "#,
    );
}

#[test]
fn pause_and_sleeping_are_readable_after_being_set() {
    run_clean(
        r"
        physics.set_paused(true)
        assert(physics.is_paused() == true)
        physics.set_paused(false)
        assert(physics.is_paused() == false)

        physics.set_sleeping_allowed(false)
        assert(physics.sleeping_allowed() == false)
        physics.set_sleeping_allowed(true)
        assert(physics.sleeping_allowed() == true)
        ",
    );
}

#[test]
fn gravity_and_clear_are_callable() {
    run_clean(
        r"
        physics.set_gravity(0.0, -1.0, 0.0)
        physics.clear()
        ",
    );
}

#[test]
fn the_2d_world_has_the_same_shape_of_api() {
    run_clean(
        r"
        physics2d.add_body(self.node, physics2d.BODY_DYNAMIC)
        physics2d.add_collider(self.node, { shape = physics2d.SHAPE_CIRCLE, radius = 0.5 })
        physics2d.set_linear_velocity(self.node, 1.0, 2.0)
        local x, y = physics2d.linear_velocity(self.node)
        assert(math.abs(x - 1.0) < 1e-4 and math.abs(y - 2.0) < 1e-4)

        physics2d.set_angular_velocity(self.node, 1.5)
        assert(math.abs(physics2d.angular_velocity(self.node) - 1.5) < 1e-4)

        physics2d.apply_impulse(self.node, 1.0, 0.0)
        physics2d.set_gravity(0.0, -9.81)
        ",
    );
}

/// A binding handed the wrong type must say so rather than take the frame
/// down: the argument came from a script.
#[test]
fn a_wrong_argument_is_reported_not_fatal() {
    let errors = run(r"physics.add_body(self.node, 42)");
    assert!(!errors.is_empty(), "a number was accepted as a body kind");
    assert!(
        errors[0].contains("string") || errors[0].contains("expected"),
        "unhelpful: {errors:#?}"
    );
}
