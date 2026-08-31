//! What an editing tool stands on: scrubbing, named inline clips, and the
//! promotion of an inline clip to a file.
//!
//! The editor's Timeline is Luau and nothing in CI runs it, so the engine
//! behaviour it depends on is pinned here instead — headless, from Rust, with
//! no editor in the picture.

use balaur_anim::{AnimationPlugin, AnimationState};
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{assets, components, project, App, AppConfig};

fn app_in(project_root: &std::path::Path) -> App {
    let mut app = App::new(AppConfig {
        project_root: project_root.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    app.add_plugin(AnimationPlugin).unwrap();
    app
}

fn app() -> App {
    app_in(std::path::Path::new("tests/fixtures"))
}

/// The clip the editor's own self-test authors: a node lifted ten units over
/// one second, written as a *named* entry so `autoplay` can name it.
const LIBRARY: &str = r#"
[library.clips.rise]
length = 1.0
tracks = [
  { property = "position", keys = [
    { t = 0.0, value = [0.0, 0.0, 0.0] },
    { t = 1.0, value = [0.0, 10.0, 0.0] },
  ] },
]
"#;

fn animated(app: &App, name: &str, params: &str) -> Entity {
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), name, root);
    let params: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, "animation", Some(&params)).unwrap();
    entity
}

fn height(app: &App, entity: Entity) -> f32 {
    app.engine
        .world()
        .get::<&Transform>(entity)
        .unwrap()
        .position
        .y
}

fn tick(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
    }
}

/// Scrubbing a timeline is a seek on a clip that is not advancing. If the seek
/// waited for a step to show, the ruler would move and the node would not.
#[test]
fn a_scrub_poses_a_paused_clip_where_the_playhead_lands() {
    let app = app();
    let entity = animated(&app, "Box", LIBRARY);
    balaur_anim::play(&app.engine, entity, "rise").unwrap();
    balaur_anim::pause(&app.engine, entity);

    balaur_anim::seek(&app.engine, entity, 0.25);
    assert!(
        (height(&app, entity) - 2.5).abs() < 1e-4,
        "a scrub did not pose the node: {}",
        height(&app, entity)
    );
    balaur_anim::seek(&app.engine, entity, 0.75);
    assert!((height(&app, entity) - 7.5).abs() < 1e-4);
}

#[test]
fn a_scrub_moves_nothing_but_the_playhead() {
    let mut app = app();
    let entity = animated(&app, "Box", LIBRARY);
    balaur_anim::play(&app.engine, entity, "rise").unwrap();
    balaur_anim::pause(&app.engine, entity);
    balaur_anim::seek(&app.engine, entity, 0.5);
    let posed = height(&app, entity);

    tick(&mut app, 30);
    assert_eq!(
        height(&app, entity).to_bits(),
        posed.to_bits(),
        "a paused clip advanced after a scrub"
    );
    assert!(!balaur_anim::is_playing(&app.engine, entity));
    assert_eq!(
        balaur_anim::current(&app.engine, entity).as_deref(),
        Some("rise"),
        "a scrub lost the clip it was scrubbing"
    );
}

/// The editor writes its clips inline first and promotes them to a file
/// second, so an inline clip has to be nameable — otherwise `autoplay` could
/// only ever start a clip that already lives in a file.
#[test]
fn an_inline_library_autoplays_the_entry_it_names() {
    let mut app = app();
    let entity = animated(
        &app,
        "Box",
        &format!("autoplay = \"rise\"\nspeed = 1.0\nroot = \"\"\n{LIBRARY}"),
    );
    tick(&mut app, 30);
    assert!(
        (height(&app, entity) - 5.0).abs() < 0.05,
        "an inline clip did not autoplay: {}",
        height(&app, entity)
    );
}

