//! Every key a sound file's import settings and `[audio.buses]` spell, for
//! their readers and the script tables alike. One list for the crate, so the
//! two cannot disagree about a spelling. The `sound` and `listener`
//! components spell theirs once, in their structs.

/// A sound file's decoder keys and a bus's, for their readers and the script
/// tables alike.
pub(crate) mod keys {
    /// A sound file's import keys for its decoder, beside core's own.
    pub(crate) const GAPLESS: &str = "gapless";
    pub(crate) const SEEKABLE: &str = "seekable";

    /// A bus's effects, in `[audio.buses]` and in `audio.bus_effects`.
    pub(crate) const AUTO_GAIN: &str = "auto_gain";
    pub(crate) const AUTO_GAIN_ATTACK_TIME: &str = "auto_gain_attack_time";
    pub(crate) const AUTO_GAIN_FLOOR: &str = "auto_gain_floor";
    pub(crate) const AUTO_GAIN_MAX: &str = "auto_gain_max";
    pub(crate) const AUTO_GAIN_RELEASE_TIME: &str = "auto_gain_release_time";
    pub(crate) const AUTO_GAIN_TARGET: &str = "auto_gain_target";
    pub(crate) const DISTORTION_GAIN: &str = "distortion_gain";
    pub(crate) const DISTORTION_THRESHOLD: &str = "distortion_threshold";
    pub(crate) const HIGH_PASS_HZ: &str = "high_pass_hz";
    pub(crate) const HIGH_PASS_Q: &str = "high_pass_q";
    pub(crate) const LOW_PASS_HZ: &str = "low_pass_hz";
    pub(crate) const LOW_PASS_Q: &str = "low_pass_q";
    pub(crate) const REVERB_LEVEL: &str = "reverb_level";
    pub(crate) const REVERB_TIME: &str = "reverb_time";

    /// A bus's limiter, in `[audio.buses]` and in `audio.bus_limit`.
    pub(crate) const LIMIT: &str = "limit";
    pub(crate) const LIMIT_ATTACK_TIME: &str = "limit_attack_time";
    pub(crate) const LIMIT_KNEE_DB: &str = "limit_knee_db";
    pub(crate) const LIMIT_RELEASE_TIME: &str = "limit_release_time";
    pub(crate) const LIMIT_THRESHOLD_DB: &str = "limit_threshold_db";
}

/// The `[audio]` settings, by path.
pub(crate) mod settings {
    pub(crate) const BUFFER_FRAMES: &str = "audio/buffer_frames";
    pub(crate) const BUSES: &str = "audio/buses";
    pub(crate) const CHANNELS: &str = "audio/channels";
    pub(crate) const DEVICE: &str = "audio/device";
    pub(crate) const DITHER_BITS: &str = "audio/dither_bits";
    pub(crate) const SAMPLE_RATE_HZ: &str = "audio/sample_rate_hz";
}
