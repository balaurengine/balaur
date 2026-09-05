//! Stable asset ids and rename refactoring: `id://` references that resolve
//! through `assets/index.toml`, and a rename that rewrites every reference
//! in the project while keeping the comments around them.

use std::any::Any;
use std::rc::Rc;

use balaur_core::project::ProjectFiles;
use balaur_core::{App, AppConfig, Pack, asset_index, assets};

#[derive(Debug, PartialEq)]
struct Note {
    pitch: f64,
}

fn parse_note(body: &toml::Value) -> anyhow::Result<Rc<dyn Any>> {
    let pitch = body
        .get("pitch")
        .and_then(toml::Value::as_float)
        .ok_or_else(|| anyhow::anyhow!("a note needs a `pitch`"))?;
    Ok(Rc::new(Note { pitch }))
}

fn app_in(root: &std::path::Path, pack: Option<Pack>) -> App {
    let mut app = App::new(AppConfig {
        pack,
        ..AppConfig::bare(root.to_path_buf())
    })
    .unwrap();
    app.register_asset_type("note", "notes", "", parse_note);
    app
}

const SCALE: &str = r#"type = "note"
pitch = 1.0 # the root

[entries.high]
pitch = 880.0
"#;

const SCENE: &str = r#"# The scene keeps its comments.
[[nodes]]
name = "Player"
instrument = { song = "notes/scale.toml#high" } # inline reference
texture = "art/hero.png"

[[nodes]]
name = "Other"
songs = ["notes/scale.toml", "notes/other.toml"]
"#;

/// A project with one asset library, one image, and a scene naming both.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for sub in ["notes", "art", "scenes"] {
        std::fs::create_dir_all(dir.path().join(sub)).unwrap();
    }
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("notes/scale.toml"), SCALE).unwrap();
    std::fs::write(dir.path().join("art/hero.png"), b"not really a png").unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), SCENE).unwrap();
    dir
}

fn pitch_of(app: &App, reference: &str) -> f64 {
    assets::load_typed::<Note>(&app.engine, reference)
        .unwrap()
        .pitch
}

fn read(dir: &tempfile::TempDir, rel: &str) -> String {
    std::fs::read_to_string(dir.path().join(rel)).unwrap()
}

#[test]
fn an_id_reference_loads_the_file_the_index_names() {
    let dir = project();
    std::fs::create_dir_all(dir.path().join("assets")).unwrap();
    std::fs::write(
        dir.path().join("assets/index.toml"),
        "abc = \"notes/scale.toml\"\n",
    )
    .unwrap();
    let app = app_in(dir.path(), None);
    assert!((pitch_of(&app, "id://abc") - 1.0).abs() < f64::EPSILON);
    assert!((pitch_of(&app, "id://abc#high") - 880.0).abs() < f64::EPSILON);
    // One cache entry however it was spelled.
    let by_id = assets::load(&app.engine, "id://abc").unwrap();
    let by_path = assets::load(&app.engine, "notes/scale.toml").unwrap();
    assert!(Rc::ptr_eq(&by_id, &by_path));
}

#[test]
fn an_unknown_id_fails_naming_the_index() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let error = assets::load(&app.engine, "id://nope")
        .unwrap_err()
        .to_string();
    assert!(error.contains("nope"), "unhelpful: {error}");
    assert!(error.contains("assets/index.toml"), "unhelpful: {error}");
}

#[test]
fn a_binary_file_reads_by_id_too() {
    let dir = project();
    std::fs::create_dir_all(dir.path().join("assets")).unwrap();
    std::fs::write(
        dir.path().join("assets/index.toml"),
        "hero = \"art/hero.png\"\n",
    )
    .unwrap();
    let app = app_in(dir.path(), None);
    let files = app.engine.resource::<ProjectFiles>();
    let bytes = files.borrow().read("id://hero").unwrap();
    assert_eq!(bytes, b"not really a png");
    assert_eq!(
        files.borrow().id_of("art/hero.png").as_deref(),
        Some("hero")
    );
}

