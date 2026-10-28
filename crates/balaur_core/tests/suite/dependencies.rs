//! Dependencies: another project imported whole under `deps/<alias>/`, its
//! paths read from its own root, in a run from the sources and in an export.

use balaur_core::components::ComponentDef;
use balaur_core::scene::find_node;
use balaur_core::{App, AppConfig, Engine, Pack};
use balaur_script::ScriptCompiler;

struct Marker(toml::Value);

/// A game at `<tmp>/game` and the folder its `path` dependency names at
/// `<tmp>/kit`, each from `(path, text)` pairs.
fn workspace(game: &[(&str, &str)], kit: &[(&str, &str)]) -> tempfile::TempDir {
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

const GAME_MANIFEST: &str = r#"[application]
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

fn app_in(game: &std::path::Path) -> App {
    let mut app = App::new(AppConfig::bare(game.to_path_buf())).unwrap();
    app.register_component(
        "marker",
        ComponentDef {
            events: &[],
            warnings: None,
            doc: "",
            schema: ComponentDef::parse_schema(
                "marker",
                r#"label = { type = "string", default = "none" }"#,
            ),
            tags: &[],
            expects: &[],
            apply: Box::new(|eng: &Engine, entity, params| {
                eng.world_mut()
                    .insert_one(entity, Marker(params.clone()))
                    .map_err(|_| anyhow::anyhow!("dead node"))?;
                Ok(())
            }),
            remove: Box::new(|eng: &Engine, entity| {
                let _ = eng.world_mut().remove_one::<Marker>(entity);
                Ok(())
            }),
            get: Box::new(|eng: &Engine, entity| {
                eng.world().get::<&Marker>(entity).ok().map(|m| m.0.clone())
            }),
        },
    );
    app
}

fn label(app: &App, path: &str) -> Option<String> {
    let world = app.engine.world();
    let entity = find_node(&world, app.engine.root(), path)?;
    let marker = world.get::<&Marker>(entity).ok()?;
    Some(marker.0.get("label")?.as_str()?.to_string())
}

fn kit() -> Vec<(&'static str, &'static str)> {
    vec![
        ("project.toml", KIT_MANIFEST),
        ("scenes/panel.toml", PANEL),
        ("art/button.png", "kit's button"),
        ("art/unused.png", "nobody names this"),
        ("wip/sketch.png", "ignored"),
    ]
}

/// The dependency's scene names `art/button.png` from its own root, while
/// the game keeps an `art/button.png` of its own.
#[test]
fn a_dependency_scene_reads_its_own_files_not_the_games() {
    let dir = workspace(
        &[
            ("project.toml", GAME_MANIFEST),
            ("art/button.png", "the game's button"),
        ],
        &kit(),
    );
    let app = app_in(&dir.path().join("game"));
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(
        &app.engine,
        r#"
[[nodes]]
id = "n_ui"
name = "Ui"
instance = "deps/kit/scenes/panel.toml"
"#,
        root,
        false,
    )
    .unwrap();
    assert_eq!(
        label(&app, "Ui").as_deref(),
        Some("deps/kit/art/button.png")
    );
    let read = balaur_core::project::ProjectFiles::read(
        &app.engine
            .resource::<balaur_core::project::ProjectFiles>()
            .borrow(),
        "deps/kit/art/button.png",
    )
    .unwrap();
    assert_eq!(read, b"kit's button");
}

/// A `path` dependency is mounted rather than linked: the game's folder holds
/// no `deps/` and every read goes through the backend.
#[test]
fn a_path_dependency_is_a_mount_through_the_backend() {
    let dir = workspace(&[("project.toml", GAME_MANIFEST)], &kit());
    let game = dir.path().join("game");
    let app = app_in(&game);
    assert!(!game.join("deps").exists());
    let backend = balaur_core::files::backend(&app.engine);
    assert_eq!(
        backend.read(&game.join("deps/kit/art/button.png")).unwrap(),
        b"kit's button"
    );
    let listed: Vec<String> = backend
        .list(&game.join("deps"))
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(listed, ["kit"]);
    // The game has no `deps/` folder, and still lists one.
    assert!(
        backend
            .list(&game)
            .iter()
            .any(|(name, dir)| name == "deps" && *dir)
    );
    assert!(backend.is_dir(&game.join("deps")));
    assert!(balaur_core::project::scene_text(&app.engine, "deps/kit/scenes/panel.toml").is_ok());
}

#[test]
fn an_installed_dependency_is_read_only() {
    let dir = workspace(&[("project.toml", GAME_MANIFEST)], &kit());
    let game = dir.path().join("game");
    let app = app_in(&game);
    let refused =
        balaur_core::dependencies::refuse_write(&app.engine, &game.join("deps/kit/x.toml"));
    assert!(refused.unwrap_err().to_string().contains("`kit`"));
    assert!(balaur_core::dependencies::refuse_write(&app.engine, &game.join("x.toml")).is_ok());
}

/// A path a dependency's script builds reads under its own folder while that
/// script calls; the game's scripts and the scene build are the game's.
#[test]
fn a_dependency_script_builds_paths_from_its_own_root() {
    let dir = workspace(&[("project.toml", GAME_MANIFEST)], &kit());
    let app = app_in(&dir.path().join("game"));
    let eng = &app.engine;
    {
        let _turn = balaur_core::dependencies::calling(eng, "deps/kit/scripts/button.bs");
        assert_eq!(
            balaur_core::project::path_of(eng, "art/hit.png").unwrap(),
            "deps/kit/art/hit.png"
        );
        assert_eq!(
            balaur_core::project::path_of(eng, "deps/other/x.png").unwrap(),
            "deps/other/x.png"
        );
        let _game = balaur_core::dependencies::as_game(eng);
        assert_eq!(
            balaur_core::project::path_of(eng, "art/hit.png").unwrap(),
            "art/hit.png"
        );
    }
    let _turn = balaur_core::dependencies::calling(eng, "scripts/player.bs");
    assert_eq!(
        balaur_core::project::path_of(eng, "art/hit.png").unwrap(),
        "art/hit.png"
    );
}

#[test]
fn a_dependency_file_imports_with_its_own_defaults() {
    let dir = workspace(&[("project.toml", GAME_MANIFEST)], &kit());
    let app = app_in(&dir.path().join("game"));
    let own = balaur_core::import::settings(&app.engine, "deps/kit/art/button.png");
    assert_eq!(
        own.get("filter").and_then(toml::Value::as_str),
        Some("nearest")
    );
    let game = balaur_core::import::settings(&app.engine, "art/button.png");
    assert!(game.get("filter").is_none());
}

/// Make local copies the file into the game, points the game's references at
/// the copy, and spells the copy's own paths the way the game names them.
#[test]
fn make_local_copies_and_repoints() {
    let mut kit = kit();
    kit.push((
        "materials/glass.toml",
        "type = \"material\"\ntexture = \"art/button.png\"\n",
    ));
    let dir = workspace(
        &[
            ("project.toml", GAME_MANIFEST),
            (
                "scenes/main.toml",
                "# kept\n[[nodes]]\nname = \"Pane\"\nmaterial = \"deps/kit/materials/glass.toml\"\n",
            ),
        ],
        &kit,
    );
    let game = dir.path().join("game");
    let app = app_in(&game);
    let rewritten = balaur_core::asset_index::make_local(
        &app.engine,
        "deps/kit/materials/glass.toml",
        "materials/glass.toml",
    )
    .unwrap();
    assert_eq!(rewritten, ["scenes/main.toml"]);
    let scene = std::fs::read_to_string(game.join("scenes/main.toml")).unwrap();
    assert!(scene.starts_with("# kept"), "{scene}");
    assert!(
        scene.contains("material = \"materials/glass.toml\""),
        "{scene}"
    );
    let copy = std::fs::read_to_string(game.join("materials/glass.toml")).unwrap();
    assert!(
        copy.contains("texture = \"deps/kit/art/button.png\""),
        "{copy}"
    );
    // The dependency itself is untouched.
    let original = std::fs::read_to_string(dir.path().join("kit/materials/glass.toml")).unwrap();
    assert!(original.contains("texture = \"art/button.png\""));
}

#[test]
fn move_into_a_path_dependency_repoints_the_game() {
    let dir = workspace(
        &[
            ("project.toml", GAME_MANIFEST),
            ("art/old.png", "kept for later"),
            (
                "scenes/main.toml",
                "[[nodes]]\nname = \"Old\"\ntexture = \"art/old.png\"\n",
            ),
        ],
        &kit(),
    );
    let game = dir.path().join("game");
    let app = app_in(&game);
    let rewritten =
        balaur_core::asset_index::move_into(&app.engine, &["art/old.png".to_string()], "kit")
            .unwrap();
    assert_eq!(rewritten, ["scenes/main.toml"]);
    assert!(!game.join("art/old.png").exists());
    assert_eq!(
        std::fs::read(dir.path().join("kit/art/old.png")).unwrap(),
        b"kept for later"
    );
    let scene = std::fs::read_to_string(game.join("scenes/main.toml")).unwrap();
    assert!(scene.contains("\"deps/kit/art/old.png\""), "{scene}");
}

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

/// An export carries what the game and the dependency's scripts name, in the
/// game's spelling, and drops the rest of the dependency.
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
    let pack = Pack::build(&dir.path().join("game"), &Keeping).unwrap();
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
    let err = Pack::build(&dir.path().join("game"), &Keeping).unwrap_err();
    assert!(format!("{err:#}").contains("balaur fetch"), "{err:#}");
}
