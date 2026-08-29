//! Deterministic replacements for the platform-dependent parts of Luau's
//! standard library.
//!
//! Two things in stock Luau break cross-platform determinism:
//!
//! 1. Transcendental `math.*` functions call the platform libm, whose
//!    results differ across OS/libc. We overwrite them with pure-Rust
//!    implementations from the `libm` crate (MUSL's algorithms), which are
//!    bit-identical everywhere.
//! 2. `math.random` is entropy-seeded. We rebind it (and expose an explicit
//!    `rng` module) to a PCG32 stream owned by the engine, seeded to a fixed
//!    default so a fresh run is reproducible by construction.
//!
//! Overwriting the `math` table is not enough on its own: at optimization
//! level 1+ the Luau compiler turns `math.sin(x)` into a fastcall that
//! bypasses the global table entirely. [`DISABLED_BUILTINS`] must therefore
//! be fed to every compiler instance (see [`super::compiler`]); dev builds
//! and exported packs share that configuration, so the routing is the same
//! in both.

use mlua::{Lua, Table, Value, Variadic};

use crate::engine::Engine;

/// Builtins that must not be compiled to fastcalls because we replace them
/// at runtime. `sqrt`, `abs`, `floor`, `ceil`, `fmod` & co stay native: they
/// are exactly specified by IEEE-754 and identical on every platform.
pub const DISABLED_BUILTINS: &[&str] = &[
    "math.sin",
    "math.cos",
    "math.tan",
    "math.asin",
    "math.acos",
    "math.atan",
    "math.atan2",
    "math.sinh",
    "math.cosh",
    "math.tanh",
    "math.exp",
    "math.log",
    "math.log10",
    "math.pow",
];

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
pub struct DetRng(pub Pcg32);

impl Default for DetRng {
    fn default() -> Self {
        Self(Pcg32::new(0))
    }
}

// Returns Result to match the signature mlua's create_function expects.
#[allow(clippy::unnecessary_wraps)]
fn lua_random(eng: &Engine, args: &Variadic<i64>) -> mlua::Result<Value> {
    let rng = eng.resource::<DetRng>();
    let mut rng = rng.borrow_mut();
    match args.len() {
        0 => Ok(Value::Number(rng.0.next_f64())),
        1 => Ok(Value::Integer(rng.0.next_range_i64(1, args[0]))),
        _ => Ok(Value::Integer(rng.0.next_range_i64(args[0], args[1]))),
    }
}

/// Install the deterministic `math` overrides and the `rng` module.
pub fn install(lua: &Lua, engine: &Engine) -> anyhow::Result<()> {
    engine.insert_resource(DetRng::default());

    let math: Table = lua.globals().get("math")?;
    macro_rules! unary {
        ($name:literal, $f:path) => {
            math.set($name, lua.create_function(|_, x: f64| Ok($f(x)))?)?;
        };
    }
    unary!("sin", libm::sin);
    unary!("cos", libm::cos);
    unary!("tan", libm::tan);
    unary!("asin", libm::asin);
    unary!("acos", libm::acos);
    unary!("sinh", libm::sinh);
    unary!("cosh", libm::cosh);
    unary!("tanh", libm::tanh);
    unary!("exp", libm::exp);
    unary!("log10", libm::log10);
    // Lua-5.x semantics: atan(y [, x]), log(x [, base]).
    math.set(
        "atan",
        lua.create_function(|_, (y, x): (f64, Option<f64>)| {
            Ok(match x {
                Some(x) => libm::atan2(y, x),
                None => libm::atan(y),
            })
        })?,
    )?;
    math.set(
        "atan2",
        lua.create_function(|_, (y, x): (f64, f64)| Ok(libm::atan2(y, x)))?,
    )?;
    math.set(
        "log",
        lua.create_function(|_, (x, base): (f64, Option<f64>)| {
            Ok(match base {
                Some(b) => libm::log(x) / libm::log(b),
                None => libm::log(x),
            })
        })?,
    )?;
    math.set(
        "pow",
        lua.create_function(|_, (x, y): (f64, f64)| Ok(libm::pow(x, y)))?,
    )?;

    // Engine-seeded randomness. Note: `math.random` is not a compiler
    // fastcall, so rebinding the table entry is sufficient.
    let eng = engine.clone();
    math.set(
        "random",
        lua.create_function(move |_, args: Variadic<i64>| lua_random(&eng, &args))?,
    )?;
    let eng = engine.clone();
    math.set(
        "randomseed",
        lua.create_function(move |_, seed: i64| {
            let rng = eng.resource::<DetRng>();
            rng.borrow_mut().0 = Pcg32::new(seed as u64);
            Ok(())
        })?,
    )?;

    // The explicit engine RNG API, same stream as math.random.
    let m = super::env::module(lua, engine, "rng")?;
    m.function("seed", |eng, seed: i64| {
        let rng = eng.resource::<DetRng>();
        rng.borrow_mut().0 = Pcg32::new(seed as u64);
        Ok(())
    })?;
    m.function("random", |eng, ()| {
        let rng = eng.resource::<DetRng>();
        let v = rng.borrow_mut().0.next_f64();
        Ok(v)
    })?;
    m.function("range", |eng, (lo, hi): (f64, f64)| {
        let rng = eng.resource::<DetRng>();
        let v = rng.borrow_mut().0.next_f64();
        Ok(v.mul_add(hi - lo, lo))
    })?;
    m.function("int", |eng, (lo, hi): (i64, i64)| {
        let rng = eng.resource::<DetRng>();
        let v = rng.borrow_mut().0.next_range_i64(lo, hi);
        Ok(v)
    })?;
    Ok(())
}