/// A path that does not resolve is a broken reference, not a broken scene: the
/// node still loads, so the editor can show it and the author can fix it.
#[test]
fn an_autoplay_that_cannot_load_leaves_the_rest_of_the_scene_standing() {
    let app = app();
    let source = r#"
[[nodes]]
name = "Box"

[nodes.animation]
library = "animations/nothing_here.toml"
autoplay = "idle"

[[nodes]]
name = "Ground"
"#;
    let root = app.engine.root();
    project::instantiate_scene(&app.engine, source, root, false).unwrap();
    assert!(
        scene::find_node(&app.engine.world(), root, "Ground").is_some(),
        "one unresolvable clip took the whole scene down"
    );
    let entity = scene::find_node(&app.engine.world(), root, "Box").unwrap();
    assert!(balaur_anim::current(&app.engine, entity).is_none());
}

/// "Save as file" writes the definition the editor was already previewing.
/// The two forms are one document, so they have to pose identically.
#[test]
fn a_clip_promoted_to_a_file_poses_exactly_as_the_inline_one_did() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(dir.path());
    let inline = animated(&app, "Inline", LIBRARY);
    balaur_anim::play(&app.engine, inline, "rise").unwrap();
    balaur_anim::pause(&app.engine, inline);
    balaur_anim::seek(&app.engine, inline, 0.4);

    // What the editor's promote does: the same table, typed, on disk.
    let document: toml::Value = toml::from_str(LIBRARY).unwrap();
    let mut promoted = document.get("library").unwrap().clone();
    promoted
        .as_table_mut()
        .unwrap()
        .insert("type".into(), toml::Value::String("animation_clip".into()));
    std::fs::create_dir_all(dir.path().join("animations")).unwrap();
    std::fs::write(
        dir.path().join("animations/box.toml"),
        toml::to_string(&promoted).unwrap(),
    )
    .unwrap();
    assets::reload(&app.engine, "animations/box.toml").unwrap();

    let external = animated(&app, "External", "library = \"animations/box.toml\"");
    balaur_anim::play(&app.engine, external, "rise").unwrap();
    balaur_anim::pause(&app.engine, external);
    balaur_anim::seek(&app.engine, external, 0.4);
    assert_eq!(
        height(&app, external).to_bits(),
        height(&app, inline).to_bits(),
        "promoting a clip to a file changed what it does"
    );

    // And it still plays: promotion is a move, not a copy that goes stale.
    balaur_anim::play(&app.engine, external, "rise").unwrap();
    tick(&mut app, 30);
    assert!((height(&app, external) - 5.0).abs() < 0.05);
}

/// The editor saves the scene by encoding its document. A clip nested under a
/// node is the deepest thing that document holds, and TOML is particular about
/// what may follow a table — so the round trip is pinned rather than assumed.
#[test]
fn a_scene_document_holding_an_inline_clip_encodes_and_parses_back() {
    let app = app();
    // Exactly the shape `model.save` encodes: one node of the `nodes` array,
    // with the clip nested two tables below it.
    let node: toml::Value = toml::from_str(
        r#"
name = "Box"
parent = ""
position = [0.0, 0.0, 0.0]
animation.autoplay = "rise"

[animation.library.clips.rise]
length = 1.0
tracks = [
  { property = "position", keys = [
    { t = 0.0, value = [0.0, 0.0, 0.0] },
    { t = 1.0, value = [0.0, 10.0, 0.0] },
  ] },
]
"#,
    )
    .unwrap();
    let document = toml::Value::Table(toml::map::Map::from_iter([(
        "nodes".to_string(),
        toml::Value::Array(vec![node]),
    )]));

    let encoded = toml::to_string(&document).expect("the editor cannot save what it cannot encode");
    let root = app.engine.root();
    project::instantiate_scene(&app.engine, &encoded, root, false).unwrap();
    let entity = scene::find_node(&app.engine.world(), root, "Box").unwrap();
    assert_eq!(
        balaur_anim::current(&app.engine, entity).as_deref(),
        Some("rise"),
        "an encoded-and-reparsed inline clip did not come back"
    );
    assert!(app
        .engine
        .resource::<AnimationState>()
        .borrow()
        .players
        .contains_key(&entity));
}
