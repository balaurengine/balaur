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
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"r\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"N\"\nscript = { source = \"scripts/s.bs\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.bs"),
        format!("pub struct N {{ node: Node }}\nimpl N {{\nfn init() {{\n{body}\n}}\n}}\n"),
    )
    .unwrap();

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

fn run_clean(body: &str) {
    let (_app, errors) = run(body);
    assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
}

#[test]
fn shapes_can_be_set_from_a_script_in_both_dimensions() {
    run_clean(
        r#"
        self.node.set_component("shape3d", map().with("kind", "sphere").with("radius", 0.5));
        self.node.set_component("shape3d", map().with("kind", "box").with("size", [2.0, 4.0, 6.0]));
        let kind = self.node.shape3d.kind;
        if kind != "box" { log::error("the last shape set should win, got {kind}"); }

        self.node.set_component("shape2d", map().with("kind", "circle").with("radius", 0.25));
        self.node.set_component("shape2d", map().with("kind", "rectangle").with("size", [2.0, 4.0]));
        let kind_2d = self.node.shape2d.kind;
        if kind_2d != "rectangle" { log::error("the last 2D shape set should win, got {kind_2d}"); }
        "#,
    );
}

#[test]
fn a_colour_set_from_a_script_reads_back() {
    run_clean(
        r#"
        self.node.set_component("shape3d", map().with("kind", "sphere").with("radius", 0.5));
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
        self.node.set_component("shape3d", map().with("kind", "sphere").with("radius", 0.5));
        self.node.patch_component("shape3d", map().with("color", [1.0, 0.0, 0.0]));
        let r = self.node.shape3d.color.r;
        if math::abs(r - 1.0) >= 0.0001 { log::error("red was not kept: {r}"); }
        "#,
    );
}

/// `set_camera` asks; `camera_pose` reports where the camera actually is,
/// which only a windowed backend knows. Headless the pose stays at its
/// default, so this checks the call surface rather than a round trip.
#[test]
fn the_camera_can_be_aimed_and_its_pose_read() {
    run_clean(
        r#"
        render::set_camera(1.0, 2.0, 3.0, 0.0, 0.0, 0.0);
        let pose = render::camera_pose();
        let count = pose.len();
        if count != 8 { log::error("camera_pose answered {count} numbers"); }
        // Screen maths divides by it; a zero made the editor's pointer NaN.
        let dpi = pose[7];
        if dpi != 1.0 { log::error("a headless scale should be one, got {dpi}"); }
        let matrix: [float] = render::camera_matrix();
        "#,
    );
}

#[test]
fn the_2d_camera_reports_its_centre_and_zoom() {
    run_clean(
        r"
        render::set_camera_2d(4.0, 5.0, 2.0);
        // Like camera_pose, this reports the viewport a backend writes each
        // frame, so headless it stays at its default rather than echoing back.
        let view: Vec3 = render::camera_2d();
        ",
    );
}

