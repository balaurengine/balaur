use super::{call, compile_error, run};
use balaur_bs::runtime::{HostFns, Limits, Runtime, Value};
use balaur_bs::{Options, compile};

const EDITOR: &str = r#"
pub struct Tally { hits: int }
pub class Gizmo { drag: int?, marks: [str] }
pub class State { sel: str, history: [str], count: int, inner: Tally, gizmo: Gizmo, next: State? }

fn select(s: State, id: str) {
    s.sel = id;
    s.history.push(id);
    s.count += 1;
    bump(s.inner);
}
fn bump(i: &mut Tally) { i.hits += 1; }
fn fresh() -> State {
    State { sel: "", history: [], count: 0, inner: Tally { hits: 0 }, gizmo: Gizmo { drag: nil, marks: [] }, next: nil }
}
fn grab(g: Gizmo, at: int) { g.drag = at; g.marks.push("{at}"); }

impl Gizmo {
    fn reset() { self.drag = nil; }
}

pub class Editor { s: State, said: str }
impl Editor {
    pub fn init() {
        self.s = fresh();
    }
    pub fn click() {
        select(self.s, "x");
        let alias = self.s;
        select(alias, "y");
        grab(self.s.gizmo, 4);
        let g = self.s.gizmo;
        grab(g, 7);
        let drag = self.s.gizmo.drag.or(0);
        self.s.gizmo.reset();
        let other = fresh();
        other.count = 40;
        self.s.next = other;
        other.count += 2;
        let ring = [fresh(), fresh()];
        ring[1].count = 5;
        let first = ring[1];
        first.count += 1;
        self.said = "{self.s.sel} {self.s.history.len()} {self.s.count} {self.s.inner.hits} {drag} {self.s.gizmo.drag.is_none()} {self.s.gizmo.marks.len()} {self.s.next.unwrap().count} {ring[1].count}";
    }
}
"#;

#[test]
fn a_class_is_one_object_every_holder_shares() {
    let mut rt = run(EDITOR);
    let base = rt.live();
    let mut e = rt.object("Editor", &[]).unwrap();
    rt.call_method(&mut e, "init", &[]).unwrap();
    rt.call_method(&mut e, "click", &[]).unwrap();
    assert_eq!(
        rt.get(&e, "said"),
        Some(Value::Str("y 2 2 2 7 true 2 42 6".into())),
        "writes through a copy, a parameter, a field and an element all land in the one object"
    );
    rt.drop_object(e);
    assert_eq!(rt.live(), base, "an object leaked or was freed twice");
}

// The module's widest signature, read with the C translator's parser.
#[cfg(feature = "c")]
#[test]
fn a_class_is_passed_as_one_value() {
    let wide = (0..300)
        .map(|k| format!("f{k}: int"))
        .collect::<Vec<_>>()
        .join(", ");
    let src = format!(
        "pub class Big {{ {wide} }}\nfn touch(b: Big) {{ b.f7 += 1; }}\npub class Keeper {{ big: Big? }}\nimpl Keeper {{ pub fn go() {{ }} }}\n"
    );
    let c = compile(&src, &Options::default()).unwrap();
    let mut widest = 0;
    for payload in wasmparser::Parser::new(0).parse_all(&c.wasm) {
        if let Ok(wasmparser::Payload::TypeSection(types)) = payload {
            for group in types {
                for t in group.unwrap().into_types() {
                    if let wasmparser::CompositeInnerType::Func(f) = &t.composite_type.inner {
                        widest = widest.max(f.params().len()).max(f.results().len());
                    }
                }
            }
        }
    }
    assert!(
        widest < 8,
        "a 300-field class crosses a call as its pointer, not {widest} values"
    );
}

