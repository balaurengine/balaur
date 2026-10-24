// A NaN's sign and payload are the hardware's, so every float a module hands
// the host is the canonical NaN, whichever engine ran it and however it left.

use balaur_bs::runtime::{EngineKind, HostFns, Limits, Runtime, Value};
use balaur_bs::{Compiled, Options, compile};
use std::cell::RefCell;
use std::rc::Rc;

const CANONICAL: u64 = 0x7ff8_0000_0000_0000;
// The sign set and a payload: what x86 makes, and what ARM carries through.
const ODD: u64 = 0xfff8_0000_0000_0001;

const SRC: &str = r"
extern fn take(x: float);
pub struct Holder { x: float }
impl Holder {
    pub fn fill(v: float) { self.x = v + 1.0; }
}
pub fn made() -> float { -(0.0 / 0.0) }
pub fn passed(v: float) -> float { v * 2.0 }
pub fn listed(v: float) -> [float] { [v + 1.0, made()] }
pub fn boxed(v: float) -> any { let a: any = v - 1.0; a }
pub fn unboxed(v: float) -> float { let a: any = v - 1.0; a.as_float() }
pub fn handed(v: float) { take(v / 3.0); }
";

fn floats(v: &Value, out: &mut Vec<u64>) {
    match v {
        Value::Float(x) => out.push(x.to_bits()),
        Value::Array(items) => items.iter().for_each(|i| floats(i, out)),
        _ => {}
    }
}

/// The bits of every float that leaves the module, one way out each.
fn handed_out(engine: EngineKind, c: &Compiled) -> Vec<u64> {
    let taken = Rc::new(RefCell::new(vec![]));
    let t = taken.clone();
    let host = HostFns::default().on("take", move |a| {
        floats(&a[0], &mut t.borrow_mut());
        Value::Void
    });
    let limits = Limits {
        engine,
        ..Limits::default()
    };
    let mut rt = Runtime::new(c, host, limits).unwrap();
    let odd = Value::Float(f64::from_bits(ODD));
    let mut out = vec![];
    for (f, args) in [
        ("made", vec![]),
        ("passed", vec![odd.clone()]),
        ("listed", vec![odd.clone()]),
        ("boxed", vec![odd.clone()]),
        ("unboxed", vec![odd.clone()]),
        ("handed", vec![odd.clone()]),
    ] {
        floats(&rt.call(f, &args).unwrap(), &mut out);
    }
    let mut h = rt.object("Holder", &[]).unwrap();
    rt.call_method(&mut h, "fill", &[odd]).unwrap();
    floats(&rt.get(&h, "x").unwrap(), &mut out);
    out.extend(taken.borrow().iter());
    assert_eq!(rt.live(), 0);
    out
}

fn every_nan_is_canonical(engine: EngineKind, c: &Compiled) {
    let out = handed_out(engine, c);
    assert_eq!(out.len(), 8, "{engine:?}: {out:x?}");
    assert!(out.iter().all(|&b| b == CANONICAL), "{engine:?}: {out:x?}");
}

#[test]
fn a_nan_leaves_a_module_as_the_canonical_nan_on_every_engine() {
    for direct in [false, true] {
        let options = Options {
            direct_imports: direct,
            ..Options::default()
        };
        let c = compile(SRC, &options).unwrap();
        every_nan_is_canonical(EngineKind::Wasmi, &c);
        #[cfg(jit)]
        every_nan_is_canonical(EngineKind::Wasmtime, &c);
    }
}

#[cfg(all(feature = "c", feature = "native"))]
#[test]
fn a_nan_leaves_a_native_build_as_the_canonical_nan() {
    use balaur_bs::runtime::load_native;
    let c = compile(SRC, &Options::default()).unwrap();
    let src = balaur_bs::c::translate(&[c.wasm.as_slice()]).unwrap();
    let dir = std::env::temp_dir().join(format!("balaur-nans-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("scripts.c"), src).unwrap();
    let lib = dir.join("scripts.lib");
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let out = std::process::Command::new(cc)
        .args(["-O2", "-ffp-contract=off", "-fPIC", "-shared", "-w", "-o"])
        .arg(&lib)
        .arg(dir.join("scripts.c"))
        .output()
        .expect("a C compiler");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(load_native(&lib), Ok(1));
    every_nan_is_canonical(EngineKind::Native, &c);
}

#[test]
fn a_float_that_is_not_a_nan_keeps_its_bits() {
    let mut rt = super::run("pub fn passed(v: float) -> float { v * 2.0 }");
    for x in [-0.0, 1.5, f64::INFINITY, f64::MIN_POSITIVE / 4.0] {
        let Value::Float(got) = rt.call("passed", &[Value::Float(x)]).unwrap() else {
            panic!()
        };
        assert_eq!(got.to_bits(), (x * 2.0).to_bits());
    }
}
