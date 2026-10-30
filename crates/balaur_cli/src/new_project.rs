//! `balaur new`: a project from nothing, or from one of the editor's
//! templates.
//!
//! The templates live beside the editor rather than in this binary: they are
//! content, the library dock lists the same ones, and a project starting from
//! one should be a project a person could have written.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Every directory this install's shipped data may sit under, in the order to
/// look: beside the binary, then up to the repository root in a checkout.
///
/// Through `balaur::standalone::data_roots`, which knows a macOS bundle keeps its
/// data in `Contents/Resources` — codesign seals `Contents/MacOS` as code, so
/// nothing may ship beside the executable there, and `Resources` is a sibling
/// of it rather than an ancestor, which no walk upwards would reach.
pub(crate) fn data_dirs() -> Vec<PathBuf> {
    let exe = std::env::current_exe().ok();
    let cwd = std::env::current_dir().ok();
    dirs_under(exe.as_deref().and_then(Path::parent), cwd.as_deref())
}

/// [`data_dirs`] over the two directories it reads from the process, so a test
/// can hand it a bundle it built rather than the one it is running from.
fn dirs_under(exe_dir: Option<&Path>, cwd: Option<&Path>) -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Some(dir) = exe_dir {
        bases.extend(balaur::standalone::data_roots(dir));
    }
    bases.extend(cwd.map(Path::to_path_buf));
    let mut found = Vec::new();
    for base in bases {
        let mut at = Some(base);
        while let Some(dir) = at {
            at = dir.parent().map(Path::to_path_buf);
            found.push(dir);
        }
    }
    found
}

/// Where the editor's own project, and so its library, is installed.
pub(crate) fn library_dir() -> Option<PathBuf> {
    data_dirs()
        .into_iter()
        .map(|dir| dir.join("editor/library"))
        .find(|library| library.is_dir())
}

/// What a template's text says in braces, filled in for one project: its
/// name, and the page a template with one is sized to. A number is written
/// quoted, `"{{width}}"`, so the template is a project that parses as it
/// stands, and its quotes go with it.
struct Filled {
    name: String,
    page: (u32, u32),
}

/// The page a project is sized to when nothing asked: the screen most
/// pictures and games are made for.
pub(crate) const PAGE: (u32, u32) = (1920, 1080);

/// Pixels to a world unit on a page: what a `camera2d` frames it at, so a
/// `text2d` or a sprite at its own default draws at its stated size.
const PAGE_PIXELS_PER_UNIT: f64 = 100.0;

impl Filled {
    fn fill(&self, text: &str) -> String {
        let (width, height) = self.page;
        let units = |pixels: u32| format!("{}", f64::from(pixels) / PAGE_PIXELS_PER_UNIT);
        text.replace("{{name}}", &self.name)
            .replace("\"{{width}}\"", &width.to_string())
            .replace("\"{{height}}\"", &height.to_string())
            .replace("\"{{page_width}}\"", &units(width))
            .replace("\"{{page_height}}\"", &units(height))
    }
}

/// Copy a template's files, filling in what it leaves in braces.
///
/// Text only: every template is scenes, scripts and a manifest, and a binary
/// starting point would be content rather than a template.
fn copy_template(from: &Path, to: &Path, filled: &Filled) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_template(&entry.path(), &target, filled)?;
            continue;
        }
        let text = std::fs::read_to_string(entry.path())
            .with_context(|| format!("reading {}", entry.path().display()))?;
        std::fs::write(&target, filled.fill(&text))?;
    }
    Ok(())
}

/// The `empty` template, built in for a binary with no editor library beside
/// it: the same files, so a project from nothing is the one the library's
/// template makes.
const EMPTY: &[(&str, &str)] = &[
    (
        "project.toml",
        include_str!("../../../editor/library/templates/empty/project.toml"),
    ),
    (
        "scenes/main.toml",
        include_str!("../../../editor/library/templates/empty/scenes/main.toml"),
    ),
    (
        "scripts/hello.bs",
        include_str!("../../../editor/library/templates/empty/scripts/hello.bs"),
    ),
];