#[test]
fn the_host_reads_a_class_field_and_a_reload_keeps_it() {
    let v1 = r#"
        pub class Doc { name: str, rev: int }
        pub class Editor { doc: Doc }
        impl Editor {
            pub fn edit() { self.doc.rev += 1; self.doc.name = "a"; }
        }
    "#;
    let v2 = r"
        pub class Doc { name: str, rev: int, saved: bool = true }
        pub class Editor { doc: Doc }
        impl Editor {
            pub fn edit() { self.doc.rev += 10; }
        }
    ";
    let c1 = compile(v1, &Options::default()).unwrap();
    let mut rt = Runtime::new(&c1, HostFns::default(), Limits::default()).unwrap();
    let mut e = rt.object("Editor", &[]).unwrap();
    rt.call_method(&mut e, "edit", &[]).unwrap();
    let doc = |rev: i64, extra: Option<bool>| {
        let mut fields = vec![
            ("name".to_string(), Value::Str("a".into())),
            ("rev".to_string(), Value::Int(rev)),
        ];
        if let Some(b) = extra {
            fields.push(("saved".to_string(), Value::Bool(b)));
        }
        Some(Value::Struct("Doc".into(), fields))
    };
    assert_eq!(rt.get(&e, "doc"), doc(1, None));
    let c2 = compile(v2, &Options::default()).unwrap();
    let kept = rt.reload(&c2, HostFns::default(), &mut [&mut e]).unwrap();
    assert_eq!(kept[0], ["doc"]);
    rt.call_method(&mut e, "edit", &[]).unwrap();
    assert_eq!(
        rt.get(&e, "doc"),
        doc(11, Some(true)),
        "a new field takes its default"
    );
    rt.drop_object(e);
    assert_eq!(rt.live(), 0);
}

// A script's own object lives in the module, so a call copies nothing in or
// out of it: a struct, which a call would copy, is refused.
#[test]
fn a_scripts_own_object_is_a_class() {
    let mut rt = run("pub struct Plain { count: int }\npub class Shared { count: int }\n");
    let refused = rt.object("Plain", &[]).unwrap_err();
    assert!(refused.msg.contains("`pub class Plain`"), "{}", refused.msg);
    let shared = rt.object("Shared", &[("count", Value::Int(4))]).unwrap();
    assert_eq!(rt.get(&shared, "count"), Some(Value::Int(4)));
    rt.drop_object(shared);
    assert_eq!(rt.live(), 0);
}

