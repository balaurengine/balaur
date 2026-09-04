//! The words both dimensions use, and the readers that turn them into
//! numbers.
//!
//! rapier2d and rapier3d are separate crates with incompatible types, so the
//! calls into them are written twice (see `dim2`). Everything *around* those
//! calls — schema property text, option lists, options-table reading, result
//! ordering — is `toml` and `Value` and belongs here, written once.

use balaur_core::components::as_f64;
use balaur_script::Value;

/// A property of a component's TOML params table, as f32.
pub(crate) fn f(params: &toml::Value, key: &str, default: f32) -> f32 {
    params
        .get(key)
        .and_then(as_f64)
        .map_or(default, |v| v as f32)
}

pub(crate) fn boolean(params: &toml::Value, key: &str, default: bool) -> bool {
    params
        .get(key)
        .and_then(toml::Value::as_bool)
        .unwrap_or(default)
}

pub(crate) fn text<'a>(params: &'a toml::Value, key: &str, default: &'a str) -> &'a str {
    params
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or(default)
}

/// One component of a vector-typed property.
pub(crate) fn axis(params: &toml::Value, key: &str, i: usize, default: f32) -> f32 {
    params
        .get(key)
        .and_then(toml::Value::as_array)
        .and_then(|a| a.get(i))
        .and_then(as_f64)
        .map_or(default, |v| v as f32)
}

pub(crate) fn vec3(params: &toml::Value, key: &str, default: [f32; 3]) -> [f32; 3] {
    [
        axis(params, key, 0, default[0]),
        axis(params, key, 1, default[1]),
        axis(params, key, 2, default[2]),
    ]
}

pub(crate) fn vec2(params: &toml::Value, key: &str, default: [f32; 2]) -> [f32; 2] {
    [
        axis(params, key, 0, default[0]),
        axis(params, key, 1, default[1]),
    ]
}

/// Whether a `flags`-typed property holds `name`.
pub(crate) fn flag(params: &toml::Value, key: &str, name: &str) -> bool {
    balaur_core::components::has_flag(params.get(key), name)
}

/// A trailing options table, as scripts write it (`#{ max = 10.0 }`).
///
/// Every query, joint and character call takes one: they have more parameters
/// than a positional list can carry legibly, and most of them are optional
/// (N9's options-table idiom).
pub(crate) struct Opts<'a>(pub Option<&'a Value>);

impl<'a> Opts<'a> {
    pub(crate) fn get(&self, name: &str) -> Option<&'a Value> {
        match self.0 {
            Some(Value::Map(fields)) => fields.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }

    pub(crate) fn boolean(&self, name: &str, default: bool) -> bool {
        match self.get(name) {
            Some(Value::Bool(b)) => *b,
            _ => default,
        }
    }

    pub(crate) fn f32(&self, name: &str, default: f32) -> f32 {
        match self.get(name) {
            Some(Value::Num(n)) => *n as f32,
            Some(Value::Int(i)) => *i as f32,
            _ => default,
        }
    }

