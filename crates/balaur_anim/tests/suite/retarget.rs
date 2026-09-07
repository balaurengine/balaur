//! Deform tracks and retargeting: the two ways a clip stops naming exactly
//! what it drives.
//!
//! A `polygon/deform` track keys two numbers per vertex, which is more than
//! the four channels every other track has; a bone map renames a track's
//! target and corrects the key against the rest it was authored at. Both are
//! asserted headlessly, on the document, with no renderer in sight.

use balaur_anim::AnimationPlugin;
use balaur_anim::clip;
use balaur_core::hecs::Entity;
use balaur_core::mesh::Deform;
use balaur_core::scene::Transform;
use balaur_core::skeleton::{angle_about_z, quat_about_z};
use balaur_core::{App, AppConfig, components, scene};
use glamx::Vec3;

fn app() -> App {
    let mut app = App::new(AppConfig::bare(std::path::PathBuf::from("tests/fixtures"))).unwrap();
    balaur_plugin::load(&mut app, &mut AnimationPlugin::default()).unwrap();
    app
}

fn parse(text: &str) -> clip::Clip {
    clip::parse(&toml::from_str::<toml::Value>(text).unwrap()).unwrap()
}

fn why(text: &str) -> String {
    let value: toml::Value = toml::from_str(text).unwrap();
    format!("{:#}", clip::parse(&value).unwrap_err())
}

#[test]
fn a_deform_track_holds_two_numbers_per_vertex() {
    let clip = parse(
        r#"
length = 1.0
[[tracks]]
property = "polygon/deform"
keys = [
  { t = 0.0, value = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0] },
  { t = 1.0, value = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0] },
]
"#,
    );
    assert_eq!(
        clip.tracks[0].channels, 6,
        "three vertices, two numbers each"
    );
    let half = balaur_anim::sampler::sample(&clip, 0.5);
    match &half[0] {
        balaur_anim::sampler::TrackValue::Deform(offsets) => {
            assert_eq!(offsets.len(), 6);
            assert!((offsets[0] - 0.5).abs() < 1e-5, "{offsets:?}");
            assert!((offsets[5] - 3.0).abs() < 1e-5, "{offsets:?}");
        }
        other => panic!("a deform, got {other:?}"),
    }
}

#[test]
fn a_deform_key_that_is_the_wrong_width_says_so() {
    let text = |value: &str| {
        format!(
            "length = 1.0\n[[tracks]]\nproperty = \"polygon/deform\"\nkeys = [{{ t = 0.0, value = \
             [0.0, 0.0] }}, {{ t = 1.0, value = {value} }}]"
        )
    };
    assert!(why(&text("[1.0, 2.0, 3.0, 4.0]")).contains("this track takes 2"));
    assert!(why(&text("[1.0, 2.0, 3.0]")).contains("two per vertex"));
    // An odd first key never fixes a width at all.
    assert!(
        why(
            "length = 1.0\n[[tracks]]\nproperty = \"polygon/deform\"\nkeys = [{ t = 0.0, value = \
             [1.0] }]"
        )
        .contains("two per vertex")
    );
}

#[test]
fn a_deform_track_writes_the_offsets_onto_the_node_it_targets() {
    let mut app = app();
    let root = app.engine.root();
    let node = scene::spawn_node(&mut app.engine.world_mut(), "Blob", root);
    let params: toml::Value = toml::from_str("autoplay = \"wave\"").unwrap();
    components::add(&app.engine, node, "animation", Some(&params)).unwrap();
    let def: toml::Value = toml::from_str(
        r#"
length = 1.0
[[tracks]]
property = "polygon/deform"
keys = [
  { t = 0.0, value = [0.0, 0.0, 0.0, 0.0] },
  { t = 1.0, value = [0.0, 1.0, 0.0, -1.0] },
]
"#,
    )
    .unwrap();
    balaur_anim::define(&app.engine, node, "wave", def).unwrap();
    balaur_anim::play(&app.engine, node, "wave").unwrap();

    // One tick in, the track is barely off its first key: the component is
    // there and every offset is still near zero.
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    let early = world
        .get::<&Deform>(node)
        .expect("the track writes a Deform");
    assert!(
        early.offsets.iter().all(|v| v.abs() < 0.05),
        "one tick in, the offsets were {:?}",
        early.offsets
    );
    drop(early);
    drop(world);

    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    let world = app.engine.world();
    let deform = world.get::<&Deform>(node).unwrap();
    assert!(!deform.is_rest(), "half a second in, something should move");
    let [_, first] = deform.at(0);
    let [_, second] = deform.at(1);
    assert!(first > 0.2 && first < 0.8, "vertex 0 moved to {first}");
    assert!(second < -0.2 && second > -0.8, "vertex 1 moved to {second}");
    // A vertex the track does not reach is not moved rather than missing.
    #[allow(
        clippy::float_cmp,
        reason = "a vertex nothing wrote, not a computed one"
    )]
    let still = deform.at(9) == [0.0, 0.0];
    assert!(still, "a vertex past the track's end stays where it is");
}

