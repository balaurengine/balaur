//! A screenshot answers whoever listens: written with the path, or failed
//! with why, as a run with no renderer does.

use balaur::{AppConfig, standard_app};
use balaur_script::Value;

const SCENE: &str = r#"
[[nodes]]
id = "n_camera"
name = "Camera"
script = { source = "scenes/shoot.rn" }
"#;

#[test]
fn a_screenshot_in_a_run_with_no_renderer_says_it_failed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), SCENE).unwrap();
    std::fs::write(
        dir.path().join("scenes/shoot.rn"),
        "pub fn init(this) { render::screenshot(\"shots/one.png\"); }\n",
    )
    .unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    // Answered at the end of the first frame, heard at the next one's pump.
    app.tick(1.0 / 60.0);
    app.tick(1.0 / 60.0);
    let failed = balaur_core::events::delivered(&app.engine, "screenshot_failed");
    let [Value::Map(said)] = failed.as_slice() else {
        panic!("no single screenshot_failed: {failed:?}");
    };
    let get = |key: &str| said.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    assert!(
        matches!(get("path"), Some(Value::Str(path)) if path.ends_with("shots/one.png")),
        "the payload names the file: {said:?}"
    );
    assert!(matches!(get("error"), Some(Value::Str(_))));
}
