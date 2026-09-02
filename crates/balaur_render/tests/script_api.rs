//! The render bindings called from a script, headless.
//!
//! Most of this module is state a windowed backend later reads — camera pose,
//! grid settings, per-node shape and colour — so it is all settable and
//! readable without a window. Only `mouse_ray` needs a real viewport, and it
//! is left to a windowed run.

use balaur::{standard_app, AppConfig};
use balaur_core::App;

/// The log buffer is global and tests run in parallel.
static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn run(body: &str) -> (App, Vec<String>) {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"r\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"N\"\nscript = \"scripts/s.luau\"\n",
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
    let errors = balaur_core::logbuf::recent(50)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect();
    (app, errors)
}

fn run_clean(body: &str) {
    let (_app, errors) = run(body);
    assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
}

#[test]
fn shapes_can_be_set_from_a_script_in_both_dimensions() {
    run_clean(
        r#"
        render.set_ball(self.node, 0.5)
        render.set_cuboid(self.node, 1.0, 2.0, 3.0)
        local kind, a = render.shape3d(self.node)
        assert(kind == "cuboid", "the last shape set should win, got " .. tostring(kind))

        render.set_circle(self.node, 0.25)
        render.set_rect(self.node, 1.0, 2.0)
        assert(render.shape2d(self.node) ~= nil)
        "#,
    );
}

#[test]
fn a_colour_set_from_a_script_reads_back() {
    run_clean(
        r#"
        render.set_ball(self.node, 0.5)
        render.set_color(self.node, 0.25, 0.5, 0.75, 1.0)
        local r, g, b, a = render.color(self.node)
        assert(math.abs(r - 0.25) < 1e-4, "red was not kept: " .. tostring(r))
        assert(math.abs(g - 0.5) < 1e-4)
        assert(math.abs(b - 0.75) < 1e-4)
        "#,
    );
}

#[test]
fn a_colour_may_be_set_without_alpha() {
    run_clean(
        r"
        render.set_ball(self.node, 0.5)
        render.set_color(self.node, 1.0, 0.0, 0.0)
        local r = render.color(self.node)
        assert(math.abs(r - 1.0) < 1e-4)
        ",
    );
}

/// `set_camera` asks; `camera_pose` reports where the camera actually is,
/// which only a windowed backend knows. Headless the pose stays at its
/// default, so this checks the call surface rather than a round trip.
#[test]
fn the_camera_can_be_aimed_and_its_pose_read() {
    run_clean(
        r#"
        render.set_camera(1.0, 2.0, 3.0, 0.0, 0.0, 0.0)
        local ex, ey, ez, tx, ty, tz = render.camera_pose()
        for _, v in {ex, ey, ez, tx, ty, tz} do
            assert(type(v) == "number", "camera_pose returned a non-number")
        end
        assert(render.camera_matrix() ~= nil)
        "#,
    );
}

#[test]
fn the_2d_camera_reports_its_centre_and_zoom() {
    run_clean(
        r#"
        render.set_camera_2d(4.0, 5.0, 2.0)
        local cx, cy, zoom = render.camera_2d()
        -- Like camera_pose, this reports the viewport a backend writes each
        -- frame, so headless it stays at its default rather than echoing back.
        assert(type(cx) == "number" and type(cy) == "number", "centre is not numeric")
        assert(type(zoom) == "number", "zoom is not numeric")
        "#,
    );
}

#[test]
fn the_grid_background_and_camera_input_are_settable() {
    run_clean(
        r"
        render.set_grid(true, 1.0, 10, 100.0)
        render.set_grid_colors(0.2, 0.2, 0.2, 0.4, 0.4, 0.4)
        render.set_background(0.1, 0.1, 0.1)
        render.set_camera_input(false)
        render.set_camera_input(true)
        ",
    );
}

#[test]
fn debug_lines_can_be_drawn_in_both_dimensions() {
    run_clean(
        r"
        render.draw_line(0, 0, 0, 1, 1, 1, 1.0, 0.0, 0.0)
        render.draw_line_2d(0, 0, 1, 1, 1.0, 0.0, 0.0, 2.0)
        ",
    );
}

#[test]
fn a_missing_app_icon_does_not_take_the_frame_down() {
    let (_app, errors) = run(r#"render.set_app_icon("no/such/icon.png")"#);
    assert!(
        errors.iter().all(|e| !e.contains("panic")),
        "a missing icon panicked: {errors:#?}"
    );
}

/// A node with no renderable answers with an empty kind rather than nil.
///
/// Worth pinning because it is inconsistent with `node:script_path()`, which
/// returns nil when there is nothing: a script has to test `kind ~= ""` here
/// and `path ~= nil` there. Recorded as the contract until one of them moves.
#[test]
fn a_node_with_no_shape_answers_with_an_empty_kind() {
    run_clean(
        r#"
        local bare = self.node:add_child("Bare")
        local kind = render.shape3d(bare)
        assert(kind == "", "expected an empty kind, got " .. tostring(kind))
        "#,
    );
}
