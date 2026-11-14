//! The render bindings called from a script, headless.
//!
//! Most of this module is state a windowed backend later reads — camera pose,
//! grid settings, per-node shape and colour — so it is all settable and
//! readable without a window. Only `mouse_ray` needs a real viewport, and it
//! is left to a windowed run.

use balaur::{AppConfig, standard_app};
use balaur_core::App;

use crate::LOG;

fn run(body: &str) -> (App, Vec<String>) {
    let (app, errors, _) = run_logged(body);
    (app, errors)
}

/// [`run`], and every line it logged. A caller that reads the buffer after
/// `run` returns has already dropped the lock, and races the next test for it.
fn run_logged(body: &str) -> (App, Vec<String>, Vec<String>) {
    run_frames(body, 1)
}

/// `body` as a node's `init`, then `frames` ticks; what an immediate verb
/// drew is still in its buffer with none.
fn run_frames(body: &str, frames: usize) -> (App, Vec<String>, Vec<String>) {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = project(body);
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
    }
    let lines = balaur_core::logbuf::recent(500);
    let errors = lines
        .iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message.clone())
        .collect();
    let all = lines.into_iter().map(|e| e.message).collect();
    (app, errors, all)
}

/// Why `body` does not compile as a node's `init`.
fn refused(body: &str) -> String {
    let dir = project(body);
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    match app.load_project() {
        Ok(_) => panic!("`{body}` compiled"),
        Err(error) => error.to_string(),
    }
}

/// A project whose one node runs `body` as its `init`.
fn project(body: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"r\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"scenario\"\nname = \"Scenario\"\nscript = { source = \"scripts/scenario.bs\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/scenario.bs"),
        format!(
            "pub class Scenario {{ node: Node }}\nimpl Scenario {{\nfn init() {{\n{body}\n}}\n}}\n"
        ),
    )
    .unwrap();
    dir
}

fn run_clean(body: &str) {
    let (_app, errors) = run(body);
    assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
}

#[test]
fn shapes_can_be_set_from_a_script_in_both_dimensions() {
    run_clean(
        r#"
        self.node.set_component("shape3d", { "shape": { "kind": "sphere", "radius": 0.5 } });
        self.node.set_component("shape3d", { "shape": { "kind": "box", "size": [2.0, 4.0, 6.0] } });
        let kind = self.node.shape3d.shape["kind"];
        if kind != "box" { log::error("the last shape set should win, got {kind}"); }

        self.node.set_component("shape2d", { "shape": { "kind": "circle", "radius": 0.25 } });
        self.node.set_component("shape2d", { "shape": { "kind": "rectangle", "size": [2.0, 4.0] } });
        let kind_2d = self.node.shape2d.shape["kind"];
        if kind_2d != "rectangle" { log::error("the last 2D shape set should win, got {kind_2d}"); }
        "#,
    );
}

#[test]
fn a_colour_set_from_a_script_reads_back() {
    run_clean(
        r#"
        self.node.set_component("shape3d", { "shape": { "kind": "sphere", "radius": 0.5 } });
        self.node.shape3d.color = color(0.25, 0.5, 0.75, 1.0);
        let c = self.node.shape3d.color;
        if math::abs(c.r - 0.25) >= 0.0001 { log::error("red was not kept: {c.r}"); }
        if math::abs(c.g - 0.5) >= 0.0001 { log::error("green was not kept: {c.g}"); }
        if math::abs(c.b - 0.75) >= 0.0001 { log::error("blue was not kept: {c.b}"); }
        "#,
    );
}

#[test]
fn a_colour_may_be_set_without_alpha() {
    run_clean(
        r#"
        self.node.set_component("shape3d", { "shape": { "kind": "sphere", "radius": 0.5 } });
        self.node.patch_component("shape3d", { "color": [1.0, 0.0, 0.0], "shape": { "kind": "sphere", "radius": 0.5 } });
        let r = self.node.shape3d.color.r;
        if math::abs(r - 1.0) >= 0.0001 { log::error("red was not kept: {r}"); }
        "#,
    );
}

