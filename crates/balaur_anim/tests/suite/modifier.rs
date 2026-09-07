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

/// A chain of `count` bones lying along +x, one unit apart, with a modifier
/// of `kind` on the rig root reaching for `target`.
fn long_chain(app: &App, kind: &str, count: usize, target: (f32, f32), extra: &str) -> Vec<Entity> {
    let root = app.engine.root();
    let rig = scene::spawn_node(&mut app.engine.world_mut(), "Rig", root);
    let mut bones = Vec::new();
    let mut parent = rig;
    for i in 0..count {
        let rest = if i == 0 { [0.0, 0.0] } else { [1.0, 0.0] };
        parent = bone(app, &format!("B{i}"), parent, rest);
        bones.push(parent);
    }
    // The last bone needs a length, or the chain ends at its own origin and
    // the solver has one segment fewer than it has bones.
    let tip: toml::Value = toml::from_str("rest_position = [1.0, 0.0]\nlength = 1.0").unwrap();
    components::add(&app.engine, parent, "bone2d", Some(&tip)).unwrap();
    balaur_core::skeleton::apply_rest(&mut app.engine.world_mut(), rig);
    node_at(app, "Target", rig, target.0, target.1);
    let params: toml::Value = toml::from_str(&format!(
        "kind = \"{kind}\"\nbone = \"B0\"\ntarget = \"Target\"\n{extra}"
    ))
    .unwrap();
    components::add(&app.engine, rig, "modifier2d", Some(&params)).unwrap();
    bones
}

/// Where the chain's tip ends up: the last bone's origin plus its length
/// along its own aim, which is what the solvers reach with.
fn tip_of(app: &App, last: Entity) -> Vec2 {
    let world = app.engine.world();
    let g = world.get::<&GlobalTransform>(last).unwrap();
    let angle = angle_about_z(g.rotation);
    Vec2::new(g.position.x, g.position.y) + Vec2::new(libm::cosf(angle), libm::sinf(angle))
}

#[test]
fn fabrik_reaches_a_target_no_two_bone_chain_could() {
    let mut app = app();
    let bones = long_chain(&app, "fabrik", 4, (1.0, 2.5), "iterations = 24");
    app.tick(1.0 / 60.0);
    let tip = tip_of(&app, bones[3]);
    assert!(
        (tip - Vec2::new(1.0, 2.5)).length() < 0.05,
        "the tip landed at {tip:?}"
    );
}

#[test]
fn fabrik_out_of_reach_straightens_the_whole_chain_at_the_target() {
    let mut app = app();
    let bones = long_chain(&app, "fabrik", 3, (0.0, 40.0), "");
    app.tick(1.0 / 60.0);
    let tip = tip_of(&app, bones[2]);
    // Three bones and a tip length: three units of reach, straight up.
    assert!((tip - Vec2::new(0.0, 3.0)).length() < 1e-2, "{tip:?}");
}

#[test]
fn ccdik_reaches_the_same_target_fabrik_does() {
    let mut app = app();
    let bones = long_chain(&app, "ccdik", 4, (1.0, 2.5), "iterations = 40");
    app.tick(1.0 / 60.0);
    let tip = tip_of(&app, bones[3]);
    assert!((tip - Vec2::new(1.0, 2.5)).length() < 0.1, "{tip:?}");
}

#[test]
fn an_angle_limit_holds_every_ccdik_bone_near_its_rest() {
    let mut app = app();
    let limit = 0.25_f32;
    let bones = long_chain(
        &app,
        "ccdik",
        4,
        (0.0, 3.0),
        &format!("iterations = 40\nangle_limit = {limit}"),
    );
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    for (i, &b) in bones.iter().enumerate() {
        let angle = angle_about_z(world.get::<&Transform>(b).unwrap().rotation);
        assert!(
            angle.abs() <= limit + 1e-4,
            "bone {i} turned {angle} rad, past the {limit} limit"
        );
    }
}

#[test]
fn a_chain_of_two_solves_two_bones_and_leaves_the_third() {
    let mut app = app();
    let bones = long_chain(&app, "fabrik", 3, (0.0, 2.0), "chain = 2");
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    let turned = |e: Entity| angle_about_z(world.get::<&Transform>(e).unwrap().rotation).abs();
    assert!(
        turned(bones[0]) > 1e-3,
        "the chain's root should have moved"
    );
    assert!(
        turned(bones[2]) < 1e-6,
        "a bone past `chain` should be left where the clip put it"
    );
}

