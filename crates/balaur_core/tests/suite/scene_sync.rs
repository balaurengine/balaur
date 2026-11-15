//! The frame's scene sync places only what changed, and what it places is
//! what a walk of the whole tree would have: every world pose and appearance,
//! bit for bit, through a move, a restyle, a reparent, a free and a restore.

use balaur_core::scene::{
    self, Appearance, GlobalAppearance, GlobalTransform, Transform, propagate_transforms,
};
use balaur_core::{App, AppConfig, components, digest, interpolate, snapshot, transform};
use glamx::{Quat, Vec3};
use hecs::Entity;

const DT: f32 = 1.0 / 60.0;
const DEPTH: usize = 12;
/// Far from anything a pose composes to, so a node the sync never touched
/// still holds it.
const UNTOUCHED: f32 = 9999.0;

/// A chain `DEPTH` deep under one root, each link with two leaves, every
/// local transform turned, scaled and some of them sheared, so composing
/// one in the wrong order shows.
fn deep_tree(app: &App) -> Vec<Entity> {
    let mut chain = Vec::new();
    let mut parent = app.engine.root();
    for depth in 0..DEPTH {
        let link = scene::spawn_node(&mut app.engine.world_mut(), &format!("L{depth}"), parent);
        let turn = 0.1 * (depth as f32 + 1.0);
        let at = toml::Value::Array(vec![
            toml::Value::Float(1.0),
            toml::Value::Float(0.5 * depth as f64),
            toml::Value::Float(0.0),
        ]);
        components::set_property(&app.engine, link, transform::COMPONENT, "position", &at).unwrap();
        {
            let world = app.engine.world();
            let mut local = world.get::<&mut Transform>(link).unwrap();
            local.rotation = Quat::from_rotation_z(turn);
            local.scale = Vec3::new(1.1, 0.9, 1.0);
            local.skew = if depth % 3 == 0 { 0.2 } else { 0.0 };
        }
        transform::written(&app.engine, link);
        for side in 0..2 {
            let leaf = scene::spawn_node(
                &mut app.engine.world_mut(),
                &format!("L{depth}S{side}"),
                link,
            );
            app.engine
                .world()
                .get::<&mut Transform>(leaf)
                .unwrap()
                .position = Vec3::new(side as f32, -1.0, 0.25);
            transform::written(&app.engine, leaf);
        }
        chain.push(link);
        parent = link;
    }
    chain
}

type Pose = ([u32; 11], (bool, [u32; 4], i32, String));

/// Every node's world pose and appearance under the root, as bits.
fn globals(app: &App) -> Vec<(String, Pose)> {
    let world = app.engine.world();
    scene::collect_subtree(&world, app.engine.root())
        .into_iter()
        .map(|node| {
            let g = *world.get::<&GlobalTransform>(node).unwrap();
            let a = *world.get::<&GlobalAppearance>(node).unwrap();
            let pose = [
                g.position.x,
                g.position.y,
                g.position.z,
                g.rotation.x,
                g.rotation.y,
                g.rotation.z,
                g.rotation.w,
                g.scale.x,
                g.scale.y,
                g.scale.z,
                g.skew,
            ]
            .map(f32::to_bits);
            let look = (
                a.visible,
                a.tint.to_array().map(f32::to_bits),
                a.z_index,
                a.material.reference().to_string(),
            );
            (scene::node_path(&world, node), (pose, look))
        })
        .collect()
}

/// What the frame's sync left is what walking the whole tree leaves, and so
/// is the digest.
#[track_caller]
fn assert_as_the_full_walk(app: &App, step: &str) {
    let synced = globals(app);
    let synced_digest = digest::digest(&app.engine);
    let root = app.engine.root();
    propagate_transforms(&mut app.engine.world_mut(), root);
    let walked = globals(app);
    for ((path, got), (_, wanted)) in synced.iter().zip(&walked) {
        assert_eq!(got, wanted, "{step}: {path} differs from the full walk");
    }
    assert_eq!(synced.len(), walked.len(), "{step}: a node came or went");
    assert_eq!(
        synced_digest,
        digest::digest(&app.engine),
        "{step}: the digest differs from the full walk's"
    );
}

fn poison(app: &App, node: Entity) {
    app.engine
        .world()
        .get::<&mut GlobalTransform>(node)
        .unwrap()
        .position
        .x = UNTOUCHED;
}

fn placed_x(app: &App, node: Entity) -> f32 {
    app.engine
        .world()
        .get::<&GlobalTransform>(node)
        .unwrap()
        .position
        .x
}

fn move_to(app: &App, node: Entity, x: f64) {
    let at = toml::Value::Array(vec![
        toml::Value::Float(x),
        toml::Value::Float(1.0),
        toml::Value::Float(0.0),
    ]);
    components::set_property(&app.engine, node, transform::COMPONENT, "position", &at).unwrap();
}

