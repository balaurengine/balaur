//! Rollback snapshots: capture the world, let it drift, restore it, and the
//! digest must say nothing ever happened. The digest is the judge because it
//! is what a replay or a netcode peer would compare -- a restore that leaves
//! the world "close" is a desync, not a rollback.

use balaur_core::components::{self, ComponentDef, StableId};
use balaur_core::snapshot::{self, SnapshotRing};
use balaur_core::Engine;
use balaur_core::{digest, App, AppConfig, Transform};

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
}

fn spawn(app: &App, id: &str, x: f32) -> balaur_core::hecs::Entity {
    let root = app.engine.root();
    let mut world = app.engine.world_mut();
    let entity = balaur_core::scene::spawn_node(&mut world, id, root);
    world.insert_one(entity, StableId(id.to_string())).unwrap();
    world.get::<&mut Transform>(entity).unwrap().position.x = x;
    entity
}

/// Spawn the way a script does: no id in hand, one minted by the allocator.
fn spawn_at_run_time(app: &App, name: &str) -> String {
    let root = app.engine.root();
    let mut world = app.engine.world_mut();
    let entity = balaur_core::scene::spawn_node(&mut world, name, root);
    balaur_core::ids::assign(&app.engine, &mut world, entity);
    balaur_core::ids::of(&world, entity).expect("a run-time spawn carries an id")
}

fn find(app: &App, id: &str) -> Option<balaur_core::hecs::Entity> {
    let world = app.engine.world();
    balaur_core::ids::find(&world, app.engine.root(), id)
}

fn move_to(app: &App, entity: balaur_core::hecs::Entity, x: f32) {
    let world = app.engine.world_mut();
    world.get::<&mut Transform>(entity).unwrap().position.x = x;
}

/// The load-bearing one: a perturbed world restored from a snapshot digests
/// exactly as it did when the snapshot was taken.
#[test]
fn a_restored_world_digests_as_it_did_when_captured() {
    let app = app();
    let a = spawn(&app, "n_a", 1.0);
    let b = spawn(&app, "n_b", 2.0);
    // Advance the RNG too, so its state is part of what has to come back.
    balaur_core::rng::with_rng(&app.engine, |rng| {
        let _ = rng.next_u32();
    });
    let before = digest::digest(&app.engine);
    let taken = snapshot::capture(&app.engine);

    move_to(&app, a, 10.0);
    move_to(&app, b, -3.0);
    balaur_core::rng::with_rng(&app.engine, |rng| {
        for _ in 0..7 {
            let _ = rng.next_u32();
        }
    });
    assert_ne!(
        digest::digest(&app.engine),
        before,
        "the perturbation has to be visible, or the test proves nothing"
    );

    snapshot::restore(&app.engine, &taken);
    assert_eq!(digest::digest(&app.engine), before);
}

/// A snapshot survives the trip through bytes, so what a ring or a network
/// carries restores the same world the in-memory one does.
#[test]
fn a_snapshot_round_trips_through_bytes() {
    let app = app();
    let a = spawn(&app, "n_a", 1.5);
    let before = digest::digest(&app.engine);
    let bytes = snapshot::capture(&app.engine).encode().unwrap();

    move_to(&app, a, 99.0);
    let decoded = snapshot::Snapshot::decode(&bytes).unwrap();
    snapshot::restore(&app.engine, &decoded);
    assert_eq!(digest::digest(&app.engine), before);
}

/// Restore puts the node set back, not just the state of nodes that survived:
/// a node the snapshot never saw goes away again.
#[test]
fn restore_frees_a_node_spawned_since_the_snapshot() {
    let app = app();
    let a = spawn(&app, "n_a", 1.0);
    let taken = snapshot::capture(&app.engine);
    let late = spawn(&app, "n_late", 5.0);
    snapshot::restore(&app.engine, &taken);
    let world = app.engine.world();
    assert!(world.contains(a));
    assert!(
        !world.contains(late),
        "n_late did not exist when the snapshot was taken"
    );
}

#[test]
fn restore_respawns_a_node_freed_since_the_snapshot() {
    let app = app();
    spawn(&app, "n_a", 1.0);
    let doomed = spawn(&app, "n_doomed", 4.0);
    let taken = snapshot::capture(&app.engine);

    balaur_core::scene::free_subtree(&mut app.engine.world_mut(), doomed);
    assert!(find(&app, "n_doomed").is_none());

    snapshot::restore(&app.engine, &taken);
    let back = find(&app, "n_doomed").expect("the node came back");
    // Bit-for-bit: a position that came back "close" is a desync.
    let x = app
        .engine
        .world()
        .get::<&Transform>(back)
        .unwrap()
        .position
        .x;
    assert_eq!(x.to_bits(), 4.0f32.to_bits(), "and came back where it was");
}

