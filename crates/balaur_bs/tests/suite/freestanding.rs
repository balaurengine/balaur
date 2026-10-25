// The C a native build compiles includes no header and calls no library: its
// object needs no symbol from outside, and it answers every call as wasmi does.

use balaur_bs::runtime::{EngineKind, HostFns, Limits, Runtime, Value, load_native};
use balaur_bs::{Compiled, Options, compile};

const SRC: &str = r#"
pub fn floors(x: float) -> float { floor(x) }
pub fn ceils(x: float) -> float { ceil(x) }
pub fn root(x: float) -> float { sqrt(x) }
pub fn divide(a: int, b: int) -> int { a / b }
pub fn index(i: int) -> int { let xs = [1, 2, 3]; xs[i] }
pub fn grow(n: int) -> int { let xs = [0; n]; xs[n - 1] = 7; xs.len() + xs[n - 1] }
pub fn deep(n: int) -> int { deep(n + 1) + 1 }
pub fn text(n: int) -> int { let s = "n {n} and {n * 2}"; s.len() }
"#;

// Under GCC the prelude takes setjmp from the C library, so only clang builds
// it needing nothing.
fn clang(cc: &str) -> bool {
    std::process::Command::new(cc)
        .arg("--version")
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("clang"))
}

fn build_bare(c: &Compiled) -> Option<std::path::PathBuf> {
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    if !clang(&cc) {
        return None;
    }
    let dir = std::env::temp_dir().join(format!("balaur-bare-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (source, obj, lib) = (
        dir.join("scripts.c"),
        dir.join("scripts.o"),
        dir.join("scripts.lib"),
    );
    std::fs::write(
        &source,
        balaur_bs::c::translate(&[c.wasm.as_slice()]).unwrap(),
    )
    .unwrap();
    let run = |cmd: &mut std::process::Command| {
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    run(std::process::Command::new(&cc)
        .args([
            "-O2",
            "-ffreestanding",
            "-fno-math-errno",
            "-fno-stack-protector",
            "-ffp-contract=off",
            "-fPIC",
            "-w",
            "-c",
            "-o",
        ])
        .arg(&obj)
        .arg(&source));
    // Apple's linker refuses a dylib without libSystem, so the object is what
    // shows the build needs nothing.
    let needs = run(std::process::Command::new("nm").arg("-u").arg(&obj));
    assert_eq!(needs.trim(), "", "the C calls outside itself");
    run(std::process::Command::new(&cc)
        .args(["-shared", "-o"])
        .arg(&lib)
        .arg(&obj));
    Some(lib)
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
    let c = compile(SRC, &Options::default()).unwrap();
    let Some(lib) = build_bare(&c) else {
        return;
    };
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
