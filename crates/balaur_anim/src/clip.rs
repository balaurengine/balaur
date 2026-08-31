//! The `animation_clip` asset: what a clip animates, and with what values.
//!
//! A clip is a length, what to do with time past that length, and a list of
//! tracks. A track names a node relative to the player, a property on it, how
//! to interpolate between keys, and the keys themselves:
//!
//! ```toml
//! type = "animation_clip"
//! length = 2.0
//! loop = "loop"                 # none | loop | pingpong
//!
//! [[tracks]]
//! target = ""                   # node path relative to the player; "" = self
//! property = "position"         # position | rotation_euler | scale
//!                               # or <component>/<property>: "color/rgba"
//! interp = "linear"             # step | linear | cubic
//! keys = [
//!   { t = 0.0, value = [0, 0, 0] },
//!   { t = 1.0, value = [0, 3, 0], ease = "out_back" },
//! ]                             # `ease` shapes the segment into its key
//!
//! [[tracks]]                    # a method track: no property, keys that call
//! keys = [ { t = 0.8, call = "on_footstep" } ]
//! ```
//!
//! A library file is the same document with named entries (`[clips.idle]`),
//! addressed `hero.toml#idle` — the asset layer cuts the entry out and the
//! entry inherits the document's `type`, so nothing here knows the word
//! `clips`.
//!
//! A parsed clip is immutable and shared by every node that names it (the
//! asset cache hands out one `Rc`), so everything a player mutates — time,
//! speed, which clip — lives in `crate::AnimationState` instead.

use anyhow::{anyhow, bail, Context, Result};
use balaur_core::components::as_f64;
use glamx::Vec4;

use crate::ease::Easing;

/// What happens to time once it runs past the end of a clip.
///
/// The document key is `loop`, which is a Rust keyword, so the field takes
/// the word for what the mode does with time instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    /// Hold the last key and stop.
    None,
    /// Start over.
    Loop,
    /// Play backwards to the start, then forwards again.
    PingPong,
}

impl Wrap {
    fn parse(text: &str) -> Result<Self> {
        match text {
            "none" => Ok(Self::None),
            "loop" => Ok(Self::Loop),
            "pingpong" => Ok(Self::PingPong),
            other => Err(anyhow!(
                "`loop = \"{other}\"` is not one of \"none\", \"loop\", \"pingpong\""
            )),
        }
    }
}

/// What a track drives.
///
/// A transform property is written bare; anything else is
/// `<component>/<property>`, which goes through the component registry and
/// `balaur_core::components::patch` — so this crate animates `color/rgba`,
/// `shape/radius` and a third-party plugin's properties alike without
/// depending on the crate that registered them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Property {
    Position,
    /// Authored as euler radians `[x, y, z]` — readable in a diff and the
    /// same spelling as `node:set_rotation_euler` — and interpolated as a
    /// quaternion, which is the only way past ±180° that takes the short way.
    RotationEuler,
    Scale,
    /// A registered component's property, addressed `component/property`.
    /// Resolved when the pose is written, not here: a clip may be parsed
    /// before the plugin owning the component has registered it.
    Component {
        component: String,
        property: String,
    },
    /// The track has no property at all: its keys name methods to dispatch.
    /// A document says so by leaving `property` out.
    Call,
}

impl Property {
    /// How many numbers a key on this track carries, when that is fixed in
    /// advance. A component property takes whatever its first key holds,
    /// because core knows one channel from four and this crate does not.
    pub(crate) const fn channels(&self) -> Option<usize> {
        match self {
            Self::Position | Self::RotationEuler | Self::Scale => Some(3),
            Self::Component { .. } => None,
            Self::Call => Some(0),
        }
    }

    pub(crate) fn parse(text: &str) -> Result<Self> {
        match text {
            "position" => Ok(Self::Position),
            "rotation_euler" => Ok(Self::RotationEuler),
            "scale" => Ok(Self::Scale),
            other => match other.split_once('/') {
                Some((component, property))
                    if !component.is_empty()
                        && !property.is_empty()
                        && !property.contains('/') =>
                {
                    Ok(Self::Component {
                        component: component.to_string(),
                        property: property.to_string(),
                    })
                }
                _ => Err(anyhow!(
                    "`property = \"{other}\"` is not \"position\", \"rotation_euler\" or \"scale\", \
                     and does not read as `component/property`"
                )),
            },
        }
    }
}