/// The one that matters for netcode: a tick that spawns a bullet and frees
/// another node has to roll back to a bit-identical digest, not a close one.
#[test]
fn a_spawn_and_a_free_roll_back_to_the_same_digest() {
    let app = app();
    spawn(&app, "n_a", 1.0);
    let doomed = spawn(&app, "n_doomed", 4.0);
    let before = digest::digest(&app.engine);
    let taken = snapshot::capture(&app.engine);

    let bullet = spawn(&app, "n_bullet", 9.0);
    balaur_core::scene::free_subtree(&mut app.engine.world_mut(), doomed);
    assert_ne!(
        digest::digest(&app.engine),
        before,
        "the spawn and the free have to be visible, or the test proves nothing"
    );

    snapshot::restore(&app.engine, &taken);
    assert_eq!(digest::digest(&app.engine), before);
    assert!(!app.engine.world().contains(bullet));
}

/// A run-time spawn mints `<authority>:<counter>`, and a rollback rewinds the
/// counter so the re-simulated spawn mints the same id rather than the next.
#[test]
fn a_rolled_back_spawn_mints_the_id_it_minted_before() {
    let app = app();
    let taken = snapshot::capture(&app.engine);
    let first = spawn_at_run_time(&app, "Bullet");
    snapshot::restore(&app.engine, &taken);
    let again = spawn_at_run_time(&app, "Bullet");
    assert_eq!(first, again);
    assert_eq!(first, "local:0");
}

/// A re-simulated tick overwrites its slot. Without this a rollback-heavy
/// session fills the ring with duplicates and forgets ticks it still needs.
#[test]
fn pushing_a_tick_twice_replaces_it_rather_than_filling_the_ring() {
    let app = app();
    let mut ring = SnapshotRing::new(4);
    for tick in 1..=3 {
        ring.push(tick, snapshot::capture(&app.engine));
    }
    for _ in 0..10 {
        ring.push(2, snapshot::capture(&app.engine));
    }
    assert_eq!(ring.len(), 3, "still three distinct ticks");
    assert_eq!(ring.earliest(), Some(1), "and tick 1 was never crowded out");
}

#[test]
fn the_ring_keeps_the_newest_and_forgets_the_oldest() {
    let app = app();
    let mut ring = SnapshotRing::new(2);
    assert!(ring.is_empty());
    for tick in 1..=3 {
        ring.push(tick, snapshot::capture(&app.engine));
    }
    assert_eq!(ring.len(), 2);
    assert_eq!(ring.earliest(), Some(2));
    assert!(ring.get(1).is_none(), "tick 1 fell out of a ring of two");
    assert!(ring.get(3).is_some());
}

struct Marker(toml::Value);

/// An app with one component, so a respawn has something to put back beyond
/// the node itself.
fn app_with_marker() -> App {
    let mut app = app();
    app.register_component(
        "marker",
        ComponentDef {
            doc: "",
            schema: ComponentDef::parse_schema(
                "marker",
                r#"size = { type = "float", default = 1.0 }"#,
            ),
            tags: &[],
            expects: &[],
            apply: Box::new(|eng: &Engine, entity, params| {
                eng.world_mut()
                    .insert_one(entity, Marker(params.clone()))
                    .map_err(|_| anyhow::anyhow!("dead node"))?;
                Ok(())
            }),
            remove: Box::new(|eng: &Engine, entity| {
                let _ = eng.world_mut().remove_one::<Marker>(entity);
                Ok(())
            }),
            get: Box::new(|eng: &Engine, entity| {
                eng.world().get::<&Marker>(entity).ok().map(|m| m.0.clone())
            }),
        },
    );
    app
}

/// A respawned node is not a bare node: its components come back through the
/// same `apply` a scene file goes through, so the digest matches.
#[test]
fn a_respawned_node_brings_its_components_back() {
    let app = app_with_marker();
    let doomed = spawn(&app, "n_doomed", 4.0);
    components::add(
        &app.engine,
        doomed,
        "marker",
        Some(&toml::Value::Table(
            [(String::from("size"), toml::Value::Float(7.5))]
                .into_iter()
                .collect(),
        )),
    )
    .unwrap();
    let before = digest::digest(&app.engine);
    let taken = snapshot::capture(&app.engine);

    balaur_core::scene::free_subtree(&mut app.engine.world_mut(), doomed);
    snapshot::restore(&app.engine, &taken);

    let back = find(&app, "n_doomed").expect("the node came back");
    let marker = components::get(&app.engine, back, "marker").expect("with its component");
    assert_eq!(marker.get("size"), Some(&toml::Value::Float(7.5)));
    assert_eq!(digest::digest(&app.engine), before);
}

