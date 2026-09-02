//! Rollback snapshots: capture the world, let it drift, restore it, and the
//! digest must say nothing ever happened. The digest is the judge because it
//! is what a replay or a netcode peer would compare -- a restore that leaves
//! the world "close" is a desync, not a rollback.

use balaur_core::components::StableId;
use balaur_core::snapshot::{self, SnapshotRing};
use balaur_core::{digest, App, AppConfig, Transform};

fn app() -> App {
    App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap()
}

fn spawn(app: &App, id: &str, x: f32) -> balaur_core::hecs::Entity {
    let root = app.engine.root();
    let mut world = app.engine.world_mut();
    let entity = balaur_core::scene::spawn_node(&mut world, id, root);
    world.insert_one(entity, StableId(id.to_string())).unwrap();
    world.get::<&mut Transform>(entity).unwrap().position.x = x;
    entity
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

/// Restore writes over nodes that exist. A node spawned after the snapshot is
/// not freed and one freed after it is not respawned -- documented in the
/// module, and pinned here so a change to that contract is deliberate.
#[test]
fn restore_does_not_respawn_or_free_nodes() {
    let app = app();
    let a = spawn(&app, "n_a", 1.0);
    let taken = snapshot::capture(&app.engine);
    let late = spawn(&app, "n_late", 5.0);
    snapshot::restore(&app.engine, &taken);
    let world = app.engine.world();
    assert!(world.contains(a));
    assert!(world.contains(late), "restore must not free a later spawn");
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
