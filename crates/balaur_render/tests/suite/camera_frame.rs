//! A frame is drawn from where its own tick put the camera: the first frame
//! from the scene's camera, and a camera a script moves from where it moved
//! it, not from the frame before. On a real GPU; a machine with none skips.

use std::cell::RefCell;
use std::rc::Rc;

use balaur::{AppConfig, standard_app};
use balaur_core::{Engine, Stage, components, scene, transform};

use crate::film::gpu_present;

/// A camera two units right of the origin, framing a red square there.
const SCENE: &str = r#"
[[nodes]]
id = "n_view"
name = "View"

[[nodes]]
id = "n_camera"
name = "Camera"
parent = "n_view"

[nodes.transform]
position = [2.0, 0.0, 0.0]

[nodes.camera2d]
pixels_per_unit = 20.0
hidpi = false

[[nodes]]
id = "n_square"
name = "Square"
parent = "n_view"

[nodes.transform]
position = [2.0, 0.0, 0.0]

[nodes.shape2d]
color = [1.0, 0.0, 0.0, 1.0]
shape = { kind = "rectangle", size = [1.0, 1.0] }
"#;

const SIDE: u32 = 320;
/// The frame the camera is moved on, and shot.
const MOVED: u64 = 4;

fn move_camera(eng: &Engine, x: f64) {
    let camera = scene::find_node(&eng.world(), eng.root(), "View/Camera").unwrap();
    let at = toml::Value::Array(vec![
        toml::Value::Float(x),
        toml::Value::Float(0.0),
        toml::Value::Float(0.0),
    ]);
    components::set_property(eng, camera, transform::COMPONENT, "position", &at).unwrap();
}

/// The first frame, and the frame the camera moves two units left on.
fn shots() -> Vec<image::RgbaImage> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"camera_frame\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), SCENE).unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    let frame = Rc::new(RefCell::new(0u64));
    {
        let frame = frame.clone();
        app.add_system(Stage::Update, move |eng, _| {
            let now = {
                let mut frame = frame.borrow_mut();
                *frame += 1;
                *frame
            };
            if now == MOVED {
                move_camera(eng, 0.0);
            }
        });
    }
    let paths = [dir.path().join("first.png"), dir.path().join("moved.png")];
    {
        let paths = paths.clone();
        app.add_system(Stage::Last, move |eng, _| {
            let now = *frame.borrow();
            if now == 1 {
                balaur_render::request_screenshot(eng, paths[0].clone());
            }
            if now == MOVED {
                balaur_render::request_screenshot(eng, paths[1].clone());
            }
            if now == MOVED + 2 {
                eng.request_quit();
            }
        });
    }
    balaur_render::kiss3d_backend::run_offscreen(app, "camera_frame", SIDE, SIDE).unwrap();
    paths
        .iter()
        .map(|path| image::open(path).unwrap().to_rgba8())
        .collect()
}

/// The mean column of the red pixels.
fn red_column(picture: &image::RgbaImage) -> Option<f32> {
    let found: Vec<u32> = picture
        .enumerate_pixels()
        .filter(|(_, _, p)| p.0[0] > 150 && p.0[1] < 90 && p.0[2] < 90)
        .map(|(x, _, _)| x)
        .collect();
    (!found.is_empty()).then(|| found.iter().sum::<u32>() as f32 / found.len() as f32)
}

#[test]
fn a_frame_is_drawn_from_where_its_own_tick_put_the_camera() {
    if !gpu_present() {
        return;
    }
    let pictures = shots();
    let middle = SIDE as f32 / 2.0;
    let first = red_column(&pictures[0]).expect("the first frame drew no square");
    assert!(
        (first - middle).abs() < 2.0,
        "the first frame was not drawn from the scene's camera: the square at column {first}"
    );
    // Two units at 20 pixels a unit: the square forty pixels right of the middle.
    let moved = red_column(&pictures[1]).expect("the moved frame drew no square");
    assert!(
        (moved - (middle + 40.0)).abs() < 2.0,
        "the frame the camera moved on was drawn from the frame before: the square at column {moved}"
    );
}
