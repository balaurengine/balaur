//! The sampler: `(clip, time) -> pose`, and nothing else.
//!
//! Pure on purpose. Blend trees and state machines are a later tier
//! (`docs/PLAN-animation-and-resources.md`), and they compose *samples*:
//! a blender that mixes two poses needs the sampler to depend on nothing but
//! its two arguments, or the data model has to change to admit it.
//!
//! Two pieces of math are written out here rather than taken from glam.
//! `Quat::from_euler` and `Quat::slerp` both go through `f32::sin` / `cos` /
//! `acos`, which route to the platform libm and differ across operating
//! systems; the `libm` crate is MUSL's algorithms in pure Rust and gives the
//! same bits everywhere. `sqrt`, `floor` and the arithmetic operators are
//! exactly specified by IEEE-754 and are used directly.

use glamx::{Quat, Vec3, Vec4};

use crate::clip::{Clip, Interp, Key, Property, Track, Wrap};

/// One track's value at a moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrackValue {
    Position(Vec3),
    /// Already a quaternion: euler keys are converted and slerped here, so a
    /// caller never sees the angles that would flip past ±180°.
    Rotation(Quat),
    Scale(Vec3),
    /// A component property's value: the channels the track declares, in the
    /// order they were authored. What component and property they belong to
    /// is on the track, which is what pairs a pose with `clip.tracks`.
    Property {
        value: Vec4,
        channels: usize,
    },
    /// A method track holds no value. It is a list of moments, and what
    /// happens at them is dispatched rather than posed.
    None,
}

/// One value per track, in the clip's own track order. Pairing a pose with
/// `clip.tracks` is what says which node each value belongs to.
pub type Pose = Vec<TrackValue>;

/// Where `elapsed` seconds of playback land inside the clip, and whether
/// playback has run off the end.
///
/// Only [`Wrap::None`] ever ends; a looping clip answers `false` forever.
#[must_use]
pub fn clip_time(clip: &Clip, elapsed: f32) -> (f32, bool) {
    let length = clip.length;
    match clip.wrap {
        Wrap::None => (elapsed.clamp(0.0, length), elapsed >= length),
        Wrap::Loop => (fold(elapsed, length), false),
        Wrap::PingPong => {
            let doubled = fold(elapsed, 2.0 * length);
            let time = if doubled <= length {
                doubled
            } else {
                2.0 * length - doubled
            };
            (time.clamp(0.0, length), false)
        }
    }
}

/// `elapsed` folded into `[0, period]`.
///
/// Written as floor-and-subtract rather than `%`: both are exact under
/// IEEE-754, and this one also gives the right answer for negative time,
/// which a clip played at a negative speed reaches.
fn fold(elapsed: f32, period: f32) -> f32 {
    if period <= 0.0 || !elapsed.is_finite() {
        return 0.0;
    }
    let folded = elapsed - (elapsed / period).floor() * period;
    folded.clamp(0.0, period)
}

/// Whether a step's two playhead positions are the same instant.
///
/// Exact equality is the point, not an approximation. These are clip-local
/// times produced by the same arithmetic on both ends of one fixed step, and
/// the question being asked is "did the playhead move at all" — a span of no
/// width has to be recognised bit-for-bit, or a method key fires on a step
/// that stood still. Spelled as a subtraction against zero because that is
/// the exact test; `clippy::float_cmp` is written for tolerance comparisons
/// of independently computed quantities, which this is not.
fn still(a: f32, b: f32) -> bool {
    a - b == 0.0
}

/// The clip-local spans one step of playback passes over, in the order it
/// passes them.
///
/// A span is directed: `(a, b)` with `b < a` is time running backwards, which
/// a pingpong clip's return leg and a negative `speed` both produce. A loop
/// that wraps mid-step gives two spans; a step that covers the whole clip
/// gives one span over all of it, because a key cannot fire twice for one
/// step and walking pass by pass would grow the list without bound.
///
/// This is what a method track fires from: [`sample`] answers where playback
/// *is*, and a call has to know what playback went *over*.
#[must_use]
pub fn spans(clip: &Clip, from: f32, to: f32) -> Vec<(f32, f32)> {
    let length = clip.length;
    if length <= 0.0 || !from.is_finite() || !to.is_finite() || still(from, to) {
        return Vec::new();
    }
    if clip.wrap == Wrap::None {
        let (a, b) = (from.clamp(0.0, length), to.clamp(0.0, length));
        return if still(a, b) {
            Vec::new()
        } else {
            vec![(a, b)]
        };
    }
    if (to - from).abs() >= length {
        return vec![if to > from {
            (0.0, length)
        } else {
            (length, 0.0)
        }];
    }
    let (lo, hi) = if to > from { (from, to) } else { (to, from) };
    let mut out = Vec::new();
    let (first, last) = (pass_of(lo, length), pass_of(hi, length));
    for pass in first..=last {
        let base = pass as f32 * length;
        let a = (lo.max(base) - base).clamp(0.0, length);
        let b = (hi.min(base + length) - base).clamp(0.0, length);
        if !still(a, b) {
            out.push(oriented(clip, pass, a, b));
        }
    }
    if to < from {
        out.reverse();
        for span in &mut out {
            *span = (span.1, span.0);
        }
    }
    out
}