/// `camera_pose` reports where the camera actually is, which only a windowed
/// backend knows. Headless the pose stays at its default, so this checks the
/// call surface rather than a round trip.
#[test]
fn the_cameras_pose_can_be_read() {
    run_clean(
        r#"
        let pose = render::camera_pose();
        let eye: Vec3 = pose.eye;
        let target: Vec3 = pose.target;
        if eye.x != 0.0 || target.z != 0.0 || pose.fov != 0.0 { log::error("a headless pose should be zeros"); }
        // Screen maths divides by it; a zero made the editor's pointer NaN.
        let dpi = pose.scale_factor;
        if dpi != 1.0 { log::error("a headless scale should be one, got {dpi}"); }
        let matrix: [float] = render::camera_matrix();
        let ray = render::mouse_ray();
        let origin: Vec3 = ray.origin;
        let direction: Vec3 = ray.direction;
        "#,
    );
}

#[test]
fn the_2d_camera_reports_its_centre_and_zoom() {
    run_clean(
        r#"
        // Like camera_pose, this reports the viewport a backend writes each
        // frame, so headless it stays at its default rather than echoing back.
        let view = render::camera_2d();
        let center: Vec2 = view.center;
        if center.x != 0.0 || view.zoom != 0.0 { log::error("a headless 2D view should be zeros"); }
        "#,
    );
}

#[test]
fn bounds_reports_the_box_a_2d_shape_draws() {
    let (_app, errors, lines) = run_logged(
        r#"
        if render::bounds(self.node) != nil { log::error("a node that draws nothing has no bounds"); }
        self.node.set_component("shape2d", { "shape": { "kind": "rectangle", "size": [2.0, 4.0] } });
        let found = render::bounds(self.node);
        if found == nil {
            log::error("a 2D shape has bounds");
            return;
        }
        let corners = found.unwrap();
        if corners.len() != 2 { log::error("bounds are two vectors, got {corners.len()}"); }
        let half = corners[1];
        if half.x != 1.0 || half.y != 2.0 || half.z != 0.0 { log::error("half the size it draws, got {half.x} {half.y} {half.z}"); }
        log::info("bounds read");
        "#,
    );
    assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
    assert!(
        lines.iter().any(|l| l == "bounds read"),
        "the script never reached its checks: {lines:#?}"
    );
}

#[test]
fn the_background_is_the_colour_a_script_sets() {
    let (app, errors) = run("render::set_background(color(0.1, 0.2, 0.3, 1.0));");
    assert!(errors.is_empty(), "{errors:#?}");
    let clear = app.engine.resource::<balaur_render::ClearColorConfig>();
    let color = clear.borrow().color;
    assert!(matches!(color, [0.1, 0.2, 0.3]), "cleared to {color:?}");
    let why = refused("render::set_background(0.1, 0.2, 0.3);");
    assert!(why.contains("takes 1 argument"), "{why}");
}

#[test]
fn a_line_is_drawn_between_two_vectors_in_a_colour() {
    let (app, errors, _) = run_frames(
        r"render::draw_line_3d(vec3(0.0, 1.0, 2.0), vec3(3.0, 4.0, 5.0), color(1.0, 0.5, 0.0, 0.25));
render::draw_line_3d(vec3(0.0, 0.0, 0.0), vec3(1.0, 0.0, 0.0), color(0.0, 0.0, 1.0, 1.0), 3.0, true, true);
render::draw_line_2d(vec2(0.0, 1.0), vec2(2.0, 3.0), color(1.0, 0.0, 0.0, 1.0), 2.0);",
        0,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let lines_3d = app.engine.resource::<balaur_render::DebugLineBuffer3d>();
    assert_eq!(
        lines_3d.borrow().lines,
        [
            (
                [0.0, 1.0, 2.0],
                [3.0, 4.0, 5.0],
                [1.0, 0.5, 0.0, 0.25],
                1.0,
                false,
                false
            ),
            (
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 1.0],
                3.0,
                true,
                true
            ),
        ]
    );
    let lines_2d = app.engine.resource::<balaur_render::DebugLineBuffer2d>();
    assert_eq!(
        lines_2d.borrow().lines,
        [([0.0, 1.0], [2.0, 3.0], [1.0, 0.0, 0.0, 1.0], 2.0)]
    );
    let why = refused("render::draw_line_2d(0.0, 1.0, 2.0, 3.0, 1.0, 0.0, 0.0, 2.0);");
    assert!(why.contains("takes 3 to 5 argument"), "{why}");
}