#[test]
fn a_pack_carries_the_index() {
    let dir = project();
    let pack = Pack {
        manifest: read(&dir, "project.toml"),
        scenes: [(
            "assets/index.toml".to_string(),
            "hero = \"art/hero.png\"\nscale = \"notes/scale.toml\"\n".to_string(),
        )]
        .into_iter()
        .collect(),
        scripts: std::collections::BTreeMap::new(),
        assets: [("art/hero.png".to_string(), b"packed".to_vec())]
            .into_iter()
            .collect(),
    };
    // A packed run reads nothing from the directory the pack was built in.
    let app = app_in(std::path::Path::new("/nowhere"), Some(pack));
    let files = app.engine.resource::<ProjectFiles>();
    let bytes = files.borrow().read("id://hero").unwrap();
    assert_eq!(bytes, b"packed");
    assert_eq!(
        files.borrow().path_of("id://scale#high").unwrap(),
        "notes/scale.toml#high"
    );
}

#[test]
fn saving_the_index_is_a_reload_the_next_id_sees() {
    let dir = project();
    let app = app_in(dir.path(), None);
    assert!(assets::load(&app.engine, "id://abc").is_err());
    std::fs::create_dir_all(dir.path().join("assets")).unwrap();
    std::fs::write(
        dir.path().join("assets/index.toml"),
        "abc = \"notes/scale.toml\"\n",
    )
    .unwrap();
    // What the watcher does for every saved `.toml`.
    let before = assets::generation(&app.engine);
    assets::reload(&app.engine, assets::INDEX_PATH).unwrap();
    assert_ne!(assets::generation(&app.engine), before);
    assert!((pitch_of(&app, "id://abc") - 1.0).abs() < f64::EPSILON);
}

#[test]
fn assigning_an_id_writes_the_index_and_stamps_an_asset_document() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let id = asset_index::assign_id(&app.engine, "notes/scale.toml").unwrap();
    assert_eq!(id.len(), 16, "an id is sixteen hex digits, not {id}");
    assert_eq!(
        asset_index::assign_id(&app.engine, "notes/scale.toml").unwrap(),
        id,
        "assigning twice is the same id"
    );
    assert_eq!(
        asset_index::id_of(&app.engine, "notes/scale.toml")
            .unwrap()
            .as_deref(),
        Some(id.as_str())
    );
    assert_eq!(
        asset_index::id_of(&app.engine, "art/hero.png").unwrap(),
        None
    );
    let index = read(&dir, "assets/index.toml");
    assert!(
        index.contains(&format!("{id} = \"notes/scale.toml\"")),
        "{index}"
    );
    let document = read(&dir, "notes/scale.toml");
    assert!(document.contains(&format!("id = \"{id}\"")), "{document}");
    assert!(
        document.contains("# the root"),
        "the comment was lost: {document}"
    );
    // The running engine sees the new id at once.
    assert!((pitch_of(&app, &format!("id://{id}#high")) - 880.0).abs() < f64::EPSILON);
}

#[test]
fn a_document_that_declares_its_own_id_keeps_it() {
    let dir = project();
    std::fs::write(
        dir.path().join("notes/scale.toml"),
        "type = \"note\"\nid = \"mine\"\npitch = 1.0\n",
    )
    .unwrap();
    let app = app_in(dir.path(), None);
    assert_eq!(
        asset_index::assign_id(&app.engine, "notes/scale.toml").unwrap(),
        "mine"
    );
    assert!(read(&dir, "assets/index.toml").contains("mine = \"notes/scale.toml\""));
}

