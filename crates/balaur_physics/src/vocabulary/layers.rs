//! The `flags` properties: rapier's flag tables, and the 32 collision layers.

use crate::vocabulary::flag;

/// The flag tables both dimensions read and write.
///
/// The bit values come from rapier3d's own constants rather than being spelled
/// out here, and rapier2d's identically-named flags take the same bits — the
/// two crates are one source compiled twice. So a name means the same thing in
/// both dimensions by construction, not by a comment asking for it.
pub(crate) mod flags {
    use crate::rapier3d::prelude::{ActiveCollisionTypes, ActiveEvents};
    use crate::vocabulary::words as w;

    pub(crate) fn events() -> [(&'static str, u32); 2] {
        [
            (w::COLLISION, ActiveEvents::COLLISION_EVENTS.bits()),
            (w::CONTACT_FORCE, ActiveEvents::CONTACT_FORCE_EVENTS.bits()),
        ]
    }

    pub(crate) fn collision_types() -> [(&'static str, u16); 6] {
        [
            (
                w::DYNAMIC_DYNAMIC,
                ActiveCollisionTypes::DYNAMIC_DYNAMIC.bits(),
            ),
            (
                w::DYNAMIC_KINEMATIC,
                ActiveCollisionTypes::DYNAMIC_KINEMATIC.bits(),
            ),
            (
                w::DYNAMIC_STATIC,
                ActiveCollisionTypes::DYNAMIC_FIXED.bits(),
            ),
            (
                w::KINEMATIC_KINEMATIC,
                ActiveCollisionTypes::KINEMATIC_KINEMATIC.bits(),
            ),
            (
                w::KINEMATIC_STATIC,
                ActiveCollisionTypes::KINEMATIC_FIXED.bits(),
            ),
            (w::STATIC_STATIC, ActiveCollisionTypes::FIXED_FIXED.bits()),
        ]
    }

    /// What an `ignore` property's words exclude from a sweep; `static` takes
    /// colliders with no body along with static bodies, as rapier's flag does.
    pub(crate) fn query_ignores() -> [(&'static str, u32); 5] {
        use crate::rapier3d::prelude::QueryFilterFlags as Flags;
        [
            (w::STATIC, Flags::EXCLUDE_FIXED.bits()),
            (w::KINEMATIC, Flags::EXCLUDE_KINEMATIC.bits()),
            (w::DYNAMIC, Flags::EXCLUDE_DYNAMIC.bits()),
            (w::SENSORS, Flags::EXCLUDE_SENSORS.bits()),
            (w::SOLIDS, Flags::EXCLUDE_SOLIDS.bits()),
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

/// The 32 collision layers a `flags` property names, as a bit set. Layers
/// count from 1, as Godot's do: layer 1 is the lowest bit.
///
/// An empty membership means layer 1 and an empty filter means every layer:
/// the alternative is 32 strings in every scene file that wants the default.
pub(crate) fn layer_bits(params: &toml::Value, key: &str, empty_is_all: bool) -> u32 {
    let mut bits = 0u32;
    for name in balaur_core::components::as_flags(params.get(key)) {
        if let Some(bit) = layer_bit(name.parse::<u32>().ok()) {
            bits |= bit;
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
            .map(|bit| toml::Value::String((bit + 1).to_string()))
            .collect(),
    )
}

/// The bit a layer number sets, for a number from 1 to 32.
pub(crate) fn layer_bit(layer: Option<u32>) -> Option<u32> {
    layer.filter(|l| (1..=32).contains(l)).map(|l| 1 << (l - 1))
}

/// The 32 collision layers, as an `options` list for a `flags` property.
/// Numbers rather than names: a name would have to come from the project file,
/// and no other component resolves its options at inspector time.
pub(crate) fn layer_options() -> String {
    (1..=32)
        .map(|i| format!("\"{i}\""))
        .collect::<Vec<_>>()
        .join(", ")
}
