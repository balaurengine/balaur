//! Clip documents and the sampler.
//!
//! Nothing here builds an `App`: the sampler is `(clip, time) -> pose` and
//! parsing is `(document) -> clip`, and both being reachable with no engine at
//! all is what a blend tree will later be built on.

use balaur_anim::clip::{self, Interp, Property, Wrap};
use balaur_anim::ease::Easing;
use balaur_anim::sampler::{self, TrackValue};
use std::f32::consts::FRAC_PI_2;

use glamx::{EulerRot, Quat, Vec3};

fn clip_of(source: &str) -> clip::Clip {
    clip::parse(&toml::from_str::<toml::Value>(source).unwrap()).unwrap()
}

fn rejection(source: &str) -> String {
    let err = clip::parse(&toml::from_str::<toml::Value>(source).unwrap()).unwrap_err();
    format!("{err:#}")
}

fn position_at(clip: &clip::Clip, time: f32) -> Vec3 {
    match sampler::sample(clip, time)[0] {
        TrackValue::Position(p) => p,
        other => panic!("track 0 sampled as {other:?}, not a position"),
    }
}

const RISE: &str = r#"
length = 2.0
loop = "pingpong"

[[tracks]]
target = "Arm"
property = "position"
interp = "step"
keys = [
  { t = 0.0, value = [0.0, 0.0, 0.0] },
  { t = 2.0, value = [0.0, 10.0, 0.0], ease = "out_back" },
]
"#;

#[test]
fn a_clip_reads_its_length_loop_mode_and_tracks() {
    let clip = clip_of(RISE);
    assert_eq!(clip.length.to_bits(), 2.0f32.to_bits());
    assert_eq!(clip.wrap, Wrap::PingPong);
    assert_eq!(clip.tracks.len(), 1);
    let track = &clip.tracks[0];
    assert_eq!(track.target, "Arm");
    assert_eq!(track.property, Property::Position);
    assert_eq!(track.interp, Interp::Step);
    assert_eq!(track.keys.len(), 2);
    assert_eq!(
        track.keys[1].ease.map(Easing::name),
        Some("out_back"),
        "a key's curve is resolved at parse and remembers its own name"
    );
}

#[test]
fn a_clip_takes_its_length_from_its_last_key_when_it_declares_none() {
    let clip = clip_of(
        r#"
[[tracks]]
property = "position"
keys = [
  { t = 0.0, value = [0.0, 0.0, 0.0] },
  { t = 1.5, value = [0.0, 1.0, 0.0] },
]
"#,
    );
    assert_eq!(clip.length.to_bits(), 1.5f32.to_bits());
    assert_eq!(
        clip.wrap,
        Wrap::None,
        "a clip that says nothing does not loop"
    );
}

#[test]
fn keys_are_sorted_by_time_however_they_were_authored() {
    let clip = clip_of(
        r#"
length = 2.0

[[tracks]]
property = "position"
keys = [
  { t = 2.0, value = [0.0, 10.0, 0.0] },
  { t = 0.0, value = [0.0, 0.0, 0.0] },
  { t = 1.0, value = [0.0, 5.0, 0.0] },
]
"#,
    );
    let times: Vec<f32> = clip.tracks[0].keys.iter().map(|key| key.t).collect();
    assert_eq!(times, vec![0.0, 1.0, 2.0]);
}

#[test]
fn step_interpolation_holds_a_key_until_the_next_one() {
    let clip = clip_of(RISE);
    assert_eq!(position_at(&clip, 0.0).y.to_bits(), 0.0f32.to_bits());
    assert_eq!(
        position_at(&clip, 1.999).y.to_bits(),
        0.0f32.to_bits(),
        "a step key was interpolated"
    );
    assert_eq!(position_at(&clip, 2.0).y.to_bits(), 10.0f32.to_bits());
}

#[test]
fn linear_interpolation_walks_evenly_between_two_keys() {
    let clip = clip_of(
        r#"
length = 2.0

[[tracks]]
property = "position"
keys = [
  { t = 0.0, value = [0.0, 0.0, 0.0] },
  { t = 2.0, value = [0.0, 10.0, 0.0] },
]
"#,
    );
    assert!((position_at(&clip, 0.5).y - 2.5).abs() < 1e-5);
    assert!((position_at(&clip, 1.5).y - 7.5).abs() < 1e-5);
}

