use super::{call, compile_error, run};
use balaur_bs::runtime::Value;

const SRC: &str = r#"
pub struct Bag { best: str?, count: int? }
fn find(xs: [str], name: str) -> int? {
    let i = 0;
    while i < xs.len() {
        if xs[i] == name { return i; }
        i += 1;
    }
    nil
}
pub fn found(name: str) -> int {
    let xs = ["a", "b", "c"];
    find(xs, name).or(-1)
}
pub fn checks(name: str) -> str {
    let xs = ["a", "b"];
    let r = find(xs, name);
    let a = r.is_some();
    let b = r.is_none();
    let c = r == nil;
    let d = nil != r;
    "{a} {b} {c} {d}"
}
pub fn names(k: int) -> str {
    let s: str? = if k > 0 { "pos {k}" } else { nil };
    s.or("none")
}
pub fn unwrapped(k: int) -> int {
    let x: int? = if k > 0 { k } else { nil };
    x.unwrap()
}
pub fn bag() -> str {
    let b = Bag { best: nil, count: 3 };
    let before = b.best.is_none();
    b.best = "x";
    "{before} {b.best.or("-")} {b.count.or(0)}"
}
pub fn echo(x: str?) -> str? { x }
pub fn arrays() -> int {
    let xs: [int?] = [1, nil, 3];
    let t = 0;
    for x in xs { t += x.or(100); }
    t
}
"#;

#[test]
fn a_value_that_may_be_nil_says_so_in_its_type() {
    let mut rt = run(SRC);
    let s = |v: &str| Value::Str(v.into());
    assert_eq!(call(&mut rt, "found", &[s("b")]), Value::Int(1));
    assert_eq!(call(&mut rt, "found", &[s("z")]), Value::Int(-1));
    assert_eq!(
        call(&mut rt, "checks", &[s("a")]),
        s("true false false true")
    );
    assert_eq!(
        call(&mut rt, "checks", &[s("q")]),
        s("false true true false")
    );
    assert_eq!(call(&mut rt, "names", &[Value::Int(2)]), s("pos 2"));
    assert_eq!(call(&mut rt, "names", &[Value::Int(0)]), s("none"));
    assert_eq!(call(&mut rt, "unwrapped", &[Value::Int(5)]), Value::Int(5));
    assert_eq!(call(&mut rt, "bag", &[]), s("true x 3"));
    assert_eq!(call(&mut rt, "arrays", &[]), Value::Int(104));
    assert_eq!(rt.live(), 0, "an optional leaked");
}

#[test]
fn unwrapping_nil_traps_and_the_module_runs_on() {
    let mut rt = run(SRC);
    assert!(rt.call("unwrapped", &[Value::Int(0)]).is_err());
    assert_eq!(call(&mut rt, "unwrapped", &[Value::Int(3)]), Value::Int(3));
}

#[test]
fn nil_crosses_to_the_host_as_nothing() {
    let mut rt = run(SRC);
    assert_eq!(call(&mut rt, "echo", &[Value::Void]), Value::Void);
    assert_eq!(
        call(&mut rt, "echo", &[Value::Str("q".into())]),
        Value::Str("q".into())
    );
    assert_eq!(rt.live(), 0);
}

#[test]
fn an_optional_is_used_only_through_its_methods() {
    let e = compile_error("pub fn f() -> int { let x = nil; 1 }");
    assert!(e.contains("`nil` needs the type"), "{e}");
    let e = compile_error("pub fn f(x: int?) -> int { x + 1 }");
    assert!(e.contains("int?") && e.contains("int"), "{e}");
    let e = compile_error("pub fn f(x: str?) -> int { x.len() }");
    assert!(e.contains("unwrap"), "{e}");
    let e = compile_error("pub fn f(x: int) -> bool { x == nil }");
    assert!(e.contains("never nil"), "{e}");
    let e = compile_error("pub fn f(x: int??) -> int { 1 }");
    assert!(e.contains("cannot itself be optional"), "{e}");
}

#[test]
fn nil_in_one_branch_takes_the_other_branchs_type() {
    let src = r#"
pub fn pick(a: any, yes: bool) -> str {
    let got = if yes { a } else { nil };
    let n = if yes { 3 } else { nil };
    "{got.is_nil()} {n.or(0)}"
}
"#;
    let mut rt = super::run(src);
    assert_eq!(
        super::call(
            &mut rt,
            "pick",
            &[
                balaur_bs::runtime::Value::Int(1),
                balaur_bs::runtime::Value::Bool(true)
            ]
        ),
        balaur_bs::runtime::Value::Str("false 3".into())
    );
    assert_eq!(
        super::call(
            &mut rt,
            "pick",
            &[
                balaur_bs::runtime::Value::Int(1),
                balaur_bs::runtime::Value::Bool(false)
            ]
        ),
        balaur_bs::runtime::Value::Str("true 0".into())
    );
}

#[test]
fn nil_where_a_node_goes_is_no_node() {
    let mut rt = super::run(
        "pub fn none() -> Node { nil }\npub fn early(x: int) -> Node { if x > 0 { return nil; } nil }",
    );
    assert_eq!(
        super::call(&mut rt, "none", &[]),
        balaur_bs::runtime::Value::Node(0)
    );
}

#[test]
fn a_value_that_is_always_there_has_no_optional_methods() {
    let e = super::compile_error("pub fn f(s: str) -> bool { s.is_some() }");
    assert!(e.contains("not `is_some`"), "{e}");
}