/// How a track gets from one key to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interp {
    /// Hold each key until the next one.
    Step,
    Linear,
    /// Catmull-Rom through the keys, clamped at both ends.
    Cubic,
}

impl Interp {
    fn parse(text: &str) -> Result<Self> {
        match text {
            "step" => Ok(Self::Step),
            "linear" => Ok(Self::Linear),
            "cubic" => Ok(Self::Cubic),
            other => Err(anyhow!(
                "`interp = \"{other}\"` is not one of \"step\", \"linear\", \"cubic\""
            )),
        }
    }
}

/// One keyframe: a time, and either a value or a method to call.
#[derive(Debug)]
pub struct Key {
    /// Seconds from the start of the clip.
    pub t: f32,
    /// The key's numbers, read as the property's units: metres for
    /// `position` and `scale`, euler radians for `rotation_euler`, and
    /// whatever the property means for a component track. Channels past the
    /// track's own count are zero, and every channel is zero on a method key.
    pub value: Vec4,
    /// The method this key dispatches through `ScriptHost::call_on`, on a
    /// method track. `None` on every value key.
    pub call: Option<String>,
    /// The curve shaping the segment that runs *into* this key, or `None`
    /// for the straight line. Easing belongs to the key a segment arrives at,
    /// which is the convention the format's own examples are written in — so
    /// the first key of a track never carries one that matters.
    pub ease: Option<Easing>,
}

/// One property of one node over time — or, with no property, one list of
/// moments at which to call a method on it.
#[derive(Debug)]
pub struct Track {
    /// Node path relative to the player's `root`; empty means the player's
    /// own node.
    pub target: String,
    pub property: Property,
    /// How many of a key's four channels this track drives: three for a
    /// transform, none for a method track, and for a component property
    /// whatever its keys were authored with — one number for `shape/radius`,
    /// four for `color/rgba`.
    pub channels: usize,
    pub interp: Interp,
    /// Sorted by time at parse, so the sampler can assume it.
    pub keys: Vec<Key>,
}

/// A parsed clip. Immutable: it is shared by every node that names it.
#[derive(Debug)]
pub struct Clip {
    /// Seconds. Always positive — a clip with no duration has no time to
    /// sample at.
    pub length: f32,
    /// What the player does with time past `length`.
    pub wrap: Wrap,
    pub tracks: Vec<Track>,
}

/// Parse one clip definition, the body of a document or of a named entry.
///
/// This is what `App::register_asset_type("animation_clip", ..)` hands the
/// asset layer, so every error here reaches a scene author with the reference
/// that named the clip already wrapped around it.
pub fn parse(value: &toml::Value) -> Result<Clip> {
    let wrap = match value.get("loop") {
        Some(v) => Wrap::parse(
            v.as_str()
                .ok_or_else(|| anyhow!("`loop` is {}, not a mode name", v.type_str()))?,
        )?,
        None => Wrap::None,
    };
    let mut tracks = Vec::new();
    match value.get("tracks") {
        None => {}
        Some(toml::Value::Array(items)) => {
            for (index, item) in items.iter().enumerate() {
                tracks.push(parse_track(item).with_context(|| format!("track {index}"))?);
            }
        }
        Some(other) => bail!("`tracks` is {}, not a list of tracks", other.type_str()),
    }
    let length = clip_length(value, &tracks)?;
    Ok(Clip {
        length,
        wrap,
        tracks,
    })
}

/// The declared `length`, or — when the document leaves it out — the last key
/// in the clip, which is the only other honest answer.
fn clip_length(value: &toml::Value, tracks: &[Track]) -> Result<f32> {
    let length = match value.get("length") {
        Some(v) => {
            as_f64(v).ok_or_else(|| anyhow!("`length` is {}, not a number", v.type_str()))? as f32
        }
        None => tracks
            .iter()
            .filter_map(|track| track.keys.last())
            .map(|key| key.t)
            .fold(0.0_f32, f32::max),
    };
    if length <= 0.0 || !length.is_finite() {
        bail!("a clip needs a positive `length` in seconds, or a key to take one from");
    }
    Ok(length)
}

