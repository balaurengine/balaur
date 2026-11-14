//! An export of a game with a dependency: what of the dependency ships.

use balaur_script::ScriptCompiler;

#[path = "../../../balaur_core/tests/support/kit.rs"]
mod kit;
use kit::{GAME_MANIFEST, kit, workspace};

/// Claims `.txt` and keeps the text, so string literals stay readable.
struct Keeping;

impl ScriptCompiler for Keeping {
    fn extensions(&self) -> &[&str] {
        &["txt"]
    }

    fn compile(&self, _rel: &str, source: &str) -> anyhow::Result<Vec<u8>> {
        Ok(source.as_bytes().to_vec())
    }
}

// An export carries what the game and the dependency's scripts name, in the
// game's spelling, and drops the rest of the dependency.
#[test]
fn an_export_carries_only_what_the_game_names() {
    let mut kit = kit();
    kit.push(("scripts/hit.txt", "load(\"art/hit.png\")"));
    kit.push(("art/hit.png", "hit"));
    kit.push(("catalog.toml", "[[entry]]\nfile = \"art/unused.png\"\n"));
    // A showroom scene the game never reaches keeps nothing it names.
    kit.push((
        "scenes/showroom.toml",
        "[[nodes]]\nname = \"Show\"\ntexture = \"art/showroom.png\"\n",
    ));
    kit.push(("art/showroom.png", "big"));
    let dir = workspace(
        &[
            ("project.toml", GAME_MANIFEST),
            (
                "scenes/main.toml",
                "[[nodes]]\nname = \"Ui\"\ninstance = \"deps/kit/scenes/panel.toml\"\n",
            ),
        ],
        &kit,
    );
    let pack = balaur_project::packing::build(&dir.path().join("game"), &Keeping).unwrap();
    let panel = &pack.scenes["deps/kit/scenes/panel.toml"];
    assert!(panel.contains("deps/kit/art/button.png"), "{panel}");
    assert!(pack.assets.contains_key("deps/kit/art/button.png"));
    assert!(pack.assets.contains_key("deps/kit/art/hit.png"));
    assert!(!pack.assets.contains_key("deps/kit/art/unused.png"));
    assert!(!pack.assets.contains_key("deps/kit/art/showroom.png"));
    assert!(!pack.assets.contains_key("deps/kit/wip/sketch.png"));
    assert!(!pack.scenes.contains_key("deps/kit/project.toml"));
    assert!(!pack.scenes.contains_key("deps/kit/catalog.toml"));
    assert!(pack.scripts.contains_key("deps/kit/scripts/hit.txt"));
    let sidecar = &pack.scenes["deps/kit/art/button.png.import.toml"];
    assert!(sidecar.contains("nearest"), "{sidecar}");
    let credits: serde_json::Value =
        serde_json::from_slice(&pack.assets["deps/credits.json"]).unwrap();
    assert_eq!(credits[0]["alias"], "kit");
    assert_eq!(credits[0]["license"], "CC0-1.0");
}

#[test]
fn an_export_names_a_dependency_that_is_not_fetched() {
    let dir = workspace(
        &[(
            "project.toml",
            "[application]\nname = \"g\"\nmain_scene = \"m.toml\"\n\n[dependencies]\nui = { project = \"ana/ui\" }\n",
        )],
        &[],
    );
    let err = balaur_project::packing::build(&dir.path().join("game"), &Keeping).unwrap_err();
    assert!(format!("{err:#}").contains("balaur fetch"), "{err:#}");
}
