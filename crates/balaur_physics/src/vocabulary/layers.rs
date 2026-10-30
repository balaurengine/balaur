//! The `flags` properties: rapier's flag tables. The 32 layers are
//! `balaur_core::layers`, shared with rendering.

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

/// The bits a set of words sets, given the table for that property: what a
/// derived `flags` field's options name.
pub(crate) fn named_bits<T: Copy + std::ops::BitOrAssign + Default>(
    names: impl IntoIterator<Item = &'static str>,
    table: &[(&str, T)],
) -> T {
    let mut out = T::default();
    for name in names {
        if let Some((_, bit)) = table.iter().find(|(word, _)| *word == name) {
            out |= *bit;
        }
    }
    out
}
