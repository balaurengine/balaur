//! The `math` script module: transcendental functions through pure-Rust
//! `libm`, so a game computes the same bits on every platform, plus the
//! everyday helpers a script reaches for.
//!
//! A language's own float methods call the platform libm and differ across
//! operating systems; `docs/DETERMINISM.md` says which to avoid.

// Every declaration shares one signature so they can sit in a table of
// function pointers; several of them have nothing to fail at.
#![allow(clippy::unnecessary_wraps)]

use anyhow::Result;
use balaur_script::{Bindings, Value};

use crate::engine::Engine;
use crate::engine_api::{number, EngineOp};

pub const MATH_OPS: &[EngineOp] = &[
    EngineOp {
        module: "math",
        name: "sin",
        call: sin,
    },
    EngineOp {
        module: "math",
        name: "cos",
        call: cos,
    },
    EngineOp {
        module: "math",
        name: "tan",
        call: tan,
    },
    EngineOp {
        module: "math",
        name: "asin",
        call: asin,
    },
    EngineOp {
        module: "math",
        name: "acos",
        call: acos,
    },
    EngineOp {
        module: "math",
        name: "atan",
        call: atan,
    },
    EngineOp {
        module: "math",
        name: "atan2",
        call: atan2,
    },
    EngineOp {
        module: "math",
        name: "sinh",
        call: sinh,
    },
    EngineOp {
        module: "math",
        name: "cosh",
        call: cosh,
    },
    EngineOp {
        module: "math",
        name: "tanh",
        call: tanh,
    },
    EngineOp {
        module: "math",
        name: "exp",
        call: exp,
    },
    EngineOp {
        module: "math",
        name: "log",
        call: log,
    },
    EngineOp {
        module: "math",
        name: "log10",
        call: log10,
    },
    EngineOp {
        module: "math",
        name: "pow",
        call: pow,
    },
    EngineOp {
        module: "math",
        name: "sqrt",
        call: sqrt,
    },
    EngineOp {
        module: "math",
        name: "abs",
        call: abs,
    },
    EngineOp {
        module: "math",
        name: "floor",
        call: floor,
    },
    EngineOp {
        module: "math",
        name: "ceil",
        call: ceil,
    },
    EngineOp {
        module: "math",
        name: "round",
        call: round,
    },
    EngineOp {
        module: "math",
        name: "min",
        call: min,
    },
    EngineOp {
        module: "math",
        name: "max",
        call: max,
    },
    EngineOp {
        module: "math",
        name: "clamp",
        call: clamp,
    },
    EngineOp {
        module: "math",
        name: "rad",
        call: rad,
    },
    EngineOp {
        module: "math",
        name: "deg",
        call: deg,
    },
];

/// Declare the module's functions and its constants.
pub fn install_math_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Deterministic float maths, backed by pure-Rust `libm`: the same \
         inputs give the same bits on every platform. A script uses these \
         rather than the language's own float methods, which reach for the \
         platform's libm and drift between machines.",
    );
    m.describe(&[
        ("sin", &[], "(angle: float)", "The sine of an angle in radians."),
        ("cos", &[], "(angle: float)", "The cosine of an angle in radians."),
        ("tan", &[], "(angle: float)", "The tangent of an angle in radians."),
        ("asin", &[], "(x: float)", "The angle in radians whose sine is the argument, from -PI/2 to PI/2; NaN outside -1..1."),
        ("acos", &[], "(x: float)", "The angle in radians whose cosine is the argument, from 0 to PI; NaN outside -1..1."),
        ("atan", &[], "(y: float, x: float?)", "The angle in radians whose tangent is the argument; with two arguments, the y-then-x form instead."),
        ("atan2", &[], "(y: float, x: float)", "The angle in radians from the positive x axis to the point, taking y first and x second."),
        ("sinh", &[], "(x: float)", "The hyperbolic sine."),
        ("cosh", &[], "(x: float)", "The hyperbolic cosine."),
        ("tanh", &[], "(x: float)", "The hyperbolic tangent, between -1 and 1."),
        ("exp", &[], "(power: float)", "e raised to the given power."),
        ("log", &[], "(x: float, base: float?)", "The natural logarithm, or the logarithm in the base given as a second argument."),
        ("log10", &[], "(x: float)", "The base-10 logarithm; -INF at zero and NaN below it."),
        ("pow", &[], "(base: float, exponent: float)", "The first argument raised to the power of the second."),
        ("sqrt", &[], "(x: float)", "The square root; NaN for a negative argument."),
        ("abs", &[], "(x: float)", "The magnitude, with the sign dropped."),
        ("floor", &[], "(x: float)", "The largest whole number at or below the argument, still a float."),
        ("ceil", &[], "(x: float)", "The smallest whole number at or above the argument, still a float."),
        ("round", &[], "(x: float)", "The nearest whole number, with halfway cases going away from zero."),
        ("min", &[], "(a: float, b: float)", "The smaller of two numbers."),
        ("max", &[], "(a: float, b: float)", "The larger of two numbers."),
        ("clamp", &[], "(x: float, low: float, high: float)", "The first argument held inside the range given by the second (low) and third (high)."),
        ("rad", &[], "(degrees: float)", "Degrees converted to radians."),
        ("deg", &[], "(radians: float)", "Radians converted to degrees."),
    ]);
    for d in MATH_OPS {
        m.function_raw(d.name, Box::new(d.call));
    }
    m.constant("PI", Value::Num(std::f64::consts::PI));
    m.constant("TAU", Value::Num(std::f64::consts::TAU));
    m.constant("INF", Value::Num(f64::INFINITY));
}

macro_rules! unary {
    ($($name:ident => $f:expr),* $(,)?) => { $(
        fn $name(_: &Engine, args: &[Value]) -> Result<Value> {
            Ok(Value::Num($f(number(args, 0)?)))
        }
    )* };
}

unary! {
    sin => libm::sin,
    cos => libm::cos,
    tan => libm::tan,
    asin => libm::asin,
    acos => libm::acos,
    atan2_of => libm::atan,
    sinh => libm::sinh,
    cosh => libm::cosh,
    tanh => libm::tanh,
    exp => libm::exp,
    log10 => libm::log10,
    sqrt => libm::sqrt,
    abs => libm::fabs,
    floor => libm::floor,
    ceil => libm::ceil,
    round => libm::round,
    rad => f64::to_radians,
    deg => f64::to_degrees,
}

/// `atan(y)`, or `atan(y, x)` for the two-argument form.
fn atan(eng: &Engine, args: &[Value]) -> Result<Value> {
    if args.len() >= 2 {
        return atan2(eng, args);
    }
    atan2_of(eng, args)
}

fn atan2(_: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Num(libm::atan2(number(args, 0)?, number(args, 1)?)))
}

/// `log(x)` is the natural log; `log(x, base)` any other.
fn log(_: &Engine, args: &[Value]) -> Result<Value> {
    let x = libm::log(number(args, 0)?);
    Ok(Value::Num(match args.get(1) {
        Some(Value::Nil) | None => x,
        Some(_) => x / libm::log(number(args, 1)?),
    }))
}

fn pow(_: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Num(libm::pow(number(args, 0)?, number(args, 1)?)))
}

fn min(_: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Num(number(args, 0)?.min(number(args, 1)?)))
}

fn max(_: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Num(number(args, 0)?.max(number(args, 1)?)))
}

fn clamp(_: &Engine, args: &[Value]) -> Result<Value> {
    let (x, lo, hi) = (number(args, 0)?, number(args, 1)?, number(args, 2)?);
    Ok(Value::Num(x.max(lo).min(hi)))
}
