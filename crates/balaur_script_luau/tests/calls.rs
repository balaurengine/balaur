//! One script calling another through `node:call` — the seam's signal path
//! driven from inside a script, return value included.

use balaur_core::{App, AppConfig};

fn make_app(dir: &std::path::Path) -> App {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("project.toml"),
        "name = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_luau::factory()),
    })
    .unwrap()
}

fn attach(app: &App, name: &str, file: &str, source: &str) {
    let dir = app.engine.resource::<balaur_core::project::ProjectRoot>();
    let path = dir.borrow().0.join("scripts").join(file);
    std::fs::write(path, source).unwrap();
    let root = app.engine.root();
    let entity = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), name, root);
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur_core::node_id_of(entity), &format!("scripts/{file}"))
        .unwrap();
}

#[test]
fn a_script_calls_another_and_gets_the_return_value() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    attach(
        &app,
        "Service",
        "service.luau",
        "local S = {}\nfunction S:sum(a, b) return a + b end\nreturn S\n",
    );
    attach(
        &app,
        "Consumer",
        "consumer.luau",
        r#"
local S = {}
function S:init()
    local service = scene.get_node("Service")
    _G.sum = service:call("sum", 2, 3)
    _G.missing = service:call("no_such_method")
end
return S
"#,
    );
    let lua = balaur_script_luau::lua_of(&app.engine);
    let sum: i64 = lua.globals().get("sum").unwrap();
    assert_eq!(sum, 5);
    // A missing method is nil, not an error: handlers are opt-in.
    let missing: Option<i64> = lua.globals().get("missing").unwrap();
    assert_eq!(missing, None);
}
