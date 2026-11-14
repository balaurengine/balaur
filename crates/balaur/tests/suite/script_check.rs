//! What `balaur check` says about a project's scripts.
//!
//! A `.bs` script compiles against the engine's API, components and their
//! properties included, so a call on a handle no module drives is an error
//! before the tick that would run it.

use std::collections::BTreeSet;
use std::path::Path;

/// A project whose one node carries a `body2d` and a `transform`, with
/// `script` as the script it attaches.
fn project(script: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Crate\"\nscript = { source = \"main.bs\" }\n\n\
         [nodes.transform]\nposition = [0, 1, 0]\n\n\
         [nodes.body2d]\nkind = \"dynamic\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.bs"), script).unwrap();
    dir
}

/// A script whose `init` is `body`, its first line on line 4.
fn script(body: &str) -> String {
    format!("pub class Main {{ node: Node }}\nimpl Main {{\n    fn init() {{\n{body}\n    }}\n}}\n")
}

/// Every finding in a project whose `init` is `body`, as `file:line:severity`
/// and the message.
fn check(body: &str) -> Vec<(String, String)> {
    let dir = project(&script(body));
    balaur::check_project(dir.path())
        .unwrap()
        .into_iter()
        .map(|one| {
            (
                format!("{}:{}:{}", one.file, one.line, one.severity),
                one.message,
            )
        })
        .collect()
}

/// The one finding a project whose `init` is `body` has, at line 4 of
/// `main.bs`, as an error; its message.
fn one_error(body: &str) -> String {
    let found = check(body);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "main.bs:4:error", "{found:?}");
    found[0].1.clone()
}

#[test]
fn a_project_whose_scripts_are_right_has_nothing_to_report() {
    let found = check(
        "        self.node.body2d.set_linear_velocity(vec2(2.0, 0.0));\n\
         \x20       let at = self.node.transform.position;\n\
         \x20       let n = self.node;\n\
         \x20       n.body2d.apply_impulse(vec2(at.x, 1.0));",
    );
    assert_eq!(found, []);
}

#[test]
fn a_method_no_module_drives_that_component_with_is_an_error_on_its_line() {
    one_error("        self.node.body2d.apply_impulze(vec2(1.0, 0.0));");
}

#[test]
fn a_property_no_schema_declares_is_an_error_on_its_line() {
    one_error("        let at = self.node.transform.postion;");
}

#[test]
fn a_field_that_is_no_component_at_all_is_named() {
    let message = one_error("        self.node.bod2d.apply_impulse(vec2(1.0, 0.0));");
    assert!(message.contains("bod2d"), "{message}");
}

/// A handle written inside a comment or a string is not one.
#[test]
fn text_that_only_looks_like_a_call_is_not_one() {
    let found = check(
        "        // self.node.body2d.commented();\n\
         \x20       let s = \"self.node.body2d.quoted()\";",
    );
    assert_eq!(found, []);
}

#[test]
fn a_call_to_no_module_names_the_line_it_is_on() {
    let message = one_error("        nowhere::at_all();");
    assert!(message.contains("nowhere"), "{message}");
}

/// The engine runs `.bs` alone: a scene that still attaches a `.rn` is told
/// so, rather than the script being skipped in silence.
#[test]
fn a_script_in_another_language_is_named() {
    let dir = project(&script(""));
    let scene = std::fs::read_to_string(dir.path().join("main.toml")).unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        scene.replace("main.bs", "main.rn"),
    )
    .unwrap();
    std::fs::write(dir.path().join("main.rn"), "pub fn init(this) {}\n").unwrap();
    let found = balaur::check_project(dir.path()).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((found[0].file.as_str(), found[0].line), ("main.rn", 0));
    assert!(found[0].message.contains(".bs"), "{}", found[0].message);
}

#[test]
fn a_script_a_scene_names_that_is_not_there_is_named() {
    let dir = project(&script(""));
    std::fs::remove_file(dir.path().join("main.bs")).unwrap();
    let found = balaur::check_project(dir.path()).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].message, "script main.bs does not exist");
}

