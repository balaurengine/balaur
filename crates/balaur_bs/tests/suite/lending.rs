//! What a prelude function, a built-in and a host call are lent rather than
//! handed: nothing freed under them, nothing left behind, and every string
//! search and map lookup answering as Rust's would.

use super::{call, run};
use balaur_bs::runtime::Value;

fn text(value: &str) -> Value {
    Value::Str(value.to_string())
}

fn whole(count: usize) -> i64 {
    i64::try_from(count).expect("a test's string is short")
}

const LENT: &str = r#"
    fn swapped(held: &mut str) -> str {
        held = "not " + "this";
        "hello"
    }

    pub fn assigned_by_a_later_argument() -> bool {
        let held = "hello " + "world";
        held.contains(if true { held = "gone"; "hello" } else { "x" })
    }

    pub fn changed_by_a_later_call() -> bool {
        let held = "hello " + "world";
        held.contains(swapped(held))
    }

    pub fn left_by_a_later_argument(leave: bool) -> int {
        let held = "hello " + "world";
        let boxed: any = [held + "!"];
        if held.contains(if leave { return boxed.len(); } else { "hello" }) {
            return boxed.len() + 1;
        }
        0
    }

    pub fn handed_back(given: str) -> str {
        given.trim()
    }

    pub fn each_method(given: str) -> int {
        let box_of: any = { "a": given, "b": [given, given] };
        let total = 0;
        for round in 0..50 {
            total += given.len() + given.find("z") + given.to_upper().len();
            total += given.replace("a", "bb").split("b").len();
            total += box_of["a"].as_str().len() + box_of["b"][1].as_str().len();
            box_of["c"] = round;
            box_of.remove("c");
            if box_of == box_of && box_of.has("a") {
                total += box_of.keys().len() + box_of.text().len();
            }
        }
        total
    }

    pub class Named { name: str }
    pub fn field_compared(given: str) -> bool {
        let named = Named { name: given + "!" };
        let names = [given + "?", named.name];
        named.name == given + "!" && names[0] != named.name && names[1] == named.name
    }
"#;

#[test]
fn a_later_argument_assigning_a_local_leaves_what_was_lent_alive() {
    let mut rt = run(LENT);
    assert_eq!(
        call(&mut rt, "assigned_by_a_later_argument", &[]),
        Value::Bool(true)
    );
    assert_eq!(rt.live(), 0);
}

#[test]
fn a_later_call_changing_a_local_leaves_what_was_lent_alive() {
    let mut rt = run(LENT);
    assert_eq!(
        call(&mut rt, "changed_by_a_later_call", &[]),
        Value::Bool(true)
    );
    assert_eq!(rt.live(), 0);
}

#[test]
fn leaving_in_a_later_argument_releases_what_is_held() {
    let mut rt = run(LENT);
    for (leave, answer) in [(true, 1), (false, 2)] {
        assert_eq!(
            call(&mut rt, "left_by_a_later_argument", &[Value::Bool(leave)]),
            Value::Int(answer)
        );
        assert_eq!(rt.live(), 0);
    }
}

#[test]
fn a_function_handing_back_what_it_was_lent_counts_it() {
    let mut rt = run(LENT);
    for given in ["kept", "  trimmed  "] {
        assert_eq!(
            call(&mut rt, "handed_back", &[text(given)]),
            text(given.trim())
        );
        assert_eq!(rt.live(), 0);
    }
}

#[test]
fn no_method_of_a_string_or_an_any_leaves_anything_behind() {
    let mut rt = run(LENT);
    let Value::Int(total) = call(&mut rt, "each_method", &[text("banana")]) else {
        panic!("each_method answers an int");
    };
    assert!(total > 0);
    assert_eq!(rt.live(), 0);
}

#[test]
fn a_string_is_compared_where_a_class_or_a_list_holds_it() {
    let mut rt = run(LENT);
    assert_eq!(
        call(&mut rt, "field_compared", &[text("name")]),
        Value::Bool(true)
    );
    assert_eq!(rt.live(), 0);
}

const SEARCHES: &str = r"
    pub fn same(first: str, second: str) -> bool { first == second }
    pub fn differ(first: str, second: str) -> bool { first != second }
    pub fn find(text: str, needle: str) -> int { text.find(needle) }
    pub fn contains(text: str, needle: str) -> bool { text.contains(needle) }
    pub fn starts_with(text: str, needle: str) -> bool { text.starts_with(needle) }
    pub fn ends_with(text: str, needle: str) -> bool { text.ends_with(needle) }
    pub fn replace(text: str, from: str, to: str) -> str { text.replace(from, to) }
    pub fn parts(text: str, sep: str) -> int { text.split(sep).len() }
";

// A short walk over a small alphabet, so needles are found and nearly found.
fn words() -> Vec<String> {
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    (0..120)
        .map(|at| {
            let length = (next() % if at % 3 == 0 { 4 } else { 40 }) as usize;
            (0..length)
                .map(|_| ["a", "a", "a", "b", "é"][(next() % 5) as usize])
                .collect()
        })
        .collect()
}

