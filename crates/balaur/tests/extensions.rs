#![cfg(feature = "extensions")]

use std::path::{Path, PathBuf};

use balaur::{AppConfig, standard_app};

/// Build the out-of-tree extension and return the library cargo produced.
///
/// Built through its own manifest into its own target directory, because that
/// is what an extension is: compiled separately, by someone who does not have
/// the engine's build tree. As a workspace member it shared the host's rlibs,
/// feature resolution and artifacts, which is the one arrangement where the
/// mismatches the fingerprint exists to catch cannot arise.
fn greeter() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let target = root.join("target/extension_greeter");
            let built = std::process::Command::new(env!("CARGO"))
                .current_dir(&root)
                .arg("build")
                .arg("--manifest-path")
                .arg(root.join("examples/extension_greeter/Cargo.toml"))
                .arg("--target-dir")
                .arg(&target)
                .status()
                .expect("cargo should run");
            assert!(built.success(), "the extension did not build");

            let suffix = balaur_plugin::library_suffix();
            let out = target.join("debug");
            let unix = out.join(format!("libextension_greeter.{suffix}"));
            if unix.exists() {
                unix
            } else {
                out.join(format!("extension_greeter.{suffix}"))
            }
        })
        .clone()
}

fn project(with_extension: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"extended\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = \"scripts/s.rn\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.rn"),
        "pub fn init(this) { this.said = greeter::greet(\"world\"); }\n\
         pub fn update(this, dt) { this.count = greeter::greetings(); }\n",
    )
    .unwrap();
    if with_extension {
        let dest = dir.path().join("extensions");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::copy(
            greeter(),
            dest.join(format!("greeter.{}", balaur_plugin::library_suffix())),
        )
        .unwrap();
    }
    dir
}

/// The scene's `Root`, the node the project's script is attached to.
fn root_node(app: &balaur::App) -> balaur::hecs::Entity {
    let world = app.engine.world();
    balaur::scene::find_node(&world, app.engine.root(), "Root").expect("the scene has a Root")
}

#[test]
fn a_project_extension_is_loaded_and_reachable_from_a_script() {
    let dir = project(true);
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();

    let said = balaur::rune::rune_of(&app.engine).text_field(root_node(&app), "said");
    assert_eq!(
        said.as_deref(),
        Some("hello, world"),
        "the extension's binding did not reach the script"
    );
}

#[test]
fn a_project_without_extensions_still_boots() {
    let dir = project(false);
    std::fs::write(dir.path().join("scripts/s.rn"), "pub fn init(this) {}\n").unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
}

/// The libraries must stay mapped for as long as the app runs; a plugin whose
/// library was unloaded has a vtable pointing into nothing.
#[test]
fn an_extension_survives_many_frames() {
    let dir = project(true);
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    for _ in 0..60 {
        app.tick(1.0 / 60.0);
    }
    let count = balaur::rune::rune_of(&app.engine).number_field(root_node(&app), "count");
    assert_eq!(
        count,
        Some(1.0),
        "the extension's state did not survive the run"
    );
}
