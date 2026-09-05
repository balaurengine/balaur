//! The `polygon` component, headless: what a scene's inline mesh resolves
//! to, how UVs default from the texture, and what reads back.

use balaur_core::scene;
use balaur_core::{components, App, AppConfig};
use balaur_render::{PolygonMesh, RenderPlugin, Renderable2d, Shape2d};
use glamx::Vec2;

/// A project on disk with a 200×100 image in it.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("art")).unwrap();
    let image = image::RgbaImage::from_pixel(200, 100, image::Rgba([255, 0, 0, 255]));
    image.save(dir.path().join("art/wide.png")).unwrap();
    dir
}

fn app_in(root: &std::path::Path) -> App {
    let mut app = App::new(AppConfig::bare(root.to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut RenderPlugin::default()).unwrap();
    app
}

fn polygon_on(app: &App, params: &str) -> (balaur_core::hecs::Entity, std::sync::Arc<PolygonMesh>) {
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), "Skin", root);
    let params: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, "polygon", Some(&params)).unwrap();
    let world = app.engine.world();
    let renderable = world.get::<&Renderable2d>(entity).unwrap();
    assert!(renderable.shape == Shape2d::Polygon);
    (entity, renderable.polygon.clone().unwrap())
}

const SQUARE: &str = r#"
texture = "art/wide.png"
[mesh]
positions = [[-1, -0.5], [1, -0.5], [1, 0.5], [-1, 0.5]]
"#;

#[test]
fn an_inline_mesh_resolves_to_triangles_in_the_nodes_space() {
    let dir = project();
    let app = app_in(dir.path());
    let (_, polygon) = polygon_on(&app, SQUARE);
    assert_eq!(polygon.positions.len(), 4);
    assert_eq!(polygon.indices.len(), 2);
    assert_eq!(polygon.positions[2], Vec2::new(1.0, 0.5));
    assert!(polygon.skin.is_none());
}

#[test]
fn uvs_default_to_the_texture_centred_on_the_origin_at_pixels_per_unit() {
    let dir = project();
    let app = app_in(dir.path());
    let (_, polygon) = polygon_on(&app, SQUARE);
    // 200 px wide at 100 px per unit is two units: the corners land on the
    // texture's corners, v downward.
    let close = |a: Vec2, b: Vec2| (a - b).length() < 1e-5;
    assert!(
        close(polygon.uvs[0], Vec2::new(0.0, 1.0)),
        "{:?}",
        polygon.uvs[0]
    );
    assert!(
        close(polygon.uvs[2], Vec2::new(1.0, 0.0)),
        "{:?}",
        polygon.uvs[2]
    );
}

#[test]
fn authored_uvs_are_kept_and_the_default_is_a_pure_function() {
    let dir = project();
    let app = app_in(dir.path());
    let (_, polygon) = polygon_on(
        &app,
        "texture = \"art/wide.png\"\n[mesh]\npositions = [[0, 0], [1, 0], [0, 1]]\nuvs = [[0.1, 0.2], [0.3, 0.4], [0.5, 0.6]]",
    );
    assert_eq!(polygon.uvs[1], Vec2::new(0.3, 0.4));
    let uv = PolygonMesh::default_uv(Vec2::new(0.5, 0.25), 100.0, (200, 100));
    assert!((uv - Vec2::new(0.75, 0.25)).length() < 1e-6, "{uv:?}");
}

#[test]
fn a_skinned_mesh_carries_its_bones_and_the_rig_path_reads_back() {
    let dir = project();
    let app = app_in(dir.path());
    let (entity, polygon) = polygon_on(
        &app,
        r#"
skeleton = "../Rig"
[mesh]
positions = [[0, 1], [1, 0], [0, -1]]
[[mesh.skin.bones]]
path = "Hip"
weights = [1, 0.5, 0]
[[mesh.skin.bones]]
path = "Hip/Thigh"
weights = [0, 0.5, 1]
"#,
    );
    let skin = polygon.skin.as_ref().unwrap();
    assert_eq!(skin.bones, vec!["Hip".to_string(), "Hip/Thigh".to_string()]);
    assert_eq!(skin.joints[2], [1, 0, 0, 0]);
    let got = components::get(&app.engine, entity, "polygon").unwrap();
    assert_eq!(got.get("skeleton").unwrap().as_str(), Some("../Rig"));
    // The inline table came back as the digest reference that now names it.
    assert!(got.get("mesh").unwrap().as_str().unwrap().starts_with("#!"));
    assert_eq!(got.get("pixels_per_unit").unwrap().as_float(), Some(100.0));
}

#[test]
fn a_polygon_with_no_mesh_yet_exists_and_draws_nothing() {
    let dir = project();
    let app = app_in(dir.path());
    let (entity, polygon) = polygon_on(&app, "");
    assert!(polygon.positions.is_empty());
    assert!(components::get(&app.engine, entity, "polygon").is_some());
}

#[test]
fn a_mesh_that_does_not_resolve_warns_and_leaves_the_polygon_empty() {
    let dir = project();
    let app = app_in(dir.path());
    let (_, polygon) = polygon_on(&app, "mesh = \"models/missing.toml\"");
    assert!(polygon.positions.is_empty());
    assert_eq!(polygon.mesh, "models/missing.toml");
}

#[test]
fn retinting_does_not_rebuild_but_new_geometry_does() {
    let dir = project();
    let app = app_in(dir.path());
    let (entity, _) = polygon_on(&app, SQUARE);
    let version = |app: &App| {
        app.engine
            .world()
            .get::<&Renderable2d>(entity)
            .unwrap()
            .version
    };
    let before = version(&app);
    let tint: toml::Value = toml::from_str("color = [1, 0, 0, 1]").unwrap();
    components::patch(&app.engine, entity, "polygon", &tint).unwrap();
    assert_eq!(version(&app), before);
    let moved: toml::Value =
        toml::from_str("[mesh]\npositions = [[-2, -0.5], [1, -0.5], [1, 0.5], [-2, 0.5]]").unwrap();
    components::patch(&app.engine, entity, "polygon", &moved).unwrap();
    assert_eq!(version(&app), before + 1);
}
