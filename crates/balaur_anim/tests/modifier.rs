//! Rig modifiers: a bone that looks at a node, and a two-bone chain whose
//! tip reaches one.

use balaur_anim::AnimationPlugin;
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, GlobalTransform, Transform};
use balaur_core::skeleton::angle_about_z;
use balaur_core::{App, AppConfig, components};
use glamx::{Vec2, Vec3};

fn app() -> App {
    let mut app = App::new(AppConfig::bare(std::path::PathBuf::from("tests/fixtures"))).unwrap();
    balaur_plugin::load(&mut app, &mut AnimationPlugin::default()).unwrap();
    app
}

fn bone(app: &App, name: &str, parent: Entity, rest: [f64; 2]) -> Entity {
    let entity = scene::spawn_node(&mut app.engine.world_mut(), name, parent);
    let params: toml::Value =
        toml::from_str(&format!("rest_position = [{}, {}]", rest[0], rest[1])).unwrap();
    components::add(&app.engine, entity, "bone2d", Some(&params)).unwrap();
    entity
}

fn node_at(app: &App, name: &str, parent: Entity, x: f32, y: f32) -> Entity {
    let entity = scene::spawn_node(&mut app.engine.world_mut(), name, parent);
    app.engine
        .world_mut()
        .get::<&mut Transform>(entity)
        .unwrap()
        .position = Vec3::new(x, y, 0.0);
    entity
}

fn global_xy(app: &App, entity: Entity) -> Vec2 {
    let p = app
        .engine
        .world()
        .get::<&GlobalTransform>(entity)
        .unwrap()
        .position;
    Vec2::new(p.x, p.y)
}

/// A rig with a root, middle and tip bone lying along +x, one unit apart,
/// and a target node beside it. The modifier goes on the rig root.
fn chain(app: &App, kind: &str, target: (f32, f32), flip: bool) -> (Entity, Entity, Entity) {
    let root = app.engine.root();
    let rig = scene::spawn_node(&mut app.engine.world_mut(), "Rig", root);
    let shoulder = bone(app, "Shoulder", rig, [0.0, 0.0]);
    let elbow = bone(app, "Elbow", shoulder, [1.0, 0.0]);
    let hand = bone(app, "Hand", elbow, [1.0, 0.0]);
    balaur_core::skeleton::apply_rest(&mut app.engine.world_mut(), rig);
    node_at(app, "Target", rig, target.0, target.1);
    let params: toml::Value = toml::from_str(&format!(
        "kind = \"{kind}\"\nbone = \"Shoulder\"\ntarget = \"Target\"\nflip = {flip}"
    ))
    .unwrap();
    components::add(&app.engine, rig, "modifier2d", Some(&params)).unwrap();
    (shoulder, elbow, hand)
}

#[test]
fn two_bone_ik_puts_the_tip_on_a_reachable_target() {
    let mut app = app();
    let (_, elbow, hand) = chain(&app, "two_bone_ik", (1.2, 0.8), false);
    app.tick(1.0 / 60.0);
    let tip = global_xy(&app, hand);
    assert!(
        (tip - Vec2::new(1.2, 0.8)).length() < 1e-3,
        "the hand landed at {tip:?}"
    );
    // The elbow bent upward, the default side.
    assert!(global_xy(&app, elbow).y > 0.0);
}

#[test]
fn flip_bends_the_elbow_the_other_way_to_the_same_tip() {
    let mut app = app();
    let (_, elbow, hand) = chain(&app, "two_bone_ik", (1.2, 0.8), true);
    app.tick(1.0 / 60.0);
    assert!((global_xy(&app, hand) - Vec2::new(1.2, 0.8)).length() < 1e-3);
    assert!(global_xy(&app, elbow).y < global_xy(&app, hand).y);
}

#[test]
fn an_unreachable_target_straightens_the_chain_toward_it() {
    let mut app = app();
    let (_, _, hand) = chain(&app, "two_bone_ik", (0.0, 5.0), false);
    app.tick(1.0 / 60.0);
    let tip = global_xy(&app, hand);
    assert!((tip - Vec2::new(0.0, 2.0)).length() < 1e-3, "{tip:?}");
}

