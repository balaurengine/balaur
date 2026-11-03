use balaur_bs::runtime::{HostFns, Limits, Runtime, Value};
use balaur_bs::{Modules, Options, compile};
use std::collections::HashMap;
use std::rc::Rc;

const UTIL: &str = r##"
use defs;
pub class Rect { pub w: float, pub h: float }
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

const DEFS: &str = r#"pub const NAME: str = "defs";"#;

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

#[test]
fn an_enum_a_module_declares_is_made_and_matched_by_its_path() {
    let shapes = "pub enum Shape { Circle(float), Square { side: float }, Empty }\n\
                  pub fn unit() -> Shape { Shape::Square { side: 1.0 } }\n\
                  pub fn area(s: Shape) -> float { match s { Shape::Circle(r) => r * r * 3.0, Shape::Square { side } => side * side, Shape::Empty => 0.0 } }";
    let main = "use shapes;\n\
                pub fn total() -> float {\n\
                    let all = [shapes::Shape::Circle(1.0), shapes::unit(), shapes::Shape::Empty];\n\
                    let t = 0.0;\n\
                    for s in all { t += shapes::area(s); }\n\
                    t\n\
                }\n\
                pub fn named() -> str { match shapes::unit() { shapes::Shape::Square { side } => \"square {side}\", _ => \"other\" } }";
    let files = [("scripts/shapes.bs", shapes)];
    let c = compile(main, &options(&files)).unwrap_or_else(|e| panic!("{e}"));
    let mut rt = Runtime::new(&c, HostFns::default(), Limits::default()).unwrap();
    assert_eq!(rt.call("total", &[]), Ok(Value::Float(4.0)));
    assert_eq!(rt.call("named", &[]), Ok(Value::Str("square 1.0".into())));
    assert_eq!(rt.live(), 0);
}

// What each file keeps: `kept` is private, `shared` is internal, `open` is pub.
const KEPT: &str = r"
const KEPT_LIMIT: int = 1;
internal const SHARED_LIMIT: int = 2;
pub const OPEN_LIMIT: int = 3;
struct Kept { n: int }
internal struct Shared { n: int }
pub class Open {
    pub n: int = 1,
    internal shared: int = 2,
    kept: int = 3,
}
impl Open {
    pub fn open() -> int { self.kept + self.helper() }
    internal fn shared_method() -> int { self.shared }
    fn helper() -> int { self.kept }
}
enum KeptKind { One }
internal fn shared_fn() -> int { 4 }
pub fn make() -> Open { Open {} }
";

fn refused(use_it: &str, why: &str) {
    let files = [("scripts/lib.bs", KEPT)];
    let src = format!("use lib;\npub fn f() {{ {use_it}; }}");
    let e = error(&src, &files);
    assert!(e.contains(why), "{use_it}: {e}");
}

#[test]
fn nothing_a_file_keeps_is_named_from_another() {
    refused(
        "let a = lib::KEPT_LIMIT",
        "`KEPT_LIMIT` is private to `lib`",
    );
    refused("let a = lib::Kept { n: 1 }", "`Kept` is private to `lib`");
    refused("let a: [lib::Kept] = []", "`Kept` is private to `lib`");
    refused(
        "let a = lib::KeptKind::One",
        "`KeptKind` is private to `lib`",
    );
    refused(
        "let a = lib::make().helper()",
        "`Open.helper` is private to `lib`",
    );
    refused(
        "let a = lib::make().kept",
        "`Open.kept` is private to `lib`",
    );
    refused(
        "let o = lib::make(); o.kept = 1",
        "`Open.kept` is private to `lib`",
    );
    refused(
        "let a = lib::Open { kept: 1 }",
        "`Open.kept` is private to `lib`",
    );
}

#[test]
fn a_signature_cannot_name_what_another_file_keeps() {
    let files = [("scripts/lib.bs", KEPT)];
    let e = error("use lib;\npub fn f(k: lib::Kept) {}", &files);
    assert!(e.contains("`Kept` is private to `lib`"), "{e}");
    let e = error("use lib;\npub struct S { k: lib::Kept }", &files);
    assert!(e.contains("`Kept` is private to `lib`"), "{e}");
}

#[test]
fn a_file_names_all_it_keeps_itself() {
    let files = [("scripts/lib.bs", KEPT)];
    let src =
        "use lib;\npub fn f() -> int { let o = lib::make(); o.n + o.open() + lib::OPEN_LIMIT }";
    let c = compile(src, &options(&files)).unwrap_or_else(|e| panic!("{e}"));
    let mut rt = Runtime::new(&c, HostFns::default(), Limits::default()).unwrap();
    assert_eq!(rt.call("f", &[]), Ok(Value::Int(1 + 6 + 3)));
}

