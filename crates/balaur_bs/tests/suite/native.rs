// Modules translated to C and compiled by this machine's own compiler, as an
// iOS export does with Xcode's: each must answer exactly what wasmi does.

use balaur_bs::runtime::{
    Command, EngineKind, HostFns, Limits, Object, Reentry, RunError, Runtime, Value, load_native,
};
use balaur_bs::{Compiled, Options, compile};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

static BUILT: AtomicUsize = AtomicUsize::new(0);

/// `modules` as one native library, loaded.
fn build(modules: &[&Compiled]) {
    let wasm: Vec<&[u8]> = modules.iter().map(|c| c.wasm.as_slice()).collect();
    let c = balaur_bs::c::translate(&wasm).unwrap_or_else(|e| panic!("{e}"));
    let dir = std::env::temp_dir().join(format!(
        "balaur-native-{}-{}",
        std::process::id(),
        BUILT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("scripts.c");
    std::fs::write(&source, &c).unwrap();
    let lib: PathBuf = dir.join("scripts.lib");
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let out = std::process::Command::new(cc)
        .args(["-O2", "-ffp-contract=off", "-fPIC", "-shared", "-w", "-o"])
        .arg(&lib)
        .arg(&source)
        .output()
        .expect("a C compiler");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(load_native(&lib), Ok(modules.len()));
}

fn on(engine: EngineKind, c: &Compiled, host: HostFns) -> Runtime {
    let limits = Limits {
        engine,
        ..Limits::default()
    };
    Runtime::new(c, host, limits).unwrap()
}

/// The same calls on wasmi and natively, with the same answers and nothing leaked.
fn same(src: &str, calls: &[(&str, &[Value])]) {
    let c = compile(src, &Options::default()).unwrap_or_else(|e| panic!("{e}"));
    build(&[&c]);
    let mut interpreted = on(EngineKind::Wasmi, &c, HostFns::default());
    let mut native = on(EngineKind::Native, &c, HostFns::default());
    for (f, args) in calls {
        let want = interpreted.call(f, args);
        let got = native.call(f, args);
        assert_eq!(got, want, "{f}");
    }
    assert_eq!(native.live(), interpreted.live(), "leaked natively");
}

const MIXED: &str = r#"
pub class Mover { x: float, steps: int, name: str }
impl Mover {
    pub fn walk(n: int) -> float {
        for i in 0..n {
            self.x += sqrt(i * 2.0) + floor(1.5) - min(3.0, i) + max(-0.0, 0.0);
            self.steps += 1;
        }
        self.name = "walked {self.steps}";
        self.x
    }
}
fn fib(n: int) -> int { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }
pub fn numbers(a: int, b: int) -> str {
    let q = a / b;
    let r = a % b;
    let f = a as float / b as float;
    "{q} {r} {f} {fib(15)} {-7 / 2} {-7 % 2} {abs(-2.5)} {ceil(1.2)}"
}
pub fn text(s: str) -> str {
    let parts = s.split(",");
    let out = "";
    for p in parts { out = out + p.trim().to_upper() + ";"; }
    out
}
pub fn arrays() -> int {
    let xs = [5, 3, 8];
    xs.push(1);
    let last = xs.pop();
    xs.insert(0, last);
    let total = 0;
    for x in xs { total += x; }
    let i = 0;
    while i < 10 { i += 3; if i > 6 { break; } }
    total * 100 + i
}
pub fn anys() -> str {
    let m = map().with("hp", 3);
    m.set("name", "imp");
    let tags: [any] = ["a"];
    let l: any = tags;
    l.push(2.5);
    m.set("tags", l);
    "{m.text()} {m["hp"].as_int()} {m.len()}"
}
pub fn walk_a_mover(n: int) -> float {
    let m = Mover { x: 0.0, steps: 0, name: "" };
    m.walk(n)
}
"#;

#[test]
fn a_module_answers_natively_what_it_answers_interpreted() {
    same(
        MIXED,
        &[
            ("numbers", &[Value::Int(17), Value::Int(5)]),
            ("numbers", &[Value::Int(-17), Value::Int(4)]),
            ("text", &[Value::Str(" a, bc ,d".into())]),
            ("arrays", &[]),
            ("anys", &[]),
            ("walk_a_mover", &[Value::Int(40)]),
        ],
    );
}

#[test]
fn a_method_hands_its_fields_back_natively() {
    let c = compile(MIXED, &Options::default()).unwrap();
    build(&[&c]);
    let mut rt = on(EngineKind::Native, &c, HostFns::default());
    let mut m = rt.object("Mover", &[]).unwrap();
    rt.call_method(&mut m, "walk", &[Value::Int(3)]).unwrap();
    let x = rt.call_method(&mut m, "walk", &[Value::Int(2)]).unwrap();
    let mut wasmi = on(EngineKind::Wasmi, &c, HostFns::default());
    let mut w = wasmi.object("Mover", &[]).unwrap();
    wasmi.call_method(&mut w, "walk", &[Value::Int(3)]).unwrap();
    assert_eq!(wasmi.call_method(&mut w, "walk", &[Value::Int(2)]), Ok(x));
    assert_eq!(rt.get(&m, "name"), Some(Value::Str("walked 5".into())));
}

const PARTICLES: &str = "
pub fn particles(steps: int) -> float {
    let n = 10000;
    let xs = [0.0; n];
    let ys = [0.0; n];
    let vxs = [0.0; n];
    let vys = [0.0; n];
    for i in 0..n {
        xs[i] = i % 100;
        ys[i] = i * 7 % 100;
        vxs[i] = i % 13 - 6;
        vys[i] = i % 11 - 5;
    }
    for s in 0..steps {
        for i in 0..n {
            let x = xs[i] + vxs[i] * 0.016;
            let y = ys[i] + vys[i] * 0.016;
            if x < 0.0 || x > 100.0 { vxs[i] = -vxs[i]; }
            if y < 0.0 || y > 100.0 { vys[i] = -vys[i]; }
            xs[i] = x;
            ys[i] = y;
        }
    }
    let t = 0.0;
    for x in xs { t += x; }
    t
}
";

#[test]
fn a_simulation_comes_out_bit_for_bit() {
    same(PARTICLES, &[("particles", &[Value::Int(30)])]);
}

fn trap(r: Result<Value, RunError>) -> String {
    r.expect_err("it trapped").msg
}

#[test]
fn a_trap_ends_the_call_and_the_module_runs_on() {
    let c = compile(
        "
        pub fn index(i: int) -> int { let xs = [1, 2]; xs[i] }
        pub fn divide(a: int, b: int) -> int { a / b }
        pub fn grow(n: int) -> int { let xs = [0; n]; xs.len() }
        ",
        &Options::default(),
    )
    .unwrap();
    build(&[&c]);
    let mut rt = on(EngineKind::Native, &c, HostFns::default());
    assert!(trap(rt.call("index", &[Value::Int(5)])).contains("unreachable"));
    assert_eq!(rt.call("index", &[Value::Int(1)]), Ok(Value::Int(2)));
    assert_eq!(
        trap(rt.call("divide", &[Value::Int(1), Value::Int(0)])),
        "integer divide by zero"
    );
    assert_eq!(
        rt.call("grow", &[Value::Int(1_000_000)]),
        Ok(Value::Int(1_000_000)),
        "memory grows"
    );
    assert_eq!(
        rt.call("divide", &[Value::Int(9), Value::Int(3)]),
        Ok(Value::Int(3))
    );
}

#[test]
fn a_module_capped_below_what_it_asks_for_fails_to_grow() {
    let c = compile(
        "pub fn grow(n: int) -> int { let xs = [0; n]; xs.len() }",
        &Options {
            memory_max: Some(4 << 20),
            ..Options::default()
        },
    )
    .unwrap();
    build(&[&c]);
    let mut rt = on(EngineKind::Native, &c, HostFns::default());
    assert!(rt.call("grow", &[Value::Int(10_000_000)]).is_err());
    assert_eq!(rt.call("grow", &[Value::Int(10)]), Ok(Value::Int(10)));
}

#[test]
fn host_functions_and_awaits_reach_the_host() {
    let src = r#"
        extern fn shout(s: str) -> str;
        extern async fn later(seconds: float) -> int;
        pub async fn count() -> str {
            let a = await later(0.5);
            let b = await later(0.25);
            shout("got {a + b}")
        }
    "#;
    let c = compile(src, &Options::default()).unwrap();
    build(&[&c]);
    let host = || {
        let next = Rc::new(RefCell::new(0));
        HostFns::default()
            .on("shout", |a| match &a[0] {
                Value::Str(s) => Value::Str(s.to_uppercase()),
                _ => Value::Void,
            })
            .on_async("later", move |_| {
                *next.borrow_mut() += 10;
                (0.5, Value::Int(*next.borrow()))
            })
    };
    let run = |engine| {
        let mut rt = on(engine, &c, host());
        let mut task = rt.start("count", &[]).unwrap();
        for _ in 0..100 {
            if let Some(v) = rt.poll(&mut task, None).unwrap() {
                return (v, rt.live());
            }
            rt.advance(0.25);
        }
        panic!("never finished")
    };
    assert_eq!(run(EngineKind::Native), run(EngineKind::Wasmi));
    assert_eq!(run(EngineKind::Native).0, Value::Str("GOT 30".into()));
}

#[test]
fn a_debug_build_stops_natively_through_its_handler() {
    let src = "pub fn sum(n: int) -> int {
    let t = 0;
    for i in 0..n {
        t += i;
    }
    t
}
";
    let c = compile(
        src,
        &Options {
            debug: true,
            ..Options::default()
        },
    )
    .unwrap();
    build(&[&c]);
    let mut rt = on(EngineKind::Native, &c, HostFns::default());
    let seen = Rc::new(RefCell::new(vec![]));
    let log = seen.clone();
    rt.debugger().set_breakpoint(4);
    rt.debugger().on_pause(move |p| {
        log.borrow_mut().push(p.frames[0].locals.clone());
        Command::Continue
    });
    assert_eq!(rt.call("sum", &[Value::Int(3)]), Ok(Value::Int(3)));
    let seen = seen.borrow();
    assert_eq!(seen.len(), 3);
    assert!(
        seen[2].contains(&("i".into(), Value::Int(2))),
        "{:?}",
        seen[2]
    );
}

#[test]
fn an_unknown_module_is_not_run_natively() {
    let c = compile("pub fn one() -> int { 1 }", &Options::default()).unwrap();
    let other = compile("pub fn two() -> int { 2 }", &Options::default()).unwrap();
    build(&[&other]);
    let limits = Limits {
        engine: EngineKind::Native,
        ..Limits::default()
    };
    assert!(Runtime::new(&c, HostFns::default(), limits).is_err());
    let mut auto = on(EngineKind::Auto, &c, HostFns::default());
    assert_eq!(auto.call("one", &[]), Ok(Value::Int(1)));
}

type Reach = Rc<RefCell<(Option<Reentry>, Option<Object>, &'static str)>>;

// A host function that calls back into the module, and a trap inside that
// inner call: each unwinds only as far as its own entry.
#[test]
fn a_call_back_in_traps_on_its_own() {
    use balaur_bs::{HostApi, HostSignature, Ty};
    let src = "pub class Counter { total: int }
impl Counter {
    pub fn outer(amount: int) -> int { other::poke(amount) + self.total }
    pub fn add(amount: int) -> int { self.total += amount; self.total }
    pub fn crash(index: int) -> int { let one = [1]; one[index] }
}
";
    let api = HostApi {
        functions: vec![HostSignature {
            key: "other::poke".into(),
            params: vec![Ty::Int],
            ret: Ty::Int,
            is_async: false,
        }],
        components: vec![],
        constants: vec![],
        structs: vec![],
    };
    let c = compile(
        src,
        &Options {
            host: api,
            ..Options::default()
        },
    )
    .unwrap();
    build(&[&c]);
    let reach: Reach = Rc::new(RefCell::new((None, None, "add")));
    let r = reach.clone();
    let host = HostFns::default().engine(move |_, args, _| {
        let (way, obj, method) = {
            let mut s = r.borrow_mut();
            (s.0.clone(), s.1.take(), s.2)
        };
        let (Some(way), Some(mut obj)) = (way, obj) else {
            return Err("nothing to reach".into());
        };
        let out = way.call_method(&mut obj, method, args);
        r.borrow_mut().1 = Some(obj);
        out.map_err(|e| e.to_string())
    });
    let mut rt = on(EngineKind::Native, &c, host);
    let mut me = rt.object("Counter", &[]).unwrap();
    let theirs = rt.object("Counter", &[]).unwrap();
    *reach.borrow_mut() = (Some(rt.reentry()), Some(theirs), "add");
    let outer =
        |rt: &mut Runtime, me: &mut Object, k| rt.call_method(me, "outer", &[Value::Int(k)]);
    assert_eq!(outer(&mut rt, &mut me, 3), Ok(Value::Int(3)));
    assert_eq!(outer(&mut rt, &mut me, 4), Ok(Value::Int(7)));
    reach.borrow_mut().2 = "crash";
    assert!(trap(outer(&mut rt, &mut me, 9)).contains("unreachable"));
    reach.borrow_mut().2 = "add";
    assert_eq!(outer(&mut rt, &mut me, 1), Ok(Value::Int(8)));
}

#[test]
fn a_fn_value_calls_through_the_table_natively() {
    same(
        super::fnrefs::SRC,
        &[
            ("twice", &[Value::Int(5)]),
            ("table", &[Value::Int(3)]),
            ("longest", &[]),
            ("picked", &[Value::Int(6)]),
        ],
    );
}

#[test]
fn a_class_object_is_shared_natively_too() {
    same(
        r#"
pub class Bag { n: int, items: [str] }
fn fill(b: Bag, k: int) { b.n += k; b.items.push("{k}"); }
pub fn count(k: int) -> str {
    let b = Bag { n: 0, items: [] };
    let alias = b;
    for i in 0..k { fill(alias, i); }
    "{b.n} {b.items.len()}"
}
"#,
        &[("count", &[Value::Int(5)])],
    );
}