/// `node:set_parent` is simulation state like any other, and the digest walks
/// the tree in order: a node left under the parent a rolled-back tick gave it
/// is a desync, not a cosmetic difference.
#[test]
fn restoring_puts_a_reparented_node_back_under_its_old_parent() {
    let app = app();
    let a = spawn(&app, "n_a", 1.0);
    let b = spawn(&app, "n_b", 2.0);
    let before = digest::digest(&app.engine);
    let taken = snapshot::capture(&app.engine);

    balaur_core::scene::reparent(&mut app.engine.world_mut(), b, a).unwrap();
    assert_ne!(
        digest::digest(&app.engine),
        before,
        "the move has to be visible, or the test proves nothing"
    );

    snapshot::restore(&app.engine, &taken);
    let root = app.engine.root();
    assert_eq!(
        app.engine.world().get::<&balaur_core::Parent>(b).unwrap().0,
        root,
        "the node came back under the parent it was moved to"
    );
    assert_eq!(digest::digest(&app.engine), before);
}

/// A rename is invisible to the digest, which labels by stable id, so only a
/// path lookup catches it -- and `find_node` is how a script reaches a node.
#[test]
fn restoring_puts_a_renamed_node_back_under_its_old_name() {
    let app = app();
    let a = spawn(&app, "n_a", 1.0);
    let taken = snapshot::capture(&app.engine);

    app.engine
        .world_mut()
        .get::<&mut balaur_core::Name>(a)
        .unwrap()
        .0 = String::from("Renamed");
    snapshot::restore(&app.engine, &taken);

    let world = app.engine.world();
    assert_eq!(world.get::<&balaur_core::Name>(a).unwrap().0, "n_a");
    assert_eq!(
        balaur_core::scene::find_node(&world, app.engine.root(), "n_a"),
        Some(a)
    );
}

/// Two siblings freed in the same tick, which is the netcode case: the frames
/// come back last-first, so each respawn has to wait for the siblings that sit
/// ahead of it or the row is rebuilt inside out.
#[test]
fn restoring_two_freed_siblings_puts_both_back_in_order() {
    let app = app();
    spawn(&app, "n_a", 1.0);
    spawn(&app, "n_b", 2.0);
    spawn(&app, "n_c", 3.0);
    let before = digest::digest(&app.engine);
    let taken = snapshot::capture(&app.engine);

    for id in ["n_a", "n_b"] {
        let doomed = find(&app, id).expect("a sibling to free");
        balaur_core::scene::free_subtree(&mut app.engine.world_mut(), doomed);
    }
    assert_ne!(
        digest::digest(&app.engine),
        before,
        "the frees have to be visible, or the test proves nothing"
    );

    snapshot::restore(&app.engine, &taken);
    let root = app.engine.root();
    let world = app.engine.world();
    let names: Vec<String> = world
        .get::<&balaur_core::scene::Children>(root)
        .unwrap()
        .0
        .iter()
        .filter_map(|&child| world.get::<&balaur_core::scene::Name>(child).ok())
        .map(|name| name.0.clone())
        .collect();
    assert_eq!(
        names,
        ["n_a", "n_b", "n_c"],
        "and in the order they were in"
    );
    drop(world);
    assert_eq!(digest::digest(&app.engine), before);
}

/// The ordering one: `collect_subtree` visits the last child first, so a
/// respawn that appended put a freed middle sibling at the end and `fold`
/// reported a divergence made of nothing.
#[test]
fn restoring_the_first_of_three_siblings_puts_it_back_where_it_was() {
    let app = app();
    spawn(&app, "n_a", 1.0);
    spawn(&app, "n_b", 2.0);
    spawn(&app, "n_c", 3.0);
    let before = digest::digest(&app.engine);
    let taken = snapshot::capture(&app.engine);

    let first = find(&app, "n_a").expect("the first sibling");
    balaur_core::scene::free_subtree(&mut app.engine.world_mut(), first);
    assert_ne!(
        digest::digest(&app.engine),
        before,
        "the free has to be visible, or the test proves nothing"
    );

    snapshot::restore(&app.engine, &taken);
    assert_eq!(
        digest::digest(&app.engine),
        before,
        "a node that came back last is a desync report about sibling order"
    );
}
