//! What `balaur check` says about a project's scripts.
//!
//! Two passes answer here: Rune's own compiler, and the scene-aware one that
//! resolves a component's method against the table the run time dispatches
//! on. The second exists because the first cannot see it — a handle picks its
//! method by component name at call time — so every case below is a call that
//! compiles clean and fails on the tick that runs it.

use std::collections::BTreeSet;
use std::path::Path;

/// A project whose one node carries a `body2d` and a `transform`, with
/// `script` as the script it attaches.
fn project(script: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Crate\"\nscript = { source = \"main.rn\" }\n\n\
         [nodes.transform]\nposition = [0, 1, 0]\n\n\
         [nodes.body2d]\nkind = \"dynamic\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.rn"), script).unwrap();
    dir
}

/// Every finding in a project whose `init` is `body`.
fn check(body: &str) -> Vec<String> {
    let dir = project(&format!("pub fn init(this) {{\n{body}\n}}\n"));
    balaur::check_project(dir.path())
        .unwrap()
        .into_iter()
        .map(|one| {
            format!(
                "{}:{}:{}: {}",
                one.file, one.line, one.severity, one.message
            )
        })
        .collect()
}

#[test]
fn a_project_whose_scripts_are_right_has_nothing_to_report() {
    assert_eq!(
        check(
            "    this.node.body2d.set_linear_velocity(2.0, 0.0);\n\
             \x20   this.node.transform.position;\n\
             \x20   this.node.body2d.has();\n\
             \x20   let n = this.node;\n\
             \x20   n.body2d.apply_impulse(0.0, 1.0);"
        ),
        Vec::<String>::new()
    );
}

#[test]
fn a_method_no_module_drives_that_component_with_is_named() {
    let found = check("    this.node.body2d.apply_impulze(1.0, 0.0);");
    assert_eq!(
        found,
        ["main.rn:2:warning: `body2d` has no `apply_impulze`; no module driving it declares one"]
    );
}

/// The plan's own example: `apply_impulse` is registered on the handle type,
/// not on `body2d`, so nothing before this pass objected to it on a sprite.
#[test]
fn a_component_the_node_does_not_carry_is_named() {
    let found = check("    this.node.sprite.apply_impulse(1.0, 0.0);");
    assert_eq!(
        found,
        ["main.rn:2:warning: no node this script is attached to has a `sprite`"]
    );
}

/// A script that adds the component itself names it as a string, and the
/// scene is then not the whole story about the node.
#[test]
fn a_component_the_script_adds_itself_is_left_alone() {
    assert_eq!(
        check(
            "    this.node.set_component(\"sprite\", #{ image: \"a.png\" });\n\
             \x20   this.node.sprite.frame = 0;"
        ),
        Vec::<String>::new()
    );
}

#[test]
fn a_property_called_as_a_method_says_so() {
    let found = check("    this.node.body2d.linear_damping();");
    assert_eq!(
        found,
        ["main.rn:2:warning: `body2d.linear_damping` is a property, not a method; drop the `()`"]
    );
}

#[test]
fn a_property_no_schema_declares_is_named() {
    let found = check("    this.node.transform.postion;");
    assert_eq!(
        found,
        ["main.rn:2:warning: `transform` has no property `postion`"]
    );
}

#[test]
fn a_field_that_is_no_component_at_all_is_named() {
    let found = check("    this.node.bod2d.apply_impulse(1.0, 0.0);");
    assert_eq!(
        found,
        ["main.rn:2:warning: `bod2d` is not a component, so the node has no field of that name"]
    );
}

/// A handle written inside a comment or a string is not one.
#[test]
fn text_that_only_looks_like_a_call_is_not_one() {
    assert_eq!(
        check(
            "    // this.node.body2d.commented();\n\
             \x20   let s = \"this.node.body2d.quoted()\";"
        ),
        Vec::<String>::new()
    );
}

/// Rune's own diagnostics still arrive, and the file they name is the one the
/// author wrote in.
#[test]
fn the_compilers_own_findings_still_arrive() {
    let found = check("    nowhere::at_all();");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("main.rn:2:error:"), "{found:?}");
}

#[test]
fn a_scene_names_the_components_beside_each_script() {
    let dir = project("pub fn init(this) {}\n");
    let attached = balaur_core::attachments::scene_attachments(dir.path());
    assert_eq!(
        attached.get("main.rn"),
        Some(&Some(BTreeSet::from([
            "body2d".to_string(),
            "transform".to_string()
        ])))
    );
    assert_eq!(balaur::scene_scripts(dir.path()), ["main.rn"]);
}

/// Rune warns that every refutable pattern might panic, and a tuple is one:
/// nothing proves a value's arity before it arrives. That is every multiple
/// return the language has, so it is not reported; a pattern that tests a
/// value still is.
#[test]
fn unpacking_a_tuple_is_not_a_warning_but_testing_a_value_is() {
    assert_eq!(
        check(
            "    let (x, y) = input::mouse_position();\n\
             \x20   let rows = [1, 2];\n\
             \x20   for (i, row) in rows.iter().enumerate() {\n\
             \x20       let _ = i + row + x + y;\n\
             \x20   }"
        ),
        Vec::<String>::new()
    );
    let found = check("    let Some(first) = [1, 2].first();\n\x20   let _ = first;");
    assert_eq!(found, ["main.rn:2:warning: Pattern might panic"]);
}

/// A directory carrying a manifest is another project: its scenes name their
/// scripts from their own root, and it is checked from there.
#[test]
fn a_nested_project_is_not_part_of_this_one() {
    let dir = project("pub fn init(this) {}\n");
    let nested = dir.path().join("templates").join("starter");
    std::fs::create_dir_all(nested.join("scenes")).unwrap();
    std::fs::write(
        nested.join("project.toml"),
        "[application]\nname = \"s\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        nested.join("scenes").join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"N\"\nscript = { source = \"scripts/starter.rn\" }\n",
    )
    .unwrap();
    assert_eq!(balaur::scene_scripts(dir.path()), ["main.rn"]);
    // Its missing script is the nested project's own business, not this one's.
    assert_eq!(balaur::check_project(dir.path()).unwrap().len(), 0);
}

/// A node the scene loader would reject is still a root to compile, and its
/// components are unknown rather than none: nothing may conclude from a node
/// it could not read that the node carries nothing.
#[test]
fn a_node_that_will_not_parse_leaves_its_components_unknown() {
    let dir = project("pub fn init(this) {\n    this.node.sprite.frame = 0;\n}\n");
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Crate\"\nscript = { source = \"main.rn\" }\ntags = \"one\"\n",
    )
    .unwrap();
    let attached = balaur_core::attachments::scene_attachments(dir.path());
    assert_eq!(attached.get("main.rn"), Some(&None));
    assert_eq!(balaur::check_project(dir.path()).unwrap().len(), 0);
}

/// `[check] strict = true` is how a project says a warning is a failure
/// without every caller having to remember the flag. The editor says it.
#[test]
fn a_project_can_ask_for_strict_checking_in_its_manifest() {
    use balaur_core::project::ProjectManifest;
    let plain = "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n";
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
