//! A sleeping loop played through a real script: what a handler starts after
//! its subsystem's step has run is still counted before the loop sleeps.

use balaur::{App, AppConfig, DEFAULT_FIXED_DT, rest, standard_app};

/// Each timeout moves the node a unit along x and, twice, winds its own
/// one-shot timer again: from inside the step that had just found it stopped.
const SCRIPT: &str = "pub class Relay { node: Node, pub rung: int }
impl Relay {
    fn on_timeout(payload: any) {
        self.rung += 1;
        let at = self.node.transform.position;
        self.node.transform.position = vec3(at.x + 1.0, at.y + 0.0, at.z + 0.0);
        if self.rung < 3 { self.node.timer.running = true; }
    }
}
";

const SCENE: &str = "[[nodes]]
id = \"relay\"
name = \"Relay\"
script = { source = \"scripts/relay.bs\" }

[nodes.timer]
wait_time = 0.05
one_shot = true
autostart = true
";

fn booted() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    let manifest = "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n";
    std::fs::write(dir.path().join("project.toml"), manifest).unwrap();
    std::fs::write(dir.path().join("main.toml"), SCENE).unwrap();
    std::fs::write(dir.path().join("scripts/relay.bs"), SCRIPT).unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

fn x(app: &App) -> f32 {
    let world = app.engine.world();
    let node = balaur_core::scene::find_node(&world, app.engine.root(), "Relay").unwrap();
    world
        .get::<&balaur_core::Transform>(node)
        .unwrap()
        .position
        .x
}

/// Frames as the windowed loop under `[window] low_processor` runs them with
/// no input: one while the simulation is owed it, none once it rests.
fn run_until_rest(app: &mut App, most: usize) -> usize {
    let mut frames = 0;
    while frames < most && rest::busy(&app.engine) {
        app.tick(DEFAULT_FIXED_DT);
        frames += 1;
    }
    frames
}

#[test]
fn a_timer_its_own_handler_winds_again_runs_out_before_the_loop_sleeps() {
    let (_dir, mut app) = booted();
    let frames = run_until_rest(&mut app, 600);
    assert!(
        (x(&app) - 3.0).abs() < f32::EPSILON,
        "the loop slept after {frames} frames with the relay at {}",
        x(&app)
    );
    assert!(frames < 600, "three short timers never came to rest");
    assert!(!rest::busy(&app.engine), "and at rest it stays there");
}
