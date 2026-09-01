//! `fixed_update` against `update`, through a real script in both languages.
//!
//! The split only means something if the two callbacks disagree about time,
//! so these tick a deliberately ragged frame and assert they diverge.

use balaur::{standard_app, AppConfig, App, FIXED_DT};

/// Both callbacks walk the node along an axis by the dt they were handed:
/// x accumulates fixed time, z accumulates measured time.
const LUAU: (&str, &str) = (
    "s.luau",
    "local S = {}
function S:fixed_update(dt) self.node:translate(dt, 0, 0) end
function S:update(dt) self.node:translate(0, 0, dt) end
return S
",
);

const RUNE: (&str, &str) = (
    "s.rn",
    "pub fn fixed_update(this, dt) { this.node.translate(dt, 0.0, 0.0); }
pub fn update(this, dt) { this.node.translate(0.0, 0.0, dt); }
",
);

fn project(dir: &std::path::Path, language: Option<&str>, script: (&str, &str)) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    let lang = language.map_or(String::new(), |l| format!("language = \"{l}\"\n"));
    std::fs::write(
        dir.join("project.toml"),
        format!("name = \"t\"\nmain_scene = \"main.toml\"\n{lang}"),
    )
    .unwrap();
    std::fs::write(
        dir.join("main.toml"),
        format!(
            "[[nodes]]\nid = \"n\"\nname = \"Walker\"\nscript = \"scripts/{}\"\n",
            script.0
        ),
    )
    .unwrap();
    std::fs::write(dir.join("scripts").join(script.0), script.1).unwrap();
}

/// The walker's (x, z): fixed time against measured time.
fn walked(app: &App) -> (f32, f32) {
    let world = app.engine.world();
    let node = balaur_core::scene::find_node(&world, app.engine.root(), "Walker")
        .expect("the scene has a Walker");
    let t = world.get::<&balaur_core::Transform>(node).unwrap();
    (t.position.x, t.position.z)
}

fn booted(dir: &std::path::Path) -> App {
    let mut app = standard_app(AppConfig::dev(dir.to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app
}

#[test]
fn fixed_update_ignores_the_frame_time_that_update_follows() {
    for (language, script) in [(None, LUAU), (Some("rune"), RUNE)] {
        let dir = tempfile::tempdir().unwrap();
        project(dir.path(), language, script);
        let mut app = booted(dir.path());

        // One ragged frame worth exactly three fixed steps.
        app.tick(FIXED_DT * 3.0);
        let (fixed, measured) = walked(&app);

        assert!(
            (fixed - FIXED_DT * 3.0).abs() < 1e-6,
            "{language:?}: fixed_update should have walked three whole steps, got {fixed}"
        );
        assert!(
            (measured - FIXED_DT * 3.0).abs() < 1e-6,
            "{language:?}: update should have walked the frame once, got {measured}"
        );
    }
}

#[test]
fn a_frame_shorter_than_a_step_ticks_update_but_not_fixed_update() {
    for (language, script) in [(None, LUAU), (Some("rune"), RUNE)] {
        let dir = tempfile::tempdir().unwrap();
        project(dir.path(), language, script);
        let mut app = booted(dir.path());

        app.tick(FIXED_DT * 0.5);
        let (fixed, measured) = walked(&app);
        assert_eq!(fixed, 0.0, "{language:?}: half a step is not a step");
        assert!(
            measured > 0.0,
            "{language:?}: update runs every frame — if this is 0 the script never ran at all"
        );

        // The dropped half is owed, not lost.
        app.tick(FIXED_DT * 0.5);
        let (fixed, _) = walked(&app);
        assert!(
            (fixed - FIXED_DT).abs() < 1e-6,
            "{language:?}: the two halves owe one whole step, got {fixed}"
        );
    }
}
