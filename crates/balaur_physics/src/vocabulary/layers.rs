//! The `flags` properties: rapier's flag tables. The 32 layers are
//! `balaur_core::layers`, shared with rendering.

/// The flag tables both dimensions read and write.
///
/// The bit values come from rapier3d's own constants rather than being spelled
/// out here, and rapier2d's identically-named flags take the same bits — the
/// two crates are one source compiled twice. So a name means the same thing in
/// both dimensions by construction, not by a comment asking for it.
pub(crate) mod flags {
    use crate::vocabulary::words as w;

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
