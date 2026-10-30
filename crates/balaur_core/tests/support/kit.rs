//! A game and the `path` dependency it names, as files: shared by the
//! kernel's dependency tests and `balaur_project`'s export tests.

/// A game at `<tmp>/game` and the folder its `path` dependency names at
/// `<tmp>/kit`, each from `(path, text)` pairs.
pub(super) fn workspace(game: &[(&str, &str)], kit: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (root, files) in [("game", game), ("kit", kit)] {
        for (name, body) in files {
            let path = dir.path().join(root).join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
    }
    dir
}

pub(super) const GAME_MANIFEST: &str = r#"[application]
name = "game"
main_scene = "scenes/main.toml"

[dependencies]
kit = { path = "../kit" }
"#;

const KIT_MANIFEST: &str = r#"[application]
name = "kit"
main_scene = "scenes/panel.toml"
license = "CC0-1.0"
ignore = ["wip/**"]

[import.texture]
filter = "nearest"
"#;

const PANEL: &str = r#"
[[nodes]]
id = "n_panel"
name = "Panel"

[nodes.marker]
label = "art/button.png"
"#;

pub(super) fn kit() -> Vec<(&'static str, &'static str)> {
    vec![
        ("project.toml", KIT_MANIFEST),
        ("scenes/panel.toml", PANEL),
        ("art/button.png", "kit's button"),
        ("art/unused.png", "nobody names this"),
        ("wip/sketch.png", "ignored"),
    ]
}