#[test]
fn cubic_interpolation_still_passes_through_every_key() {
    let clip = clip_of(
        r#"
length = 3.0

[[tracks]]
property = "position"
interp = "cubic"
keys = [
  { t = 0.0, value = [0.0, 0.0, 0.0] },
  { t = 1.0, value = [0.0, 4.0, 0.0] },
  { t = 2.0, value = [0.0, 1.0, 0.0] },
  { t = 3.0, value = [0.0, 9.0, 0.0] },
]
"#,
    );
    for (time, expected) in [(0.0, 0.0), (1.0, 4.0), (2.0, 1.0), (3.0, 9.0)] {
        assert!(
            (position_at(&clip, time).y - expected).abs() < 1e-4,
            "cubic missed its own key at t={time}: {}",
            position_at(&clip, time).y
        );
    }
    let overshoot = position_at(&clip, 1.5).y;
    assert!(
        overshoot < 4.0 && overshoot > 0.0,
        "a curve between 4 and 1 went somewhere strange: {overshoot}"
    );
}

#[test]
fn a_looping_clip_never_ends_and_a_plain_one_does() {
    let looping = clip_of("length = 2.0\nloop = \"loop\"\n[[tracks]]\nproperty = \"position\"\nkeys = [ { t = 0.0, value = [0.0, 0.0, 0.0] } ]");
    let (time, finished) = sampler::clip_time(&looping, 5.0);
    assert!(!finished);
    assert!(
        (time - 1.0).abs() < 1e-5,
        "5s into a 2s loop is 1s, not {time}"
    );

    let once = clip_of("length = 2.0\n[[tracks]]\nproperty = \"position\"\nkeys = [ { t = 0.0, value = [0.0, 0.0, 0.0] } ]");
    let (time, finished) = sampler::clip_time(&once, 5.0);
    assert!(finished);
    assert_eq!(
        time.to_bits(),
        2.0f32.to_bits(),
        "a finished clip holds its end"
    );
}

#[test]
fn a_pingpong_clip_folds_time_back_on_itself() {
    let clip = clip_of("length = 2.0\nloop = \"pingpong\"\n[[tracks]]\nproperty = \"position\"\nkeys = [ { t = 0.0, value = [0.0, 0.0, 0.0] } ]");
    for (elapsed, expected) in [(0.5, 0.5), (2.5, 1.5), (3.5, 0.5), (4.5, 0.5)] {
        let (time, finished) = sampler::clip_time(&clip, elapsed);
        assert!(!finished);
        assert!(
            (time - expected).abs() < 1e-5,
            "{elapsed}s of pingpong is {expected}s in, not {time}"
        );
    }
}

#[test]
fn a_rotation_track_is_sampled_as_a_quaternion_the_short_way_round() {
    let clip = clip_of(
        r#"
length = 1.0

[[tracks]]
property = "rotation_euler"
keys = [
  { t = 0.0, value = [0.0, 0.0, 2.9670597] },
  { t = 1.0, value = [0.0, 0.0, -2.9670597] },
]
"#,
    );
    let TrackValue::Rotation(rotation) = sampler::sample(&clip, 0.5)[0] else {
        panic!("a rotation track sampled as something else");
    };
    let facing = rotation * Vec3::X;
    assert!(
        facing.x < -0.999,
        "the short way across ±180° faces -x; got {facing:?}"
    );
}

#[test]
fn the_euler_convention_matches_the_engines_own() {
    // The sampler's `libm` euler conversion must agree with `Quat::from_euler`,
    // or an authored key and a scene's `rotation_euler` would disagree.
    for angles in [
        [0.0, 0.0, 0.0],
        [0.3, -1.2, 2.5],
        [-2.9, 0.7, -0.4],
        [FRAC_PI_2, FRAC_PI_2, FRAC_PI_2],
    ] {
        let mine = sampler::quat_from_euler(Vec3::from(angles));
        let glam = Quat::from_euler(EulerRot::ZYX, angles[2], angles[1], angles[0]);
        assert!(
            mine.abs_diff_eq(glam, 1e-5),
            "euler {angles:?}: {mine:?} against the engine's {glam:?}"
        );
    }
}

#[test]
fn a_track_can_name_a_component_property() {
    let clip = clip_of(
        "length = 1.0\n[[tracks]]\nproperty = \"color/rgba\"\nkeys = [ { t = 0.0, value = [1.0, 0.0, 0.0, 1.0] } ]",
    );
    assert_eq!(
        clip.tracks[0].property,
        Property::Component {
            component: "color".into(),
            property: "rgba".into()
        }
    );
    assert_eq!(clip.tracks[0].channels, 4, "rgba is four numbers wide");
}