    pub(crate) fn text(&self, name: &str) -> Option<&'a str> {
        match self.get(name) {
            Some(Value::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    /// A node argument, as `Value::Node` or as the node id a script holds.
    pub(crate) fn node(&self, name: &str) -> Option<u64> {
        match self.get(name) {
            Some(Value::Node(bits)) => Some(*bits),
            _ => None,
        }
    }

    pub(crate) fn list(&self, name: &str) -> Option<&'a [Value]> {
        match self.get(name) {
            Some(Value::List(items)) => Some(items.as_slice()),
            _ => None,
        }
    }

    /// A vector written as `[x, y, z]`, or the default when absent.
    pub(crate) fn vec3(&self, name: &str, default: [f32; 3]) -> [f32; 3] {
        match self.get(name) {
            Some(Value::Vec3(v)) => *v,
            Some(Value::List(items)) => {
                let at = |i: usize| match items.get(i) {
                    Some(Value::Num(n)) => *n as f32,
                    Some(Value::Int(n)) => *n as f32,
                    _ => default[i],
                };
                [at(0), at(1), at(2)]
            }
            _ => default,
        }
    }

    pub(crate) fn vec2(&self, name: &str, default: [f32; 2]) -> [f32; 2] {
        match self.get(name) {
            Some(Value::Vec2(v)) => *v,
            Some(Value::List(items)) => {
                let at = |i: usize| match items.get(i) {
                    Some(Value::Num(n)) => *n as f32,
                    Some(Value::Int(n)) => *n as f32,
                    _ => default[i],
                };
                [at(0), at(1)]
            }
            _ => default,
        }
    }
}

/// A `Value::Map` from pairs, which is what every query and character call
/// returns.
pub(crate) fn map<const N: usize>(pairs: [(&str, Value); N]) -> Value {
    Value::Map(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// The flag tables both dimensions read and write.
///
/// The bit values come from rapier3d's own constants rather than being spelled
/// out here, and rapier2d's identically-named flags take the same bits — the
/// two crates are one source compiled twice. So a name means the same thing in
/// both dimensions by construction, not by a comment asking for it.
pub(crate) mod flags {
    use crate::rapier3d::prelude::{ActiveCollisionTypes, ActiveEvents};

    pub(crate) fn events() -> [(&'static str, u32); 2] {
        [
            ("collision", ActiveEvents::COLLISION_EVENTS.bits()),
            ("contact_force", ActiveEvents::CONTACT_FORCE_EVENTS.bits()),
        ]
    }

    pub(crate) fn collision_types() -> [(&'static str, u16); 6] {
        [
            (
                "dynamic_dynamic",
                ActiveCollisionTypes::DYNAMIC_DYNAMIC.bits(),
            ),
            (
                "dynamic_kinematic",
                ActiveCollisionTypes::DYNAMIC_KINEMATIC.bits(),
            ),
            ("dynamic_static", ActiveCollisionTypes::DYNAMIC_FIXED.bits()),
            (
                "kinematic_kinematic",
                ActiveCollisionTypes::KINEMATIC_KINEMATIC.bits(),
            ),
            (
                "kinematic_static",
                ActiveCollisionTypes::KINEMATIC_FIXED.bits(),
            ),
            ("static_static", ActiveCollisionTypes::FIXED_FIXED.bits()),
        ]
    }
}

/// The bits a `flags` property sets, given the table for that property.
pub(crate) fn bits<T: Copy + std::ops::BitOrAssign + Default>(
    params: &toml::Value,
    key: &str,
    table: &[(&str, T)],
) -> T {
    let mut out = T::default();
    for (name, bit) in table {
        if flag(params, key, name) {
            out |= *bit;
        }
    }
    out
}

/// The names a bit set holds, as a `flags` property's array.
pub(crate) fn names<T: Copy + Into<u32>>(set: T, table: &[(&str, T)]) -> toml::Value {
    let set: u32 = set.into();
    toml::Value::Array(
        table
            .iter()
            .filter(|(_, bit)| {
                let bit: u32 = (*bit).into();
                bit != 0 && set & bit == bit
            })
            .map(|(name, _)| toml::Value::String((*name).to_string()))
            .collect(),
    )
}

/// The 32 collision layers a `flags` property names, as a bit set.
///
/// An empty membership means layer 0 and an empty filter means every layer:
/// the alternative is 32 strings in every scene file that wants the default.
pub(crate) fn layer_bits(params: &toml::Value, key: &str, empty_is_all: bool) -> u32 {
    let mut bits = 0u32;
    for name in balaur_core::components::as_flags(params.get(key)) {
        if let Ok(layer) = name.parse::<u32>() {
            if layer < 32 {
                bits |= 1 << layer;
            }
        }
    }
    if bits != 0 {
        bits
    } else if empty_is_all {
        u32::MAX
    } else {
        1
    }
}

/// A layer set as the numbers a `flags` property holds; every layer reads back
/// as the empty list, which is how the schema spells "everything".
pub(crate) fn layer_names(bits: u32) -> toml::Value {
    if bits == u32::MAX {
        return toml::Value::Array(Vec::new());
    }
    toml::Value::Array(
        (0..32)
            .filter(|bit| bits & (1 << bit) != 0)
            .map(|bit| toml::Value::String(bit.to_string()))
            .collect(),
    )
}

/// The 32 collision layers, as an `options` list for a `flags` property.
/// Numbers rather than names: a name would have to come from the project file,
/// and no other component resolves its options at inspector time.
pub(crate) fn layer_options() -> String {
    (0..32)
        .map(|i| format!("\"{i}\""))
        .collect::<Vec<_>>()
        .join(", ")
}
