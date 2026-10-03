//! A `mesh` node's own keys, as a scene writes them and a read-back reports.

use balaur_core::{App, AppConfig, components, project, scene};
use balaur_render::RenderPlugin;

const SCENE: &str = r#"
[[assets]]
id = "tri"
type = "mesh"
positions = [[0, 0, 0], [1, 0, 0], [0, 1, 0]]
indices = [[0, 1, 2]]
"#;

fn color_of(params: &str) -> Vec<f64> {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig::bare(dir.path().to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut RenderPlugin::default()).unwrap();
    let root = app.engine.root();
    project::instantiate_scene(&app.engine, SCENE, root, false).unwrap();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), "Tri", root);
    let params: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, "mesh", Some(&params)).unwrap();
    let read = components::get(&app.engine, entity, "mesh").expect("a mesh reads back");
    read["color"]
        .as_array()
        .expect("the colour is four channels")
        .iter()
        .map(|c| c.as_float().unwrap())
        .collect()
}

#[test]
fn a_mesh_that_names_no_colour_draws_its_texture_at_full_brightness() {
    assert_eq!(color_of(r#"source = "tri""#), vec![1.0, 1.0, 1.0, 1.0]);
}

#[test]
fn a_mesh_takes_the_colour_its_scene_names() {
    assert_eq!(
        color_of(
            r#"source = "tri"
color = [0.5, 0.25, 1.0, 0.5]"#
        ),
        vec![0.5, 0.25, 1.0, 0.5]
    );
}
