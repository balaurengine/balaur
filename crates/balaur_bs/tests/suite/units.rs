// A script built as units: each file compiled alone, the modules it uses
// linked in when it runs, and every answer the same as the whole file's.

use balaur_bs::runtime::{EngineKind, HostFns, Limits, Runtime, Value};
use balaur_bs::{Compiled, Modules, Options, Parsed, compile};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

fn modules(files: &[(&str, &str)]) -> Modules {
    let files: HashMap<String, String> = files
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    // Keys are `/`-separated on every OS, as a host's are: `Path::join` would
    // write a `\` on Windows.
    Modules(Rc::new(move |from: &str, name: &str| {
        let file = format!("{}.bs", name.replace("::", "/"));
        let mut dir = Some(from.rsplit_once('/').map_or("", |(d, _)| d));
        while let Some(d) = dir {
            let key = if d.is_empty() {
                file.clone()
            } else {
                format!("{d}/{file}")
            };
            if let Some(src) = files.get(&key) {
                return Some((key, src.clone()));
            }
            dir = (!d.is_empty()).then(|| d.rsplit_once('/').map_or("", |(up, _)| up));
        }
        None
    }))
}

/// `path` and every module it uses built as units and linked, as a host
/// builds a script.
fn linked(path: &str, files: &[(&str, &str)]) -> Compiled {
    let parsed = Parsed::default();
    let source = |key: &str| {
        files
            .iter()
            .find(|(k, _)| *k == key)
            .map_or_else(|| panic!("no {key}"), |(_, s)| *s)
    };
    let unit = |key: &str| {
        let options = Options {
            modules: Some(modules(files)),
            path: key.into(),
            unit: true,
            parsed: Some(parsed.clone()),
            ..Options::default()
        };
        compile(source(key), &options).unwrap_or_else(|e| panic!("{key}: {e}"))
    };
    let mut root = unit(path);
    let mut queue = root.meta.uses.clone();
    let mut linked: Vec<(String, Arc<Compiled>)> = vec![];
    while let Some(k) = queue.pop() {
        if k == path || linked.iter().any(|(l, _)| *l == k) {
            continue;
        }
        let u = unit(&k);
        queue.extend(u.meta.uses.iter().cloned());
        linked.push((k, Arc::new(u)));
    }
    root.linked = linked;
    root
}

fn whole(path: &str, files: &[(&str, &str)]) -> Compiled {
    let src = files.iter().find(|(k, _)| *k == path).unwrap().1;
    let options = Options {
        modules: Some(modules(files)),
        path: path.into(),
        ..Options::default()
    };
    compile(src, &options).unwrap_or_else(|e| panic!("{e}"))
}

fn engines() -> Vec<EngineKind> {
    vec![EngineKind::Wasmi, EngineKind::Auto]
}

fn runtime(c: &Compiled, engine: EngineKind) -> Runtime {
    let limits = Limits {
        engine,
        ..Limits::default()
    };
    Runtime::new(c, HostFns::default(), limits).unwrap_or_else(|e| panic!("{e}"))
}

const UTIL: &str = r##"
use defs;
pub struct Rect { w: float, h: float }
impl Rect {
    pub fn area() -> float { self.w * self.h }
}
pub class Bag { items: [str], total: int }
impl Bag {
    pub fn add(item: str) { self.items.push(item); self.total += item.len(); }
}
pub fn bag() -> Bag { Bag { items: [], total: 0 } }
const SCALE: float = 2.0;
fn twice(x: float) -> float { x * 2.0 }
pub fn grow(r: Rect) -> Rect { Rect { w: r.w * SCALE, h: twice(r.h) } }
pub fn label(n: int) -> str { "#{n} {defs::NAME}" }
pub fn pick(f: fn(int) -> int, x: int) -> int { f(x) }
pub fn first(xs: [str]) -> str? { if xs.len() > 0 { xs[0] } else { nil } }
pub fn bump(n: &mut int) { n += 1; }
pub fn table() -> any { map().with("a", 1).with("b", [1, 2, 3]) }
pub fn halve() -> fn(int) -> int { half }
fn half(n: int) -> int { n / 2 }
"##;

const DEFS: &str = r#"const NAME: str = "defs";
pub fn shout(s: str) -> str { s.to_upper() }"#;

