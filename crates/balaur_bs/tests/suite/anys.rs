use super::{call, run};
use balaur_bs::runtime::Value;

const SCRIPT: &str = r#"
pub fn damage(hit: any) -> int {
    hit["damage"].as_int() + hit.get("bonus").as_int()
}

pub fn first_tag(hit: any) -> str {
    hit["tags"][0].as_str()
}

pub fn describe(hit: any) -> str {
    let tagged = hit.has("tags");
    let missing = hit["missing"].is_nil();
    let past = hit["tags"][5].kind();
    "{hit.kind()} {hit.len()} {tagged} {missing} {past}"
}

pub fn boxed() -> str {
    let a: any = 3;
    let b: any = "x";
    let l: [any] = [a, b, 2.5, true, nil()];
    let all: any = l;
    all.text()
}

pub fn keys(hit: any) -> [str] {
    hit.keys()
}

pub fn echo(v: any) -> any {
    v
}

pub fn built() -> any {
    let tags: [any] = ["a", "b"];
    map().with("hp", 10).with("tags", tags).with("hp", 12)
}

pub fn shared() -> str {
    let a = map();
    let b = a;
    a.set("hp", 10);
    b.set("hp", 12);
    b.set("name", "imp");
    let l: [any] = [];
    let list: any = l;
    for i in 0..9 {
        list.push(i);
    }
    a.set("list", list);
    let changed = nil().set("x", 1) || a.push(1) || list.set("x", 1);
    "{a.text()} {list.len()} {changed}"
}

pub fn numbers(v: any) -> float {
    v.as_float() + v.as_int()
}
"#;

fn hit() -> Value {
    Value::Map(vec![
        ("damage".into(), Value::Int(3)),
        ("bonus".into(), Value::Int(2)),
        (
            "tags".into(),
            Value::Array(vec![Value::Str("fire".into()), Value::Str("ice".into())]),
        ),
    ])
}

#[test]
fn a_script_reads_what_the_host_handed_over() {
    let mut rt = run(SCRIPT);
    let base = rt.live();
    assert_eq!(call(&mut rt, "damage", &[hit()]), Value::Int(5));
    assert_eq!(
        call(&mut rt, "first_tag", &[hit()]),
        Value::Str("fire".into())
    );
    assert_eq!(
        call(&mut rt, "describe", &[hit()]),
        Value::Str("map 3 true true nil".into())
    );
    assert_eq!(
        call(&mut rt, "keys", &[hit()]),
        Value::Array(
            ["damage", "bonus", "tags"]
                .map(|k| Value::Str(k.into()))
                .to_vec()
        )
    );
    assert_eq!(call(&mut rt, "echo", &[hit()]), hit());
    assert_eq!(call(&mut rt, "echo", &[Value::Void]), Value::Void);
    assert_eq!(
        call(&mut rt, "numbers", &[Value::Float(2.5)]),
        Value::Float(4.5)
    );
    assert_eq!(rt.live(), base, "an `any` leaked");
}

