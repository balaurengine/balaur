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

use balaur_core::engine::Engine;
use balaur_core::rng::{with_rng, Pcg32};

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

// Returns Result to match the signature mlua's create_function expects.
#[allow(clippy::unnecessary_wraps)]
fn lua_random(eng: &Engine, args: &Variadic<i64>) -> mlua::Result<Value> {
    with_rng(eng, |rng| match args.len() {
        0 => Ok(Value::Number(rng.next_f64())),
        1 => Ok(Value::Integer(rng.next_range_i64(1, args[0]))),
        _ => Ok(Value::Integer(rng.next_range_i64(args[0], args[1]))),
    })
}

/// Install the deterministic `math` overrides and the `rng` module.
pub fn install(lua: &Lua, eng: &Engine) -> anyhow::Result<()> {
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
    let random_eng = eng.clone();
    math.set(
        "random",
        lua.create_function(move |_, args: Variadic<i64>| lua_random(&random_eng, &args))?,
    )?;
    let seed_eng = eng.clone();
    math.set(
        "randomseed",
        lua.create_function(move |_, seed: i64| {
            with_rng(&seed_eng, |rng| *rng = Pcg32::new(seed as u64));
            Ok(())
        })?,
    )?;

    Ok(())
}
