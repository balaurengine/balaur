use super::{call, compile_error, run};
use balaur_bs::runtime::Value;

pub(crate) const SRC: &str = r#"
fn double(x: int) -> int { x * 2 }
fn square(x: int) -> int { x * x }
fn longer(a: str, b: str) -> bool { a.len() > b.len() }
fn apply(f: fn(int) -> int, x: int) -> int { f(x) }
pub struct Picker { pick: fn(int) -> int }
impl Picker {
    pub fn run(x: int) -> int {
        let f = self.pick;
        f(x)
    }
}
pub fn twice(k: int) -> int {
    let f = double;
    let g: fn(int) -> int = square;
    apply(f, k) + apply(g, k)
}
pub fn table(k: int) -> int {
    let fs: [fn(int) -> int] = [double, square, double];
    let t = 0;
    for f in fs { t += f(k); }
    t
}
pub fn longest() -> str {
    let xs = ["a", "ccc", "bb"];
    let best = xs[0];
    let better = longer;
    for x in xs { if better(x, best) { best = x; } }
    best
}
pub fn picked(k: int) -> int {
    let p = Picker { pick: square };
    p.run(k)
}
"#;

#[test]
fn a_function_is_a_value_called_later() {
    let mut rt = run(SRC);
    assert_eq!(call(&mut rt, "twice", &[Value::Int(5)]), Value::Int(35));
    assert_eq!(call(&mut rt, "table", &[Value::Int(3)]), Value::Int(21));
    assert_eq!(call(&mut rt, "longest", &[]), Value::Str("ccc".into()));
    assert_eq!(call(&mut rt, "picked", &[Value::Int(6)]), Value::Int(36));
    assert_eq!(rt.live(), 0);
}

#[test]
fn a_fn_never_set_traps() {
    let mut rt = run(SRC);
    let mut p = rt.object("Picker", &[]).unwrap();
    assert!(rt.call_method(&mut p, "run", &[Value::Int(1)]).is_err());
    assert_eq!(call(&mut rt, "picked", &[Value::Int(2)]), Value::Int(4));
}

#[test]
fn only_a_plain_function_is_a_fn_value() {
    let e = compile_error(
        "pub struct S { x: int }\nimpl S { fn m() -> int { 1 } }\npub fn f() -> int { let g = S.m; 1 }",
    );
    assert!(!e.is_empty());
    let e = compile_error("pub fn f(x: int) -> int { x(1) }");
    assert!(e.contains("not a function"), "{e}");
    let e = compile_error(
        "fn one() -> int { 1 }\npub fn f() -> int { let g: fn(int) -> int = one; 1 }",
    );
    assert!(
        e.contains("fn(int) -> int") && e.contains("fn() -> int"),
        "{e}"
    );
}

// A module that calls a `fn` value it took out of an `any`, and names no
// function as a value itself, still has the table the call goes through.
#[test]
fn a_call_through_a_value_from_an_any_needs_no_named_function() {
    let mut rt = crate::run(
        "fn through(x: any) -> int { let f: fn(int) -> int = x.get(\"f\"); f(2) }\n\
         pub fn plain() -> int { 1 }\n",
    );
    assert_eq!(crate::call(&mut rt, "plain", &[]), Value::Int(1));
}
