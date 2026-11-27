//! A node whose picture changes keeps its place in the draw order: its slot
//! is built again, kiss3d puts the new object last, and the order is laid out
//! again whole, so the node still draws under the ones after it in the tree.
//! On a real GPU; a machine with none skips.

use std::cell::RefCell;
use std::rc::Rc;

use balaur::{AppConfig, standard_app};
use balaur_core::{Stage, components, scene};

use crate::film::gpu_present;

/// Three squares overlapping left to right, A then B then C in the tree.
const SCENE: &str = r#"
[[nodes]]
id = "n_view"
name = "View"

[nodes.camera2d]
pixels_per_unit = 100.0
hidpi = false

[[nodes]]
id = "n_a"
name = "A"
parent = "n_view"

[nodes.transform]
position = [-0.4, 0.0, 0.0]

[nodes.sprite]
texture = "art/red.png"

[[nodes]]
id = "n_b"
name = "B"
parent = "n_view"

[nodes.sprite]
texture = "art/green.png"

[[nodes]]
id = "n_c"
name = "C"
parent = "n_view"

[nodes.transform]
position = [0.4, 0.0, 0.0]

[nodes.sprite]
texture = "art/blue.png"
"#;

const SIDE: u32 = 320;
const CHANGED: u64 = 4;

fn shot() -> image::RgbaImage {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::create_dir_all(dir.path().join("art")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"order\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), SCENE).unwrap();
    for (name, rgb) in [
        ("red", [220, 30, 30]),
        ("green", [30, 220, 30]),
        ("blue", [30, 30, 220]),
        ("yellow", [230, 220, 30]),
    ] {
        image::RgbaImage::from_pixel(100, 100, image::Rgba([rgb[0], rgb[1], rgb[2], 255]))
            .save(dir.path().join(format!("art/{name}.png")))
            .unwrap();
    }
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
            if now == CHANGED {
                let b = scene::find_node(&eng.world(), eng.root(), "View/B").unwrap();
                let picture = toml::Value::String("art/yellow.png".into());
                components::set_property(eng, b, "sprite", "texture", &picture).unwrap();
            }
        });
    }
    let path = dir.path().join("after.png");
    {
        let path = path.clone();
        app.add_system(Stage::Last, move |eng, _| {
            let now = *frame.borrow();
            if now == CHANGED + 2 {
                balaur_render::request_screenshot(eng, path.clone());
            }
            if now == CHANGED + 4 {
                eng.request_quit();
            }
        });
    }
    balaur_render::kiss3d_backend::run_offscreen(app, "order", SIDE, SIDE).unwrap();
    image::open(&path).unwrap().to_rgba8()
}

#[test]
fn a_node_whose_picture_changes_keeps_its_place_in_the_draw_order() {
    if !gpu_present() {
        return;
    }
    let picture = shot();
    // B and C overlap from 0.1 to 0.5 across, 100 pixels a unit from the middle.
    let [r, g, b, _] = picture.get_pixel(SIDE / 2 + 30, SIDE / 2).0;
    assert!(
        b > 150 && r < 90 && g < 90,
        "where B and C overlap, C should cover B; drew [{r}, {g}, {b}]"
    );
    // A and B overlap from -0.5 to -0.1: B, now yellow, covers A.
    let [r, g, b, _] = picture.get_pixel(SIDE / 2 - 30, SIDE / 2).0;
    assert!(
        r > 150 && g > 150 && b < 90,
        "where A and B overlap, B should cover A; drew [{r}, {g}, {b}]"
    );
}
