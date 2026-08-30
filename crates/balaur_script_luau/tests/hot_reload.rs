//! End-to-end tests for the script host: hot reload semantics, error
//! resilience, and precompiled pack execution.

use balaur_core::{App, AppConfig, Pack};

fn write_project(root: &std::path::Path, script: &str) {
    std::fs::create_dir_all(root.join("scenes")).unwrap();
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::write(
        root.join("project.toml"),
        "name = \"test\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("scenes/main.toml"),
        "[[nodes]]\nid = \"n_counter\"\nname = \"Counter\"\nscript = \"scripts/counter.luau\"\n",
    )
    .unwrap();
    std::fs::write(root.join("scripts/counter.luau"), script).unwrap();
}

const V1: &str = r"
local Counter = {}
function Counter:init()
    self.count = 0
end
function Counter:update(dt)
    self.count += 1
    _G.count = self.count
end
return Counter
";

const V2: &str = r"
local Counter = {}
function Counter:init()
    self.count = 0
end
function Counter:update(dt)
    self.count += 10
    _G.count = self.count
end
function Counter:hot_reload()
    _G.migrated = true
end
return Counter
";

fn global_i64(app: &App, name: &str) -> i64 {
    let lua = balaur_script_luau::lua_of(&app.engine);
    lua.globals()
        .get::<Option<i64>>(name)
        .unwrap()
        .unwrap_or(-1)
}

#[test]
fn hot_reload_swaps_code_and_preserves_state() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path(), V1);
    let mut app = App::new(AppConfig {
        project_root: dir.path().to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_luau::factory()),
    })
    .unwrap();
    app.load_project().unwrap();

    for _ in 0..3 {
        app.tick(1.0 / 60.0);
    }
    assert_eq!(global_i64(&app, "count"), 3);

    // Edit the script on disk and force a reload (the watcher does exactly
    // this automatically; tests call it directly for determinism).
    std::fs::write(dir.path().join("scripts/counter.luau"), V2).unwrap();
    app.engine
        .script_host()
        .unwrap()
        .reload("scripts/counter.luau")
        .unwrap();

    let lua = balaur_script_luau::lua_of(&app.engine);
    assert_eq!(
        lua.globals().get::<Option<bool>>("migrated").unwrap(),
        Some(true),
        "hot_reload hook must run on live instances"
    );

    // New code runs, old state survives: 3 + 10.
    app.tick(1.0 / 60.0);
    assert_eq!(global_i64(&app, "count"), 13);
}

#[test]
fn compile_error_keeps_previous_version_running() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path(), V1);
    let mut app = App::new(AppConfig {
        project_root: dir.path().to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_luau::factory()),
    })
    .unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);

    std::fs::write(
        dir.path().join("scripts/counter.luau"),
        "this is not luau at all (",
    )
    .unwrap();
    let result = app
        .engine
        .script_host()
        .unwrap()
        .reload("scripts/counter.luau");
    assert!(result.is_err(), "broken script must report an error");

    // The previous version keeps running.
    app.tick(1.0 / 60.0);
    assert_eq!(global_i64(&app, "count"), 2);
}

#[test]
fn watcher_reloads_automatically() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path(), V1);
    let mut app = App::new(AppConfig {
        project_root: dir.path().to_path_buf(),
        pack: None,
        watch: true,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_luau::factory()),
    })
    .unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);

    std::fs::write(dir.path().join("scripts/counter.luau"), V2).unwrap();

    // The reload pump runs every frame; give the OS watcher a few seconds.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.tick(1.0 / 60.0);
        let lua = balaur_script_luau::lua_of(&app.engine);
        if lua
            .globals()
            .get::<Option<bool>>("migrated")
            .unwrap()
            .unwrap_or(false)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "watcher never delivered the change"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn pack_roundtrip_runs_from_bytecode_only() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path(), V1);
    let pack = Pack::build(dir.path(), &balaur_script_luau::Compiler).unwrap();
    let bytes = pack.encode();
    // Sources are gone from this point on: only the pack remains.
    drop(dir);

    let pack = Pack::decode(&bytes).unwrap();
    assert_eq!(pack.scripts.len(), 1);
    let mut app = App::new(AppConfig {
        script_backend: Some(balaur_script_luau::factory()),
        ..AppConfig::packed(pack)
    })
    .unwrap();
    app.load_project().unwrap();
    for _ in 0..5 {
        app.tick(1.0 / 60.0);
    }
    assert_eq!(global_i64(&app, "count"), 5);
}