#[test]
fn many_3d_lines_cross_in_one_call_each_with_its_own_colour_and_width() {
    let (app, errors, _) = run_frames(
        r"let lines = [
    render::Line3d { from: vec3(0.0, 0.0, 0.0), to: vec3(1.0, 0.0, 0.0), color: color(1.0, 0.0, 0.0, 1.0) },
    render::Line3d { from: vec3(0.0, 2.0, 0.0), to: vec3(0.0, 3.0, 0.0), color: color(0.0, 1.0, 0.0, 0.5), width: 4.0, on_top: true },
];
render::draw_lines_3d(lines);",
        0,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let lines = app.engine.resource::<balaur_render::DebugLineBuffer3d>();
    assert_eq!(
        lines.borrow().lines,
        [
            (
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 0.0, 1.0],
                1.0,
                false,
                false
            ),
            (
                [0.0, 2.0, 0.0],
                [0.0, 3.0, 0.0],
                [0.0, 1.0, 0.0, 0.5],
                4.0,
                false,
                true
            ),
        ]
    );
}

#[test]
fn text_is_drawn_at_a_vector_in_either_pass() {
    let (app, errors, _) = run_frames(
        r#"render::draw_text_2d(vec2(1.0, 2.0), "flat");
render::draw_text_3d(vec3(3.0, 4.0, 5.0), "deep", render::TextOptions { font_size: 20.0 });"#,
        0,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let drawn = app
        .engine
        .resource::<balaur_render::world_text::TextDrawBuffer>();
    let placed: Vec<(String, [f32; 3], bool)> = drawn
        .borrow()
        .items
        .iter()
        .map(|item| (item.text.clone(), item.at, item.in_3d))
        .collect();
    assert_eq!(
        placed,
        [
            ("flat".to_string(), [1.0, 2.0, 0.0], false),
            ("deep".to_string(), [3.0, 4.0, 5.0], true),
        ]
    );
}

#[test]
fn a_missing_app_icon_does_not_take_the_frame_down() {
    let (_app, errors) = run(r#"window::set_app_icon("no/such/icon.png");"#);
    assert!(
        errors.iter().all(|e| !e.contains("panic")),
        "a missing icon panicked: {errors:#?}"
    );
    assert!(
        errors.iter().any(|e| e.contains("no/such/icon.png")),
        "the call never reached the icon: {errors:#?}"
    );
}

/// Any file reads here: the picture is decoded when a window hands it over.
#[test]
fn an_app_icon_keeps_the_plate_a_script_names() {
    let (app, errors) = run(
        r#"window::set_app_icon("project.toml", window::AppIconOptions { plate: color(0.1, 0.2, 0.3, 1.0) });"#,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let icon = app.engine.resource::<balaur_render::AppIconConfig>();
    assert_eq!(icon.borrow().plate, [26, 51, 77, 255]);
    let (app, _) = run(r#"window::set_app_icon("project.toml");"#);
    let icon = app.engine.resource::<balaur_render::AppIconConfig>();
    assert_eq!(icon.borrow().plate, balaur_render::WHITE_PLATE);
}

#[test]
fn an_app_icon_option_it_does_not_know_does_not_compile() {
    let why =
        refused(r#"window::set_app_icon("project.toml", window::AppIconOptions { tint: 1 });"#);
    assert!(why.contains("plate") && why.contains("tint"), "{why}");
}

/// A node with no renderable answers with an empty kind rather than unit.
///
#[test]
fn a_node_with_no_shape_says_so() {
    run_clean(
        r#"
        let bare = self.node.add_child("Bare");
        if bare.has_component("shape3d") { log::error("a bare node should carry no shape3d"); }
        "#,
    );
}

// A box is twelve edges, a sphere three rings of twenty-four, and a capsule
// two rings and four sides: each lands in the frame's lines where it was put.
#[test]
fn a_3d_shape_is_drawn_as_a_wireframe_about_the_vector_it_is_given() {
    let (app, errors, _) = run_frames(
        r"render::draw_box_3d(vec3(10.0, 0.0, 0.0), vec3(2.0, 4.0, 6.0), color(1.0, 1.0, 1.0, 1.0));
render::draw_sphere_3d(vec3(0.0, 20.0, 0.0), 0.5, color(0.0, 1.0, 0.0, 1.0));
render::draw_capsule_3d(vec3(0.0, 0.0, 30.0), 0.25, 1.0, color(0.0, 0.0, 1.0, 1.0));",
        0,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let lines = app.engine.resource::<balaur_render::DebugLineBuffer3d>();
    let lines = lines.borrow();
    assert_eq!(lines.lines.len(), 12 + 3 * 24 + 2 * 24 + 4);
    let reach = |of: &[balaur_render::DebugLine3d], axis: usize| {
        let ends = of.iter().flat_map(|line| [line.0[axis], line.1[axis]]);
        ends.fold((f32::MAX, f32::MIN), |(low, high), at| {
            (low.min(at), high.max(at))
        })
    };
    let (cube, rest) = lines.lines.split_at(12);
    assert_eq!(reach(cube, 0), (9.0, 11.0), "a size of 2 across x");
    assert_eq!(reach(cube, 1), (-2.0, 2.0), "a size of 4 across y");
    assert_eq!(reach(cube, 2), (-3.0, 3.0), "a size of 6 across z");
    let (ball, pill) = rest.split_at(3 * 24);
    assert_eq!(reach(ball, 1), (19.5, 20.5), "a radius of 0.5 about y 20");
    assert!(
        matches!(ball[0].2, [0.0, 1.0, 0.0, 1.0]),
        "the ball is green"
    );
    let (low, high) = reach(pill, 2);
    assert!((low - 29.75).abs() < 1e-5 && (high - 30.25).abs() < 1e-5);
    assert_eq!(reach(pill, 1), (-0.5, 0.5), "a straight part of 1 along y");
}

#[test]
fn a_2d_shape_is_drawn_at_the_vector_it_is_given() {
    use balaur_render::Draw2d;
    let (app, errors, _) = run_frames(
        r#"let triangle = [vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0)];
render::draw_polygon_2d(triangle, color(1.0, 1.0, 0.0, 1.0));
render::draw_circle_2d(vec2(1.0, 2.0), 3.0, color(1.0, 0.0, 0.0, 1.0));
render::draw_rect_2d(vec2(0.5, 1.5), vec2(2.0, 1.0));
render::draw_arc_2d(vec2(4.0, 5.0), 1.0, 0.0, math::PI / 2.0, 2.0);
let line = [vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(2.0, 0.0)];
render::draw_polyline_2d(line, 1.0, color(0.0, 1.0, 0.0, 1.0));
render::draw_texture_2d("art/missing.png", vec2(6.0, 7.0), vec2(8.0, 9.0));"#,
        0,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let drawn = app.engine.resource::<balaur_render::DrawBuffer2d>();
    let drawn = drawn.borrow();
    let shapes: Vec<&Draw2d> = drawn.shapes.iter().map(|d| &d.shape).collect();
    assert!(
        matches!(
            shapes[1],
            Draw2d::Circle {
                center: [1.0, 2.0],
                radius: 3.0,
                ..
            }
        ),
        "{shapes:?}"
    );
    assert!(
        matches!(
            shapes[2],
            Draw2d::Rect {
                center: [0.5, 1.5],
                size: [2.0, 1.0],
                ..
            }
        ),
        "{shapes:?}"
    );
    assert!(
        matches!(
            shapes[3],
            Draw2d::Arc {
                center: [4.0, 5.0],
                ..
            }
        ),
        "{shapes:?}"
    );
    assert!(
        matches!(
            shapes[5],
            Draw2d::Texture {
                center: [6.0, 7.0],
                size: [8.0, 9.0],
                ..
            }
        ),
        "{shapes:?}"
    );
    let why = refused("render::draw_circle_2d(1.0, 2.0, 3.0);");
    assert!(why.contains("Vec2"), "{why}");
}

// The split closes `init` early: an outline resolves in the frame after its
// occluder is written, so the reading is `update`'s.
#[test]
fn an_occluders_outline_answers_its_points_as_vectors() {
    let (_app, errors, lines) = run_frames(
        r#"self.node.set_component("shape2d", { "shape": { "kind": "rectangle", "size": [2.0, 4.0] } });
self.node.set_component("occluder2d", {});
}
fn update(dt: float) {
let points = self.node.occluder2d.outline();
if points.len() == 5 {
    let first: Vec2 = points[0];
    let last: Vec2 = points[4];
    let closed = first.x == last.x && first.y == last.y;
    let wide = math::abs(first.x);
    let tall = math::abs(first.y);
    log::info("outline {closed} {wide} {tall}");
}"#,
        3,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(
        lines.iter().any(|l| l == "outline true 1.0 2.0"),
        "a 2 by 4 rectangle was not traced corner to corner: {lines:#?}"
    );
}

#[test]
fn a_node_is_tinted_by_one_colour_that_its_child_multiplies_in() {
    let (_app, errors, lines) = run_logged(
        r#"let child = self.node.add_child("Child");
self.node.set_tint(color(0.5, 1.0, 1.0, 1.0));
child.set_tint(color(0.5, 0.5, 1.0, 0.5));
let own = self.node.tint();
let seen = child.effective_tint();
log::info("tint {own.r} {seen.r} {seen.g} {seen.a}");"#,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(
        lines.iter().any(|l| l == "tint 0.5 0.25 0.5 0.5"),
        "{lines:#?}"
    );
    let why = refused("self.node.set_tint(0.5, 1.0, 1.0, 1.0);");
    assert!(why.contains("takes 1 argument"), "{why}");
}

#[test]
fn a_textures_size_answers_as_one_vector() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = project(
        r#"let size = render::texture_size("art/sprite.png");
log::info("size {size.x} {size.y}");"#,
    );
    std::fs::create_dir_all(dir.path().join("art")).unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/sprite_200x100.png"
        ),
        dir.path().join("art/sprite.png"),
    )
    .unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    let lines: Vec<String> = balaur_core::logbuf::recent(500)
        .into_iter()
        .map(|e| e.message)
        .collect();
    assert!(lines.iter().any(|l| l == "size 200.0 100.0"), "{lines:#?}");
}

#[test]
fn a_shape_drawn_at_a_z_index_carries_it_and_a_picture_its_region() {
    let (app, errors, _) = run_frames(
        r#"render::draw_circle_2d(vec2(0.0, 0.0), 1.0, color(1.0, 0.0, 0.0, 1.0), render::DrawOptions { z_index: 2 });
render::draw_line_2d(vec2(0.0, 0.0), vec2(1.0, 1.0), color(1.0, 1.0, 1.0, 1.0), 2.0, render::DrawOptions { z_index: -1 });
render::draw_texture_2d("art/missing.png", vec2(0.0, 0.0), vec2(1.0, 1.0), nil, render::TextureOptions { region_origin: vec2(8.0, 0.0), region_size: vec2(8.0, 8.0) });"#,
        0,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let shapes = app
        .engine
        .resource::<balaur_render::DrawBuffer2d>()
        .borrow()
        .shapes
        .clone();
    let placed: Vec<Option<i32>> = shapes.iter().map(|d| d.z_index).collect();
    assert_eq!(placed, [Some(2), Some(-1), None], "{shapes:?}");
    assert!(
        matches!(
            &shapes[2].shape,
            balaur_render::Draw2d::Texture {
                region: Some([8.0, 0.0, 8.0, 8.0]),
                ..
            }
        ),
        "{shapes:?}"
    );
    let why = refused(
        r"render::draw_rect_2d(vec2(0.0, 0.0), vec2(1.0, 1.0), nil, render::DrawOptions { z: 1 });",
    );
    assert!(
        why.contains("no field `z`"),
        "a misspelt option does not compile: {why}"
    );
}

#[test]
fn a_tile_set_from_a_script_reads_back_and_the_map_grows_to_fit() {
    let (app, errors, lines) = run_logged(
        r#"let rows: [any] = [[-1, -1]];
self.node.set_component("tilemap", { "cells": { "kind": "inline", "rows": rows } });
self.node.tilemap.set_cell(3, 1, 7);
log::info("cell {self.node.tilemap.cell(3, 1)} {self.node.tilemap.cell(0, 0)} {self.node.tilemap.cell(9, 9)}");"#,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(
        lines.iter().any(|l| l.contains("cell 7 -1 -1")),
        "expected the cell readback, got {lines:#?}"
    );
    drop(app);
}

/// The scale a sprite that says 0 is drawn at: the image's own import
/// setting, else 100. A tool turning a sprite into a polygon traces at it.
#[test]
fn a_texture_s_pixels_per_unit_is_its_import_setting_or_the_default() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("art")).unwrap();
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    let image = image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255]));
    image.save(root.join("art/set.png")).unwrap();
    image.save(root.join("art/plain.png")).unwrap();
    std::fs::write(
        root.join("art/set.png.import.toml"),
        "pixels_per_unit = 64.0\n",
    )
    .unwrap();
    std::fs::write(
        root.join("project.toml"),
        "[application]\nname = \"r\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("main.toml"),
        "[[nodes]]\nid = \"scenario\"\nname = \"Scenario\"\nscript = { source = \"scripts/scenario.bs\" }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("scripts/scenario.bs"),
        r#"pub class Scenario { node: Node }
impl Scenario {
    fn init() {
        if render::texture_pixels_per_unit("art/set.png") != 64.0 { log::error("the import setting was not read"); }
        if render::texture_pixels_per_unit("art/plain.png") != 100.0 { log::error("an image with none is not at 100"); }
        log::error("checked: pixels per unit");
    }
}
"#,
    )
    .unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut config = AppConfig::dev(root.to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    let errors: Vec<String> = balaur_core::logbuf::recent(500)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect();
    assert!(
        errors.len() == 1 && errors[0].contains("checked: pixels per unit"),
        "{errors:#?}"
    );
}

/// Every loop of an image with `holes`: each island winds counter-clockwise
/// with y up, the way a polygon's outline does, and each hole the other way.
#[test]
fn a_traced_island_winds_counter_clockwise_and_a_hole_clockwise() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("art")).unwrap();
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    // A block, and a ring beside it: two islands and one hole.
    let image = image::RgbaImage::from_fn(24, 10, |x, y| {
        let block = (1..5).contains(&x) && (2..6).contains(&y);
        let ring = (9..21).contains(&x) && (1..9).contains(&y);
        let hole = (12..18).contains(&x) && (3..7).contains(&y);
        let alpha = if block || (ring && !hole) { 255 } else { 0 };
        image::Rgba([255, 255, 255, alpha])
    });
    image.save(root.join("art/shapes.png")).unwrap();
    std::fs::write(
        root.join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("main.toml"),
        "[[nodes]]\nid = \"scenario\"\nname = \"Scenario\"\nscript = { source = \"scripts/scenario.bs\" }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("scripts/scenario.bs"),
        r#"fn windings(loops: [[Vec2]]) -> [str] {
    let out: [str] = [];
    for found in loops {
        out.push(if geometry2d::is_clockwise(found) { "cw" } else { "ccw" });
    }
    return out;
}

fn count(words: [str], word: str) -> int {
    let n = 0;
    for w in words {
        if w == word { n += 1; }
    }
    return n;
}

pub class Scenario { node: Node }
impl Scenario {
    fn init() {
        let all = windings(render::trace_texture("art/shapes.png", render::TraceOptions { tolerance: 0.0, holes: true }));
        let largest = windings(render::trace_texture("art/shapes.png", render::TraceOptions { tolerance: 0.0 }));
        log::error("checked: {all.len()} {largest.len()} {largest[0]}");
        let ccw = count(all, "ccw");
        let cw = count(all, "cw");
        log::error("ccw {ccw} cw {cw}");
    }
}
"#,
    )
    .unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut config = AppConfig::dev(root.to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    let errors: Vec<String> = balaur_core::logbuf::recent(500)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect();
    assert!(
        errors.len() == 2
            && errors[0].contains("checked: 3 1 ccw")
            && errors[1].contains("ccw 2 cw 1"),
        "{errors:#?}"
    );
}