#[test]
fn internal_opens_an_item_to_its_package_and_no_further() {
    let lib = (
        "addons/kit/lib.bs",
        "internal fn secret() -> int { 7 }\n\
         internal const LIMIT: int = 2;\n\
         pub class Box { internal n: int = 1 }\n\
         impl Box { internal fn m() -> int { self.n } }\n\
         pub fn make() -> Box { Box {} }\n",
    );
    let inside = "use lib;\npub fn f() -> int { lib::secret() + lib::LIMIT + lib::make().n + lib::make().m() }";
    let beside = ("addons/kit/beside.bs", inside);
    let options = |path: &str| Options {
        path: path.into(),
        modules: Some(modules(&[lib, beside])),
        ..Options::default()
    };
    let c = compile(inside, &options("addons/kit/beside.bs")).unwrap_or_else(|e| panic!("{e}"));
    let mut rt = Runtime::new(&c, HostFns::default(), Limits::default()).unwrap();
    assert_eq!(rt.call("f", &[]), Ok(Value::Int(7 + 2 + 1 + 1)));

    let outside = |body: &str, why: &str| {
        let src = format!("use addons::kit::lib;\npub fn f() -> int {{ {body} }}");
        let e = compile(&src, &options("scripts/game.bs"))
            .err()
            .map_or_else(|| panic!("compiled: {body}"), |e| e.to_string());
        assert!(e.contains(why), "{body}: {e}");
    };
    outside("lib::secret()", "`secret` is internal to `addons/kit`");
    outside("lib::LIMIT", "`LIMIT` is internal to `addons/kit`");
    outside("lib::make().n", "`Box.n` is internal to `addons/kit`");
    outside("lib::make().m()", "`Box.m` is internal to `addons/kit`");
}

#[test]
fn an_internal_function_is_no_entry_point_for_the_engine() {
    let c = compile(
        "internal fn f() -> int { 1 }\npub fn g() -> int { 2 }",
        &Options::default(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let mut rt = Runtime::new(&c, HostFns::default(), Limits::default()).unwrap();
    assert_eq!(rt.call("g", &[]), Ok(Value::Int(2)));
    assert!(rt.call("f", &[]).is_err());
}

#[test]
fn a_field_may_be_called_internal() {
    let src = "pub struct S { internal: int, pub other: int }\n\
               pub fn f() -> int { let s = S { internal: 1, other: 2 }; s.internal + s.other }";
    let c = compile(src, &Options::default()).unwrap_or_else(|e| panic!("{e}"));
    let mut rt = Runtime::new(&c, HostFns::default(), Limits::default()).unwrap();
    assert_eq!(rt.call("f", &[]), Ok(Value::Int(3)));
}

#[test]
fn an_any_converts_to_a_struct_whatever_its_fields_keep() {
    let lib = "pub struct Stats { pub hp: int = 0, kept: int = 0 }\n\
               pub fn hp(s: Stats) -> int { s.hp + s.kept }\n";
    let files = [("scripts/lib.bs", lib)];
    let src = "use lib;\npub fn f() -> int { let m: any = {\"hp\": 2, \"kept\": 3}; let s: lib::Stats = m; lib::hp(s) }";
    let c = compile(src, &options(&files)).unwrap_or_else(|e| panic!("{e}"));
    let mut rt = Runtime::new(&c, HostFns::default(), Limits::default()).unwrap();
    assert_eq!(rt.call("f", &[]), Ok(Value::Int(5)));
}

#[test]
fn a_module_cannot_reach_what_the_root_keeps() {
    let root = "use other;\nfn secret() -> int { 1 }\npub fn go() -> int { other::peek() }";
    let refused = |peek: &str| {
        let files = [
            ("scripts/panels/main.bs", root),
            ("scripts/panels/other.bs", peek),
        ];
        let c = compile(root, &options(&files));
        c.err()
            .map_or_else(|| panic!("compiled"), |e| e.to_string())
    };
    let e = refused("pub fn peek() -> int { secret() }");
    assert!(e.contains("`secret` is private to"), "{e}");
    let e = refused("use main;\npub fn peek() -> int { main::secret() }");
    assert!(e.contains("`secret` is private to"), "{e}");
}

#[test]
fn completion_offers_only_what_the_file_may_name() {
    let files = [("scripts/lib.bs", KEPT)];
    let at = |src: &str, line: u32, col: u32| -> Vec<String> {
        let host = balaur_bs::HostApi::default();
        balaur_bs::analysis::analyze_in(src, &host, Some(modules(&files)), "scripts/main.bs")
            .complete(line, col)
            .into_iter()
            .map(|c| c.label)
            .collect()
    };
    let names = at("use lib;\npub fn f() { lib::; }", 2, 19);
    assert!(names.iter().any(|n| n == "make"), "{names:?}");
    assert!(names.iter().any(|n| n == "Open"), "{names:?}");
    assert!(!names.iter().any(|n| n == "Kept"), "{names:?}");
    // One package: what `lib` makes `internal`, this file may name.
    assert!(names.iter().any(|n| n == "shared_fn"), "{names:?}");
    let members = at("use lib;\npub fn f() { let o = lib::make(); o.; }", 2, 37);
    assert!(members.iter().any(|n| n == "n"), "{members:?}");
    assert!(members.iter().any(|n| n == "open"), "{members:?}");
    assert!(!members.iter().any(|n| n == "kept"), "{members:?}");
    assert!(!members.iter().any(|n| n == "helper"), "{members:?}");
}
