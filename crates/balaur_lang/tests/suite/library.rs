use super::{call, run};
use balaur_lang::runtime::Value;

const SCRIPT: &str = r##"
pub fn words(s: str) -> [str] { s.split(" ") }
pub fn shout(s: str) -> str { s.trim().to_upper() }
pub fn whisper(s: str) -> str { s.to_lower() }
pub fn tagged(s: str) -> bool { s.starts_with("#") && s.ends_with("!") && s.contains("go") }
pub fn where_is(s: str, t: str) -> int { s.find(t) }
pub fn cut(s: str, a: int, b: int) -> str { s.slice(a, b) }
pub fn swap(s: str) -> str { s.replace("cat", "dog") }
pub fn numbers(s: str) -> float {
    let total = 0.0;
    for part in s.split(",") {
        total += part.parse_float();
    }
    total + "7".parse_int()
}

pub fn shuffle() -> [int] {
    let xs = [1, 2, 3];
    xs.insert(0, 0);
    xs.insert(4, 4);
    xs.insert(2, 9);
    let gone = xs.remove(2);
    xs.push(gone);
    xs
}

pub fn names() -> [str] {
    let xs = ["a", "b", "c"];
    let b = xs.remove(1);
    xs.insert(0, b);
    xs.insert(3, "{b}{b}");
    xs
}

pub fn past_the_end() { let xs = [1]; xs.insert(2, 5); }
pub fn remove_nothing() -> int { let xs: [int] = []; xs.remove(0) }
"##;

fn strs(items: &[&str]) -> Value {
    Value::Array(items.iter().map(|s| Value::Str((*s).into())).collect())
}

#[test]
fn strings_have_the_everyday_methods() {
    let mut rt = run(SCRIPT);
    let s = |x: &str| Value::Str(x.into());
    assert_eq!(
        call(&mut rt, "words", &[s("a bb c")]),
        strs(&["a", "bb", "c"])
    );
    assert_eq!(call(&mut rt, "shout", &[s("  hi  ")]), s("HI"));
    assert_eq!(call(&mut rt, "whisper", &[s("ŞTEFAN")]), s("ştefan"));
    assert_eq!(
        call(&mut rt, "tagged", &[s("#let's go!")]),
        Value::Bool(true)
    );
    assert_eq!(
        call(&mut rt, "tagged", &[s("let's go!")]),
        Value::Bool(false)
    );
    assert_eq!(
        call(&mut rt, "where_is", &[s("hello"), s("ll")]),
        Value::Int(2)
    );
    assert_eq!(
        call(&mut rt, "where_is", &[s("hello"), s("x")]),
        Value::Int(-1)
    );
    assert_eq!(
        call(&mut rt, "cut", &[s("hello"), Value::Int(1), Value::Int(3)]),
        s("el")
    );
    assert_eq!(
        call(&mut rt, "cut", &[s("hello"), Value::Int(3), Value::Int(99)]),
        s("lo")
    );
    // Offsets are bytes; one inside a character moves back onto it.
    assert_eq!(
        call(&mut rt, "cut", &[s("ăb"), Value::Int(1), Value::Int(2)]),
        s("ă")
    );
    assert_eq!(call(&mut rt, "swap", &[s("cat and cat")]), s("dog and dog"));
    assert_eq!(
        call(&mut rt, "numbers", &[s("1.5, 2,x")]),
        Value::Float(10.5)
    );
    assert_eq!(rt.live(), 0, "a string method leaked");
}

#[test]
fn arrays_insert_and_remove_anywhere() {
    let mut rt = run(SCRIPT);
    let ints = |xs: &[i64]| Value::Array(xs.iter().map(|x| Value::Int(*x)).collect());
    assert_eq!(call(&mut rt, "shuffle", &[]), ints(&[0, 1, 2, 3, 4, 9]));
    assert_eq!(call(&mut rt, "names", &[]), strs(&["b", "a", "c", "bb"]));
    assert_eq!(rt.live(), 0, "an insert or remove leaked");
    // A trap leaves what the call allocated behind; it only must not run on.
    assert!(rt.call("past_the_end", &[]).is_err());
    assert!(rt.call("remove_nothing", &[]).is_err());
}
