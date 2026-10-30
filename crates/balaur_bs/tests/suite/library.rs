use super::{call, run};
use balaur_bs::runtime::Value;

fn strs(items: &[&str]) -> Value {
    Value::Array(items.iter().map(|s| Value::Str((*s).into())).collect())
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
