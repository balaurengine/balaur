//! Easing: twelve transitions in four modes, and the names Godot gives them.
//!
//! A curve is named `<mode>_<transition>` — `in_quad`, `out_back`,
//! `in_out_elastic` — with mode one of `in`, `out`, `in_out`, `out_in` and
//! transition one of the twelve below. `linear` is also spelled bare, because
//! all four of its modes are the same straight line. The shapes are Godot's,
//! so a curve ported from a Godot project moves the same way here.
//!
//! Every transcendental is `libm`'s. `f32::sin`, `f32::powf` and `f32::exp2`
//! route to the platform's libm and differ between operating systems, which
//! would put a per-platform drift into any simulation an animation touches —
//! the same reason [`crate::sampler`] writes out its own slerp. `sqrt` and
//! the arithmetic operators are exactly specified by IEEE-754 and are used
//! directly.
//!
//! A curve maps `0 -> 0` and `1 -> 1` exactly. In between it is free to leave
//! the interval: `back`, `elastic` and `spring` overshoot on purpose, and the
//! interpolation they shape carries the overshoot through to the value.

use anyhow::{Result, anyhow};

/// The shape of a curve, before a [`Mode`] decides which end it applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    Linear,
    Sine,
    Quad,
    Cubic,
    Quart,
    Quint,
    Expo,
    Circ,
    Back,
    Elastic,
    Bounce,
    Spring,
}

/// Which end of a segment the transition shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Slow at the start.
    In,
    /// Slow at the end: `In` reflected through both axes.
    Out,
    /// `In` over the first half, `Out` over the second.
    InOut,
    /// `Out` over the first half, `In` over the second.
    OutIn,
}

/// Every curve, by the name a document spells it with.
///
/// Index 0 is the bare `linear`; after it come the twelve transitions in
/// four modes each, in `TRANSITIONS` × `MODES` order, which is what
/// [`Easing::transition`] and [`Easing::mode`] read back out of the index.
const NAMES: [&str; 49] = [
    "linear",
    "in_linear",
    "out_linear",
    "in_out_linear",
    "out_in_linear",
    "in_sine",
    "out_sine",
    "in_out_sine",
    "out_in_sine",
    "in_quad",
    "out_quad",
    "in_out_quad",
    "out_in_quad",
    "in_cubic",
    "out_cubic",
    "in_out_cubic",
    "out_in_cubic",
    "in_quart",
    "out_quart",
    "in_out_quart",
    "out_in_quart",
    "in_quint",
    "out_quint",
    "in_out_quint",
    "out_in_quint",
    "in_expo",
    "out_expo",
    "in_out_expo",
    "out_in_expo",
    "in_circ",
    "out_circ",
    "in_out_circ",
    "out_in_circ",
    "in_back",
    "out_back",
    "in_out_back",
    "out_in_back",
    "in_elastic",
    "out_elastic",
    "in_out_elastic",
    "out_in_elastic",
    "in_bounce",
    "out_bounce",
    "in_out_bounce",
    "out_in_bounce",
    "in_spring",
    "out_spring",
    "in_out_spring",
    "out_in_spring",
];

const TRANSITIONS: [Transition; 12] = [
    Transition::Linear,
    Transition::Sine,
    Transition::Quad,
    Transition::Cubic,
    Transition::Quart,
    Transition::Quint,
    Transition::Expo,
    Transition::Circ,
    Transition::Back,
    Transition::Elastic,
    Transition::Bounce,
    Transition::Spring,
];

const MODES: [Mode; 4] = [Mode::In, Mode::Out, Mode::InOut, Mode::OutIn];

/// One named easing curve.
///
/// Held as an index into the crate-private name table so that it is `Copy`,
/// comparable, and still knows the name it was authored with — which is what
/// lets a clip carrying easing round-trip back to the document it came from.
/// [`names`] is that table, in the same order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Easing(u8);

impl std::fmt::Debug for Easing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl Easing {
    /// The straight line: what a segment with no `ease` already does.
    pub const LINEAR: Self = Self(0);

    /// The curve `name` spells.
    ///
    /// # Errors
    /// If no curve has that name. The message lists the vocabulary, because
    /// the wrong half of the name is usually the mode.
    pub fn parse(name: &str) -> Result<Self> {
        NAMES
            .iter()
            .position(|known| *known == name)
            .and_then(|index| u8::try_from(index).ok())
            .map(Self)
            .ok_or_else(|| {
                anyhow!(
                    "`ease = \"{name}\"` is not a curve; a curve is <mode>_<transition> with \
                     mode one of \"in\", \"out\", \"in_out\", \"out_in\" and transition one of \
                     \"linear\", \"sine\", \"quad\", \"cubic\", \"quart\", \"quint\", \"expo\", \
                     \"circ\", \"back\", \"elastic\", \"bounce\", \"spring\" — or plain \"linear\""
                )
            })
    }

