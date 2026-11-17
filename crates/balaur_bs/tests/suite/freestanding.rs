// The C a native build compiles includes no header and calls no library: its
// object needs no symbol from outside, and it answers every call as wasmi does.

use crate::programs::corpus::native::{kept, run, version};
use balaur_bs::runtime::{EngineKind, HostFns, Limits, Runtime, Value, load_native};
use balaur_bs::{Compiled, Options, compile};
use std::path::PathBuf;
use std::process::Command;

const SRC: &str = r#"
pub fn floors(x: float) -> float { math::floor(x) }
pub fn ceils(x: float) -> float { math::ceil(x) }
pub fn root(x: float) -> float { math::sqrt(x) }
pub fn divide(a: int, b: int) -> int { a / b }
pub fn index(i: int) -> int { let xs = [1, 2, 3]; xs[i] }
pub fn grow(n: int) -> int { let xs = [0; n]; xs[n - 1] = 7; xs.len() + xs[n - 1] }
pub fn deep(n: int) -> int { deep(n + 1) + 1 }
pub fn text(n: int) -> int { let s = "n {n} and {n * 2}"; s.len() }
pub fn bits(a: int, b: int) -> int { a & b ^ (a | ~b) ^ a << b ^ a >> b }
"#;

// Under GCC the prelude takes setjmp from the C library, so only clang builds
// it needing nothing: `clang` by name, else `$CC` or `cc` when it is clang.
fn clang() -> Option<(String, String)> {
    let named = std::env::var("CC").into_iter().chain(["cc".to_string()]);
    std::iter::once("clang".to_string())
        .chain(named)
        .find_map(|cc| Some((cc.clone(), version(&cc).filter(|v| v.contains("clang"))?)))
}

const BARE: [&str; 8] = [
    "-O2",
    "-ffreestanding",
    "-fno-math-errno",
    "-fno-stack-protector",
    "-ffp-contract=off",
    "-fPIC",
    "-w",
    "-c",
];

fn build_bare(c: &Compiled, cc: &str, version: &str) -> PathBuf {
    let source = balaur_bs::c::translate(&[c.wasm.as_slice()]).unwrap();
    let obj = kept(&[cc, version, &BARE.join(" "), &source], "o", |out| {
        let file = out.with_extension("c");
        std::fs::write(&file, &source).map_err(|e| e.to_string())?;
        let built = run(Command::new(cc).args(BARE).arg("-o").arg(out).arg(&file));
        let _ = std::fs::remove_file(&file);
        built.map(|_| ())
    })
    .unwrap();
    // Apple's linker refuses a dylib without libSystem, so the object is what
    // shows the build needs nothing.
    let needs = run(Command::new("nm").arg("-u").arg(&obj)).unwrap();
    assert_eq!(needs.trim(), "", "the C calls outside itself");
    let named = obj.display().to_string();
    kept(&[cc, version, "-shared", &named], "lib", |out| {
        run(Command::new(cc).arg("-shared").arg("-o").arg(out).arg(&obj)).map(|_| ())
    })
    .unwrap()
}

fn bits(r: &Result<Value, balaur_bs::runtime::RunError>) -> Option<u64> {
    match r {
        Ok(Value::Float(x)) => Some(x.to_bits()),
        Ok(Value::Int(x)) => Some(x.cast_unsigned()),
        _ => None,
    }
}

#[test]
fn a_build_linked_against_nothing_answers_as_wasmi_does() {
    let Some((cc, version)) = clang() else {
        eprintln!("no clang here; the bare build is not exercised");
        return;
    };
    let c = compile(SRC, &Options::default()).unwrap();
    let lib = build_bare(&c, &cc, &version);
    assert_eq!(load_native(&lib), Ok(1));
    let on = |engine| {
        let limits = Limits {
            engine,
            memory_bytes: Some(64 << 20),
            ..Limits::default()
        };
        Runtime::new(&c, HostFns::default(), limits).unwrap()
    };
    let (mut native, mut wasmi) = (on(EngineKind::Native), on(EngineKind::Wasmi));
    let mut calls: Vec<(&str, Vec<Value>)> = vec![
        ("divide", vec![Value::Int(7), Value::Int(-2)]),
        ("divide", vec![Value::Int(1), Value::Int(0)]),
        ("index", vec![Value::Int(3)]),
        ("grow", vec![Value::Int(300_000)]),
        ("deep", vec![Value::Int(0)]),
        ("text", vec![Value::Int(-42)]),
    ];
    // A count past 63 or below 0 is where C, unmasked, would be undefined.
    for count in [0, 1, 63, 64, 65, -1, -64, 200, i64::MIN] {
        for value in [1, -1, i64::MIN, 0x0123_4567_89ab_cdef] {
            calls.push(("bits", vec![Value::Int(value), Value::Int(count)]));
        }
    }
    for x in [
        -0.0,
        0.5,
        -0.5,
        2.5,
        -2.5,
        4_503_599_627_370_495.5,
        -1e-300,
        f64::NAN,
    ] {
        for f in ["floors", "ceils", "root"] {
            calls.push((f, vec![Value::Float(x)]));
        }
    }
    let mut trapped = 0;
    for (f, args) in &calls {
        let (n, w) = (native.call(f, args), wasmi.call(f, args));
        assert_eq!(
            bits(&n),
            bits(&w),
            "{f}({args:?}): native {n:?}, wasmi {w:?}"
        );
        assert_eq!(n.is_err(), w.is_err(), "{f}({args:?})");
        trapped += usize::from(n.is_err());
    }
    // Dividing by zero, the index past the end and the endless recursion.
    assert_eq!(trapped, 3);
    assert_eq!(native.live(), wasmi.live());
}
