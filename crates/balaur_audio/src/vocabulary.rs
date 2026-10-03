//! Every key the `sound` and `listener` components spell, for their schemas
//! and their readers alike. One list for the crate, so the two cannot
//! disagree about a spelling.

/// The `sound` and `listener` components' keys, for their schemas and readers alike.
pub(crate) mod keys {
    pub(crate) const ATTENUATION: &str = "attenuation";
    pub(crate) const AUTO_GAIN: &str = "auto_gain";
    pub(crate) const AUTO_GAIN_ATTACK_TIME: &str = "auto_gain_attack_time";
    pub(crate) const AUTO_GAIN_FLOOR: &str = "auto_gain_floor";
    pub(crate) const AUTO_GAIN_MAX: &str = "auto_gain_max";
    pub(crate) const AUTO_GAIN_RELEASE_TIME: &str = "auto_gain_release_time";
    pub(crate) const AUTO_GAIN_TARGET: &str = "auto_gain_target";
    pub(crate) const AUTOPLAY: &str = "autoplay";
    pub(crate) const BUS: &str = "bus";
    pub(crate) const CROSSFADE_TIME: &str = "crossfade_time";
    pub(crate) const CURRENT: &str = "current";
    pub(crate) const DELAY: &str = "delay";
    pub(crate) const DISTORTION_GAIN: &str = "distortion_gain";
    pub(crate) const DISTORTION_THRESHOLD: &str = "distortion_threshold";
    pub(crate) const DOPPLER_LEVEL: &str = "doppler_level";
    pub(crate) const EAR_DISTANCE: &str = "ear_distance";
    pub(crate) const END_TIME: &str = "end_time";
    pub(crate) const FADE_IN_TIME: &str = "fade_in_time";
    pub(crate) const FADE_OUT_TIME: &str = "fade_out_time";
    pub(crate) const FILE: &str = "file";
    pub(crate) const HIGH_PASS_HZ: &str = "high_pass_hz";
    pub(crate) const HIGH_PASS_Q: &str = "high_pass_q";
    pub(crate) const LAYERS: &str = "layers";
    pub(crate) const LOOP: &str = "loop";
    pub(crate) const LOOP_OFFSET: &str = "loop_offset";
    pub(crate) const LOW_PASS_HZ: &str = "low_pass_hz";
    pub(crate) const LOW_PASS_Q: &str = "low_pass_q";
    pub(crate) const MAX_DISTANCE: &str = "max_distance";
    pub(crate) const MAX_DOPPLER: &str = "max_doppler";
    pub(crate) const MIN_DISTANCE: &str = "min_distance";
    pub(crate) const PAN: &str = "pan";
    pub(crate) const PAUSED: &str = "paused";
    pub(crate) const PITCH_SCALE: &str = "pitch_scale";
    pub(crate) const PLAYBACK_TIME: &str = "playback_time";
    pub(crate) const POSITIONAL: &str = "positional";
    pub(crate) const QUEUE: &str = "queue";
    pub(crate) const REVERB_LEVEL: &str = "reverb_level";
    pub(crate) const REVERB_TIME: &str = "reverb_time";
    pub(crate) const SPEED_OF_SOUND: &str = "speed_of_sound";
    pub(crate) const START_TIME: &str = "start_time";
    pub(crate) const VOLUME_LINEAR: &str = "volume_linear";

    /// A sound file's import keys for its decoder, beside core's own.
    pub(crate) const GAPLESS: &str = "gapless";
    pub(crate) const SEEKABLE: &str = "seekable";

    /// A bus's limiter, in `[audio.buses]` and in `audio.bus_limit`.
    pub(crate) const LIMIT: &str = "limit";
    pub(crate) const LIMIT_ATTACK_TIME: &str = "limit_attack_time";
    pub(crate) const LIMIT_KNEE_DB: &str = "limit_knee_db";
    pub(crate) const LIMIT_RELEASE_TIME: &str = "limit_release_time";
    pub(crate) const LIMIT_THRESHOLD_DB: &str = "limit_threshold_db";
}

/// The values a `sound`'s enum keys take.
pub(crate) mod words {
    /// `attenuation`: the gain halves with every doubling of distance.
    pub(crate) const INVERSE: &str = "inverse";
    /// `attenuation`: the gain quarters with every doubling of distance.
    pub(crate) const INVERSE_SQUARE: &str = "inverse_square";
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
