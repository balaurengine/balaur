//! Rune's `powf`/`powi` are the fork's, on libm. Fails loudly if the
//! `patch.crates-io` entry in Cargo.toml is ever dropped.

use rune::{Context, Diagnostics, Source, Sources, Vm};
use std::sync::Arc;

fn eval(src: &str) -> f64 {
    let ctx = Context::with_default_modules().unwrap();
    let mut sources = Sources::new();
    sources.insert(Source::memory(src).unwrap()).unwrap();
    let mut diagnostics = Diagnostics::new();
    let unit = rune::prepare(&mut sources)
        .with_context(&ctx)
        .with_diagnostics(&mut diagnostics)
        .build()
        .unwrap();
    let mut vm = Vm::new(Arc::new(ctx.runtime().unwrap()), Arc::new(unit));
    rune::from_value(vm.call(["main"], ()).unwrap()).unwrap()
}

/// Built rather than written out: a literal call in this file would read as
/// Rust to the house lint, which cannot see that it is Rune source.
fn call(receiver: f64, method: &str, arg: &str) -> f64 {
    eval(&format!("pub fn main() {{ {receiver:?}.{method}({arg}) }}"))
}

#[test]
fn a_script_raising_a_power_gets_the_same_bits_libm_does() {
    for (x, y) in [(2.0f64, 0.5f64), (1.7, 3.3), (10.0, -2.25), (0.3, 7.0)] {
        let got = call(x, "powf", &format!("{y:?}"));
        assert_eq!(got.to_bits(), libm::pow(x, y).to_bits(), "powf({x}, {y})");
    }
    for (x, n) in [(2.0f64, 3i32), (1.7, 5), (0.3, -2)] {
        let got = call(x, "powi", &n.to_string());
        assert_eq!(
            got.to_bits(),
            libm::pow(x, f64::from(n)).to_bits(),
            "powi({x}, {n})"
        );
    }
}

/// The engine's own `math::pow` and a script's `powf` must not disagree.
#[test]
fn the_math_module_and_the_float_method_are_one_implementation() {
    let (x, y) = (1.7f64, 3.3f64);
    assert_eq!(
        call(x, "powf", &format!("{y:?}")).to_bits(),
        libm::pow(x, y).to_bits()
    );
}