#[test]
fn a_component_track_is_as_wide_as_its_keys() {
    let one = clip_of(
        "length = 1.0\n[[tracks]]\nproperty = \"shape/radius\"\nkeys = [ { t = 0.0, value = 0.5 }, { t = 1.0, value = 2.0 } ]",
    );
    assert_eq!(one.tracks[0].channels, 1, "a single number is one channel");
    match sampler::sample(&one, 0.5)[0] {
        TrackValue::Property { value, channels } => {
            assert_eq!(channels, 1);
            assert!((value.x - 1.25).abs() < 1e-6, "sampled {value:?}");
        }
        other => panic!("a component track sampled as {other:?}"),
    }
    let why = rejection(
        "length = 1.0\n[[tracks]]\nproperty = \"shape/radius\"\nkeys = [ { t = 0.0, value = 0.5 }, { t = 1.0, value = [1.0, 2.0] } ]",
    );
    assert!(why.contains("key 1"), "the message owes a key: {why}");
    assert!(why.contains("takes 1"), "unhelpful: {why}");
}

#[test]
fn a_property_that_is_neither_a_transform_nor_a_path_is_rejected() {
    for (source, word) in [
        ("property = \"wobble\"", "wobble"),
        ("property = \"color/\"", "color/"),
        ("property = \"/rgba\"", "/rgba"),
    ] {
        let why = rejection(&format!(
            "length = 1.0\n[[tracks]]\n{source}\nkeys = [ {{ t = 0.0, value = [0.0, 0.0, 0.0] }} ]"
        ));
        assert!(why.contains(word), "unhelpful: {why}");
        assert!(why.contains("component/property"), "unhelpful: {why}");
    }
}

#[test]
fn a_track_with_no_property_is_a_method_track() {
    let clip = clip_of(
        "length = 1.0\n[[tracks]]\ntarget = \"Feet\"\nkeys = [ { t = 0.8, call = \"on_footstep\" } ]",
    );
    assert_eq!(clip.tracks[0].property, Property::Call);
    assert_eq!(clip.tracks[0].keys[0].call.as_deref(), Some("on_footstep"));
    assert_eq!(
        sampler::sample(&clip, 0.9)[0],
        TrackValue::None,
        "a method track holds no pose"
    );
}

#[test]
fn a_key_that_neither_calls_nor_carries_a_value_says_which_it_needs() {
    let why = rejection("length = 1.0\n[[tracks]]\nkeys = [ { t = 0.5 } ]");
    assert!(why.contains("call"), "unhelpful: {why}");
    assert!(why.contains("property"), "unhelpful: {why}");
}

#[test]
fn a_call_on_a_track_that_animates_a_value_is_rejected() {
    let why = rejection(
        "length = 1.0\n[[tracks]]\nproperty = \"position\"\nkeys = [ { t = 0.5, call = \"boom\", value = [0.0, 0.0, 0.0] } ]",
    );
    assert!(why.contains("call"), "unhelpful: {why}");
}

#[test]
fn a_method_key_is_passed_once_per_loop_and_never_by_a_seek() {
    let clip = clip_of(
        "length = 1.0\nloop = \"loop\"\n[[tracks]]\nkeys = [ { t = 0.5, call = \"step\" } ]",
    );
    let crossed = |from: f32, to: f32| {
        sampler::spans(&clip, from, to)
            .iter()
            .filter(|&&span| sampler::passes(span, 0.5))
            .count()
    };
    assert_eq!(crossed(0.4, 0.6), 1, "one step over the key is one pass");
    assert_eq!(crossed(0.6, 0.7), 0, "past the key is not a pass");
    assert_eq!(crossed(0.9, 1.1), 0, "the wrap does not re-pass an old key");
    assert_eq!(crossed(1.4, 1.6), 1, "the second loop passes it again");
    // A seek is a jump, not a step: nothing runs over what it skipped, so
    // there is no span to pass anything.
    assert_eq!(crossed(0.9, 0.9), 0, "a seek that lands is not a pass");
}

#[test]
fn a_step_over_the_whole_clip_passes_every_key_exactly_once() {
    let clip = clip_of(
        "length = 1.0\nloop = \"loop\"\n[[tracks]]\nkeys = [ { t = 0.2, call = \"a\" }, { t = 0.8, call = \"b\" } ]",
    );
    let spans = sampler::spans(&clip, 0.1, 3.7);
    for t in [0.2_f32, 0.8] {
        let hits = spans.iter().filter(|&&s| sampler::passes(s, t)).count();
        assert_eq!(hits, 1, "key at {t} was passed {hits} times, not once");
    }
}

#[test]
fn a_pingpong_return_leg_passes_a_key_the_other_way_round() {
    let clip = clip_of(
        "length = 1.0\nloop = \"pingpong\"\n[[tracks]]\nkeys = [ { t = 0.5, call = \"step\" } ]",
    );
    let out = sampler::spans(&clip, 0.4, 0.6);
    assert!(out[0].1 > out[0].0, "the outward leg runs forwards");
    assert!(out.iter().any(|&s| sampler::passes(s, 0.5)));
    let back = sampler::spans(&clip, 1.4, 1.6);
    assert!(back[0].1 < back[0].0, "the return leg runs backwards");
    assert!(back.iter().any(|&s| sampler::passes(s, 0.5)));
}

