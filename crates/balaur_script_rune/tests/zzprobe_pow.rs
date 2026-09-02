use rune::{Context, Diagnostics, Source, Sources, Vm};
use std::sync::Arc;

fn eval(src: &str) -> f64 {
    let ctx = Context::with_default_modules().unwrap();
    let mut sources = Sources::new();
    sources.insert(Source::memory(src).unwrap()).unwrap();
    let mut d = Diagnostics::new();
    let unit = rune::prepare(&mut sources)
        .with_context(&ctx)
        .with_diagnostics(&mut d)
        .build()
        .unwrap();
    let mut vm = Vm::new(Arc::new(ctx.runtime().unwrap()), Arc::new(unit));
    rune::from_value(vm.call(["main"], ()).unwrap()).unwrap()
}

#[test]
fn rune_pow_matches_libm_bit_for_bit() {
    for (x, y) in [(2.0f64, 0.5f64), (1.7, 3.3), (10.0, -2.25), (0.3, 7.0)] {
        let got: f64 = eval(&format!("pub fn main() {{ {x:?}.powf({y:?}) }}"));
        println!(
            "PROBE powf({x}, {y}) rune={got:e} libm={:e}",
            libm::pow(x, y)
        );
        assert_eq!(got.to_bits(), libm::pow(x, y).to_bits(), "powf({x}, {y})");
    }
    for (x, n) in [(2.0f64, 3i32), (1.7, 5), (0.3, -2)] {
        let got: f64 = eval(&format!("pub fn main() {{ {x:?}.powi({n}) }}"));
        println!(
            "PROBE powi({x}, {n}) rune={got:e} libm={:e}",
            libm::pow(x, f64::from(n))
        );
        assert_eq!(
            got.to_bits(),
            libm::pow(x, f64::from(n)).to_bits(),
            "powi({x}, {n})"
        );
    }
}
