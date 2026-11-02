//! Dependencies: another project imported whole under `deps/<alias>/`, its
//! paths read from its own root in a run from the sources. What an export
//! ships of one is `balaur_project`'s.

use balaur_core::components::ComponentDef;
use balaur_core::scene::find_node;
use balaur_core::{App, AppConfig, Engine};

#[path = "../support/kit.rs"]
mod kit;
use kit::{GAME_MANIFEST, kit, workspace};

struct Marker(toml::Value);

fn app_in(game: &std::path::Path) -> App {
    let mut app = App::new(AppConfig::bare(game.to_path_buf())).unwrap();
    app.register_component(
        "marker",
        ComponentDef::by_hand(
            ComponentDef::parse_schema(
                "marker",
                r#"label = { type = "string", default = "none" }"#,
            ),
            Box::new(|eng: &Engine, entity, params| {
                eng.world_mut()
                    .insert_one(entity, Marker(params.clone()))
                    .map_err(|_| anyhow::anyhow!("dead node"))?;
                Ok(())
            }),
            Box::new(|eng: &Engine, entity| {
                let _ = eng.world_mut().remove_one::<Marker>(entity);
                Ok(())
            }),
            Box::new(|eng: &Engine, entity| {
                eng.world().get::<&Marker>(entity).ok().map(|m| m.0.clone())
            }),
        ),
    );
    app
}

fn label(app: &App, path: &str) -> Option<String> {
    let world = app.engine.world();
    let entity = find_node(&world, app.engine.root(), path)?;
    let marker = world.get::<&Marker>(entity).ok()?;
    Some(marker.0.get("label")?.as_str()?.to_string())
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
