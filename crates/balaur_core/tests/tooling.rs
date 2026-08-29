//! Tests for the tooling layer the editor is built on: `toml` roundtrip,
//! `require` with module hot reload, and runtime scene instantiation.

use balaur_core::{App, AppConfig};

fn make_app(dir: &std::path::Path) -> App {
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("project.toml"),
        "name = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("scenes/main.toml"), "").unwrap();
    App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: false,
        script_args: vec!["arg-one".into()],
    })
    .unwrap()
}

#[test]
fn toml_roundtrips_through_lua() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    let lua = app.engine.scripts().unwrap().lua();
    let out: String = lua
        .load(
            r#"
            local doc = toml.parse('[[nodes]]\nid = "n_a"\nname = "A"\nposition = [1.5, 2, 3]\n')
            assert(doc.nodes[1].name == "A")
            assert(doc.nodes[1].position[1] == 1.5)
            doc.nodes[1].name = "B"
            return toml.encode(doc)
            "#,
        )
        .eval()
        .unwrap();
    assert!(out.contains("name = \"B\""), "{out}");
    assert!(out.contains("[[nodes]]"), "{out}");
}

#[test]
fn require_caches_and_hot_reloads_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    std::fs::write(
        dir.path().join("scripts/mod.luau"),
        "return { value = 1 }\n",
    )
    .unwrap();
    let lua = app.engine.scripts().unwrap().lua();
    lua.load(
        r#"
        local a = require("scripts/mod")
        local b = require("scripts/mod.luau")
        assert(a == b, "require must cache")
        _G.held = a
        "#,
    )
    .exec()
    .unwrap();

    std::fs::write(
        dir.path().join("scripts/mod.luau"),
        "return { value = 2 }\n",
    )
    .unwrap();
    app.engine
        .scripts()
        .unwrap()
        .reload("scripts/mod.luau")
        .unwrap();
    // The previously-required table sees the new contents in place.
    let v: i64 = lua.load("return _G.held.value").eval().unwrap();
    assert_eq!(v, 2);
}

#[test]
fn scenes_instantiate_at_runtime_and_args_reach_scripts() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    let lua = app.engine.scripts().unwrap().lua();
    lua.load(
        r#"
        assert(engine.args()[1] == "arg-one")
        local parent = scene.spawn("Holder")
        scene.instantiate('[[nodes]]\nid = "n_a"\nname = "A"\n\n[[nodes]]\nid = "n_b"\nname = "B"\nparent = "n_a"\nposition = [0, 3, 0]\n', parent, { scripts = false })
        local b = parent:get_node("A/B")
        assert(b ~= nil, "nested instantiation failed")
        local _, y = b:position()
        assert(y == 3)
        "#,
    )
    .exec()
    .unwrap();
}