#[test]
fn a_class_boxes_as_its_fields_and_comes_back_a_new_object() {
    let src = r#"
        pub class P { x: int, tags: [str] }
        pub class Packer { said: str }
        impl Packer {
            pub fn go() {
                let p = P { x: 3, tags: ["a"] };
                let held: any = p;
                p.x = 4;
                let back: P = held;
                back.x += 10;
                let q = P { x: 4, tags: ["a"] };
                self.said = "{held.x} {p.x} {back.x} {p == q} {p}";
            }
        }
    "#;
    let mut rt = run(src);
    let base = rt.live();
    let mut packer = rt.object("Packer", &[]).unwrap();
    rt.call_method(&mut packer, "go", &[]).unwrap();
    assert_eq!(
        rt.get(&packer, "said"),
        Some(Value::Str(r#"3 4 13 true {"x": 4, "tags": ["a"]}"#.into()))
    );
    rt.drop_object(packer);
    assert_eq!(rt.live(), base);
}

#[test]
fn a_class_names_its_missing_field() {
    let e = compile_error("pub class P { x: int }\nfn f(p: P) -> int { p.y }");
    assert!(e.contains("class P has no field `y`"), "{e}");
}

#[test]
fn a_stop_shows_a_class_by_its_fields() {
    use balaur_bs::runtime::Command;
    use std::cell::RefCell;
    use std::rc::Rc;
    let src = "pub class Bag { n: int }
pub fn fill(k: int) -> int {
    let b = Bag { n: k };
    b.n += 1;
    b.n
}
";
    let mut rt = super::run_with(
        src,
        HostFns::default(),
        &Options {
            debug: true,
            ..Options::default()
        },
    );
    let seen = Rc::new(RefCell::new(None));
    let into = seen.clone();
    rt.debugger().set_breakpoint(5);
    rt.debugger().on_pause(move |p| {
        *into.borrow_mut() = p.frames[0]
            .locals
            .iter()
            .find(|(n, _)| n == "b")
            .map(|(_, v)| v.clone());
        Command::Continue
    });
    assert_eq!(
        super::call(&mut rt, "fill", &[Value::Int(2)]),
        Value::Int(3)
    );
    assert_eq!(
        *seen.borrow(),
        Some(Value::Struct(
            "Bag".into(),
            vec![("n".into(), Value::Int(3))]
        ))
    );
}

#[test]
fn only_a_class_may_hold_itself_through_an_optional() {
    let e = compile_error("pub struct A { next: A? }");
    assert!(e.contains("contains itself through `next`"), "{e}");
    let e = compile_error("pub class N { next: N }");
    assert!(e.contains("make it `N?`"), "{e}");
    let ok = compile("pub class N { next: N?, kids: [N] }", &Options::default());
    assert!(ok.is_ok(), "{:?}", ok.err());
}

// A script's own object is a class, so `self.node` is read out of memory:
// its component's property is still the engine's to read and write.
#[test]
fn a_class_reaches_its_nodes_components() {
    use balaur_bs::{HostApi, Ty};
    let host_api = HostApi {
        components: vec![(
            "transform".into(),
            vec![("position".into(), Ty::Struct("Vec3".into()))],
        )],
        ..HostApi::default()
    };
    let src = "pub class Ball { node: Node, seen: float }
impl Ball {
    pub fn update() {
        self.seen = self.node.transform.position.y;
        self.node.transform.position = vec3(1.0, self.seen + 1.0, 0.0);
    }
}
";
    let options = Options {
        host: host_api,
        ..Options::default()
    };
    let compiled = compile(src, &options).unwrap_or_else(|error| panic!("{error}"));
    let set = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
    let wrote = set.clone();
    let host = HostFns::default().engine(move |key, args, _| match key {
        "transform.position.get" => Ok(Value::Struct(
            "Vec3".into(),
            vec![
                ("x".into(), Value::Float(0.0)),
                ("y".into(), Value::Float(2.5)),
                ("z".into(), Value::Float(0.0)),
            ],
        )),
        "transform.position.set" => {
            wrote.borrow_mut().push(args[1].clone());
            Ok(Value::Void)
        }
        other => Err(format!("no {other}")),
    });
    let mut rt = Runtime::new(&compiled, host, Limits::default()).unwrap();
    let mut ball = rt.object("Ball", &[("node", Value::Node(7))]).unwrap();
    rt.call_method(&mut ball, "update", &[]).unwrap();
    assert_eq!(rt.get(&ball, "seen"), Some(Value::Float(2.5)));
    let Value::Struct(_, fields) = &set.borrow()[0] else {
        panic!("the position was not written");
    };
    assert_eq!(fields[1], ("y".to_string(), Value::Float(3.5)));
    rt.drop_object(ball);
    assert_eq!(rt.live(), 0);
}

// A spec is a struct: a literal names what differs from the declared
// defaults, and an optional field it leaves out is nil.
#[test]
fn a_struct_literal_leaves_out_what_has_a_default() {
    let mut rt = run(
        r#"pub struct Label { kind: str = "label", size: int = 3, text: str, tip: str? }
pub fn plain() -> str {
    let label = Label { text: "hi" };
    "{label.kind} {label.size} {label.text} {label.tip.is_none()}"
}
pub fn sized() -> int { Label { text: "x", size: 9 }.size }
"#,
    );
    assert_eq!(
        call(&mut rt, "plain", &[]),
        Value::Str("label 3 hi true".into())
    );
    assert_eq!(call(&mut rt, "sized", &[]), Value::Int(9));
    let missing = compile_error("pub struct Label { text: str }\npub fn f() { let l = Label {}; }");
    assert!(
        missing.contains("missing field `text` of Label"),
        "{missing}"
    );
}