/// A rig whose bones rest turned, so retargeting has something to correct.
fn rig(app: &App, rest: f32) -> (Entity, Entity) {
    let root = app.engine.root();
    let rig = scene::spawn_node(&mut app.engine.world_mut(), "Rig", root);
    let hips = scene::spawn_node(&mut app.engine.world_mut(), "pelvis", rig);
    let params: toml::Value = toml::from_str(&format!(
        "rest_position = [0.0, 1.0]\nrest_rotation = {rest}"
    ))
    .unwrap();
    components::add(&app.engine, hips, "bone2d", Some(&params)).unwrap();
    balaur_core::skeleton::apply_rest(&mut app.engine.world_mut(), rig);
    (rig, hips)
}

#[test]
fn a_bone_map_renames_a_track_and_corrects_it_against_the_rest() {
    let mut app = app();
    let rest = 0.75_f32;
    let (rig, hips) = rig(&app, rest);
    let params: toml::Value = toml::from_str("autoplay = \"\"").unwrap();
    components::add(&app.engine, rig, "animation", Some(&params)).unwrap();
    // The clip names the canonical bone, not this rig's `pelvis`.
    let def: toml::Value = toml::from_str(
        r#"
length = 1.0
[[tracks]]
target = "Hips"
property = "rotation_euler"
keys = [ { t = 0.0, value = [0.0, 0.0, 0.0] } ]
"#,
    )
    .unwrap();
    balaur_anim::define(&app.engine, rig, "idle", def).unwrap();

    // Without a map the track names no node and the bone keeps its rest.
    balaur_anim::play(&app.engine, rig, "idle").unwrap();
    app.tick(1.0 / 60.0);
    let angle = angle_about_z(app.engine.world().get::<&Transform>(hips).unwrap().rotation);
    assert!(
        (angle - rest).abs() < 1e-4,
        "unmapped, the bone sat at {angle}"
    );

    balaur_anim::set_retarget(&app.engine, rig, "maps/hero.toml").unwrap();
    balaur_anim::play(&app.engine, rig, "idle").unwrap();
    app.tick(1.0 / 60.0);
    // The key is the profile's rest — no turn at all — so on a rig resting at
    // 0.75 the bone lands at 0.75, not at zero.
    let angle = angle_about_z(app.engine.world().get::<&Transform>(hips).unwrap().rotation);
    assert!(
        (angle - rest).abs() < 1e-4,
        "mapped, the bone sat at {angle}"
    );

    // And a key that *is* a turn arrives as that turn on top of the rest.
    let turned: toml::Value = toml::from_str(
        r#"
length = 1.0
[[tracks]]
target = "Hips"
property = "rotation_euler"
keys = [ { t = 0.0, value = [0.0, 0.0, 0.5] } ]
"#,
    )
    .unwrap();
    balaur_anim::define(&app.engine, rig, "turn", turned).unwrap();
    balaur_anim::play(&app.engine, rig, "turn").unwrap();
    app.tick(1.0 / 60.0);
    let angle = angle_about_z(app.engine.world().get::<&Transform>(hips).unwrap().rotation);
    assert!(
        (angle - (rest + 0.5)).abs() < 1e-4,
        "a half-radian turn on a rig resting at {rest} should land at {}, got {angle}",
        rest + 0.5
    );
}