#[test]
fn every_step_leaves_the_tree_as_the_full_walk_would() {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    let chain = deep_tree(&app);
    app.tick(DT);
    assert_as_the_full_walk(&app, "the first frame");
    let taken = snapshot::capture(&app.engine);

    move_to(&app, chain[3], 4.0);
    app.tick(DT);
    assert_as_the_full_walk(&app, "a parent moved through the registry");

    app.engine
        .world()
        .get::<&mut Transform>(chain[5])
        .unwrap()
        .rotation = Quat::from_rotation_z(1.3);
    transform::written(&app.engine, chain[5]);
    app.tick(DT);
    assert_as_the_full_walk(&app, "a parent turned by a direct write");

    scene::set_visible(&app.engine, chain[2], false);
    app.engine
        .world()
        .get::<&mut Appearance>(chain[6])
        .unwrap()
        .tint = glamx::Vec4::new(0.5, 0.25, 1.0, 0.75);
    scene::appearance_written(&app.engine, chain[6]);
    app.tick(DT);
    assert_as_the_full_walk(&app, "a parent hidden and another tinted");

    app.engine.reparent(chain[8], chain[1]).unwrap();
    app.tick(DT);
    assert_as_the_full_walk(&app, "a subtree reparented");

    let born = scene::spawn_node(&mut app.engine.world_mut(), "Born", chain[9]);
    move_to(&app, chain[9], -2.0);
    app.tick(DT);
    assert!(app.engine.world().contains(born));
    assert_as_the_full_walk(&app, "a node spawned under a parent that moved");

    components::remove(&app.engine, chain[10], transform::COMPONENT).unwrap();
    app.tick(DT);
    assert_as_the_full_walk(&app, "a transform taken off");

    app.engine.defer_free(chain[4]);
    app.tick(DT);
    assert!(!app.engine.world().contains(chain[7]));
    assert_as_the_full_walk(&app, "a parent freed");

    // A snapshot leaves a `local` subtree out: it keeps where it moved to.
    let hud = scene::spawn_node(&mut app.engine.world_mut(), "Hud", app.engine.root());
    app.engine
        .world_mut()
        .insert_one(hud, scene::Tags(vec![digest::TAG_LOCAL.to_string()]))
        .unwrap();
    let gauge = scene::spawn_node(&mut app.engine.world_mut(), "Gauge", hud);
    app.tick(DT);
    move_to(&app, hud, 7.0);
    snapshot::restore(&app.engine, &taken);
    app.tick(DT);
    assert_as_the_full_walk(&app, "a snapshot restored");
    assert!(
        scene::find_node(&app.engine.world(), app.engine.root(), "L0/L1/L2/L3/L4/L5").is_some(),
        "the restore did not bring the freed branch back"
    );
    assert!(
        (placed_x(&app, gauge) - 7.0).abs() < 1e-6,
        "the local subtree lost its move"
    );
}

#[test]
fn a_still_frame_places_nothing() {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    let chain = deep_tree(&app);
    app.tick(DT);
    let leaf = scene::find_node(&app.engine.world(), chain[DEPTH - 1], "L11S0").unwrap();
    poison(&app, leaf);
    app.tick(DT);
    assert_eq!(
        placed_x(&app, leaf).to_bits(),
        UNTOUCHED.to_bits(),
        "nothing moved, and the sync placed the leaf again"
    );
}

#[test]
fn a_moved_parent_places_its_subtree_and_nothing_beside_it() {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    let chain = deep_tree(&app);
    app.tick(DT);
    let beside = scene::find_node(&app.engine.world(), chain[2], "L2S1").unwrap();
    let below = scene::find_node(&app.engine.world(), chain[DEPTH - 1], "L11S0").unwrap();
    poison(&app, beside);
    poison(&app, below);
    move_to(&app, chain[3], 2.0);
    app.tick(DT);
    assert_ne!(
        placed_x(&app, below).to_bits(),
        UNTOUCHED.to_bits(),
        "the subtree was left behind"
    );
    assert_eq!(
        placed_x(&app, beside).to_bits(),
        UNTOUCHED.to_bits(),
        "a sibling of the moved branch was placed again"
    );
}

#[test]
fn a_blend_is_drawn_every_frame_and_taken_back_once_it_stops() {
    let app = App::new(AppConfig::bare(".")).unwrap();
    let chain = deep_tree(&app);
    interpolate::enable(&app.engine, chain[1]);
    scene::sync_globals(&app.engine, 1.0);
    app.engine
        .world()
        .get::<&mut Transform>(chain[1])
        .unwrap()
        .position
        .x = 6.0;
    transform::written(&app.engine, chain[1]);
    interpolate::capture(&app.engine);
    for alpha in [0.25, 0.5, 0.75, 1.0] {
        scene::sync_globals(&app.engine, alpha);
        let synced = globals(&app);
        let root = app.engine.root();
        scene::propagate_transforms_at(&mut app.engine.world_mut(), root, alpha);
        assert_eq!(synced, globals(&app), "at alpha {alpha}");
    }
}