/// Which pass over the clip `elapsed` falls in. Negative for time before the
/// start, which a negative speed reaches.
fn pass_of(elapsed: f32, length: f32) -> i32 {
    (elapsed / length).floor() as i32
}

/// A forward pass's local span, turned the way that pass actually runs: a
/// pingpong clip plays its odd passes backwards, and a method key on the
/// return leg is passed in the other direction.
fn oriented(clip: &Clip, pass: i32, a: f32, b: f32) -> (f32, f32) {
    if clip.wrap == Wrap::PingPong && pass.rem_euclid(2) == 1 {
        (clip.length - a, clip.length - b)
    } else {
        (a, b)
    }
}

/// Whether a key at `t` is passed by `span`.
///
/// Half-open, exclusive at the start and inclusive at the end, in whichever
/// direction the span runs. That is what makes a key fire exactly once per
/// loop rather than twice at the seam, and it is what makes `seek` land
/// *onto* a key without firing it — a seek must not fire what it skipped.
/// The cost is that a key at exactly `t = 0` is the clip's opening pose
/// rather than something playback passes.
#[must_use]
pub fn passes(span: (f32, f32), t: f32) -> bool {
    let (a, b) = span;
    if b >= a {
        t > a && t <= b
    } else {
        t < a && t >= b
    }
}

/// The pose a clip holds at `time` seconds into itself.
///
/// `time` is a position *inside* the clip — [`clip_time`] is what turns
/// elapsed playback into one.
#[must_use]
pub fn sample(clip: &Clip, time: f32) -> Pose {
    clip.tracks
        .iter()
        .map(|track| sample_track(track, time))
        .collect()
}

fn sample_track(track: &Track, time: f32) -> TrackValue {
    match track.property {
        Property::Position => TrackValue::Position(sample_channels(track, time).truncate()),
        Property::Scale => TrackValue::Scale(sample_channels(track, time).truncate()),
        Property::RotationEuler => TrackValue::Rotation(sample_rotation(track, time, |key| {
            quat_from_euler(key.value.truncate())
        })),
        Property::Rotation => TrackValue::Rotation(sample_rotation(track, time, |key| {
            Quat::from_vec4(key.value).normalize()
        })),
        Property::Component { .. } => TrackValue::Property {
            value: sample_channels(track, time),
            channels: track.channels,
        },
        Property::Call => TrackValue::None,
    }
}

/// The key at or before `time`, and how far from it to the next key `time`
/// sits. Outside the track's own span the answer is an endpoint at `0.0`, so
/// every caller can treat `u == 0` as "exactly on key `index`".
fn segment(keys: &[Key], time: f32) -> (usize, f32) {
    let last = keys.len() - 1;
    if time <= keys[0].t {
        return (0, 0.0);
    }
    if time >= keys[last].t {
        return (last, 0.0);
    }
    let mut index = 0;
    while index + 1 < last && keys[index + 1].t <= time {
        index += 1;
    }
    let span = keys[index + 1].t - keys[index].t;
    if span <= 0.0 {
        return (index, 0.0);
    }
    (index, (time - keys[index].t) / span)
}

/// How far along a segment the curve on the key it arrives at says we are.
///
/// A curve is free to leave `[0, 1]` — `back`, `elastic` and `spring`
/// overshoot on purpose — and every interpolation below carries that through
/// to the value, which is what an overshoot means.
fn eased(keys: &[Key], index: usize, u: f32) -> f32 {
    keys[index + 1].ease.map_or(u, |curve| curve.apply(u))
}

/// A track's four channels at `time`. Channels the track does not drive stay
/// at zero all the way through, so interpolating them costs nothing and means
/// nothing.
fn sample_channels(track: &Track, time: f32) -> Vec4 {
    let keys = &track.keys;
    let (index, raw) = segment(keys, time);
    let before = keys[index].value;
    if raw <= 0.0 {
        return before;
    }
    let u = eased(keys, index, raw);
    let after = keys[index + 1].value;
    match track.interp {
        Interp::Step => before,
        Interp::Linear => before + (after - before) * u,
        Interp::Cubic => catmull_rom(
            keys[index.saturating_sub(1)].value,
            before,
            after,
            keys[(index + 2).min(keys.len() - 1)].value,
            u,
        ),
    }
}

