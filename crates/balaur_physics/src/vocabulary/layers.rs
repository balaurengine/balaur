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

    /// The body kinds an `exclude_kinds` row passes through; `static` takes
    /// colliders with no body along with static bodies, as rapier's flag does.
    fn exclude_kinds() -> [(&'static str, u32); 3] {
        use crate::rapier3d::prelude::QueryFilterFlags as Flags;
        [
            (w::STATIC, Flags::EXCLUDE_FIXED.bits()),
            (w::KINEMATIC, Flags::EXCLUDE_KINEMATIC.bits()),
            (w::DYNAMIC, Flags::EXCLUDE_DYNAMIC.bits()),
        ]
    }

    /// The query flags a filter's rows set, shared by a query, a character's
    /// move and a vehicle's wheel rays: the body kinds `exclude_kinds` names,
    /// and sensors or solids when `hit_sensors` or `hit_solids` is off.
    pub(crate) fn filter_bits<'a>(
        exclude_kinds: impl IntoIterator<Item = &'a str>,
        hit_sensors: bool,
        hit_solids: bool,
    ) -> u32 {
        use crate::rapier3d::prelude::QueryFilterFlags as Flags;
        let table = self::exclude_kinds();
        let mut bits = 0;
        for kind in exclude_kinds {
            if let Some((_, bit)) = table.iter().find(|(word, _)| *word == kind) {
                bits |= bit;
            }
        }
        if !hit_sensors {
            bits |= Flags::EXCLUDE_SENSORS.bits();
        }
        if !hit_solids {
            bits |= Flags::EXCLUDE_SOLIDS.bits();
        }
        bits
    }
}
