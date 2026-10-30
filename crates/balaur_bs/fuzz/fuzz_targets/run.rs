//! Any program that compiles: calling each of its public functions, under the
//! fuel and memory limits, traps or answers and never brings the host down.
#![no_main]

use balaur_bs::runtime::{EngineKind, HostFns, Limits, Runtime, Value};
use balaur_bs::{Options, Ty, compile};
use libfuzzer_sys::fuzz_target;

fn zero(ty: &Ty) -> Option<Value> {
    Some(match ty {
        Ty::Int => Value::Int(0),
        Ty::Float => Value::Float(0.0),
        Ty::Bool => Value::Bool(false),
        Ty::Str => Value::Str(String::new()),
        Ty::Node => Value::Node(0),
        Ty::Void | Ty::Any | Ty::Optional(_) => Value::Void,
        Ty::Array(_) => Value::Array(Vec::new()),
        Ty::Map(..) => Value::Map(Vec::new()),
        Ty::Struct(_) | Ty::Fn(..) => return None,
    })
}

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(c) = compile(src, &Options::default()) else {
        return;
    };
    let limits = Limits {
        fuel: Some(1_000_000),
        memory_bytes: Some(64 << 20),
        engine: EngineKind::Wasmi,
    };
    let Ok(mut rt) = Runtime::new(&c, HostFns::default(), limits) else {
        return;
    };
    for f in &c.meta.fns {
        if !f.public || f.is_async || f.owner.is_some() {
            continue;
        }
        let Some(args) = f.params.iter().map(zero).collect::<Option<Vec<_>>>() else {
            continue;
        };
        let _ = rt.call(&f.key, &args);
    }
});
