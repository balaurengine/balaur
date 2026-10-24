use super::{compile_error, run};
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

pub struct Editor { s: State, said: str }
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
        "pub class Big {{ {wide} }}\nfn touch(b: Big) {{ b.f7 += 1; }}\npub struct E {{ b: Big? }}\nimpl E {{ pub fn go() {{ }} }}\n"
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
        pub struct Editor { doc: Doc }
        impl Editor {
            pub fn edit() { self.doc.rev += 1; self.doc.name = "a"; }
        }
    "#;
    let v2 = r"
        pub class Doc { name: str, rev: int, saved: bool = true }
        pub struct Editor { doc: Doc }
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
    assert!(
        rt.object("Doc", &[]).is_err(),
        "a class is no script's own object"
    );
}

#[test]
fn a_class_boxes_as_its_fields_and_comes_back_a_new_object() {
    let src = r#"
        pub class P { x: int, tags: [str] }
        pub struct E { said: str }
        impl E {
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
    let mut e = rt.object("E", &[]).unwrap();
    rt.call_method(&mut e, "go", &[]).unwrap();
    assert_eq!(
        rt.get(&e, "said"),
        Some(Value::Str(r#"3 4 13 true {"x": 4, "tags": ["a"]}"#.into()))
    );
    rt.drop_object(e);
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
