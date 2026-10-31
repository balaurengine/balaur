//! Relationships (PLAN-ecs §2): a source's target and the target's sources
//! kept in step, the scene tree first.

use balaur_core::relationships::{self, Relationship, Sources};
use balaur_core::{App, AppConfig, Children, Parent, digest, scene};
use hecs::Entity;

/// A rope tied to a post: a relationship of a test's own.
struct TiedTo(Entity);
#[derive(Default)]
struct Ropes(Vec<Entity>);

impl Relationship for TiedTo {
    type Sources = Ropes;
    fn new(target: Entity) -> Self {
        Self(target)
    }
    fn target(&self) -> Entity {
        self.0
    }
}

impl Sources for Ropes {
    fn list(&self) -> &[Entity] {
        &self.0
    }
    fn list_mut(&mut self) -> &mut Vec<Entity> {
        &mut self.0
    }
}

#[test]
fn relating_moves_a_source_between_targets_in_order() {
    let mut world = hecs::World::new();
    let left = world.spawn((Ropes::default(),));
    let right = world.spawn((Ropes::default(),));
    let a = world.spawn(());
    let b = world.spawn(());
    let c = world.spawn(());
    relationships::relate::<TiedTo>(&mut world, a, left, None).unwrap();
    relationships::relate::<TiedTo>(&mut world, b, left, None).unwrap();
    relationships::relate::<TiedTo>(&mut world, c, left, Some(0)).unwrap();
    assert_eq!(
        relationships::sources_of::<TiedTo>(&world, left),
        vec![c, a, b]
    );
    relationships::relate::<TiedTo>(&mut world, a, right, None).unwrap();
    assert_eq!(
        relationships::sources_of::<TiedTo>(&world, left),
        vec![c, b]
    );
    assert_eq!(relationships::sources_of::<TiedTo>(&world, right), vec![a]);
    assert_eq!(relationships::target_of::<TiedTo>(&world, a), Some(right));
    assert!(relationships::place::<TiedTo>(&world, c, 9));
    assert_eq!(
        relationships::sources_of::<TiedTo>(&world, left),
        vec![b, c]
    );
    assert_eq!(relationships::unlink::<TiedTo>(&world, b), Some(left));
    assert_eq!(relationships::sources_of::<TiedTo>(&world, left), vec![c]);
}

#[test]
fn the_scene_tree_is_a_relationship() {
    let app = App::new(AppConfig::bare(".")).unwrap();
    let root = app.engine.root();
    let ship = app.engine.spawn_node("Ship", root);
    let mast = app.engine.spawn_node("Mast", ship);
    let sail = app.engine.spawn_node("Sail", ship);
    {
        let world = app.engine.world();
        assert_eq!(relationships::target_of::<Parent>(&world, mast), Some(ship));
        assert_eq!(
            relationships::sources_of::<Parent>(&world, ship),
            world.get::<&Children>(ship).unwrap().0
        );
        assert_eq!(
            relationships::sources_of::<Parent>(&world, ship),
            vec![mast, sail]
        );
    }
    app.engine.reparent(sail, root).unwrap();
    let world = app.engine.world();
    assert_eq!(
        relationships::sources_of::<Parent>(&world, ship),
        vec![mast]
    );
    assert_eq!(world.get::<&Parent>(sail).unwrap().0, root);
    assert_eq!(
        *relationships::sources_of::<Parent>(&world, root)
            .last()
            .unwrap(),
        sail
    );
}

#[test]
fn a_subtree_is_what_points_at_it_last_first() {
    let app = App::new(AppConfig::bare(".")).unwrap();
    let root = app.engine.root();
    let ship = app.engine.spawn_node("Ship", root);
    let mast = app.engine.spawn_node("Mast", ship);
    let flag = app.engine.spawn_node("Flag", mast);
    let sail = app.engine.spawn_node("Sail", ship);
    let world = app.engine.world();
    assert_eq!(
        scene::collect_subtree(&world, ship),
        vec![ship, sail, mast, flag]
    );
}

#[test]
fn freeing_through_the_relationship_keeps_the_digest() {
    // The same tree built and trimmed twice digests the same: the children
    // stay in the order the digest walks.
    let build = || {
        let app = App::new(AppConfig::bare(".")).unwrap();
        let root = app.engine.root();
        let ship = app.engine.spawn_node("Ship", root);
        let nodes: Vec<Entity> = (0..6)
            .map(|i| app.engine.spawn_node(&format!("N{i}"), ship))
            .collect();
        scene::free_nodes(&app.engine, &[nodes[1], nodes[4]]);
        scene::move_child_to(&app.engine.world(), ship, nodes[5], 0);
        let order: Vec<String> = app
            .engine
            .world()
            .get::<&Children>(ship)
            .unwrap()
            .0
            .iter()
            .map(|&e| {
                app.engine
                    .world()
                    .get::<&balaur_core::Name>(e)
                    .unwrap()
                    .0
                    .clone()
            })
            .collect();
        (digest::digest(&app.engine), order)
    };
    let (first, order) = build();
    assert_eq!(order, vec!["N5", "N0", "N2", "N3"]);
    assert_eq!(build().0, first);
}
