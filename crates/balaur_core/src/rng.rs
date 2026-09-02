//! The engine's deterministic random stream.
//!
//! Integer-only PCG32, so the sequence is identical on every platform — which
//! is the whole point: a replay that diverges by one random number diverges
//! completely. Lives here rather than in a backend because a seeded generator
//! is an engine service, not a language feature.

/// Minimal PCG32 (Melissa O'Neill's pcg32_oneseq): integer-only, so the
/// stream is identical on every platform.
pub struct Pcg32 {
    state: u64,
}

const PCG_MULT: u64 = 6_364_136_223_846_793_005;
const PCG_INC: u64 = 1_442_695_040_888_963_407;

impl Pcg32 {
    pub const fn new(seed: u64) -> Self {
        let mut rng = Self {
            state: seed.wrapping_add(PCG_INC),
        };
        rng.next_u32();
        rng
    }

    pub const fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(PCG_MULT).wrapping_add(PCG_INC);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// The whole generator: one `u64` is the entire stream position, which
    /// is what lets a digest fold the RNG in and a replay resume mid-session.
    pub const fn state(&self) -> u64 {
        self.state
    }

    /// Resume a stream at a position [`Pcg32::state`] reported. Not a seed:
    /// this is the raw position, and it skips the seeding advance.
    pub const fn from_state(state: u64) -> Self {
        Self { state }
    }

    /// Uniform in `[0, 1)` with 53 bits of precision.
    pub fn next_f64(&mut self) -> f64 {
        let hi = u64::from(self.next_u32() >> 6); // 26 bits
        let lo = u64::from(self.next_u32() >> 5); // 27 bits
        ((hi << 27) | lo) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform integer in `[lo, hi]` (inclusive), bias negligible for game
    /// ranges (widening-multiply bound).
    pub fn next_range_i64(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        let span = (hi - lo) as u64 + 1;
        let r = (u64::from(self.next_u32()) * span) >> 32;
        // span fits in u32, so r < 2^32 and the cast cannot wrap.
        lo + i64::try_from(r).unwrap_or(i64::MAX)
    }
}

/// The engine-owned RNG stream backing `math.random` and the `rng` module.
pub struct RngState(pub Pcg32);

impl Default for RngState {
    fn default() -> Self {
        Self(Pcg32::new(0))
    }
}

/// Borrow the one engine stream for the duration of `f`.
///
/// `App::new` inserts exactly one `RngState`; every consumer — the `rng`
/// module, a backend's `math.random` override — goes through here rather than
/// pulling the resource out of the typemap itself, so the stream has one
/// owner and nobody is tempted to insert a second.
///
/// An `Engine` built by hand rather than through `App::new` has no stream, and
/// `balaur_script_rune::RuneHost::new` is `pub` and takes a raw `Engine`, so
/// that case is reachable from outside the workspace. Rather than panic, seed
/// the default stream here on first use: it is the same seed `App::new`
/// inserts, so the first draw is identical either way, and there is still
/// exactly one owner — this function.
pub fn with_rng<R>(eng: &crate::engine::Engine, f: impl FnOnce(&mut Pcg32) -> R) -> R {
    let rng = eng.try_resource::<RngState>().unwrap_or_else(|| {
        eng.insert_resource(RngState::default());
        eng.resource::<RngState>()
    });
    let mut rng = rng.borrow_mut();
    f(&mut rng.0)
}
