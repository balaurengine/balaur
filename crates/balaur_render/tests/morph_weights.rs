//! Morph weights as a component property: a scene sets one, a patch leaves
//! the others where they were, and the names come from the asset rather than
//! from the track that drives them.

use balaur_core::hecs::Entity;
use balaur_core::{App, AppConfig, components, project, scene};
use balaur_render::{MorphWeights, RenderPlugin};

/// A face with two shapes to blend towards.
const SCENE: &str = r#"
[[assets]]
id = "face"
type = "mesh"
positions = [[0, 0, 0], [1, 0, 0], [0, 1, 0]]
indices = [[0, 1, 2]]

[[assets.morphs]]
name = "smile"
positions = [[0, 0.1, 0], [0, 0.2, 0], [0, 0.1, 0]]

[[assets.morphs]]
name = "blink"
positions = [[0, -0.1, 0], [0, 0, 0], [0, -0.1, 0]]
"#;

fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig::bare(dir.path().to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut RenderPlugin::default()).unwrap();
    let root = app.engine.root();
    project::instantiate_scene(&app.engine, SCENE, root, false).unwrap();
    (dir, app)
}

fn face(app: &App, params: &str) -> Entity {
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), "Face", root);
    let params: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, "mesh", Some(&params)).unwrap();
    entity
}

fn weights(app: &App, entity: Entity) -> Vec<(String, f32)> {
    let world = app.engine.world();
    let morphs = world
        .get::<&MorphWeights>(entity)
        .expect("a mesh with shapes");
    morphs
        .names
        .iter()
        .cloned()
        .zip(morphs.weights.iter().copied())
        .collect()
}

#[test]
fn a_mesh_carries_a_weight_for_every_shape_its_asset_has() {
    let (_dir, app) = app();
    let entity = face(&app, "source = \"#face\"");
    assert_eq!(
        weights(&app, entity),
        vec![("smile".to_string(), 0.0), ("blink".to_string(), 0.0)],
        "named by the asset, and none of them blended yet"
    );
}

#[test]
fn a_scene_can_set_one_shapes_weight() {
    let (_dir, app) = app();
    let entity = face(&app, "source = \"#face\"\n\"morph.smile\" = 0.75");
    assert_eq!(
        weights(&app, entity),
        vec![("smile".to_string(), 0.75), ("blink".to_string(), 0.0)]
    );
}

/// What an animation track does: one property at a time, through `patch`,
/// which is why the others have to survive it.
#[test]
fn driving_one_weight_leaves_the_others_alone() {
    let (_dir, app) = app();
    let entity = face(
        &app,
        "source = \"#face\"\n\"morph.smile\" = 0.5\n\"morph.blink\" = 0.25",
    );
    let track: toml::Value = toml::from_str("\"morph.blink\" = 1.0").unwrap();
    components::patch(&app.engine, entity, "mesh", &track).unwrap();
    assert_eq!(
        weights(&app, entity),
        vec![("smile".to_string(), 0.5), ("blink".to_string(), 1.0)],
        "the smile should not have been reset by a track about the blink"
    );
}

/// The weights read back as `morph.<name>` keys, which is what makes them
/// addressable as `mesh/morph.<name>` from a clip.
#[test]
fn the_weights_read_back_under_their_names() {
    let (_dir, app) = app();
    let entity = face(&app, "source = \"#face\"\n\"morph.smile\" = 0.4");
    let back = components::get(&app.engine, entity, "mesh").expect("it reads back");
    // A weight is stored as `f32` and read back as `f64`, so the comparison
    // is against what that round trip leaves, not against the literal.
    let weight = |key: &str| {
        back.get(key)
            .and_then(balaur_core::components::as_f64)
            .expect("a weight for every shape")
    };
    assert!((weight("morph.smile") - 0.4).abs() < 1e-6);
    assert!(weight("morph.blink").abs() < 1e-6);
}

/// A mesh with nothing to blend carries no weights at all, rather than an
/// empty list nothing will ever write to.
#[test]
fn a_mesh_with_no_shapes_carries_no_weights() {
    let (_dir, app) = app();
    let root = app.engine.root();
    project::instantiate_scene(
        &app.engine,
        "[[assets]]\nid = \"plain\"\ntype = \"mesh\"\npositions = [[0, 0, 0], [1, 0, 0], [0, 1, 0]]\nindices = [[0, 1, 2]]\n",
        root,
        false,
    )
    .unwrap();
    let entity = face(&app, "source = \"#plain\"");
    let world = app.engine.world();
    assert!(world.get::<&MorphWeights>(entity).is_err());
}