#[test]
fn every_search_answers_as_rusts_does() {
    let mut rt = run(SEARCHES);
    let words = words();
    for pair in words.windows(2) {
        let (hay, needle) = (&pair[0], &pair[1]);
        let args = [text(hay), text(needle)];
        let found = hay.find(needle.as_str()).map_or(-1, whole);
        assert_eq!(call(&mut rt, "find", &args), Value::Int(found));
        assert_eq!(
            call(&mut rt, "contains", &args),
            Value::Bool(hay.contains(needle.as_str()))
        );
        assert_eq!(
            call(&mut rt, "starts_with", &args),
            Value::Bool(hay.starts_with(needle.as_str()))
        );
        assert_eq!(
            call(&mut rt, "ends_with", &args),
            Value::Bool(hay.ends_with(needle.as_str()))
        );
        assert_eq!(
            call(&mut rt, "same", &args),
            Value::Bool(hay == needle),
            "{hay} {needle}"
        );
        // A needle cut from the text itself is found, where a walk may miss it.
        let middle = hay.char_indices().nth(hay.chars().count() / 2);
        if let Some((cut, _)) = middle {
            let piece = &hay[cut..];
            assert_eq!(
                call(&mut rt, "find", &[text(hay), text(piece)]),
                Value::Int(hay.find(piece).map_or(-1, whole))
            );
        }
        if !needle.is_empty() {
            assert_eq!(
                call(&mut rt, "replace", &[text(hay), text(needle), text("X")]),
                text(&hay.replace(needle.as_str(), "X"))
            );
            assert_eq!(
                call(&mut rt, "parts", &args),
                Value::Int(whole(hay.split(needle.as_str()).count()))
            );
        }
    }
    assert_eq!(rt.live(), 0);
}

#[test]
fn strings_differ_at_their_last_byte_whatever_their_length() {
    let mut rt = run(SEARCHES);
    for length in 0..40 {
        let same = "x".repeat(length);
        let args = [text(&same), text(&same)];
        assert_eq!(call(&mut rt, "same", &args), Value::Bool(true));
        assert_eq!(call(&mut rt, "differ", &args), Value::Bool(false));
        assert_eq!(call(&mut rt, "same", &[text(&same), text("")]), {
            Value::Bool(length == 0)
        });
        if length > 0 {
            let other = format!("{}y", "x".repeat(length - 1));
            let args = [text(&same), text(&other)];
            assert_eq!(call(&mut rt, "same", &args), Value::Bool(false));
            assert_eq!(call(&mut rt, "differ", &args), Value::Bool(true));
        }
    }
    assert_eq!(rt.live(), 0);
}

const LOOKUPS: &str = r#"
    pub fn read(key: str) -> int {
        let map: any = { "": 1, "a": 2, "b": 3, "ab": 4, "ba": 5, "abcdefghij": 6, "abcdefghik": 7 };
        map[key].as_int()
    }

    pub fn holds(key: str) -> bool {
        let map: any = { "": 1, "a": 2 };
        map.has(key)
    }

    pub fn empty_reads(key: str) -> bool {
        let map: any = {};
        map[key] == nil && !map.has(key) && map.remove(key) == nil
    }

    pub fn written_twice(key: str) -> int {
        let map: any = { "k": 1 };
        map[key] = 2;
        map[key] = 3;
        map.len() * 10 + map[key].as_int()
    }

    pub fn taken_out(key: str) -> int {
        let map: any = { "a": 1, "b": 2 };
        let taken = map.remove(key).as_int();
        taken * 10 + map.len()
    }
"#;

#[test]
fn a_key_is_found_by_its_text_wherever_it_lives() {
    let mut rt = run(LOOKUPS);
    let keys = [
        ("", 1),
        ("a", 2),
        ("b", 3),
        ("ab", 4),
        ("ba", 5),
        ("abcdefghij", 6),
        ("abcdefghik", 7),
        ("c", 0),
        ("abcdefghi", 0),
    ];
    for (key, answer) in keys {
        assert_eq!(call(&mut rt, "read", &[text(key)]), Value::Int(answer));
    }
    assert_eq!(call(&mut rt, "holds", &[text("")]), Value::Bool(true));
    assert_eq!(call(&mut rt, "holds", &[text("b")]), Value::Bool(false));
    assert_eq!(call(&mut rt, "empty_reads", &[text("")]), Value::Bool(true));
    assert_eq!(call(&mut rt, "written_twice", &[text("k")]), Value::Int(13));
    assert_eq!(call(&mut rt, "written_twice", &[text("j")]), Value::Int(23));
    assert_eq!(call(&mut rt, "taken_out", &[text("b")]), Value::Int(21));
    assert_eq!(call(&mut rt, "taken_out", &[text("z")]), Value::Int(2));
    assert_eq!(rt.live(), 0);
}

#[test]
fn a_prelude_function_is_no_fn_value() {
    let message = super::compile_error("pub fn run() -> int { let found = str::find; 0 }");
    assert!(message.contains("`fn` value"), "{message}");
}

// A browser's engine meters nothing, so a scan the prelude's built-in runs is
// charged as the loop it replaced was.
#[test]
fn a_metered_module_pays_for_a_scan() {
    use balaur_bs::runtime::{HostFns, Limits, Runtime};
    const SCAN: &str = r#"
        pub fn scan() -> int {
            let long = "a";
            for doubling in 0..20 {
                long = long + long;
            }
            long.find("b")
        }
    "#;
    let options = balaur_bs::Options {
        meter: true,
        ..balaur_bs::Options::default()
    };
    let compiled = balaur_bs::compile(SCAN, &options).unwrap();
    // Enough to make the module and the string, far from enough to scan it.
    let limits = Limits {
        fuel: Some(20_000),
        ..Limits::default()
    };
    let mut rt = Runtime::new(&compiled, HostFns::default(), limits).unwrap();
    let error = rt.call("scan", &[]).unwrap_err();
    assert!(error.msg.contains("fuel"), "{}", error.msg);
}
