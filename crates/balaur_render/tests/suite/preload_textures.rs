//! A preloaded scene's pictures go up before anything draws them: a sprite
//! a scene names is decoded off the frame and on the GPU while nothing in the
//! world names it, and `scene::preloaded` says so. On a real GPU; a machine
//! with none skips.

use std::cell::RefCell;
use std::rc::Rc;

use balaur::{AppConfig, standard_app};
use balaur_core::Stage;

use crate::film::gpu_present;

const MAIN: &str = r#"
[[nodes]]
id = "n_view"
name = "View"

[nodes.camera2d]
pixels_per_unit = 20.0
"#;

/// A scene with one sprite, never switched to or instanced here.
const LATER: &str = r#"
[[nodes]]
id = "n_later"
name = "Later"

[nodes.sprite]
texture = "art/later.png"
"#;

/// How many frames the scene took to be ready, and whether it was ever not.
fn frames_to_ready() -> (Option<u64>, bool) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::create_dir_all(dir.path().join("art")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"preload\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), MAIN).unwrap();
    std::fs::write(dir.path().join("scenes/later.toml"), LATER).unwrap();
    image::RgbaImage::from_pixel(256, 256, image::Rgba([200, 40, 40, 255]))
        .save(dir.path().join("art/later.png"))
        .unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    let seen = Rc::new(RefCell::new((0u64, None, false)));
    {
        let seen = seen.clone();
        app.add_system(Stage::Update, move |eng, _| {
            let mut seen = seen.borrow_mut();
            seen.0 += 1;
            if seen.0 == 1 {
                balaur_core::preload::scene(eng, "scenes/later.toml").unwrap();
            }
            if seen.1.is_none() {
                if balaur_core::preload::ready(eng, "scenes/later.toml").unwrap() {
                    seen.1 = Some(seen.0);
                } else {
                    seen.2 = true;
                }
            }
            if seen.1.is_some() || seen.0 > 600 {
                eng.request_quit();
            }
        });
    }
    balaur_render::kiss3d_backend::run_offscreen(app, "preload", 64, 64).unwrap();
    let seen = seen.borrow();
    (seen.1, seen.2)
}

#[test]
fn a_preloaded_scene_s_pictures_go_up_before_anything_draws_them() {
    if !gpu_present() {
        return;
    }
    let (ready_at, waited) = frames_to_ready();
    assert!(
        ready_at.is_some(),
        "the preloaded sprite's picture never went up"
    );
    assert!(
        waited,
        "ready on the frame it was preloaded: nothing was decoded off the frame"
    );
}