const MAIN: &str = r#"
use util;
use defs;
pub struct Main { n: int }
fn triple(x: int) -> int { x * 3 }
pub fn area() -> float {
    let r = util::grow(util::Rect { w: 1.0, h: 2.0 });
    r.area()
}
pub fn label() -> str { "{util::label(5)} {defs::NAME} {defs::shout("x")}" }
pub fn picked() -> int { util::pick(triple, 4) }
pub fn first() -> str { util::first(["a"]).or("none") }
pub fn bag() -> int {
    let b = util::bag();
    b.add("one");
    b.add("three");
    b.total + b.items.len()
}
pub fn bumped() -> int { let n = 1; util::bump(n); util::bump(n); n }
pub fn table() -> int { let t = util::table(); t.get("a").as_int() + t.get("b").len() }
pub fn halved() -> int { let f = util::halve(); f(10) }
"#;

const FILES: [(&str, &str); 3] = [
    ("scripts/main.bs", MAIN),
    ("scripts/util.bs", UTIL),
    ("scripts/defs.bs", DEFS),
];

#[test]
fn units_answer_as_the_whole_file_does() {
    let units = linked("scripts/main.bs", &FILES);
    assert!(units.meta.unit.is_some());
    assert_eq!(units.linked.len(), 2);
    let all = whole("scripts/main.bs", &FILES);
    for engine in engines() {
        let mut a = runtime(&units, engine);
        let mut b = runtime(&all, engine);
        for f in [
            "area", "label", "picked", "first", "bag", "bumped", "table", "halved",
        ] {
            let got = a.call(f, &[]).unwrap_or_else(|e| panic!("{f}: {e}"));
            assert_eq!(Ok(got), b.call(f, &[]), "{f} on {engine:?}");
        }
        assert_eq!(a.live(), 0, "every unit's allocations are let go of");
    }
}

#[test]
fn a_unit_carries_only_its_own_functions() {
    let units = linked("scripts/main.bs", &FILES);
    let all = whole("scripts/main.bs", &FILES);
    assert!(
        units.wasm.len() < all.wasm.len(),
        "{} against {}",
        units.wasm.len(),
        all.wasm.len()
    );
    // The module's own unit is shared: built once, whoever uses it.
    let other = linked("scripts/main.bs", &FILES);
    let util = |c: &Compiled| {
        c.linked
            .iter()
            .find(|(k, _)| k == "scripts/util.bs")
            .map(|(_, u)| u.wasm.clone())
            .unwrap()
    };
    assert_eq!(util(&units), util(&other));
}

#[test]
fn units_may_use_each_other_and_the_file_that_uses_them() {
    let main = "use a;\nuse gamend::core;\npub struct S { n: int }\n\
                pub fn bump(n: int) -> int { n + 1 }\n\
                pub fn run() -> int { a::up(0) + core::given() }";
    let files = [
        ("scripts/main.bs", main),
        (
            "scripts/a.bs",
            "use b;\npub fn up(n: int) -> int { if n > 3 { n } else { b::up(n + 1) } }",
        ),
        (
            "scripts/b.bs",
            "use a;\nuse main;\npub fn up(n: int) -> int { a::up(main::bump(n)) }",
        ),
        ("scripts/gamend/core.bs", "pub fn given() -> int { 40 }"),
    ];
    let units = linked("scripts/main.bs", &files);
    for engine in engines() {
        let mut rt = runtime(&units, engine);
        assert_eq!(rt.call("run", &[]), Ok(Value::Int(44)), "{engine:?}");
    }
}

const WAITS: &str = r"
extern async fn wait(seconds: float);
pub async fn slow(n: int) -> int {
    await wait(0.5);
    n * 2
}
pub struct Counter { n: int }
impl Counter {
    pub async fn count(to: int) -> int {
        while self.n < to {
            await wait(0.25);
            self.n += 1;
        }
        self.n
    }
}
pub async fn counted(to: int) -> int {
    let n = 1;
    while n < to {
        let more = await slow(1);
        n += more / 2;
    }
    n
}
";

const WAITER: &str = r"
use waits;
pub async fn run() -> int {
    let a = await waits::slow(4);
    let b = await waits::counted(3);
    a + b
}
";

