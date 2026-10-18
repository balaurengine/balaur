use super::{call, compile_error, run};
use balaur_lang::runtime::Value;

#[test]
fn methods_have_an_implicit_mutable_self() {
    let mut rt = run(r"
        pub struct Counter { n: int, step: int }
        impl Counter {
            pub fn bump() { self.n += self.step; }
            pub fn bump_twice(&mut self) -> int { self.bump(); self.bump(); self.n }
        }
    ");
    let mut c = rt.object("Counter", &[("step", Value::Int(3))]).unwrap();
    assert_eq!(
        rt.call_method(&mut c, "bump_twice", &[]).unwrap(),
        Value::Int(6)
    );
    assert_eq!(rt.get(&c, "n"), Some(Value::Int(6)));
}

#[test]
fn ints_mix_with_floats_without_casts() {
    let mut rt = run(r"
        pub fn half(n: int) -> float { n / 2.0 }
        pub fn scaled(speed: float) -> float { let x: float = 3; speed * 2 + x }
        pub fn pick(big: bool) -> float { if big { 10 } else { 0.5 } }
    ");
    assert_eq!(call(&mut rt, "half", &[Value::Int(3)]), Value::Float(1.5));
    assert_eq!(
        call(&mut rt, "scaled", &[Value::Float(1.25)]),
        Value::Float(5.5)
    );
    assert_eq!(
        call(&mut rt, "pick", &[Value::Bool(true)]),
        Value::Float(10.0)
    );
}

#[test]
fn strings_interpolate_and_compare() {
    let mut rt = run(r#"
        const NAME = "ball";
        pub fn label(hp: int, speed: float, alive: bool) -> str { "{NAME}: hp {hp}, speed {speed}, alive {alive}, {{braces}}" }
        pub fn same(a: str, b: str) -> bool { a == b + "" }
        pub fn size(s: str) -> int { s.len() }
    "#);
    assert_eq!(
        call(
            &mut rt,
            "label",
            &[Value::Int(7), Value::Float(2.0), Value::Bool(true)]
        ),
        Value::Str("ball: hp 7, speed 2.0, alive true, {braces}".into())
    );
    assert_eq!(
        call(
            &mut rt,
            "same",
            &[Value::Str("x".into()), Value::Str("x".into())]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        call(&mut rt, "size", &[Value::Str("héllo".into())]),
        Value::Int(6)
    );
}

#[test]
fn arrays_of_values_strings_and_structs() {
    let mut rt = run(r#"
        struct P { x: float, name: str }
        pub fn sum() -> float {
            let xs = [1.0, 2.0, 3.5];
            xs.push(4);
            let total = 0.0;
            for x in xs { total += x; }
            total
        }
        pub fn names() -> str {
            let ps: [P] = [];
            ps.push(P { x: 1.0, name: "a" });
            ps.push(P { x: 2.0, name: "b" });
            ps[1].x += 40;
            ps[0].name = ps[0].name + "!";
            let last = ps.pop();
            "{ps[0].name} {last.name} {last.x} {ps.len()}"
        }
        pub fn grid() -> int {
            let g = [[0; 3]; 2];
            g[1][2] = 5;
            g[1][2] + g.len() + g[0].len()
        }
    "#);
    assert_eq!(call(&mut rt, "sum", &[]), Value::Float(10.5));
    assert_eq!(
        call(&mut rt, "names", &[]),
        Value::Str("a! b 42.0 1".into())
    );
    assert_eq!(call(&mut rt, "grid", &[]), Value::Int(10));
}

#[test]
fn control_flow() {
    let mut rt = run(r"
        pub fn first_square_over(limit: int) -> int {
            let i = 0;
            loop {
                i += 1;
                if i * i <= limit { continue; }
                break;
            }
            i
        }
        pub fn evens(n: int) -> int {
            let count = 0;
            for i in 0..n { if i % 2 == 0 { count += 1; } }
            while count > 100 { return -1; }
            count
        }
    ");
    assert_eq!(
        call(&mut rt, "first_square_over", &[Value::Int(50)]),
        Value::Int(8)
    );
    assert_eq!(call(&mut rt, "evens", &[Value::Int(9)]), Value::Int(5));
}

#[test]
fn errors_name_the_line_and_the_problem() {
    assert_eq!(
        compile_error("pub fn f() -> int {\n    let x = 1;\n    x + true\n}"),
        "3:7: cannot combine int and bool"
    );
    assert_eq!(
        compile_error("fn f() { let s: str = 3; }"),
        "1:23: expected str, got int"
    );
    assert!(compile_error("fn f() { g(); }").contains("unknown function `g`"));
    assert!(compile_error("struct A { b: B }").contains("unknown type `B`"));
    assert!(compile_error("struct A { a: A }").contains("contains itself"));
    assert!(compile_error("fn f() { \"{x\" }").contains("without `}`"));
}
