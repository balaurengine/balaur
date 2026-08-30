//! The scene tree: naming, paths, parenting, and world transforms.

use balaur_core::scene::{
    self, collect_subtree, find_node, free_subtree, node_path, propagate_transforms, Children,
    GlobalTransform, Name, Parent, Transform,
};
use balaur_core::Engine;
use glamx::Vec3;
use hecs::Entity;

fn tree() -> (Engine, Entity, Entity, Entity) {
    let engine = Engine::new();
    let root = engine.root();
    let (a, b, c);
    {
        let mut world = engine.world_mut();
        a = scene::spawn_node(&mut world, "A", root);
        b = scene::spawn_node(&mut world, "B", a);
        c = scene::spawn_node(&mut world, "C", b);
    }
    (engine, a, b, c)
}

#[test]
fn a_spawned_node_is_named_parented_and_has_a_transform() {
    let (engine, a, _, _) = tree();
    let world = engine.world();
    assert_eq!(world.get::<&Name>(a).unwrap().0, "A");
    assert_eq!(world.get::<&Parent>(a).unwrap().0, engine.root());
    assert!(
        world.get::<&Transform>(a).is_ok(),
        "no transform on a new node"
    );
    assert!(world
        .get::<&Children>(engine.root())
        .unwrap()
        .0
        .contains(&a));
}

#[test]
fn a_path_finds_a_descendant_and_missing_ones_are_none() {
    let (engine, _, _, c) = tree();
    let world = engine.world();
    assert_eq!(find_node(&world, engine.root(), "A/B/C"), Some(c));
    assert_eq!(find_node(&world, engine.root(), "A/B/Nope"), None);
    // An empty path is the node you started from, not a miss.
    assert_eq!(find_node(&world, engine.root(), ""), Some(engine.root()));
}

#[test]
fn a_node_path_round_trips_through_find() {
    let (engine, _, _, c) = tree();
    let world = engine.world();
    // node_path is absolute: it includes the root's own name.
    let path = node_path(&world, c);
    assert_eq!(path, "Root/A/B/C");
    let below_root = path.strip_prefix("Root/").expect("paths start at the root");
    assert_eq!(find_node(&world, engine.root(), below_root), Some(c));
}

#[test]
fn world_transforms_compose_down_the_chain() {
    let (engine, a, b, c) = tree();
    {
        let world = engine.world();
        for e in [a, b, c] {
            world.get::<&mut Transform>(e).unwrap().position = Vec3::new(1.0, 0.0, 0.0);
        }
    }
    let root = engine.root();
    propagate_transforms(&mut engine.world_mut(), root);

    let world = engine.world();
    assert!((world.get::<&GlobalTransform>(c).unwrap().position.x - 3.0).abs() < 1e-5);
    assert!((world.get::<&GlobalTransform>(b).unwrap().position.x - 2.0).abs() < 1e-5);
}

#[test]
fn moving_a_parent_moves_the_subtree() {
    let (engine, a, _, c) = tree();
    engine.world().get::<&mut Transform>(a).unwrap().position = Vec3::new(0.0, 5.0, 0.0);
    let root = engine.root();
    propagate_transforms(&mut engine.world_mut(), root);
    assert!(
        (engine
            .world()
            .get::<&GlobalTransform>(c)
            .unwrap()
            .position
            .y
            - 5.0)
            .abs()
            < 1e-5
    );
}

#[test]
fn a_subtree_is_collected_parent_first() {
    let (engine, a, b, c) = tree();
    let collected = collect_subtree(&engine.world(), a);
    assert_eq!(collected.len(), 3, "expected A, B and C: {collected:?}");
    assert_eq!(collected[0], a, "the root of the subtree comes first");
    assert!(collected.contains(&b) && collected.contains(&c));
}

#[test]
fn a_leaf_subtree_is_just_itself() {
    let (engine, _, _, c) = tree();
    assert_eq!(collect_subtree(&engine.world(), c), vec![c]);
}

#[test]
fn freeing_a_subtree_removes_all_of_it_and_unlinks_the_parent() {
    let (engine, a, b, c) = tree();
    let root = engine.root();
    free_subtree(&mut engine.world_mut(), a);

    let world = engine.world();
    for e in [a, b, c] {
        assert!(!world.contains(e), "{e:?} survived");
    }
    assert!(
        !world.get::<&Children>(root).unwrap().0.contains(&a),
        "the parent still lists a freed child"
    );
}

#[test]
fn siblings_keep_their_order() {
    let engine = Engine::new();
    let root = engine.root();
    let made: Vec<Entity> = {
        let mut world = engine.world_mut();
        ["one", "two", "three"]
            .iter()
            .map(|n| scene::spawn_node(&mut world, n, root))
            .collect()
    };
    assert_eq!(engine.world().get::<&Children>(root).unwrap().0, made);
}

#[test]
fn a_duplicate_name_resolves_the_same_way_every_time() {
    let engine = Engine::new();
    let root = engine.root();
    {
        let mut world = engine.world_mut();
        scene::spawn_node(&mut world, "Same", root);
        scene::spawn_node(&mut world, "Same", root);
    }
    let world = engine.world();
    let first = find_node(&world, root, "Same");
    assert!(first.is_some());
    for _ in 0..5 {
        assert_eq!(find_node(&world, root, "Same"), first);
    }
}
