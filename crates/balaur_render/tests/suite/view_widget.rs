//! A `camera2d` naming a `view` widget draws into that widget's box, where
//! the layout put it, and nowhere else in the window. On a real GPU; a
//! machine with none skips.

use std::cell::RefCell;
use std::rc::Rc;

use balaur::{AppConfig, standard_app};
use balaur_core::Stage;

use crate::film::gpu_present;

/// A 160 by 80 view at the window's top left, its camera on a red square at
/// the world's origin; a blue square far off that only a window-wide 2D
/// camera would show.
const SCENE: &str = r#"
[[nodes]]
id = "n_root"
name = "Root"

[[nodes]]
id = "n_view"
name = "View"
parent = "n_root"

[nodes.widget]
kind = "view"
anchor = "top_left"
x = 0
y = 0
width = 160
height = 80

[[nodes]]
id = "n_camera"
name = "Camera"
parent = "n_root"

[nodes.camera2d]
pixels_per_unit = 20.0
hidpi = false
view = "../View"

[[nodes]]
id = "n_square"
name = "Square"
parent = "n_root"

[nodes.shape2d]
color = [1.0, 0.0, 0.0, 1.0]
shape = { kind = "rectangle", size = [1.0, 1.0] }
"#;

const SIDE: u32 = 320;

fn shot() -> image::RgbaImage {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"view\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), SCENE).unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    let frame = Rc::new(RefCell::new(0u64));
    let path = dir.path().join("view.png");
    {
        let path = path.clone();
        app.add_system(Stage::Last, move |eng, _| {
            let now = {
                let mut frame = frame.borrow_mut();
                *frame += 1;
                *frame
            };
            if now == 6 {
                balaur_render::request_screenshot(eng, path.clone());
            }
            if now == 8 {
                eng.request_quit();
            }
        });
    }
    balaur_render::kiss3d_backend::run_offscreen(app, "view", SIDE, SIDE).unwrap();
    image::open(&path).unwrap().to_rgba8()
}

fn red(p: image::Rgba<u8>) -> bool {
    p.0[0] > 150 && p.0[1] < 90 && p.0[2] < 90
}

#[test]
fn a_camera_draws_into_the_view_widget_it_names() {
    if !gpu_present() {
        return;
    }
    let picture = shot();
    let reds: Vec<(u32, u32)> = picture
        .enumerate_pixels()
        .filter(|(_, _, p)| red(**p))
        .map(|(x, y, _)| (x, y))
        .collect();
    assert!(!reds.is_empty(), "the view drew nothing");
    // The square is a unit at 20 pixels, in the middle of the 160 by 80 box.
    let (x0, x1) = (
        reds.iter().map(|r| r.0).min().unwrap(),
        reds.iter().map(|r| r.0).max().unwrap(),
    );
    let (y0, y1) = (
        reds.iter().map(|r| r.1).min().unwrap(),
        reds.iter().map(|r| r.1).max().unwrap(),
    );
    assert!(
        x1 < 160 && y1 < 80,
        "drawn outside the view's box: x {x0}..{x1}, y {y0}..{y1}"
    );
    let (cx, cy) = ((x0 + x1) as f32 / 2.0, (y0 + y1) as f32 / 2.0);
    assert!(
        (cx - 80.0).abs() < 3.0 && (cy - 40.0).abs() < 3.0,
        "not in the box's middle: {cx}, {cy}"
    );
    assert!(
        (x1 - x0) as f32 >= 17.0 && (x1 - x0) as f32 <= 22.0,
        "not a unit at 20 pixels: {}",
        x1 - x0
    );
}
