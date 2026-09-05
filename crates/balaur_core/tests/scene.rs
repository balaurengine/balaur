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

/// Transform propagation recurses once per level of nesting, so scene depth is
/// bounded by the stack rather than by anything the engine checks. Measured on
/// a 2 MiB test thread it gives out between 4000 and 4500 levels in a debug
/// build; this pins a depth well inside that, so making a frame heavier shows
/// up here rather than as an abort in someone's game.
#[test]
fn deep_nesting_propagates_without_overflowing() {
    let engine = Engine::new();
    let root = engine.root();
    {
        let mut world = engine.world_mut();
        let mut parent = root;
        for i in 0..1000 {
            parent = scene::spawn_node(&mut world, &format!("n{i}"), parent);
        }
    }
    propagate_transforms(&mut engine.world_mut(), root);
}

#[test]
fn a_dot_dot_segment_climbs_to_the_parent() {
    let (engine, a, b, c) = tree();
    let world = engine.world();
    assert_eq!(find_node(&world, c, ".."), Some(b));
    assert_eq!(find_node(&world, c, "../.."), Some(a));
    assert_eq!(find_node(&world, b, "../B/C"), Some(c));
    assert_eq!(find_node(&world, engine.root(), ".."), None);
}

#[test]
fn reparenting_keeps_the_world_pose_and_moves_the_child() {
    let (engine, a, b, c) = tree();
    {
        let world = engine.world_mut();
        world.get::<&mut Transform>(a).unwrap().position = Vec3::new(10.0, 0.0, 0.0);
        world.get::<&mut Transform>(b).unwrap().position = Vec3::new(0.0, 5.0, 0.0);
        world.get::<&mut Transform>(c).unwrap().position = Vec3::new(1.0, 1.0, 0.0);
    }
    let before = scene::composed_global(&engine.world(), c).position;
    scene::reparent(&mut engine.world_mut(), c, a).unwrap();
    let world = engine.world();
    assert_eq!(world.get::<&Parent>(c).unwrap().0, a);
    assert!(world.get::<&Children>(a).unwrap().0.contains(&c));
    assert!(!world.get::<&Children>(b).unwrap().0.contains(&c));
    let after = scene::composed_global(&world, c).position;
    assert!(
        (after - before).length() < 1e-5,
        "{before:?} became {after:?}"
    );
    // Its local transform absorbed the move: it is now B's old offset plus
    // its own, relative to A.
    let local = world.get::<&Transform>(c).unwrap().position;
    assert!(
        (local - Vec3::new(1.0, 6.0, 0.0)).length() < 1e-5,
        "{local:?}"
    );
}

#[test]
fn a_node_cannot_be_moved_under_itself_or_its_descendants() {
    let (engine, a, _, c) = tree();
    assert!(scene::reparent(&mut engine.world_mut(), a, a).is_err());
    assert!(scene::reparent(&mut engine.world_mut(), a, c).is_err());
    let world = engine.world();
    assert_eq!(world.get::<&Parent>(a).unwrap().0, engine.root());
}

#[test]
fn hiding_a_node_hides_everything_under_it() {
    let (engine, a, b, c) = tree();
    {
        let world = engine.world();
        world.get::<&mut scene::Appearance>(a).unwrap().visible = false;
    }
    propagate_transforms(&mut engine.world_mut(), engine.root());
    let world = engine.world();
    for entity in [a, b, c] {
        assert!(
            !world
                .get::<&scene::GlobalAppearance>(entity)
                .unwrap()
                .visible,
            "a hidden ancestor should hide the whole subtree"
        );
    }
}

#[test]
fn a_relative_z_index_adds_to_its_parents_and_an_absolute_one_does_not() {
    let (engine, a, b, c) = tree();
    {
        let world = engine.world();
        world.get::<&mut scene::Appearance>(a).unwrap().z_index = 10;
        world.get::<&mut scene::Appearance>(b).unwrap().z_index = 5;
        let mut leaf = world.get::<&mut scene::Appearance>(c).unwrap();
        leaf.z_index = 2;
        leaf.z_relative = false;
    }
    propagate_transforms(&mut engine.world_mut(), engine.root());
    let world = engine.world();
    assert_eq!(
        world.get::<&scene::GlobalAppearance>(b).unwrap().z_index,
        15
    );
    assert_eq!(world.get::<&scene::GlobalAppearance>(c).unwrap().z_index, 2);
}

#[test]
fn composed_appearance_matches_what_propagation_wrote() {
    let (engine, a, b, c) = tree();
    {
        let world = engine.world();
        world.get::<&mut scene::Appearance>(b).unwrap().visible = false;
        world.get::<&mut scene::Appearance>(a).unwrap().z_index = 3;
    }
    propagate_transforms(&mut engine.world_mut(), engine.root());
    let world = engine.world();
    for entity in [a, b, c] {
        let propagated = *world.get::<&scene::GlobalAppearance>(entity).unwrap();
        let composed = scene::composed_appearance(&world, entity);
        assert_eq!(propagated.visible, composed.visible);
        assert_eq!(propagated.z_index, composed.z_index);
    }
}

#[test]
fn a_scene_files_a_node_under_its_tags() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nname = \"Gate\"\ntags = [\"door\", \"exit\"]\n[[nodes]]\nname = \"Rock\"\n",
    )
    .unwrap();
    let mut app = balaur_core::App::new(balaur_core::AppConfig {
        project_root: dir.path().to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    app.load_project().unwrap();
    let world = app.engine.world();
    let found = scene::tagged(&world, app.engine.root(), "door");
    assert_eq!(found.len(), 1);
    assert_eq!(world.get::<&Name>(found[0]).unwrap().0, "Gate");
    assert!(scene::tagged(&world, app.engine.root(), "lava").is_empty());
}
