//! The language chapter's examples that use the engine, `docs/language/`, held
//! to the engine's real API: a project of them, as `balaur check` reads it.
//! `balaur_bs`'s suite answers for the rest.

#[path = "../../../balaur_bs/tests/corpus/chapter.rs"]
mod fences;

use fences::{is_fragment, module_name, needs_the_engine, pages};
use std::collections::HashMap;
use std::fmt::Write as _;

// `040-data/classes.md` is `classes`: a page's fences are named after it.
fn stem_of(page: &str) -> String {
    let file = page.rsplit('/').next().unwrap_or(page);
    file.trim_end_matches(".md").replace('-', "_")
}

// `spinner_top` is `SpinnerTop`, the class a script of that name declares.
fn class_of(stem: &str) -> String {
    stem.split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect()
}

#[test]
fn every_example_that_uses_the_engine_compiles_against_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"chapter\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    let mut origin: HashMap<String, String> = HashMap::new();
    let (mut uses, mut nodes) = (String::new(), String::new());
    for page in pages() {
        for (line, code) in page.fences.iter().filter(|(_, c)| needs_the_engine(c)) {
            let (stem, source) = match module_name(code) {
                Some(path) => {
                    let file = path.rsplit('/').next().unwrap_or(path);
                    (file.trim_end_matches(".bs").to_string(), code.clone())
                }
                None if is_fragment(code) => (
                    format!("{}_{line}", stem_of(&page.name)),
                    format!("pub fn snippet(node: Node) {{\n{code}}}\n"),
                ),
                None => (format!("{}_{line}", stem_of(&page.name)), code.clone()),
            };
            let rel = format!("scripts/{stem}.bs");
            std::fs::write(dir.path().join(&rel), &source).unwrap();
            origin.insert(rel.clone(), format!("{}:{line}", page.name));
            let class = class_of(&stem);
            if source.contains(&format!("pub class {class}")) {
                writeln!(
                    nodes,
                    "[[nodes]]\nname = \"{class}\"\nparent = \"Root\"\nscript = {{ source = \"{rel}\" }}\n"
                )
                .unwrap();
            } else {
                writeln!(uses, "use {stem};").unwrap();
            }
        }
    }
    assert!(!origin.is_empty(), "the chapter was not read");
    std::fs::write(
        dir.path().join("scripts/root.bs"),
        format!("{uses}pub class Root {{ node: Node }}\n"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        format!(
            "[[nodes]]\nname = \"Root\"\nscript = {{ source = \"scripts/root.bs\" }}\n\n{nodes}"
        ),
    )
    .unwrap();
    let found: Vec<String> = balaur::check_project(dir.path())
        .unwrap()
        .into_iter()
        .map(|one| {
            let page = origin
                .get(&one.file)
                .map_or(one.file.as_str(), String::as_str);
            format!(
                "{page} (line {}): {}: {}",
                one.line, one.severity, one.message
            )
        })
        .collect();
    assert!(found.is_empty(), "{}", found.join("\n"));
}