#[test]
fn an_async_function_of_another_unit_is_awaited() {
    let files = [("scripts/run.bs", WAITER), ("scripts/waits.bs", WAITS)];
    let units = linked("scripts/run.bs", &files);
    for engine in engines() {
        let host = HostFns::default().on_async("wait", |args| {
            let Value::Float(s) = args[0] else { panic!() };
            (s, Value::Void)
        });
        let limits = Limits {
            engine,
            ..Limits::default()
        };
        let mut rt = Runtime::new(&units, host, limits).unwrap_or_else(|e| panic!("{e}"));
        let mut task = rt.start("run", &[]).unwrap();
        let mut out = None;
        for _ in 0..1000 {
            if let Some(v) = rt.poll(&mut task, None).unwrap() {
                out = Some(v);
                break;
            }
            rt.advance(0.125);
        }
        assert_eq!(out, Some(Value::Int(11)), "{engine:?}");
        assert_eq!(rt.live(), 0);
    }
}

#[test]
fn a_unit_is_made_only_when_it_is_first_called() {
    let main = "use never;\npub fn run() -> int { 1 }\npub fn other() -> int { never::boom() }";
    let files = [
        ("scripts/main.bs", main),
        (
            "scripts/never.bs",
            "pub fn boom() -> int { let xs: [int] = []; xs[3] }",
        ),
    ];
    let units = linked("scripts/main.bs", &files);
    for engine in engines() {
        let mut rt = runtime(&units, engine);
        assert_eq!(rt.call("run", &[]), Ok(Value::Int(1)));
        let e = rt.call("other", &[]).unwrap_err();
        assert!(!e.msg.is_empty(), "{engine:?}");
        // A trap in another unit leaves the script running.
        assert_eq!(rt.call("run", &[]), Ok(Value::Int(1)));
    }
}

#[test]
fn what_a_file_declares_is_hashed_without_its_bodies() {
    let parsed = Parsed::default();
    let a = parsed
        .declarations("a.bs", "pub fn f() -> int { 1 }")
        .unwrap();
    let b = parsed
        .declarations("a.bs", "\n\npub fn f() -> int { 2 + 3 }")
        .unwrap();
    assert_eq!(a, b, "a body or a moved line changes no caller");
    let c = parsed
        .declarations("a.bs", "pub fn f() -> float { 1.0 }")
        .unwrap();
    assert_ne!(a, c);
    assert_eq!(parsed.known("a.bs"), Some(c));
}

#[test]
fn a_unit_reads_a_deeper_module_only_for_the_types_it_meets() {
    let main = "use shapes;\npub struct M { n: int }\n\
                pub fn width() -> float { let s = shapes::square(3.0); s.size.w + s.area() }";
    let files = [
        ("scripts/main.bs", main),
        (
            "scripts/shapes.bs",
            "use geo;\nuse unused;\npub struct Square { size: geo::Size }\n\
             impl Square { pub fn area() -> float { geo::area(self.size) } }\n\
             pub fn square(w: float) -> Square { Square { size: geo::Size { w: w, h: w } } }\n\
             pub fn noise() -> int { unused::n() }",
        ),
        (
            "scripts/geo.bs",
            "pub struct Size { w: float, h: float }\npub fn area(s: Size) -> float { s.w * s.h }",
        ),
        ("scripts/unused.bs", "pub fn n() -> int { 7 }"),
    ];
    let units = linked("scripts/main.bs", &files);
    // `geo` for the type `shapes` hands back; `unused` only through `shapes`'s own unit.
    assert_eq!(units.meta.uses, ["scripts/shapes.bs", "scripts/geo.bs"]);
    assert_eq!(units.linked.len(), 3);
    for engine in engines() {
        let mut rt = runtime(&units, engine);
        assert_eq!(rt.call("width", &[]), Ok(Value::Float(12.0)), "{engine:?}");
    }
}

#[test]
fn a_unit_kept_as_bytes_runs_as_it_was_built() {
    let mut units = linked("scripts/main.bs", &FILES);
    let back = Compiled::from_bytes(&units.to_bytes()).expect("reads back");
    assert_eq!(back.wasm, units.wasm);
    assert_eq!(back.meta.uses, units.meta.uses);
    assert_eq!(back.meta.fns.len(), units.meta.fns.len());
    let linked = std::mem::take(&mut units.linked);
    let mut back = back;
    back.linked = linked
        .iter()
        .map(|(k, u)| {
            (
                k.clone(),
                Arc::new(Compiled::from_bytes(&u.to_bytes()).unwrap()),
            )
        })
        .collect();
    let mut rt = runtime(&back, EngineKind::Auto);
    assert_eq!(rt.call("bag", &[]), Ok(Value::Int(10)));
    assert_eq!(rt.call("halved", &[]), Ok(Value::Int(5)));
    assert!(
        Compiled::from_bytes(&back.wasm).is_none(),
        "not bytes it wrote"
    );
}