#[test]
fn a_scene_and_a_binary_are_indexed_but_not_stamped() {
    let dir = project();
    let app = app_in(dir.path(), None);
    asset_index::assign_id(&app.engine, "scenes/main.toml").unwrap();
    asset_index::assign_id(&app.engine, "art/hero.png").unwrap();
    assert_eq!(
        read(&dir, "scenes/main.toml"),
        SCENE,
        "a scene is not an asset document"
    );
    assert_eq!(read(&dir, "art/hero.png"), "not really a png");
}

#[test]
fn renaming_a_file_rewrites_every_reference_and_keeps_comments() {
    let dir = project();
    let app = app_in(dir.path(), None);
    assert!((pitch_of(&app, "notes/scale.toml#high") - 880.0).abs() < f64::EPSILON);
    let rewritten =
        asset_index::rename(&app.engine, "notes/scale.toml", "notes/tune.toml").unwrap();
    assert_eq!(rewritten, vec!["scenes/main.toml".to_string()]);
    assert!(!dir.path().join("notes/scale.toml").exists());
    assert!(dir.path().join("notes/tune.toml").exists());
    let scene = read(&dir, "scenes/main.toml");
    assert!(
        scene.contains("song = \"notes/tune.toml#high\" } # inline reference"),
        "{scene}"
    );
    assert!(
        scene.contains("songs = [\"notes/tune.toml\", \"notes/other.toml\"]"),
        "{scene}"
    );
    assert!(scene.contains("# The scene keeps its comments."), "{scene}");
    assert!(scene.contains("texture = \"art/hero.png\""), "{scene}");
    // The cache forgot the old path and the new one loads.
    assert!(assets::load(&app.engine, "notes/scale.toml").is_err());
    assert!((pitch_of(&app, "notes/tune.toml#high") - 880.0).abs() < f64::EPSILON);
}

#[test]
fn renaming_a_directory_rewrites_the_paths_under_it_and_the_index() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let id = asset_index::assign_id(&app.engine, "art/hero.png").unwrap();
    let rewritten = asset_index::rename(&app.engine, "art", "images").unwrap();
    assert_eq!(
        rewritten,
        vec![
            "assets/index.toml".to_string(),
            "scenes/main.toml".to_string()
        ]
    );
    assert!(read(&dir, "scenes/main.toml").contains("texture = \"images/hero.png\""));
    assert!(read(&dir, "assets/index.toml").contains(&format!("{id} = \"images/hero.png\"")));
    let files = app.engine.resource::<ProjectFiles>();
    let bytes = files.borrow().read(&format!("id://{id}")).unwrap();
    assert_eq!(bytes, b"not really a png", "the id follows the moved file");
}

#[test]
fn a_rename_refuses_to_overwrite_and_to_move_nothing() {
    let dir = project();
    let app = app_in(dir.path(), None);
    let error = asset_index::rename(&app.engine, "notes/scale.toml", "scenes/main.toml")
        .unwrap_err()
        .to_string();
    assert!(error.contains("scenes/main.toml"), "unhelpful: {error}");
    assert!(dir.path().join("notes/scale.toml").exists());
    let error = asset_index::rename(&app.engine, "notes/none.toml", "notes/x.toml")
        .unwrap_err()
        .to_string();
    assert!(error.contains("notes/none.toml"), "unhelpful: {error}");
}

#[test]
fn a_rename_reaches_another_project_by_absolute_path() {
    let editor = project();
    let game = project();
    let app = app_in(editor.path(), None);
    balaur_core::file_api::add_root(&app.engine, game.path());
    let from = game.path().join("notes/scale.toml");
    let to = game.path().join("notes/tune.toml");
    let rewritten =
        asset_index::rename(&app.engine, &from.to_string_lossy(), &to.to_string_lossy()).unwrap();
    assert_eq!(rewritten, vec!["scenes/main.toml".to_string()]);
    assert!(read(&game, "scenes/main.toml").contains("notes/tune.toml#high"));
    assert!(
        read(&editor, "scenes/main.toml").contains("notes/scale.toml#high"),
        "the editor's own scene is not the game's"
    );
}