#[test]
fn look_at_turns_the_bone_toward_the_target() {
    let mut app = app();
    let (shoulder, elbow, _) = chain(&app, "look_at", (0.0, 3.0), false);
    app.tick(1.0 / 60.0);
    let angle = angle_about_z(
        app.engine
            .world()
            .get::<&Transform>(shoulder)
            .unwrap()
            .rotation,
    );
    assert!(
        (angle - std::f32::consts::FRAC_PI_2).abs() < 1e-4,
        "the shoulder turned {angle} rad"
    );
    // Its child follows: the elbow now sits straight above the shoulder.
    let elbow_at = global_xy(&app, elbow);
    assert!(
        (elbow_at - Vec2::new(0.0, 1.0)).length() < 1e-4,
        "{elbow_at:?}"
    );
}

#[test]
fn a_disabled_modifier_leaves_the_pose_alone() {
    let mut app = app();
    let (shoulder, _, _) = chain(&app, "look_at", (0.0, 3.0), false);
    let rig = app
        .engine
        .world()
        .get::<&balaur_core::scene::Parent>(shoulder)
        .unwrap()
        .0;
    let off: toml::Value = toml::from_str("enabled = false").unwrap();
    components::patch(&app.engine, rig, "modifier2d", &off).unwrap();
    app.tick(1.0 / 60.0);
    let angle = angle_about_z(
        app.engine
            .world()
            .get::<&Transform>(shoulder)
            .unwrap()
            .rotation,
    );
    assert!(angle.abs() < 1e-6);
    let got = components::get(&app.engine, rig, "modifier2d").unwrap();
    assert_eq!(got.get("enabled").unwrap().as_bool(), Some(false));
    assert_eq!(got.get("kind").unwrap().as_str(), Some("look_at"));
}

#[test]
fn two_runs_solve_to_the_same_bits() {
    let run = || {
        let mut app = app();
        let (shoulder, elbow, _) = chain(&app, "two_bone_ik", (0.7, 1.3), false);
        for _ in 0..3 {
            app.tick(1.0 / 60.0);
        }
        let world = app.engine.world();
        [shoulder, elbow].map(|e| {
            world
                .get::<&Transform>(e)
                .unwrap()
                .rotation
                .to_array()
                .map(f32::to_bits)
        })
    };
    assert_eq!(run(), run());
}

#[test]
fn a_bone_too_short_for_the_solver_leaves_the_pose_alone() {
    let mut app = app();
    let root = app.engine.root();
    let rig = scene::spawn_node(&mut app.engine.world_mut(), "Rig", root);
    let shoulder = bone(&app, "Shoulder", rig, [0.0, 0.0]);
    // Under the solver's floor: the reach clamp inverts below it and
    // `f32::clamp` panics when its own minimum is above its maximum.
    let elbow = bone(&app, "Elbow", shoulder, [1e-6, 0.0]);
    bone(&app, "Hand", elbow, [1.0, 0.0]);
    balaur_core::skeleton::apply_rest(&mut app.engine.world_mut(), rig);
    node_at(&app, "Target", rig, 1.0, 1.0);
    let params: toml::Value =
        toml::from_str("kind = \"two_bone_ik\"\nbone = \"Shoulder\"\ntarget = \"Target\"").unwrap();
    components::add(&app.engine, rig, "modifier2d", Some(&params)).unwrap();

    app.tick(1.0 / 60.0);

    let angle = angle_about_z(
        app.engine
            .world()
            .get::<&Transform>(shoulder)
            .unwrap()
            .rotation,
    );
    assert!(angle.abs() < 1e-6, "the shoulder turned {angle} rad");
}

#[test]
fn a_target_that_is_not_a_finite_point_leaves_the_pose_alone() {
    let mut app = app();
    let (shoulder, _, hand) = chain(&app, "two_bone_ik", (1.2, 0.8), false);
    let rig = app
        .engine
        .world()
        .get::<&balaur_core::scene::Parent>(shoulder)
        .unwrap()
        .0;
    let target = scene::find_node(&app.engine.world(), rig, "Target").unwrap();
    app.engine
        .world_mut()
        .get::<&mut Transform>(target)
        .unwrap()
        .position = Vec3::new(f32::NAN, 0.0, 0.0);

    app.tick(1.0 / 60.0);

    let tip = global_xy(&app, hand);
    assert!(
        tip.x.is_finite() && tip.y.is_finite(),
        "a NaN target must not be written into the rig: {tip:?}"
    );
}
