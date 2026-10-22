use balaur_bs::runtime::{HostFns, Limits, Runtime, Value};
use balaur_bs::{Modules, Options, compile};
use std::collections::HashMap;
use std::rc::Rc;

const UTIL: &str = r##"
use defs;
pub struct Rect { w: float, h: float }
impl Rect {
    pub fn area() -> float { self.w * self.h }
}
const SCALE: float = 2.0;
fn twice(x: float) -> float { x * 2.0 }
pub fn grow(r: Rect) -> Rect { Rect { w: r.w * SCALE, h: twice(r.h) } }
pub fn label(n: int) -> str { "#{n} {defs::NAME}" }
pub fn pick(f: fn(int) -> int, x: int) -> int { f(x) }
pub fn first(xs: [str]) -> str? { if xs.len() > 0 { xs[0] } else { nil } }
"##;

const DEFS: &str = r#"const NAME: str = "defs";"#;

const MAIN: &str = r#"
use util;
use defs;
fn triple(x: int) -> int { x * 3 }
pub fn area() -> float {
    let r = util::grow(util::Rect { w: 1.0, h: 2.0 });
    r.area()
}
pub fn label() -> str { "{util::label(5)} {defs::NAME}" }
pub fn picked() -> int { util::pick(triple, 4) }
pub fn first() -> str { util::first(["a"]).or("none") }
"#;

/// `name.bs` beside `from` or in a folder above it, from `files`.
fn modules(files: &[(&str, &str)]) -> Modules {
    let files: HashMap<String, String> = files
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    Modules(Rc::new(move |from: &str, name: &str| {
        let mut dir = std::path::Path::new(from).parent();
        while let Some(d) = dir {
            let file = format!("{}.bs", name.replace("::", "/"));
            let key = d.join(file).to_string_lossy().into_owned();
            if let Some(src) = files.get(&key) {
                return Some((key, src.clone()));
            }
            dir = d.parent();
        }
        None
    }))
}

fn options(files: &[(&str, &str)]) -> Options {
    Options {
        modules: Some(modules(files)),
        path: "scripts/panels/main.bs".into(),
        ..Options::default()
    }
}

#[test]
fn a_file_uses_another_and_reaches_only_what_it_makes_pub() {
    let files = [("scripts/util.bs", UTIL), ("scripts/defs.bs", DEFS)];
    let c = compile(MAIN, &options(&files)).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(c.meta.uses, ["scripts/defs.bs", "scripts/util.bs"]);
    let mut rt = Runtime::new(&c, HostFns::default(), Limits::default()).unwrap();
    assert_eq!(rt.call("area", &[]), Ok(Value::Float(8.0)));
    assert_eq!(rt.call("label", &[]), Ok(Value::Str("#5 defs defs".into())));
    assert_eq!(rt.call("picked", &[]), Ok(Value::Int(12)));
    assert_eq!(rt.call("first", &[]), Ok(Value::Str("a".into())));
    assert_eq!(rt.live(), 0);
}

fn error(src: &str, files: &[(&str, &str)]) -> String {
    compile(src, &options(files))
        .err()
        .map_or_else(|| panic!("compiled"), |e| e.to_string())
}

#[test]
fn a_missing_or_private_module_is_named() {
    let files = [("scripts/util.bs", UTIL), ("scripts/defs.bs", DEFS)];
    let e = error("use nope;\npub fn f() -> int { 1 }", &files);
    assert!(e.contains("no module `nope`"), "{e}");
    let e = error(
        "use util;\npub fn f() -> float { util::twice(1.0) }",
        &files,
    );
    assert!(e.contains("`twice` is private to `util`"), "{e}");
    let bad = [("scripts/bad.bs", "pub fn f() -> int { true }")];
    let e = error("use bad;\npub fn g() -> int { bad::f() }", &bad);
    assert!(e.contains("in scripts/bad.bs"), "{e}");
}

#[test]
fn modules_may_use_each_other_and_the_file_that_uses_them() {
    // `a` and `b` use each other, and `b` calls back into the root file.
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
    let options = Options {
        modules: Some(modules(&files)),
        path: "scripts/main.bs".into(),
        ..Options::default()
    };
    let c = compile(main, &options).unwrap_or_else(|e| panic!("{e}"));
    let mut rt = Runtime::new(&c, HostFns::default(), Limits::default()).unwrap();
    // a 0, b 1 bumped to a 2, b 3 bumped to a 4: then 40 from `core`.
    assert_eq!(rt.call("run", &[]), Ok(Value::Int(44)));
}

#[test]
fn a_root_function_a_module_calls_is_no_local_of_the_same_name() {
    let root = "use user;\npub fn layers() -> int { 2 }\npub fn run() -> int { user::go() }";
    let files = [
        ("scripts/tiles.bs", root),
        (
            "scripts/user.bs",
            "use tiles;\npub fn go() -> int { let layers = 40; layers + tiles::layers() }",
        ),
    ];
    let options = Options {
        modules: Some(modules(&files)),
        path: "scripts/tiles.bs".into(),
        ..Options::default()
    };
    let c = compile(root, &options).unwrap_or_else(|e| panic!("{e}"));
    let mut rt = Runtime::new(&c, HostFns::default(), Limits::default()).unwrap();
    assert_eq!(rt.call("run", &[]), Ok(Value::Int(42)));
}
