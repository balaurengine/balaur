//! A dependency's scripts through the `.bs` host: `use` stops at the
//! dependency's folder, and a path its script builds reads from its own root,
//! from the sources and from an export pack.

#![cfg(any(feature = "bs", feature = "bs-jit"))]

use balaur::{AppConfig, standard_app};
use balaur_script::Value::{Bool, Int};

use super::bs::{booted, field, ticked};

const BUTTON: &str = "use util;

pub class Button {
    node: Node,
    value: int,
    found: bool,
}

impl Button {
    fn init() {
        self.value = util::value();
        self.found = fs::exists(\"art/hit.png\");
    }
}
";

/// A game at `<tmp>/game` instancing a scene of the `path` dependency at
/// `<tmp>/kit`, whose script `use`s `util` and looks for its own file. The
/// game keeps a `util.bs` of its own, and no `art/hit.png`.
fn workspace(kit_has_util: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut files = vec![
        (
            "game/project.toml",
            "[application]\nname = \"game\"\nmain_scene = \"main.toml\"\n\n\
             [dependencies]\nkit = { path = \"../kit\" }\n",
        ),
        (
            "game/main.toml",
            "[[nodes]]\nid = \"ui\"\nname = \"Ui\"\ninstance = \"deps/kit/panel.toml\"\n",
        ),
        ("game/util.bs", "pub fn value() -> int { 1 }\n"),
        (
            "kit/project.toml",
            "[application]\nname = \"kit\"\nmain_scene = \"panel.toml\"\n",
        ),
        (
            "kit/panel.toml",
            "[[nodes]]\nid = \"b\"\nname = \"Button\"\nscript = { source = \"scripts/button.bs\" }\n",
        ),
        ("kit/scripts/button.bs", BUTTON),
        ("kit/art/hit.png", "hit"),
        ("kit/art/unused.png", "nobody names this"),
    ];
    if kit_has_util {
        files.push(("kit/util.bs", "pub fn value() -> int { 2 }\n"));
    }
    for (name, body) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    dir
}

#[test]
fn a_dependency_script_reads_its_own_module_and_files() {
    let dir = workspace(true);
    let mut app = booted(&dir.path().join("game"));
    ticked(&mut app, 1);
    assert_eq!(field(&app, "Ui", "value"), Some(Int(2)));
    assert_eq!(field(&app, "Ui", "found"), Some(Bool(true)));
}

#[test]
fn use_inside_a_dependency_never_reaches_the_games_module() {
    let dir = workspace(false);
    let mut app = standard_app(AppConfig::dev(
        dir.path().join("game").to_string_lossy().as_ref(),
    ))
    .unwrap();
    let Err(err) = app.load_project() else {
        panic!("the dependency's `use util;` linked the game's util.bs");
    };
    assert!(format!("{err:#}").contains("util"), "{err:#}");
}

#[test]
fn an_exported_dependency_script_reads_its_own_files_from_the_pack() {
    let dir = workspace(true);
    let pack = balaur::build_pack(&dir.path().join("game")).unwrap();
    assert!(pack.assets.contains_key("deps/kit/art/hit.png"));
    assert!(!pack.assets.contains_key("deps/kit/art/unused.png"));
    let mut app = standard_app(AppConfig::packed(pack)).unwrap();
    app.load_project().unwrap();
    ticked(&mut app, 1);
    assert_eq!(field(&app, "Ui", "value"), Some(Int(2)));
    assert_eq!(field(&app, "Ui", "found"), Some(Bool(true)));
}
