#![cfg(feature = "extensions")]

use std::path::{Path, PathBuf};

use balaur::{standard_app, AppConfig};

fn greeter() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let built = std::process::Command::new(env!("CARGO"))
                .current_dir(&root)
                .args(["build", "-p", "extension_greeter"])
                .status()
                .expect("cargo should run");
            assert!(built.success(), "the extension did not build");
            let suffix = balaur_plugin::library_suffix();
            let target = root.join("target/debug");
            let unix = target.join(format!("libextension_greeter.{suffix}"));
            if unix.exists() {
                unix
            } else {
                target.join(format!("extension_greeter.{suffix}"))
            }
        })
        .clone()
}

fn project(with_extension: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"extended\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = \"scripts/s.luau\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.luau"),
        "local S = {}\nfunction S:init() _G.said = greeter.greet('world') end\nreturn S\n",
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

#[test]
fn a_project_extension_is_loaded_and_reachable_from_a_script() {
    let dir = project(true);
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();

    let lua = balaur::luau::lua_of(&app.engine);
    let said: String = lua.globals().get("said").unwrap_or_default();
    assert_eq!(
        said, "hello, world",
        "the extension's binding did not reach the script"
    );
}

/// A project with no extensions directory is the common case and must not be
/// an error.
#[test]
fn a_project_without_extensions_still_boots() {
    let dir = project(false);
    std::fs::write(
        dir.path().join("scripts/s.luau"),
        "local S = {}\nfunction S:init() end\nreturn S\n",
    )
    .unwrap();
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
    let lua = balaur::luau::lua_of(&app.engine);
    let count: i64 = lua.load("return greeter.greetings()").eval().unwrap();
    assert_eq!(count, 1, "the extension's state did not survive the run");
}