fn parse_track(value: &toml::Value) -> Result<Track> {
    let target = match value.get("target") {
        Some(v) => v
            .as_str()
            .ok_or_else(|| anyhow!("`target` is {}, not a node path", v.type_str()))?
            .to_string(),
        None => String::new(),
    };
    // No `property` at all is a method track: a list of moments, and what it
    // does at each of them is on the key rather than on the track.
    let property = match value.get("property") {
        Some(v) => Property::parse(
            v.as_str()
                .ok_or_else(|| anyhow!("`property` is {}, not a property name", v.type_str()))?,
        )?,
        None => Property::Call,
    };
    let interp = match value.get("interp") {
        Some(v) => Interp::parse(
            v.as_str()
                .ok_or_else(|| anyhow!("`interp` is {}, not a mode name", v.type_str()))?,
        )?,
        None => Interp::Linear,
    };
    let mut channels = property.channels();
    let keys = parse_keys(value, &property, &mut channels)?;
    Ok(Track {
        target,
        property,
        channels: channels.unwrap_or(0),
        interp,
        keys,
    })
}

fn parse_keys(
    track: &toml::Value,
    property: &Property,
    channels: &mut Option<usize>,
) -> Result<Vec<Key>> {
    let items = track
        .get("keys")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| anyhow!("a track needs a `keys` list"))?;
    if items.is_empty() {
        bail!("a track needs at least one key");
    }
    let mut keys = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        keys.push(parse_key(item, property, channels).with_context(|| format!("key {index}"))?);
    }
    // Sampling assumes ascending time; keys may arrive unsorted. `total_cmp`
    // keeps NaN ordering platform-independent.
    keys.sort_by(|a, b| a.t.total_cmp(&b.t));
    Ok(keys)
}

fn parse_key(
    value: &toml::Value,
    property: &Property,
    channels: &mut Option<usize>,
) -> Result<Key> {
    let t = value
        .get("t")
        .and_then(as_f64)
        .ok_or_else(|| anyhow!("a key needs `t`, its time in seconds"))? as f32;
    let ease = match value.get("ease") {
        Some(v) => Some(Easing::parse(v.as_str().ok_or_else(|| {
            anyhow!("`ease` is {}, not a curve name", v.type_str())
        })?)?),
        None => None,
    };
    if *property == Property::Call {
        let call = value
            .get("call")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| {
                anyhow!(
                    "a key needs `call`, the method to dispatch — or the track needs a \
                     `property` and this key a `value`"
                )
            })?
            .to_string();
        return Ok(Key {
            t,
            value: Vec4::ZERO,
            call: Some(call),
            ease,
        });
    }
    if value.get("call").is_some() {
        bail!("`call` belongs to a track that declares no `property`; this one animates a value");
    }
    Ok(Key {
        t,
        value: parse_value(value, channels)?,
        call: None,
        ease,
    })
}

/// A key's numbers, and the arity check that keeps a track square.
///
/// One number or a list of up to four: `radius = 0.5` and
/// `rgba = [1, 0, 0, 1]` are both properties a component can declare, and a
/// transform is always three. The first key of a component track decides how
/// wide the track is and every later key must agree, so a pose never has to
/// guess which channel a missing number was.
fn parse_value(key: &toml::Value, channels: &mut Option<usize>) -> Result<Vec4> {
    let numbers: Vec<&toml::Value> = match key.get("value") {
        Some(toml::Value::Array(items)) => items.iter().collect(),
        Some(scalar) if as_f64(scalar).is_some() => vec![scalar],
        Some(other) => bail!(
            "`value` is {}, not a number or a list of numbers",
            other.type_str()
        ),
        None => bail!("a key needs a `value`"),
    };
    match *channels {
        Some(wanted) if numbers.len() != wanted => bail!(
            "`value` holds {} numbers; this track takes {wanted}",
            numbers.len()
        ),
        None if numbers.is_empty() || numbers.len() > 4 => {
            bail!(
                "`value` holds {} numbers; a track takes one to four",
                numbers.len()
            )
        }
        _ => {}
    }
    *channels = Some(numbers.len());
    let mut xyzw = [0.0_f32; 4];
    for (slot, number) in xyzw.iter_mut().zip(numbers) {
        *slot = as_f64(number)
            .ok_or_else(|| anyhow!("`value` holds {}, not a number", number.type_str()))?
            as f32;
    }
    Ok(Vec4::from(xyzw))
}