/// A project at `path`, from `template` or from the `empty` one, its page
/// `page` pixels where the template has one.
pub(crate) fn create(path: &Path, template: Option<&str>, page: Option<(u32, u32)>) -> Result<()> {
    let filled = Filled {
        name: path
            .file_name()
            .map_or_else(|| "game".to_string(), |n| n.to_string_lossy().into_owned()),
        page: page.unwrap_or(PAGE),
    };
    let name = &filled.name;
    let library = library_dir();
    if template.is_some() && library.is_none() {
        anyhow::bail!("no editor library beside this binary, so no templates to start from");
    }
    let template = template.unwrap_or("empty");
    match library {
        Some(library) => {
            let from = library.join("templates").join(template);
            if !from.is_dir() {
                let mut known: Vec<String> = std::fs::read_dir(library.join("templates"))?
                    .filter_map(|e| Some(e.ok()?.file_name().to_string_lossy().into_owned()))
                    .collect();
                known.sort();
                anyhow::bail!(
                    "no template `{template}`; the templates are {}",
                    known.join(", ")
                );
            }
            copy_template(&from, path, &filled)?;
        }
        None => {
            for (file, text) in EMPTY {
                let target = path.join(file);
                if let Some(dir) = target.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(&target, filled.fill(text))?;
            }
        }
    }
    keep_deps_out_of_git(path)?;
    tracing::info!(
        "created project '{name}' from the `{template}` template at {}",
        path.display()
    );
    tracing::info!("run it with: balaur run {}", path.display());
    Ok(())
}

/// `/deps/` in a project's `.gitignore`, made if there is none: fetched
/// dependencies are `project.lock`'s to restore, not the repository's to hold.
pub(crate) fn keep_deps_out_of_git(project: &Path) -> Result<()> {
    let path = project.join(".gitignore");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    if let Some(text) = balaur::dependencies::gitignored(&text) {
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{EMPTY, create, dirs_under};
    use std::path::Path;

    #[test]
    fn a_page_is_the_size_asked_for_and_its_paper_the_same_in_world_units() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("poster");
        create(&path, Some("page"), Some((1080, 1350))).unwrap();
        let project = std::fs::read_to_string(path.join("project.toml")).unwrap();
        let project: toml::Value = toml::from_str(&project).unwrap();
        assert_eq!(project["window"]["width"].as_integer(), Some(1080));
        assert_eq!(project["window"]["height"].as_integer(), Some(1350));
        assert_eq!(project["application"]["name"].as_str(), Some("poster"));
        let scene = std::fs::read_to_string(path.join("scenes/main.toml")).unwrap();
        let scene: toml::Value = toml::from_str(&scene).unwrap();
        let paper = scene["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["name"].as_str() == Some("Paper"))
            .unwrap();
        let size: Vec<f64> = paper["shape2d"]["size"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_float().unwrap())
            .collect();
        assert_eq!(size, [10.8, 13.5], "100 pixels to a unit");
    }

    #[test]
    fn a_project_from_nothing_is_the_empty_template() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("blank");
        create(&path, None, None).unwrap();
        for (file, text) in EMPTY {
            let written = std::fs::read_to_string(path.join(file)).unwrap();
            assert_eq!(written, text.replace("{{name}}", "blank"), "{file}");
        }
    }

    /// `library_dir` and `examples_dir` over a set of candidate directories.
    fn holds(dirs: &[std::path::PathBuf], marker: &str) -> bool {
        dirs.iter().any(|dir| dir.join(marker).exists())
    }

    #[test]
    fn a_macos_bundle_keeps_its_data_beside_the_executable_rather_than_above_it() {
        let root = tempfile::tempdir().unwrap();
        let contents = root.path().join("Balaur.app/Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        std::fs::create_dir_all(contents.join("Resources/editor/library")).unwrap();
        std::fs::create_dir_all(contents.join("Resources/examples/hello")).unwrap();
        std::fs::write(
            contents.join("Resources/examples/hello/project.toml"),
            "[application]\n",
        )
        .unwrap();

        // Finder starts a bundle with a working directory of `/`, so the
        // executable's own directory is the only thing pointing at the data.
        let dirs = dirs_under(Some(&contents.join("MacOS")), Some(Path::new("/")));
        assert!(
            holds(&dirs, "editor/library"),
            "the templates the New button copies"
        );
        assert!(
            holds(&dirs, "examples/hello/project.toml"),
            "the examples the Examples tab lists"
        );
    }

    #[test]
    fn a_checkout_is_found_by_walking_up_from_the_binary() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("editor/library")).unwrap();
        let exe_dir = root.path().join("target/debug");
        std::fs::create_dir_all(&exe_dir).unwrap();

        let dirs = dirs_under(Some(&exe_dir), None);
        assert!(holds(&dirs, "editor/library"));
    }
}
