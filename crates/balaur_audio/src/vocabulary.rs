//! Every key the `sound` and `listener` components spell, for their schemas
//! and their readers alike. One list for the crate, so the two cannot
//! disagree about a spelling.

/// The `sound` and `listener` components' keys, for their schemas and readers alike.
pub(crate) mod keys {
    pub(crate) const AUTOPLAY: &str = "autoplay";
    pub(crate) const BUS: &str = "bus";
    pub(crate) const CURRENT: &str = "current";
    pub(crate) const DOPPLER_LEVEL: &str = "doppler_level";
    pub(crate) const FILE: &str = "file";
    pub(crate) const LOOP: &str = "loop";
    pub(crate) const MAX_DISTANCE: &str = "max_distance";
    pub(crate) const MIN_DISTANCE: &str = "min_distance";
    pub(crate) const PITCH_SCALE: &str = "pitch_scale";
    pub(crate) const POSITIONAL: &str = "positional";
    pub(crate) const VOLUME_LINEAR: &str = "volume_linear";
}
