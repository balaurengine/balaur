//! The watcher and the files no parser reads: textures, models, fonts and
//! sounds.
//!
//! A `.toml` asset is cached as a parsed object, so saving one drops that
//! object. A `.png` is not: it is a source the renderer builds from, the way
//! a `.wesl` shader is. Nothing to drop, so what has to move is the
//! generation counter its consumers watch.

use std::path::Path;
use std::time::{Duration, Instant};

use balaur_core::{App, AppConfig};

/// A 1x1 PNG, so what is written is a file an image decoder would accept.
const PIXEL: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn project(files: &[(&str, &[u8])]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.toml"), "[project]\nname = \"t\"\n").unwrap();
    std::fs::write(dir.path().join("main.rn"), "pub fn init(this) {}\n").unwrap();
    for (name, body) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }
    dir
}

fn app_in(dir: &Path) -> App {
    App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: true,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_rune::factory()),
    })
    .unwrap()
}

fn rune(app: &App) -> balaur_script_rune::RuneHost {
    app.engine
        .script_host()
        .unwrap()
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .expect("the app is running Rune")
        .clone()
}

/// Swallow the events the watcher saw for the files the project was written
/// with, so the test measures only its own save.
#[allow(
    clippy::disallowed_methods,
    reason = "the wait is on an OS file watcher, not on simulation"
)]
fn settle(host: &balaur_script_rune::RuneHost) {
    for _ in 0..10 {
        host.pump_reloads();
        std::thread::sleep(Duration::from_millis(20));
    }
    host.pump_reloads();
}

/// Pump the watcher until `check` holds, or give up. A file watcher is
/// asynchronous on every platform this runs on, so the wait is a real one.
#[allow(
    clippy::disallowed_methods,
    reason = "the wait is on an OS file watcher, not on simulation"
)]
fn pump_until(host: &balaur_script_rune::RuneHost, check: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        host.pump_reloads();
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn saving_a_texture_moves_the_asset_generation() {
    let dir = project(&[("art/hero.png", PIXEL)]);
    let app = app_in(dir.path());
    let host = rune(&app);
    settle(&host);
    let before = balaur_core::assets::generation(&app.engine);
    let mut edited = PIXEL.to_vec();
    edited.extend_from_slice(&[0u8; 4]);
    std::fs::write(dir.path().join("art/hero.png"), &edited).unwrap();
    assert!(
        pump_until(&host, || balaur_core::assets::generation(&app.engine)
            != before),
        "saving a texture left the asset generation at {before}"
    );
}

#[test]
fn saving_a_model_moves_it_too() {
    let dir = project(&[("models/blade.obj", b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n")]);
    let app = app_in(dir.path());
    let host = rune(&app);
    settle(&host);
    let before = balaur_core::assets::generation(&app.engine);
    std::fs::write(
        dir.path().join("models/blade.obj"),
        "v 0 0 0\nv 2 0 0\nv 0 2 0\nf 1 2 3\n",
    )
    .unwrap();
    assert!(
        pump_until(&host, || balaur_core::assets::generation(&app.engine)
            != before),
        "saving a model left the asset generation at {before}"
    );
}

/// The counter is what a renderer rebuilds on, so a file nothing draws from
/// must not move it: every sprite in the scene would be uploaded again.
#[test]
fn saving_a_file_that_is_not_an_asset_leaves_it_alone() {
    let dir = project(&[("notes.md", b"nothing here is an asset\n")]);
    let app = app_in(dir.path());
    let host = rune(&app);
    settle(&host);
    let before = balaur_core::assets::generation(&app.engine);
    std::fs::write(dir.path().join("notes.md"), "still nothing\n").unwrap();
    assert!(
        !pump_until(&host, || balaur_core::assets::generation(&app.engine)
            != before),
        "a saved README moved the asset generation"
    );
}