#[test]
fn values_box_into_an_any_where_one_is_wanted() {
    let mut rt = run(SCRIPT);
    assert_eq!(
        call(&mut rt, "boxed", &[]),
        Value::Str(r#"[3, "x", 2.5, true, nil]"#.into())
    );
    assert_eq!(rt.live(), 0, "a boxed value leaked");
}

#[test]
fn an_any_has_only_its_own_methods() {
    let e = super::compile_error("fn f(a: any) -> int { a.size() }");
    assert!(e.contains("an `any` has `kind`"), "{e}");
}

#[test]
fn a_script_builds_a_map_to_hand_over() {
    let mut rt = run(SCRIPT);
    let tags = Value::Array(vec![Value::Str("a".into()), Value::Str("b".into())]);
    assert_eq!(
        call(&mut rt, "built", &[]),
        Value::Map(vec![("hp".into(), Value::Int(12)), ("tags".into(), tags)])
    );
    assert_eq!(rt.live(), 0, "building a map leaked");
}

#[test]
fn a_map_or_a_list_changes_in_place_for_every_holder() {
    let mut rt = run(SCRIPT);
    assert_eq!(
        call(&mut rt, "shared", &[]),
        Value::Str(
            r#"{"hp": 12, "name": "imp", "list": [0, 1, 2, 3, 4, 5, 6, 7, 8]} 9 false"#.into()
        )
    );
    assert_eq!(rt.live(), 0, "a changed map or list leaked");
}

#[test]
fn the_invalid_node_boxes_to_nil() {
    let mut rt = run(r#"
fn holds(v: any) -> bool { !v.is_nil() }
pub fn boxed() -> str {
    let none: Node = nil;
    let list: [any] = [none];
    "{holds(none)} {list[0].is_nil()}"
}
"#);
    assert_eq!(call(&mut rt, "boxed", &[]), Value::Str("false true".into()));
}

#[test]
fn a_list_in_an_any_is_written_by_index() {
    let mut rt = run(r#"
pub fn edited() -> str {
    let l: any = map().with("xs", [1, 2, 3]).xs;
    let held = l;
    l[1] = "two";
    let put = l.put(0, 10);
    let past = l.put(3, 4);
    let mid = l.insert(1, 1.5);
    let end = l.insert(4, "end");
    let beyond = l.insert(9, 0);
    let first = map().with("n", 0).n.insert(0, 1);
    "{held} {put} {past} {mid} {end} {beyond} {first}"
}
"#);
    assert_eq!(
        call(&mut rt, "edited", &[]),
        Value::Str(r#"[10, 1.5, "two", 3, "end"] true false true true false false"#.into())
    );
    assert_eq!(rt.live(), 0, "a replaced element leaked");
}

#[test]
fn an_any_prints_inside_a_string_as_its_text() {
    let mut rt = run("pub fn say(a: any) -> str { \"got {a}!\" }");
    let base = rt.live();
    assert_eq!(
        call(&mut rt, "say", &[Value::Int(3)]),
        Value::Str("got 3!".into())
    );
    assert_eq!(
        call(&mut rt, "say", &[Value::Str("hi".into())]),
        Value::Str("got hi!".into())
    );
    assert_eq!(rt.live(), base);
}

#[test]
fn a_list_a_struct_or_an_optional_boxes_into_an_any() {
    let src = r#"
pub struct P { name: str, tags: [str], at: Vec2 }
fn keep(a: any) -> any { a }
pub fn list() -> str { let a = keep(["x", "y"]); "{a.len()} {a[1].as_str()}" }
pub fn record() -> str {
    let a = keep(P { name: "p", tags: ["t"], at: vec2(1.0, 2.0) });
    "{a["name"].as_str()} {a["tags"][0].as_str()} {a["at"]["y"].as_float()}"
}
pub fn maybe(yes: bool) -> str {
    let v: int? = if yes { 4 } else { nil };
    let a = keep(v);
    "{a.is_nil()} {a.as_int()}"
}
"#;
    let mut rt = run(src);
    let base = rt.live();
    assert_eq!(call(&mut rt, "list", &[]), Value::Str("2 y".into()));
    assert_eq!(call(&mut rt, "record", &[]), Value::Str("p t 2.0".into()));
    assert_eq!(
        call(&mut rt, "maybe", &[Value::Bool(true)]),
        Value::Str("false 4".into())
    );
    assert_eq!(
        call(&mut rt, "maybe", &[Value::Bool(false)]),
        Value::Str("true 0".into())
    );
    assert_eq!(rt.live(), base, "boxing leaked or freed twice");
}

#[test]
fn an_any_becomes_the_typed_value_a_place_wants() {
    let src = r#"
pub struct P { name: str, n: int, tags: [str] }
fn label(s: str) -> str { "<{s}>" }
pub fn scalars(a: any) -> str {
    let n: int = a["n"];
    let f: float = a["n"];
    "{label(a["name"])} {n + 1} {f} {a["n"] + 2} {if n > 1 { a["name"] } else { "x" }}"
}
pub fn record(a: any) -> str {
    let p: P = a;
    let tags: [str] = a["tags"];
    let missing: int? = a["nope"];
    "{p.name} {p.n} {p.tags.len()} {tags[1]} {missing.is_none()}"
}
"#;
    let mut rt = run(src);
    let base = rt.live();
    let v = Value::Map(vec![
        ("name".into(), Value::Str("orc".into())),
        ("n".into(), Value::Int(3)),
        (
            "tags".into(),
            Value::Array(vec![Value::Str("a".into()), Value::Str("b".into())]),
        ),
    ]);
    assert_eq!(
        call(&mut rt, "scalars", std::slice::from_ref(&v)),
        Value::Str("<orc> 4 3.0 5 orc".into())
    );
    assert_eq!(
        call(&mut rt, "record", &[v]),
        Value::Str("orc 3 2 b true".into())
    );
    assert_eq!(rt.live(), base, "unboxing leaked or freed twice");
}

#[test]
fn an_any_answers_for_what_it_holds() {
    let src = r#"
pub fn run(a: any) -> str {
    let gone = a.remove("n");
    let list = a.tags;
    let first = list.remove(0);
    let total = 0;
    for t in a.tags { total += 1; }
    let name: str = a.name;
    "{gone.as_int()} {first.as_str()} {total} {a.has("n")} {name.starts_with("or")} {a.name == a.name} {a.name.or("x")} {a.missing.is_none()} {-a.score}"
}
pub fn plain(n: int, s: str) -> str { "{n.as_float()} {s.as_str()} {n.as_str()}" }
"#;
    let mut rt = run(src);
    let base = rt.live();
    let v = Value::Map(vec![
        ("name".into(), Value::Str("orc".into())),
        ("n".into(), Value::Int(3)),
        ("score".into(), Value::Float(1.5)),
        (
            "tags".into(),
            Value::Array(vec![Value::Str("a".into()), Value::Str("b".into())]),
        ),
    ]);
    assert_eq!(
        call(&mut rt, "run", &[v]),
        Value::Str("3 a 1 false true true orc true -1.5".into())
    );
    assert_eq!(
        call(&mut rt, "plain", &[Value::Int(2), Value::Str("s".into())]),
        Value::Str("2.0 s 2".into())
    );
    assert_eq!(rt.live(), base, "a held method leaked or freed twice");
}

#[test]
fn a_list_of_one_kind_goes_where_a_list_of_another_is_wanted() {
    let src = r#"
fn count(xs: [any]) -> int { xs.len() }
fn firsts(xs: [str]) -> str { xs[0] }
fn some(xs: [int?]) -> int { xs[1].or(0) }
pub fn run() -> str {
    let mixed = [1, "two", true];
    let back: [any] = ["a", "b"];
    "{count([1.5, 2.5])} {firsts(back)} {some([4, 5])} {mixed.len()} {mixed[1].as_str()} {[1, 2.5][0]}"
}
"#;
    let mut rt = run(src);
    let base = rt.live();
    assert_eq!(
        call(&mut rt, "run", &[]),
        Value::Str("2 a 5 3 two 1.0".into())
    );
    assert_eq!(rt.live(), base);
}

#[test]
fn the_ported_editor_idioms_compile_and_run() {
    let src = r#"
pub struct Box2 { size: [int]? }
fn pick(w: str) -> [str] { if w == "a" { ["x", "y"] } else { [] } }
fn nothing(a: any) -> any { if a.is_nil() { 1 } else { nil } }
fn none(a: any) -> any { nil }
pub fn run(a: any) -> str {
    let b = Box2 { size: [] };
    let v = vec3(1.0, 2.0, 3.0);
    a.name = "elf";
    a["hp"] = 7;
    let o: str? = if a.has("name") { a.name } else { nil };
    let log = 0;
    if log > 1 { log = 2; } else { nil };
    "{pick("a").len()} {pick("b").len()} {b.size.or([]).len()} {v.at(1)} {v.get("z")} {a.name} {a["hp"].as_int()} {o == "elf"} {[1, 2] == [1, 2]} {["q"]} {none(a).is_nil()} {nothing(a).as_int()}"
}
"#;
    let mut rt = run(src);
    let base = rt.live();
    let v = Value::Map(vec![("name".into(), Value::Str("orc".into()))]);
    assert_eq!(
        call(&mut rt, "run", &[v]),
        Value::Str("2 0 0 2.0 3.0 elf 7 true true [\"q\"] true 0".into())
    );
    assert_eq!(rt.live(), base);
}

#[test]
fn a_list_reads_at_extends_and_sorts() {
    let src = r#"
pub fn run() -> str {
    let xs = [3, 1, 2];
    xs.extend([5, 4]);
    xs.sort();
    let names = ["b", "a"];
    names.sort();
    "{xs.at(0)} {xs.len()} {xs[4]} {names[0]}"
}
"#;
    let mut rt = run(src);
    let base = rt.live();
    assert_eq!(call(&mut rt, "run", &[]), Value::Str("1 5 5 a".into()));
    assert_eq!(rt.live(), base);
}