#[test]
fn a_jiggle_bone_lags_the_pose_and_then_settles_on_it() {
    let mut app = app();
    let root = app.engine.root();
    let rig = scene::spawn_node(&mut app.engine.world_mut(), "Rig", root);
    let hip = bone(&app, "Hip", rig, [0.0, 0.0]);
    let tail = bone(&app, "Tail", hip, [1.0, 0.0]);
    let tip: toml::Value = toml::from_str("rest_position = [1.0, 0.0]\nlength = 1.0").unwrap();
    components::add(&app.engine, tail, "bone2d", Some(&tip)).unwrap();
    balaur_core::skeleton::apply_rest(&mut app.engine.world_mut(), rig);
    let params: toml::Value =
        toml::from_str("kind = \"jiggle\"\nbone = \"Hip\"\nstiffness = 8.0\ndamping = 0.2")
            .unwrap();
    components::add(&app.engine, rig, "modifier2d", Some(&params)).unwrap();
    // Settle first, so the swing below is the one the shove causes.
    for _ in 0..120 {
        app.tick(1.0 / 60.0);
    }
    let resting = angle_about_z(app.engine.world().get::<&Transform>(hip).unwrap().rotation);

    // Whip the rig sideways: the spring's points stay where they were in the
    // world, so the bones must fall behind the pose.
    app.engine
        .world_mut()
        .get::<&mut Transform>(rig)
        .unwrap()
        .position = Vec3::new(3.0, 0.0, 0.0);
    app.tick(1.0 / 60.0);
    let swung = angle_about_z(app.engine.world().get::<&Transform>(hip).unwrap().rotation);
    assert!(
        (swung - resting).abs() > 1e-3,
        "the bone should lag the move, was {resting} now {swung}"
    );

    for _ in 0..400 {
        app.tick(1.0 / 60.0);
    }
    let settled = angle_about_z(app.engine.world().get::<&Transform>(hip).unwrap().rotation);
    assert!(
        settled.abs() < 0.05,
        "the spring should come back to the pose, sitting at {settled} rad"
    );
}

