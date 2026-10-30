use super::{call, run};
use balaur_bs::runtime::Value;

pub(crate) const SRC: &str = r#"
fn double(x: int) -> int { x * 2 }
fn square(x: int) -> int { x * x }
fn longer(a: str, b: str) -> bool { a.len() > b.len() }
fn apply(f: fn(int) -> int, x: int) -> int { f(x) }
pub class Picker { pick: fn(int) -> int }
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
fn a_fn_never_set_traps() {
    let mut rt = run(SRC);
    let mut p = rt.object("Picker", &[]).unwrap();
    assert!(rt.call_method(&mut p, "run", &[Value::Int(1)]).is_err());
    assert_eq!(call(&mut rt, "picked", &[Value::Int(2)]), Value::Int(4));
}
