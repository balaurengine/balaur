//! The simulation digest: what makes a determinism claim checkable.

use balaur_core::components::StableId;
use balaur_core::digest::{self, Entry};
use balaur_core::{App, AppConfig, Name, Transform};

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

fn spawn(app: &App, id: &str, name: &str, x: f32) -> balaur_core::hecs::Entity {
    let root = app.engine.root();
    let mut world = app.engine.world_mut();
    let entity = balaur_core::scene::spawn_node(&mut world, name, root);
    world.insert_one(entity, StableId(id.to_string())).unwrap();
    world.get::<&mut Transform>(entity).unwrap().position.x = x;
    entity
}

#[test]
fn an_untouched_world_hashes_the_same_twice() {
    let app = app();
    spawn(&app, "n_ball", "Ball", 1.0);
    assert_eq!(digest::digest(&app.engine), digest::digest(&app.engine));
}

#[test]
fn moving_a_node_by_one_ulp_changes_the_digest() {
    let app = app();
    let entity = spawn(&app, "n_ball", "Ball", 1.0);
    let before = digest::digest(&app.engine);
    {
        let mut world = app.engine.world_mut();
        let mut t = world.get::<&mut Transform>(entity).unwrap();
        t.position.x = f32::from_bits(1.0f32.to_bits() + 1);
    }
    assert_ne!(
        before,
        digest::digest(&app.engine),
        "a digest that misses one ulp misses the divergence that matters"
    );
}

#[test]
fn first_divergence_names_the_node_that_moved() {
    let app = app();
    spawn(&app, "n_ground", "Ground", 0.0);
    let ball = spawn(&app, "n_ball", "Ball", 1.0);
    let before = digest::entries(&app.engine);
    {
        let mut world = app.engine.world_mut();
        world.get::<&mut Transform>(ball).unwrap().position.x = 2.0;
    }
    let after = digest::entries(&app.engine);
    let report = digest::first_divergence(&before, &after).expect("the ball moved");
    assert!(
        report.starts_with("n_ball/transform:"),
        "a divergence report has to name the node and the slice, got {report}"
    );
}

/// The property Godot's NodePath-keyed replication cannot offer: the label
/// two peers compare on survives a rename.
#[test]
fn renaming_a_node_does_not_change_its_digest_label() {
    let app = app();
    let entity = spawn(&app, "n_ball", "Ball", 1.0);
    let before = digest::entries(&app.engine);
    {
        let world = app.engine.world_mut();
        world.get::<&mut Name>(entity).unwrap().0 = String::from("RenamedEntirely");
    }
    let after = digest::entries(&app.engine);
    assert_eq!(digest::first_divergence(&before, &after), None);
}

#[test]
fn a_node_present_on_one_side_only_is_reported_as_missing() {
    let short = vec![Entry {
        label: String::from("n_a/transform"),
        digest: digest::Digest(1),
    }];
    let long = vec![
        Entry {
            label: String::from("n_a/transform"),
            digest: digest::Digest(1),
        },
        Entry {
            label: String::from("n_b/transform"),
            digest: digest::Digest(2),
        },
    ];
    assert_eq!(
        digest::first_divergence(&short, &long).as_deref(),
        Some("n_b/transform: missing on the left")
    );
}

#[test]
fn the_rng_stream_position_is_part_of_the_digest() {
    let app = app();
    let before = digest::digest(&app.engine);
    balaur_core::rng::with_rng(&app.engine, |rng| rng.next_u32());
    assert_ne!(
        before,
        digest::digest(&app.engine),
        "two peers that drew a different number of randoms have diverged"
    );
}