#[test]
fn bounds_reports_the_box_a_2d_shape_draws() {
    let (_app, errors, lines) = run_logged(
        r#"
        if render::bounds(self.node).is_some() { log::error("a node that draws nothing has no bounds"); }
        self.node.set_component("shape2d", map().with("kind", "rectangle").with("size", [2.0, 4.0]));
        let found = render::bounds(self.node);
        if found.is_none() {
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
fn the_grid_and_background_are_settable() {
    run_clean(
        r"
        render::set_grid(true, 1.0, 10, 100);
        render::set_grid_colors(0.2, 0.2, 0.2, 0.4, 0.4, 0.4);
        render::set_background(0.1, 0.1, 0.1);
        ",
    );
}

#[test]
fn debug_lines_can_be_drawn_in_both_dimensions() {
    run_clean(
        r"
        render::draw_line_3d(0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0);
        render::draw_line_2d(0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 2.0);
        ",
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
    let (app, errors) =
        run(r#"window::set_app_icon("project.toml", map().with("plate", [0.1, 0.2, 0.3]));"#);
    assert!(errors.is_empty(), "{errors:#?}");
    let icon = app.engine.resource::<balaur_render::AppIconConfig>();
    assert_eq!(icon.borrow().plate, [26, 51, 77, 255]);
    let (app, _) = run(r#"window::set_app_icon("project.toml");"#);
    let icon = app.engine.resource::<balaur_render::AppIconConfig>();
    assert_eq!(icon.borrow().plate, balaur_render::WHITE_PLATE);
}

#[test]
fn an_app_icon_option_it_does_not_know_is_refused_by_name() {
    let (_app, errors) = run(r#"window::set_app_icon("project.toml", map().with("tint", 1));"#);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("plate") && e.contains("tint")),
        "{errors:#?}"
    );
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

#[test]
fn immediate_shapes_are_accepted_from_a_script() {
    run_clean(
        r#"render::draw_box(0.0, 0.0, 0.0, 0.5, 0.5, 0.5, [1.0, 1.0, 1.0]);
render::draw_sphere(1.0, 0.0, 0.0, 0.5, [0.0, 1.0, 0.0]);
render::draw_capsule(2.0, 0.0, 0.0, 0.25, 1.0, [0.0, 0.0, 1.0]);
let triangle: [any] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
render::draw_polygon_2d(triangle, [1.0, 1.0, 0.0, 1.0]);
render::draw_circle_2d(0.0, 0.0, 1.0, [1.0, 0.0, 0.0]);
render::draw_rect_2d(0.5, 0.5, 2.0, 1.0);
render::draw_arc_2d(0.0, 0.0, 1.0, 0.0, math::PI / 2.0, 2.0);
let line: [any] = [[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]];
render::draw_polyline_2d(line, 1.0, [0.0, 1.0, 0.0, 1.0]);
render::draw_texture_2d("art/missing.png", 0.0, 0.0, 1.0, 1.0);"#,
    );
}

#[test]
fn a_shape_drawn_at_a_z_index_carries_it_and_a_picture_its_region() {
    let (app, errors, _) = run_frames(
        r#"render::draw_circle_2d(0.0, 0.0, 1.0, [1.0, 0.0, 0.0], map().with("z_index", 2));
render::draw_line_2d(0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 2.0, map().with("z_index", -1));
render::draw_texture_2d("art/missing.png", 0.0, 0.0, 1.0, 1.0, nil(), map().with("region_origin", [8, 0]).with("region_size", [8, 8]));"#,
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
    let (_app, errors) =
        run(r#"render::draw_rect_2d(0.0, 0.0, 1.0, 1.0, nil(), map().with("z", 1));"#);
    assert!(
        errors.iter().any(|e| e.contains("not 'z'")),
        "a misspelt option is refused: {errors:#?}"
    );
}

#[test]
fn a_tile_set_from_a_script_reads_back_and_the_map_grows_to_fit() {
    let (app, errors, lines) = run_logged(
        r#"let cells: [any] = [[-1, -1]];
self.node.set_component("tilemap", map().with("cells", cells));
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
        "[[nodes]]\nid = \"n\"\nname = \"N\"\nscript = { source = \"scripts/s.bs\" }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("scripts/s.bs"),
        r#"pub struct N { node: Node }
impl N {
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
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"N\"\nscript = { source = \"scripts/s.bs\" }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("scripts/s.bs"),
        r#"fn windings(loops: [[Vec2]]) -> [str] {
    let out: [str] = [];
    for found in loops {
        let points: [any] = [];
        for p in found {
            points.push([p.x, p.y]);
        }
        out.push(if geometry2d::is_clockwise(points) { "cw" } else { "ccw" });
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

pub struct N { node: Node }
impl N {
    fn init() {
        let all = windings(render::trace_texture("art/shapes.png", map().with("tolerance", 0.0).with("holes", true)));
        let largest = windings(render::trace_texture("art/shapes.png", map().with("tolerance", 0.0)));
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