#[test]
fn a_scene_names_the_components_beside_each_script() {
    let dir = project(&script(""));
    let attached = balaur_core::attachments::scene_attachments(dir.path());
    assert_eq!(
        attached.get("main.bs"),
        Some(&Some(BTreeSet::from([
            "body2d".to_string(),
            "transform".to_string()
        ])))
    );
    assert_eq!(balaur::scene_scripts(dir.path()), ["main.bs"]);
}

/// A directory carrying a manifest is another project: its scenes name their
/// scripts from their own root, and it is checked from there.
#[test]
fn a_nested_project_is_not_part_of_this_one() {
    let dir = project(&script(""));
    let nested = dir.path().join("templates").join("starter");
    std::fs::create_dir_all(nested.join("scenes")).unwrap();
    std::fs::write(
        nested.join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        nested.join("scenes").join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"N\"\nscript = { source = \"scripts/starter.bs\" }\n",
    )
    .unwrap();
    assert_eq!(balaur::scene_scripts(dir.path()), ["main.bs"]);
    // Its missing script is the nested project's own business, not this one's.
    assert_eq!(balaur::check_project(dir.path()).unwrap().len(), 0);
}

/// A node the scene loader would reject is still a root to compile, and its
/// components are unknown rather than none: nothing may conclude from a node
/// it could not read that the node carries nothing.
#[test]
fn a_node_that_will_not_parse_leaves_its_components_unknown() {
    let dir = project(&script("        let frame = self.node.sprite.frame;"));
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Crate\"\nscript = { source = \"main.bs\" }\ntags = \"one\"\n",
    )
    .unwrap();
    let attached = balaur_core::attachments::scene_attachments(dir.path());
    assert_eq!(attached.get("main.bs"), Some(&None));
    // The script is not blamed; the scene the loader will not read is.
    let found = findings(dir.path());
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "main.toml:0:error", "{found:?}");
    assert!(found[0].1.contains("tags"), "{found:?}");
}

/// `[check] strict = true` is how a project says a warning is a failure
/// without every caller having to remember the flag. The editor says it.
#[test]
fn a_project_can_ask_for_strict_checking_in_its_manifest() {
    use balaur_core::project::ProjectManifest;
    let plain = "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n";
    assert!(!ProjectManifest::parse(plain).unwrap().check.strict);
    let strict = format!("{plain}\n[check]\nstrict = true\n");
    assert!(ProjectManifest::parse(&strict).unwrap().check.strict);
    let editor = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editor/project.toml"),
    )
    .unwrap();
    assert!(ProjectManifest::parse(&editor).unwrap().check.strict);
}

/// A directory with no scenes in it has no roots, and a checker with no roots
/// has nothing to say rather than something to fail on.
#[test]
fn a_project_with_no_scenes_reports_nothing() {
    assert_eq!(
        balaur_core::attachments::scene_attachments(Path::new("does/not/exist")).len(),
        0
    );
}

/// A module `[application] ignore` leaves out compiles here, beside the
/// script that uses it, and is missing from an export: the check says so
/// before the export does.
#[test]
fn a_module_the_project_ignores_is_an_error_on_its_user() {
    let dir = project(&format!(
        "use addons::steps;\n{}",
        script("        let n = steps::next(1);")
    ));
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\nignore = [\"addons/**\"]\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("addons")).unwrap();
    std::fs::write(
        dir.path().join("addons/steps.bs"),
        "pub fn next(n: int) -> int { n + 1 }\n",
    )
    .unwrap();
    let found = balaur::check_project(dir.path()).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].file, "main.bs", "{found:?}");
    assert!(
        found[0].message.contains("addons/steps.bs") && found[0].message.contains("\"addons/**\""),
        "{found:?}"
    );
}

