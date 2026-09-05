//! The `[application] splash`: over everything for its seconds, then gone.

mod support;

use balaur::{AppConfig, standard_app};
#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use support::*;

#[test]
fn the_splash_covers_the_screen_for_its_seconds_and_no_longer() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"w\"\nmain_scene = \"main.toml\"\nsplash = \"splash.png\"\nsplash_seconds = 0.5\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    let mut picture = image::RgbaImage::new(4, 4);
    for pixel in picture.pixels_mut() {
        *pixel = image::Rgba([0, 0, 255, 255]);
    }
    picture.save(dir.path().join("splash.png")).unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    let ctx = egui::Context::default();
    let covers = |out: &egui::FullOutput| {
        out.shapes.iter().any(|s| match &s.shape {
            egui::Shape::Rect(rect) => {
                rect.rect.width() >= 640.0
                    && rect.rect.height() >= 480.0
                    && rect.fill == egui::Color32::BLACK
            }
            _ => false,
        })
    };
    pass(&app, &ctx, vec![]);
    let early = pass(&app, &ctx, vec![]);
    assert!(
        covers(&early),
        "the splash did not cover the screen at the start"
    );
    let pictures = early
        .shapes
        .iter()
        .filter(|s| matches!(s.shape, egui::Shape::Mesh(_)))
        .count();
    assert!(pictures >= 1, "the picture itself was not drawn");
    for _ in 0..40 {
        app.tick(1.0 / 60.0);
    }
    let late = pass(&app, &ctx, vec![]);
    assert!(!covers(&late), "the splash outstayed its seconds");
}