#[test]
fn an_unknown_loop_mode_is_rejected_naming_it() {
    let why = rejection(
        "length = 1.0\nloop = \"boomerang\"\n[[tracks]]\nproperty = \"position\"\nkeys = [ { t = 0.0, value = [0.0, 0.0, 0.0] } ]",
    );
    assert!(why.contains("boomerang"), "unhelpful: {why}");
    assert!(
        why.contains("pingpong"),
        "the message owes the author the options: {why}"
    );
}

#[test]
fn a_clip_with_no_positive_length_is_rejected() {
    for source in [
        "length = 0.0\n[[tracks]]\nproperty = \"position\"\nkeys = [ { t = 0.0, value = [0.0, 0.0, 0.0] } ]",
        "[[tracks]]\nproperty = \"position\"\nkeys = [ { t = 0.0, value = [0.0, 0.0, 0.0] } ]",
    ] {
        let why = rejection(source);
        assert!(why.contains("length"), "unhelpful: {why}");
    }
}

#[test]
fn a_track_says_which_of_its_keys_is_malformed() {
    let why = rejection(
        "length = 1.0\n[[tracks]]\nproperty = \"position\"\nkeys = [ { t = 0.0, value = [0.0, 0.0, 0.0] }, { t = 1.0, value = [0.0, 1.0] } ]",
    );
    assert!(why.contains("track 0"), "the message owes a track: {why}");
    assert!(why.contains("key 1"), "the message owes a key: {why}");
}

#[test]
fn a_track_without_keys_is_rejected() {
    let why = rejection("length = 1.0\n[[tracks]]\nproperty = \"position\"\nkeys = []");
    assert!(why.contains("key"), "unhelpful: {why}");
}

#[test]
fn an_eased_key_bends_the_segment_that_arrives_at_it() {
    let clip = clip_of(
        r#"
length = 1.0

[[tracks]]
property = "position"
keys = [
  { t = 0.0, value = [0.0, 0.0, 0.0] },
  { t = 1.0, value = [0.0, 10.0, 0.0], ease = "in_quad" },
]
"#,
    );
    // in_quad is a quarter of the way at half the time, where the straight
    // line would be halfway.
    assert!(
        (position_at(&clip, 0.5).y - 2.5).abs() < 1e-4,
        "the curve was not applied: {}",
        position_at(&clip, 0.5).y
    );
    assert_eq!(
        position_at(&clip, 0.0).y.to_bits(),
        0.0f32.to_bits(),
        "an eased key still starts"
    );
    assert_eq!(
        position_at(&clip, 1.0).y.to_bits(),
        10.0f32.to_bits(),
        "and still arrives"
    );
}

#[test]
fn an_easing_curve_can_carry_a_value_past_the_key_it_is_heading_for() {
    let clip = clip_of(
        r#"
length = 1.0

[[tracks]]
property = "position"
keys = [
  { t = 0.0, value = [0.0, 0.0, 0.0] },
  { t = 1.0, value = [0.0, 10.0, 0.0], ease = "out_back" },
]
"#,
    );
    let peak = (0..=100)
        .map(|i| position_at(&clip, i as f32 / 100.0).y)
        .fold(0.0_f32, f32::max);
    assert!(
        peak > 10.0,
        "out_back overshoots and this one did not: {peak}"
    );
}

#[test]
fn a_clip_naming_a_curve_that_does_not_exist_is_rejected() {
    let why = rejection(
        r#"
length = 1.0

[[tracks]]
property = "position"
keys = [ { t = 0.0, value = [0.0, 0.0, 0.0], ease = "out_wobble" } ]
"#,
    );
    assert!(why.contains("out_wobble"), "{why}");
    assert!(why.contains("key 0"), "the message locates the key: {why}");
}

#[test]
fn an_euler_triple_survives_the_round_trip_through_a_quaternion() {
    // What a tween reads when it captures the rotation a node already has.
    // The middle angle stays inside a quarter turn, which is the range this
    // convention can represent unambiguously.
    for angles in [
        [0.0, 0.0, 0.0],
        [0.3, -1.2, 2.5],
        [-2.9, 0.7, -0.4],
        [1.1, 1.5, -3.0],
    ] {
        let there = sampler::quat_from_euler(Vec3::from(angles));
        let back = sampler::euler_from_quat(there);
        let again = sampler::quat_from_euler(back);
        assert!(
            there.abs_diff_eq(again, 1e-4),
            "euler {angles:?} came back as {back:?}, a different rotation"
        );
    }
}
