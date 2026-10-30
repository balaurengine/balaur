use super::{call, run};
use balaur_bs::runtime::Value;

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

const QUOTED: &str = r#"
pub fn quoted(hit: any) -> str {
    "{hit.has("tags")} {hit["tags"][1].as_str()} {"a}}b"} {{x}} {"say \"hi\""}"
}
"#;

#[test]
fn a_string_inside_an_interpolation_keeps_its_quotes() {
    let mut rt = run(QUOTED);
    let hit = Value::Map(vec![(
        "tags".into(),
        Value::Array(vec![Value::Str("fire".into()), Value::Str("ice".into())]),
    )]);
    assert_eq!(
        call(&mut rt, "quoted", &[hit]),
        Value::Str(r#"true ice a}b {x} say "hi""#.into())
    );
    assert_eq!(rt.live(), 0);
}

#[test]
fn an_int_prints_as_rust_prints_it() {
    let mut rt = run("pub fn show(x: int) -> str { \"<{x}>\" }");
    for x in [0, 7, -7, 1_234_567_890, i64::MAX, i64::MIN] {
        assert_eq!(
            call(&mut rt, "show", &[Value::Int(x)]),
            Value::Str(format!("<{x}>"))
        );
    }
    assert_eq!(rt.live(), 0);
}

#[test]
fn a_string_splits_once_and_into_its_characters() {
    let mut rt = run(r##"
pub fn path(s: str) -> str {
    let parts = s.split_once("#");
    if parts.len() == 2 { "{parts[0]}|{parts[1]}" } else { "none" }
}
pub fn letters(s: str) -> int { s.chars().len() }
"##);
    assert_eq!(
        call(&mut rt, "path", &[Value::Str("a.png#tile#2".into())]),
        Value::Str("a.png|tile#2".into())
    );
    assert_eq!(
        call(&mut rt, "path", &[Value::Str("a.png".into())]),
        Value::Str("none".into())
    );
    assert_eq!(
        call(&mut rt, "letters", &[Value::Str("ăbc".into())]),
        Value::Int(3)
    );
}

// The prelude's methods against Rust's own, over text that is not all ASCII.
const PARITY: &str = "
pub fn up(text: str) -> str { text.to_upper() }
pub fn low(text: str) -> str { text.to_lower() }
pub fn trimmed(text: str) -> str { text.trim() }
pub fn int_of(text: str) -> int { text.parse_int() }
pub fn parts(text: str, sep: str) -> [str] { text.split(sep) }
pub fn swapped(text: str, from: str, to: str) -> str { text.replace(from, to) }
pub fn cut(text: str, start: int, end: int) -> str { text.slice(start, end) }
";

// Case maps a character at a time, so a final sigma stays `σ` where Rust's
// `str::to_lowercase` writes `ς`.
#[test]
fn a_string_cases_trims_and_slices_as_rust_does() {
    let mut rt = run(PARITY);
    let string = |text: &str| Value::Str(text.into());
    let texts = [
        "",
        "a",
        "Hello, World",
        "straße",
        "ǅungla",
        "ΣΑΣ σας",
        "İstanbul",
        "ﬃ ŉ ΐ",
        "日本語",
        " \u{3000}x\u{2028}\u{85} ",
        "\tmixed Ünïcödé\n",
    ];
    for text in texts {
        let up: String = text.chars().flat_map(char::to_uppercase).collect();
        let low: String = text.chars().flat_map(char::to_lowercase).collect();
        assert_eq!(
            call(&mut rt, "up", &[string(text)]),
            string(&up),
            "{text:?}"
        );
        assert_eq!(
            call(&mut rt, "low", &[string(text)]),
            string(&low),
            "{text:?}"
        );
        assert_eq!(
            call(&mut rt, "trimmed", &[string(text)]),
            string(text.trim()),
            "{text:?}"
        );
        for (start, end) in [(0, 2), (1, 3), (2, 99), (-4, 5), (5, 1)] {
            let clamp = |at: i64| {
                let mut at = usize::try_from(at.max(0)).unwrap_or(0).min(text.len());
                while !text.is_char_boundary(at) {
                    at -= 1;
                }
                at
            };
            let (from, to) = (clamp(start), clamp(end));
            assert_eq!(
                call(
                    &mut rt,
                    "cut",
                    &[string(text), Value::Int(start), Value::Int(end)]
                ),
                string(&text[from..to.max(from)]),
                "{text:?} {start}..{end}"
            );
        }
    }
    assert_eq!(rt.live(), 0, "a string method leaked");
}

#[test]
fn a_string_parses_splits_and_replaces_as_rust_does() {
    let mut rt = run(PARITY);
    let string = |text: &str| Value::Str(text.into());
    for text in [
        "0",
        "-0",
        "+7",
        "-12345",
        " 42 ",
        "9223372036854775807",
        "-9223372036854775808",
        "9223372036854775808",
        "-",
        "+",
        "",
        "1_000",
        "12a",
        "٣",
    ] {
        let want = text.trim().parse::<i64>().unwrap_or(0);
        assert_eq!(
            call(&mut rt, "int_of", &[string(text)]),
            Value::Int(want),
            "{text:?}"
        );
    }
    for (text, sep) in [
        ("a,b,,c,", ","),
        ("abc", ""),
        ("", ","),
        ("aaa", "aa"),
        ("ünï", ""),
    ] {
        let want: Vec<String> = if sep.is_empty() {
            text.chars().map(String::from).collect()
        } else {
            text.split(sep).map(String::from).collect()
        };
        let want: Vec<&str> = want.iter().map(String::as_str).collect();
        assert_eq!(
            call(&mut rt, "parts", &[string(text), string(sep)]),
            strs(&want),
            "{text:?} {sep:?}"
        );
    }
    for (text, from, to) in [
        ("aaaa", "aa", "b"),
        ("abc", "", "x"),
        ("ünï", "n", "N"),
        ("", "a", "b"),
    ] {
        let want = if from.is_empty() {
            text.to_string()
        } else {
            text.replace(from, to)
        };
        assert_eq!(
            call(
                &mut rt,
                "swapped",
                &[string(text), string(from), string(to)]
            ),
            string(&want),
            "{text:?}"
        );
    }
    assert_eq!(rt.live(), 0, "a string method leaked");
}
