//! A converted project's scripts compiled against the engine, for the tests:
//! the files written to a project of their own, one script attached, and
//! every error `balaur check` would report.

/// Every error compiling `attach` among `files`, each `path: line:col: msg`.
pub(crate) fn errors(files: &[(&str, String)], attach: &str) -> Vec<String> {
    let dir = tempfile::tempdir().expect("a temporary project");
    let root = dir.path();
    let put = |path: &str, text: &str| {
        let full = root.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("the project's folders");
        }
        std::fs::write(full, text).expect("a project file");
    };
    put(
        "project.toml",
        "[application]\nname = \"check\"\nmain_scene = \"main.toml\"\n",
    );
    for (path, text) in files {
        put(path, text);
    }
    // A module is compiled through a script that uses it.
    let script = if files
        .iter()
        .any(|(path, text)| *path == attach && text.contains("pub struct "))
    {
        attach.to_string()
    } else {
        let module = crate::godot::gdscript::module_use(attach);
        put(
            "zz_check.bs",
            &format!("use {module};\n\npub struct Check {{\n    node: Node,\n}}\n"),
        );
        "zz_check.bs".to_string()
    };
    put(
        "main.toml",
        &format!("[[nodes]]\nid = \"check\"\nname = \"Check\"\nscript = {{ source = \"{script}\" }}\n"),
    );
    match balaur::check_project(root) {
        Ok(found) => found
            .into_iter()
            .map(|f| format!("{}:{}:{}: {}", f.file, f.line, f.column, f.message))
            .collect(),
        Err(e) => vec![format!("the project did not boot: {e:#}")],
    }
}
