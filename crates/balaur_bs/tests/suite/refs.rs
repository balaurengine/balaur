use super::{compile_error, run};
use balaur_bs::runtime::Value;

const EDITOR: &str = r#"
pub struct Tally { hits: int }
pub struct State { sel: str, history: [str], count: int, inner: Tally }

fn select(s: &mut State, id: str) {
    s.sel = id;
    s.history.push(id);
    s.count += 1;
    bump(s.inner);
}
fn bump(i: &mut Tally) { i.hits += 1; }
fn count(s: State) -> int { s.count }
fn two(a: &mut Tally, b: &mut Tally) -> int {
    a.hits += 10;
    b.hits += 100;
    a.hits + b.hits
}
fn copy(s: State) { s.count = 99; }

pub struct Editor { s: State, a: Tally, b: Tally, said: str }
impl Editor {
    fn tick(i: &mut Tally) -> int {
        i.hits += 1;
        i.hits
    }
    pub fn click() {
        select(self.s, "x");
        select(&mut self.s, "y");
        copy(self.s);
        let both = two(self.a, self.b);
        let local = Tally { hits: 1 };
        bump(local);
        let other = Editor { s: self.s, a: Tally { hits: 0 }, b: Tally { hits: 5 }, said: "" };
        let spare = Tally { hits: 0 };
        let ticked = other.tick(spare);
        self.said = "{self.s.sel} {self.s.history.len()} {count(self.s)} {self.s.inner.hits} {self.a.hits} {self.b.hits} {both} {local.hits} {ticked} {spare.hits}";
    }
}
"#;

#[test]
fn a_mut_parameter_is_the_callers_own_value() {
    let mut rt = run(EDITOR);
    let base = rt.live();
    let mut e = rt.object("Editor", &[]).unwrap();
    rt.call_method(&mut e, "click", &[]).unwrap();
    assert_eq!(
        rt.get(&e, "said"),
        Some(Value::Str("y 2 2 2 10 100 110 2 1 1".into())),
        "select twice, a copy ignored, two at once, a local, a method's own"
    );
    rt.drop_object(e);
    assert_eq!(rt.live(), base, "a `&mut` value leaked or was freed twice");
}

#[test]
fn a_mut_argument_is_a_place_and_changed_once() {
    let src = |body: &str| {
        format!(
            "pub struct I {{ n: int }}\nfn bump(i: &mut I) {{ i.n += 1; }}\n\
             fn two(a: &mut I, b: &mut I) {{}}\n\
             pub struct E {{ a: I }}\nimpl E {{\n    fn m(i: &mut I) {{}}\n    pub fn f() {{ {body} }}\n}}\n"
        )
    };
    let e = compile_error(&src("bump(I { n: 1 });"));
    assert!(e.contains("pass a variable or a field"), "{e}");
    let e = compile_error(&src("two(self.a, self.a);"));
    assert!(e.contains("change this twice"), "{e}");
    let e = compile_error(&src("self.m(self.a);"));
    assert!(e.contains("change this twice"), "{e}");
    let e = compile_error("pub struct I { n: int }\nasync fn w(i: &mut I) {}");
    assert!(e.contains("`async fn` takes no `&mut`"), "{e}");
}

#[test]
fn a_fn_value_may_take_the_callers_state() {
    let src = r#"
pub struct State { hits: int, last: str }
pub struct Row { name: str, on: fn(&mut State, str) }
fn open_row(s: &mut State, name: str) {
    s.hits += 1;
    s.last = name;
}
pub struct Editor { s: State, said: str }
impl Editor {
    pub fn click() {
        let row = Row { name: "a.bs", on: open_row };
        let on = row.on;
        on(self.s, row.name);
        on(self.s, "b.bs");
        self.said = "{self.s.hits} {self.s.last}";
    }
}
"#;
    let mut rt = run(src);
    let base = rt.live();
    let mut e = rt.object("Editor", &[]).unwrap();
    rt.call_method(&mut e, "click", &[]).unwrap();
    assert_eq!(rt.get(&e, "said"), Some(Value::Str("2 b.bs".into())));
    rt.drop_object(e);
    assert_eq!(rt.live(), base);
}
