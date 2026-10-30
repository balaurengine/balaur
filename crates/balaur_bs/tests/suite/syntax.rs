use super::run;
use balaur_bs::runtime::Value;

#[test]
fn methods_have_an_implicit_mutable_self() {
    let mut rt = run(r"
        pub class Counter { n: int, step: int }
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