/// A rotation track at `time`, whichever spelling its keys use: `quat` is
/// how one key becomes a quaternion.
fn sample_rotation(track: &Track, time: f32, quat: impl Fn(&Key) -> Quat) -> Quat {
    let keys = &track.keys;
    let (index, raw) = segment(keys, time);
    let at = |i: usize| quat(&keys[i]);
    let before = at(index);
    if raw <= 0.0 {
        return before;
    }
    let u = eased(keys, index, raw);
    let after = at(index + 1);
    match track.interp {
        Interp::Step => before,
        Interp::Linear => slerp(before, after, u),
        // Catmull-Rom on the quaternion components, every control point first
        // flipped into `before`'s hemisphere so the curve takes the same short
        // way slerp does, then renormalised back onto the unit sphere.
        Interp::Cubic => {
            let earlier = at(index.saturating_sub(1));
            let later = at((index + 2).min(keys.len() - 1));
            catmull_rom_quat(
                align(earlier, before),
                before,
                align(after, before),
                align(later, before),
                u,
            )
            .normalize()
        }
    }
}

/// The uniform Catmull-Rom point between `p1` and `p2`, in the usual basis:
/// it passes through every key and only needs the two neighbours.
fn catmull_rom(p0: Vec4, p1: Vec4, p2: Vec4, p3: Vec4, u: f32) -> Vec4 {
    let (u2, u3) = (u * u, u * u * u);
    let linear = (p2 - p0) * u;
    let square = (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * u2;
    let cube = (p3 + p1 * 3.0 - p0 - p2 * 3.0) * u3;
    (p1 * 2.0 + linear + square + cube) * 0.5
}

/// [`catmull_rom`] over quaternion components. Valid only on control points
/// that share a hemisphere, and the result needs normalising.
fn catmull_rom_quat(p0: Quat, p1: Quat, p2: Quat, p3: Quat, u: f32) -> Quat {
    let (u2, u3) = (u * u, u * u * u);
    let linear = (p2 - p0) * u;
    let square = (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * u2;
    let cube = (p3 + p1 * 3.0 - p0 - p2 * 3.0) * u3;
    (p1 * 2.0 + linear + square + cube) * 0.5
}

/// `q` flipped into the same hemisphere as `reference`.
///
/// A quaternion and its negation are the same rotation, so this changes
/// nothing about what is being represented and everything about which way
/// round the interpolation goes.
fn align(q: Quat, reference: Quat) -> Quat {
    if q.dot(reference) < 0.0 {
        -q
    } else {
        q
    }
}

/// Shortest-arc spherical interpolation, on `libm`.
///
/// Rotation keys are authored as euler because that is what reads in a diff,
/// and they must not be interpolated that way: lerping euler angles from
/// +170° to -170° travels 340° the wrong way round, which is the bug the
/// editor prototype has.
#[must_use]
pub fn slerp(from: Quat, to: Quat, t: f32) -> Quat {
    let mut end = to;
    let mut dot = from.dot(to);
    if dot < 0.0 {
        end = -to;
        dot = -dot;
    }
    // Nearly parallel: sin(theta) underflows and a normalised lerp is
    // indistinguishable from the arc it approximates.
    if dot > 0.9995 {
        return (from + (end - from) * t).normalize();
    }
    let theta = libm::acosf(dot.min(1.0));
    let sin_theta = libm::sinf(theta);
    let before = libm::sinf(theta * (1.0 - t)) / sin_theta;
    let after = libm::sinf(theta * t) / sin_theta;
    (from * before + end * after).normalize()
}

/// An euler triple as a quaternion, in the engine's own convention.
///
/// `[x, y, z]` are rotations about X, Y and Z, composed Z then Y then X —
/// exactly what a scene file's `rotation_euler` and `node:set_rotation_euler`
/// mean, so an authored key and an authored transform agree. Written out on
/// `libm` rather than calling `Quat::from_euler`, whose sin/cos are the
/// platform's.
#[must_use]
pub fn quat_from_euler(euler: Vec3) -> Quat {
    // One implementation for a clip key and a bone's rest pose, so the two
    // agree to the bit.
    balaur_core::skeleton::quat_from_euler(euler)
}

/// A quaternion back to the euler triple [`quat_from_euler`] would build it
/// from, in the same convention and on the same `libm`.
///
/// This is what reads a node's *current* rotation when a tween captures its
/// start value: a rotation track's keys are euler, so the value it starts
/// from has to be one. Straight up or straight down the X and Z angles trade
/// places, as they do in every euler convention; the rotation the pair
/// describes is still the right one.
#[must_use]
pub fn euler_from_quat(q: Quat) -> Vec3 {
    balaur_core::skeleton::euler_from_quat(q)
}