    /// The name this curve was spelled with.
    #[must_use]
    pub const fn name(self) -> &'static str {
        NAMES[self.0 as usize]
    }

    /// The shape half of the name.
    #[must_use]
    pub const fn transition(self) -> Transition {
        match self.0.checked_sub(1) {
            Some(index) => TRANSITIONS[(index / 4) as usize],
            None => Transition::Linear,
        }
    }

    /// The mode half of the name. Bare `linear` is `In`, which for a straight
    /// line is every other mode as well.
    #[must_use]
    pub const fn mode(self) -> Mode {
        match self.0.checked_sub(1) {
            Some(index) => MODES[(index % 4) as usize],
            None => Mode::In,
        }
    }

    /// Where `u` — how far along a segment, `0` to `1` — actually is once the
    /// curve has shaped it.
    #[must_use]
    pub fn apply(self, u: f32) -> f32 {
        let which = self.transition();
        match self.mode() {
            Mode::In => shape(which, false, u),
            Mode::Out => 1.0 - shape(which, false, 1.0 - u),
            // The two halves are the widened shape: `back` and `elastic` take
            // a stronger parameter in this mode, which is Godot's choice and
            // is why `shape` is asked which it wants.
            Mode::InOut => {
                if u < 0.5 {
                    shape(which, true, u + u) * 0.5
                } else {
                    1.0 - shape(which, true, 2.0 - (u + u)) * 0.5
                }
            }
            Mode::OutIn => {
                if u < 0.5 {
                    (1.0 - shape(which, false, 1.0 - (u + u))) * 0.5
                } else {
                    0.5 + shape(which, false, u + u - 1.0) * 0.5
                }
            }
        }
    }
}

/// Every curve's name, in declaration order: the bare `linear`, then the
/// twelve transitions in four modes each.
///
/// The catalogue the editor's curve picker reads, and what a test asserts is
/// reachable one by one.
#[must_use]
pub fn names() -> Vec<&'static str> {
    NAMES.to_vec()
}

/// A transition's `in` shape: `0 -> 0`, `1 -> 1`, slow at the start.
///
/// `wide` asks for the stronger parameter the `in_out` mode uses; every
/// transition but `back` and `elastic` ignores it.
fn shape(which: Transition, wide: bool, u: f32) -> f32 {
    const PI: f32 = std::f32::consts::PI;
    match which {
        Transition::Linear => u,
        Transition::Sine => 1.0 - libm::cosf(u * PI * 0.5),
        Transition::Quad => u * u,
        Transition::Cubic => u * u * u,
        Transition::Quart => u * u * u * u,
        Transition::Quint => u * u * u * u * u,
        // 2^-10 at u = 0 is not zero, so the start is pinned rather than
        // stepped into from a hundredth of the way up.
        Transition::Expo => {
            if u <= 0.0 {
                0.0
            } else {
                libm::exp2f(10.0 * (u - 1.0))
            }
        }
        Transition::Circ => 1.0 - (1.0 - (u * u).min(1.0)).sqrt(),
        Transition::Back => {
            let s = if wide { 1.70158 * 1.525 } else { 1.70158 };
            u * u * ((s + 1.0) * u - s)
        }
        Transition::Elastic => elastic_in(if wide { 0.45 } else { 0.3 }, u),
        Transition::Bounce => 1.0 - bounce_out(1.0 - u),
        Transition::Spring => 1.0 - spring_out(1.0 - u),
    }
}

/// A decaying oscillation that settles onto the target: `p` is the period,
/// and the quarter-period phase shift is what makes both ends land exactly.
fn elastic_in(p: f32, u: f32) -> f32 {
    if u <= 0.0 {
        return 0.0;
    }
    if u >= 1.0 {
        return 1.0;
    }
    let s = p * 0.25;
    let t = u - 1.0;
    -(libm::exp2f(10.0 * t) * libm::sinf((t - s) * (2.0 * std::f32::consts::PI) / p))
}

/// Four parabolic hops of shrinking height, each one flat where it lands.
fn bounce_out(u: f32) -> f32 {
    const K: f32 = 7.5625;
    const D: f32 = 2.75;
    if u < 1.0 / D {
        K * u * u
    } else if u < 2.0 / D {
        let v = u - 1.5 / D;
        K * v * v + 0.75
    } else if u < 2.5 / D {
        let v = u - 2.25 / D;
        K * v * v + 0.9375
    } else {
        let v = u - 2.625 / D;
        K * v * v + 0.984_375
    }
}

/// An overshoot that wobbles in with a rising frequency, damped by a power
/// of the distance left to travel.
fn spring_out(u: f32) -> f32 {
    let s = 1.0 - u;
    let wobble = libm::sinf(u * std::f32::consts::PI * (0.2 + 2.5 * u * u * u));
    (wobble * libm::powf(s, 2.2) + u) * (1.0 + 1.2 * s)
}