#[test]
fn a_map_that_will_not_load_is_an_error_where_the_caller_can_see_it() {
    let app = app();
    let (rig, _) = rig(&app, 0.0);
    let params: toml::Value = toml::from_str("autoplay = \"\"").unwrap();
    components::add(&app.engine, rig, "animation", Some(&params)).unwrap();
    let why = balaur_anim::set_retarget(&app.engine, rig, "maps/nothing.toml").unwrap_err();
    assert!(format!("{why:#}").contains("maps/nothing.toml"), "{why:#}");
    // And taking it off again is not an error, whatever went before.
    balaur_anim::set_retarget(&app.engine, rig, "").unwrap();
}

#[test]
fn the_built_in_humanoid_is_what_a_map_naming_no_profile_uses() {
    let map = balaur_anim::retarget::parse_map(
        &toml::from_str::<toml::Value>("[bones]\nHips = \"pelvis\"").unwrap(),
    )
    .unwrap();
    assert!(map.profile.is_empty());
    assert_eq!(map.bones.get("Hips").map(String::as_str), Some("pelvis"));
    let profile = balaur_anim::SkeletonProfile::humanoid();
    assert!(profile.bones.iter().any(|b| b.name == "Hips"));
    assert!(profile.bones.iter().any(|b| b.name == "RightToes"));
}

#[test]
fn a_deform_and_a_transform_track_share_one_clip() {
    let mut app = app();
    let root = app.engine.root();
    let node = scene::spawn_node(&mut app.engine.world_mut(), "Blob", root);
    let params: toml::Value = toml::from_str("autoplay = \"\"").unwrap();
    components::add(&app.engine, node, "animation", Some(&params)).unwrap();
    let def: toml::Value = toml::from_str(
        r#"
length = 1.0
[[tracks]]
property = "position"
keys = [ { t = 0.0, value = [0.0, 0.0, 0.0] }, { t = 1.0, value = [4.0, 0.0, 0.0] } ]
[[tracks]]
property = "polygon/deform"
keys = [ { t = 0.0, value = [0.0, 0.0] }, { t = 1.0, value = [8.0, 0.0] } ]
"#,
    )
    .unwrap();
    balaur_anim::define(&app.engine, node, "both", def).unwrap();
    balaur_anim::play(&app.engine, node, "both").unwrap();
    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    let world = app.engine.world();
    let moved = world.get::<&Transform>(node).unwrap().position;
    let deform = world.get::<&Deform>(node).unwrap().at(0)[0];
    assert!((moved.x - 2.0).abs() < 0.2, "the node moved to {moved:?}");
    assert!((deform - 4.0).abs() < 0.4, "the vertex moved by {deform}");
    // The two are independent: the deform is in the mesh's own space and the
    // position is the node's, so a doubled deform is not a doubled move.
    assert!((deform - 2.0 * moved.x).abs() < 0.5);
}

/// The rest-pose helpers directly, which is where the retarget arithmetic is.
#[test]
fn a_position_key_is_scaled_by_how_much_longer_this_rigs_bone_rests() {
    use balaur_anim::retarget::{BoneMap, ProfileBone, Retarget, SkeletonProfile};
    use balaur_core::skeleton::Bone;
    let profile = SkeletonProfile {
        bones: vec![ProfileBone {
            name: "Hips".into(),
            rest_position: Vec3::new(0.0, 2.0, 0.0),
            ..ProfileBone::default()
        }],
    };
    let mut map = BoneMap::default();
    map.bones.insert("Hips".into(), "pelvis".into());
    let r = Retarget {
        map: std::rc::Rc::new(map),
        profile: std::rc::Rc::new(profile),
    };
    let rest = Bone {
        rest_position: Vec3::new(0.0, 3.0, 0.0),
        ..Bone::default()
    };
    let out = r.position("Hips", Some(&rest), Vec3::new(0.0, 1.0, 0.0));
    assert!((out.y - 1.5).abs() < 1e-5, "got {out:?}");
    // A rig with no `Bone` at the target is left exactly as authored.
    let same = r.position("Hips", None, Vec3::new(0.0, 1.0, 0.0));
    assert!((same.y - 1.0).abs() < 1e-5);
    let _ = quat_about_z(0.0);
}
