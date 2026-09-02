//! Record and replay end to end: a session driven by real input, played back
//! against its own digest chain.

use balaur::input::InputSnapshot;
use balaur::{digest, replay, standard_app, App, AppConfig, FIXED_DT};

const SCRIPT: &str = "pub fn fixed_update(this, dt) {
    if input::is_down(input::KEY_SPACE) { this.node.translate(dt, 0.0, 0.0); }
}
";

fn project(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("project.toml"),
        "name = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Runner\"\nscript = \"scripts/s.rn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("scripts").join("s.rn"), SCRIPT).unwrap();
}

fn booted(dir: &std::path::Path) -> App {
    let mut app = standard_app(AppConfig::dev(dir.to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app
}

fn walked(app: &App) -> f32 {
    let world = app.engine.world();
    let node = balaur_core::scene::find_node(&world, app.engine.root(), "Runner").unwrap();
    let t = world.get::<&balaur_core::Transform>(node).unwrap();
    t.position.x
}

/// Hold Space for ticks 10..20 of 30, so the recording has an edge in it
/// rather than one flat state.
fn record(dir: &std::path::Path) -> Vec<(serde_json::Map<String, serde_json::Value>, u64)> {
    let mut app = booted(dir);
    let mut frames = Vec::new();
    for tick in 0..30 {
        {
            let input = app.engine.resource::<InputSnapshot>();
            let mut input = input.borrow_mut();
            input.begin_frame();
            if tick == 10 {
                input.key_event("Space", true);
            } else if tick == 20 {
                input.key_event("Space", false);
            }
        }
        app.tick(FIXED_DT);
        frames.push((replay::capture(&app.engine), digest::digest(&app.engine).0));
    }
    assert!(
        walked(&app) > 0.0,
        "the recording is worthless if the script never moved the node"
    );
    frames
}

#[test]
fn a_replayed_session_reproduces_every_tick_digest() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let recorded = record(dir.path());

    let mut app = booted(dir.path());
    for (tick, (sources, digest)) in recorded.iter().enumerate() {
        replay::restore(&app.engine, sources);
        app.tick(FIXED_DT);
        assert_eq!(
            digest::digest(&app.engine).0,
            *digest,
            "replay parted from the recording at tick {tick}"
        );
    }
}

/// The control: without the recorded input the same code diverges, which is
/// what proves the test above is checking the input path and not just that
/// two identical runs agree.
#[test]
fn replaying_without_the_recorded_input_diverges() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let recorded = record(dir.path());

    let mut app = booted(dir.path());
    for _ in &recorded {
        app.tick(FIXED_DT);
    }
    assert!(
        walked(&app).abs() < 1e-9,
        "nobody pressed Space this time, so the runner should not have moved"
    );
    assert_ne!(
        digest::digest(&app.engine).0,
        recorded.last().unwrap().1,
        "a run fed no input must not land on the recorded digest"
    );
}
