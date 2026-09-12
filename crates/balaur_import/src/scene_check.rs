//! Loading a generated scene the way the editor would, so a document that
//! only reads right but will not build is a failing test rather than a
//! surprise at open.

use balaur_core::hecs::{self, Entity};
use balaur_core::scene::{Children, Name};

/// Build `scene` under a standard app's root and answer one entry per node
/// that landed directly under it, each with the names of its own children.
///
/// A document with two roots fails here, as it would at load: the loader
/// takes the first node with no parent as the scene's one root.
pub(crate) fn loaded(scene: &str) -> Vec<(String, Vec<String>)> {
    let dir = tempfile::tempdir().expect("a temporary project root for the scene");
    let mut config = balaur::AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let app = balaur::standard_app(config).expect("a standard app the scene loads into");
    let base = app.engine.root();
    balaur_core::project::instantiate_scene(&app.engine, scene, base, false)
        .unwrap_or_else(|why| panic!("the generated scene loads: {why:#}"));
    let world = app.engine.world();
    children(&world, base)
        .into_iter()
        .map(|node| (name(&world, node), named(&world, children(&world, node))))
        .collect()
}

fn children(world: &hecs::World, node: Entity) -> Vec<Entity> {
    world
        .get::<&Children>(node)
        .map(|kids| kids.0.clone())
        .unwrap_or_default()
}

fn named(world: &hecs::World, nodes: Vec<Entity>) -> Vec<String> {
    nodes.into_iter().map(|node| name(world, node)).collect()
}

fn name(world: &hecs::World, node: Entity) -> String {
    world
        .get::<&Name>(node)
        .map(|name| name.0.clone())
        .unwrap_or_default()
}