#[test]
fn a_jiggle_chain_solves_to_the_same_bits_twice() {
    let run = || {
        let mut app = app();
        let root = app.engine.root();
        let rig = scene::spawn_node(&mut app.engine.world_mut(), "Rig", root);
        let hip = bone(&app, "Hip", rig, [0.0, 0.0]);
        let tail = bone(&app, "Tail", hip, [1.0, 0.0]);
        let tip: toml::Value = toml::from_str("rest_position = [1.0, 0.0]\nlength = 1.0").unwrap();
        components::add(&app.engine, tail, "bone2d", Some(&tip)).unwrap();
        balaur_core::skeleton::apply_rest(&mut app.engine.world_mut(), rig);
        let params: toml::Value =
            toml::from_str("kind = \"jiggle\"\nuse_gravity = true\nstiffness = 4.0").unwrap();
        components::add(&app.engine, rig, "modifier2d", Some(&params)).unwrap();
        for _ in 0..30 {
            app.tick(1.0 / 60.0);
        }
        let world = app.engine.world();
        [hip, tail].map(|e| {
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
fn a_3d_modifier_aims_a_bone_at_its_target() {
    let mut app = app();
    let root = app.engine.root();
    let rig = scene::spawn_node(&mut app.engine.world_mut(), "Rig", root);
    let arm = scene::spawn_node(&mut app.engine.world_mut(), "Arm", rig);
    let params: toml::Value = toml::from_str("rest_position = [0.0, 0.0, 0.0]").unwrap();
    components::add(&app.engine, arm, "bone3d", Some(&params)).unwrap();
    let hand = scene::spawn_node(&mut app.engine.world_mut(), "Hand", arm);
    let child: toml::Value = toml::from_str("rest_position = [1.0, 0.0, 0.0]").unwrap();
    components::add(&app.engine, hand, "bone3d", Some(&child)).unwrap();
    balaur_core::skeleton::apply_rest(&mut app.engine.world_mut(), rig);
    let target = scene::spawn_node(&mut app.engine.world_mut(), "Target", rig);
    app.engine
        .world_mut()
        .get::<&mut Transform>(target)
        .unwrap()
        .position = Vec3::new(0.0, 0.0, 4.0);
    let m: toml::Value =
        toml::from_str("kind = \"look_at\"\nbone = \"Arm\"\ntarget = \"Target\"").unwrap();
    components::add(&app.engine, rig, "modifier3d", Some(&m)).unwrap();

    app.tick(1.0 / 60.0);

    // The hand rested one unit along +x; aimed at +z it should be there.
    let at = app
        .engine
        .world()
        .get::<&GlobalTransform>(hand)
        .unwrap()
        .position;
    assert!(
        (at - Vec3::new(0.0, 0.0, 1.0)).length() < 1e-4,
        "the hand landed at {at:?}"
    );
}

#[test]
fn a_3d_fabrik_chain_reaches_its_target() {
    let mut app = app();
    let root = app.engine.root();
    let rig = scene::spawn_node(&mut app.engine.world_mut(), "Rig", root);
    let mut parent = rig;
    let mut bones = Vec::new();
    for i in 0..4 {
        let node = scene::spawn_node(&mut app.engine.world_mut(), &format!("B{i}"), parent);
        let rest = if i == 0 {
            "[0.0, 0.0, 0.0]"
        } else {
            "[1.0, 0.0, 0.0]"
        };
        let params: toml::Value =
            toml::from_str(&format!("rest_position = {rest}\nlength = 1.0")).unwrap();
        components::add(&app.engine, node, "bone3d", Some(&params)).unwrap();
        bones.push(node);
        parent = node;
    }
    balaur_core::skeleton::apply_rest(&mut app.engine.world_mut(), rig);
    let target = scene::spawn_node(&mut app.engine.world_mut(), "Target", rig);
    app.engine
        .world_mut()
        .get::<&mut Transform>(target)
        .unwrap()
        .position = Vec3::new(1.0, 0.0, 2.0);
    let m: toml::Value =
        toml::from_str("kind = \"fabrik\"\nbone = \"B0\"\ntarget = \"Target\"\niterations = 24")
            .unwrap();
    components::add(&app.engine, rig, "modifier3d", Some(&m)).unwrap();

    app.tick(1.0 / 60.0);

    let world = app.engine.world();
    let g = world.get::<&GlobalTransform>(bones[3]).unwrap();
    let tip = g.position + g.rotation * Vec3::X;
    assert!(
        (tip - Vec3::new(1.0, 0.0, 2.0)).length() < 0.05,
        "the tip landed at {tip:?}"
    );
}

#[test]
fn every_kind_reads_back_the_way_it_was_written() {
    let app = app();
    let root = app.engine.root();
    let rig = scene::spawn_node(&mut app.engine.world_mut(), "Rig", root);
    for kind in ["look_at", "two_bone_ik", "fabrik", "ccdik", "jiggle"] {
        let params: toml::Value = toml::from_str(&format!(
            "kind = \"{kind}\"\ntarget = \"T\"\nchain = 3\niterations = 7\nangle_limit = 0.5\n\
             stiffness = 2.5\ndamping = 0.25\nmass = 1.5\ngravity = [0.0, -3.0, 0.0]\n\
             use_gravity = true"
        ))
        .unwrap();
        components::add(&app.engine, rig, "modifier2d", Some(&params)).unwrap();
        let got = components::get(&app.engine, rig, "modifier2d").unwrap();
        assert_eq!(got.get("kind").unwrap().as_str(), Some(kind));
        assert_eq!(got.get("chain").unwrap().as_integer(), Some(3));
        assert_eq!(got.get("iterations").unwrap().as_integer(), Some(7));
        assert_eq!(got.get("use_gravity").unwrap().as_bool(), Some(true));
        let gravity = got.get("gravity").unwrap().as_array().unwrap();
        assert_eq!(gravity.len(), 3);
    }
    let bad: toml::Value = toml::from_str("kind = \"wobble\"").unwrap();
    let why = components::add(&app.engine, rig, "modifier2d", Some(&bad)).unwrap_err();
    assert!(format!("{why:#}").contains("wobble"), "{why:#}");
}
