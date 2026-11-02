//! What a script hears when its file is rebuilt under a running scene.

#[test]
fn a_reload_tells_the_instances_of_that_file_and_keeps_their_state() {
    let dir = tempfile::tempdir().unwrap();
    let script = |class: &str| {
        format!(
            "pub class {class} {{ node: Node, pub reloads: int, pub kept: int }}\nimpl {class} {{\n    fn init() {{ self.kept = 7; }}\n    fn on_hot_reload() {{ self.reloads = self.reloads + 1; }}\n}}\n"
        )
    };
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"a\"\nname = \"A\"\nscript = { source = \"scripts/a.bs\" }\n\n[[nodes]]\nid = \"b\"\nname = \"B\"\nparent = \"a\"\nscript = { source = \"scripts/b.bs\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/a.bs"), script("A")).unwrap();
    std::fs::write(dir.path().join("scripts/b.bs"), script("B")).unwrap();
    let mut app = balaur::standard_app(balaur::AppConfig::dev(
        dir.path().to_string_lossy().as_ref(),
    ))
    .unwrap();
    app.load_project().unwrap();
    let host = app.engine.script_host().unwrap();
    let field = |id: &str, name: &str| {
        let world = app.engine.world();
        let node = balaur_core::ids::find(&world, app.engine.root(), id).unwrap();
        host.script_field(balaur::node_id_of(node), name)
    };
    let int = balaur_script::Value::Int;

    host.reload("scripts/a.bs").unwrap();
    assert_eq!(
        field("a", "reloads"),
        Some(int(1)),
        "the file reloaded hears"
    );
    assert_eq!(field("a", "kept"), Some(int(7)), "with its state kept");
    assert_eq!(field("b", "reloads"), Some(int(0)), "another file does not");
    assert_eq!(
        field("b", "kept"),
        Some(int(7)),
        "though it was carried too"
    );
}