#[test]
fn a_trailing_nil_a_call_may_leave_out_is_a_warning() {
    let found = check(r#"self.node.set_component("transform", nil);"#);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "main.bs:4:warning", "{found:?}");
    assert!(found[0].1.contains("leave this `nil` out"), "{found:?}");
}

/// Every finding of a project as `file:line:severity`, with its message.
fn findings(root: &Path) -> Vec<(String, String)> {
    balaur::check_project(root)
        .unwrap()
        .into_iter()
        .map(|one| {
            (
                format!("{}:{}:{}", one.file, one.line, one.severity),
                one.message,
            )
        })
        .collect()
}

/// `text` added to the end of the fixture's scene, which ends in its `body2d`.
fn add_to_scene(root: &Path, text: &str) {
    let scene = std::fs::read_to_string(root.join("main.toml")).unwrap();
    std::fs::write(root.join("main.toml"), format!("{scene}{text}")).unwrap();
}

#[test]
fn a_scene_with_a_property_no_schema_declares_is_an_error_on_its_line() {
    let dir = project(&script(""));
    assert_eq!(findings(dir.path()), []);
    add_to_scene(dir.path(), "restitution = 0.85\n");
    let found = findings(dir.path());
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "main.toml:11:error", "{found:?}");
    assert!(
        found[0].1.contains("node 'Crate'")
            && found[0]
                .1
                .contains("`body2d` has no property `restitution`"),
        "{found:?}"
    );
}

#[test]
fn a_scene_with_a_component_nothing_registers_is_an_error_on_its_line() {
    let dir = project(&script(""));
    add_to_scene(dir.path(), "\n[nodes.bodi2d]\nkind = \"static\"\n");
    let found = findings(dir.path());
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "main.toml:12:error", "{found:?}");
    assert!(found[0].1.contains("bodi2d"), "{found:?}");
}

// A prefab answers for its own nodes, once, however many scenes instance
// it; the scene answers for the overrides it lays over them.
#[test]
fn a_value_a_prefab_gets_wrong_is_reported_once_in_the_prefab() {
    let dir = project(&script(""));
    std::fs::write(
        dir.path().join("crate.toml"),
        "[[nodes]]\nname = \"Box\"\n\n[nodes.body2d]\nkind = \"wobbly\"\n",
    )
    .unwrap();
    add_to_scene(
        dir.path(),
        "\n[[nodes]]\nname = \"One\"\nparent = \"n\"\ninstance = \"crate.toml\"\n\n\
         [[nodes]]\nname = \"Two\"\nparent = \"n\"\ninstance = \"crate.toml\"\n\n\
         [nodes.overrides.\".\".body2d]\nkind = 7\n",
    );
    let found = findings(dir.path());
    let places: Vec<&str> = found.iter().map(|(place, _)| place.as_str()).collect();
    assert_eq!(
        places,
        ["crate.toml:5:error", "main.toml:23:error"],
        "{found:?}"
    );
    assert!(found[0].1.contains("node 'Box'"), "{found:?}");
    assert!(
        found[1].1.contains("override '..body2d' on node 'Two'"),
        "{found:?}"
    );
}

#[test]
fn an_addon_no_scene_attaches_is_compiled_and_its_warning_reported() {
    let dir = project(&script(""));
    std::fs::create_dir_all(dir.path().join("addons/kit/editor")).unwrap();
    std::fs::write(
        dir.path().join("addons/kit/lib.bs"),
        "pub fn clear(node: Node) {\n    node.set_component(\"transform\", nil);\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("addons/kit/editor/dock.bs"),
        "use lib;\npub fn tidy(node: Node) {\n    lib::clear(node);\n}\n",
    )
    .unwrap();
    let found = findings(dir.path());
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "addons/kit/lib.bs:2:warning", "{found:?}");
    assert!(found[0].1.contains("leave this `nil` out"), "{found:?}");
}

// Each file answers for itself: a module two addon files use is reported
// once, under its own name, not once under each of its users.
#[test]
fn an_addon_file_that_does_not_compile_is_an_error_in_that_file() {
    let dir = project(&script(""));
    std::fs::create_dir_all(dir.path().join("addons/kit")).unwrap();
    std::fs::write(
        dir.path().join("addons/kit/lib.bs"),
        "pub fn next(n: int) -> int {\n    nowhere::at_all(n)\n}\n",
    )
    .unwrap();
    for user in ["a", "b"] {
        std::fs::write(
            dir.path().join(format!("addons/kit/{user}.bs")),
            "use lib;\npub fn twice(n: int) -> int {\n    lib::next(lib::next(n))\n}\n",
        )
        .unwrap();
    }
    let found = findings(dir.path());
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "addons/kit/lib.bs:2:error", "{found:?}");
    assert!(found[0].1.contains("nowhere"), "{found:?}");
}
