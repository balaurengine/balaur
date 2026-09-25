//! What the editor is told is off about a physics node: a body nothing
//! collides with, a soft body the node cannot draw bent, and a write refused.

use balaur::{AppConfig, standard_app};
use balaur_core::warnings::{NodeWarning, warnings};

use crate::LOG;

/// The warnings on node `id` of a scene of `nodes`, after two ticks.
fn warned(nodes: &str, id: &str) -> Vec<NodeWarning> {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    let scene = format!(
        r#"[[assets]]
id = "hub"
type = "mesh"
positions = [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5], [0.0, 0.0]]
indices = [[0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4]]

[[nodes]]
id = "n_root"
name = "Root"

{nodes}"#
    );
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    let node = balaur_core::ids::find(&world, app.engine.root(), id).expect("the node");
    drop(world);
    warnings(&app.engine, node)
}

fn says(found: &[NodeWarning], component: &str, text: &str) -> bool {
    found
        .iter()
        .any(|w| w.component == component && w.warning.message.contains(text))
}

#[test]
fn a_dynamic_body_nothing_collides_with_warns() {
    let bare = warned(
        "[[nodes]]\nid = \"n_ball\"\nname = \"Ball\"\nparent = \"n_root\"\nbody2d = { kind = \"dynamic\" }\n",
        "n_ball",
    );
    assert!(says(&bare, "body2d", "nothing collides"), "{bare:?}");
    let held = warned(
        "[[nodes]]\nid = \"n_ball\"\nname = \"Ball\"\nparent = \"n_root\"\nbody2d = { kind = \"dynamic\" }\n\n[[nodes]]\nid = \"n_shape\"\nname = \"Shape\"\nparent = \"n_ball\"\ncollider2d = { kind = \"circle\", radius = 0.5 }\n",
        "n_ball",
    );
    assert!(
        held.is_empty(),
        "a collider on a child is the body's: {held:?}"
    );
}

#[test]
fn a_soft_body_on_a_sprite_warns_that_the_sprite_stays_rigid() {
    let found = warned(
        "[[nodes]]\nid = \"n_blob\"\nname = \"Blob\"\nparent = \"n_root\"\nsprite = { texture = \"\" }\nsoftbody2d = { kind = \"grid\" }\n",
        "n_blob",
    );
    assert!(says(&found, "softbody2d", "draws a sprite"), "{found:?}");
}

#[test]
fn a_polygon_of_another_mesh_than_the_body_warns() {
    let found = warned(
        "[[nodes]]\nid = \"n_blob\"\nname = \"Blob\"\nparent = \"n_root\"\npolygon = { mesh = \"#hub\" }\nsoftbody2d = { kind = \"grid\" }\n",
        "n_blob",
    );
    assert!(says(&found, "softbody2d", "does not bend"), "{found:?}");
    let matched = warned(
        "[[nodes]]\nid = \"n_blob\"\nname = \"Blob\"\nparent = \"n_root\"\npolygon = { mesh = \"#hub\" }\nsoftbody2d = { kind = \"polygon\", mesh = \"#hub\", particle_radius = 0.05 }\n",
        "n_blob",
    );
    assert!(
        matched.is_empty(),
        "the polygon's own mesh bends it: {matched:?}"
    );
}

#[test]
fn a_worked_out_radius_that_makes_the_body_hover_warns() {
    let found = warned(
        "[[nodes]]\nid = \"n_blob\"\nname = \"Blob\"\nparent = \"n_root\"\nsoftbody2d = { kind = \"polygon\", mesh = \"#hub\" }\n",
        "n_blob",
    );
    let hover = found
        .iter()
        .find(|w| w.warning.message.contains("rests"))
        .unwrap_or_else(|| panic!("{found:?}"));
    assert_eq!(hover.warning.property.as_deref(), Some("particle_radius"));
}

#[test]
fn a_refused_layout_is_a_warning_on_what_was_asked() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = balaur_core::App::new(balaur_core::AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut balaur_physics::PhysicsPlugin::default()).unwrap();
    let root = app.engine.root();
    let node = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Blob", root);
    let grid = |cells: f64| {
        toml::from_str(&format!("kind = \"grid\"\ncells = [{cells:?}, {cells:?}]")).unwrap()
    };
    balaur_core::components::add(&app.engine, node, "softbody2d", Some(&grid(2.0))).unwrap();
    let refused =
        balaur_core::components::add(&app.engine, node, "softbody2d", Some(&grid(5000.0)));
    assert!(refused.is_err(), "the cap let 25 million particles through");
    let found = warnings(&app.engine, node);
    let warning = found
        .iter()
        .find(|w| w.warning.message.contains("particles"))
        .unwrap_or_else(|| panic!("{found:?}"));
    assert_eq!(warning.warning.property.as_deref(), Some("cells"));
}
