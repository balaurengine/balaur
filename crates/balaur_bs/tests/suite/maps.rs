// `{str: V}` and `{int: V}`: built in, typed, a hash table in the module,
// held to Rust's own map and to the order keys were first set.

use super::{call, compile_error, run};
use balaur_bs::runtime::Value;
use std::collections::HashMap;

const SCRIPT: &str = r#"
pub class Player { name: str, score: int }

pub fn ages() -> {str: int} {
    let ages: {str: int} = {"ada": 36, "alan": 41};
    ages["grace"] = 85;
    ages["ada"] += 1;
    ages
}

pub fn read(name: str) -> int {
    let ages = ages();
    ages[name].or(-1)
}

pub fn order() -> [str] {
    let ages = ages();
    ages["alan"] = 42;
    ages.remove("ada");
    ages["ada"] = 1;
    ages.keys()
}

pub fn counted(words: [str]) -> [int] {
    let counts: {str: int} = {};
    for word in words {
        if counts.has(word) {
            counts[word] += 1;
        } else {
            counts[word] = 1;
        }
    }
    let out: [int] = [];
    for word in counts.keys() {
        out.push(counts[word].unwrap());
    }
    out
}

pub fn squares(n: int) -> int {
    let squares: {int: int} = {};
    for i in 0..n {
        squares[i * 7] = i * i;
    }
    for i in 0..n {
        if i % 3 == 0 {
            squares.remove(i * 7);
        }
    }
    let total = 0;
    for value in squares.values() {
        total += value;
    }
    total + squares.len()
}

pub fn nested() -> int {
    let teams: {str: {str: int}} = {};
    teams["red"] = {"ada": 1};
    teams["red"].unwrap()["alan"] = 2;
    teams["blue"] = {};
    teams["red"].unwrap().len() * 10 + teams["blue"].unwrap().len()
}

pub fn players() -> int {
    let by_name: {str: Player} = {};
    let ada = Player { name: "ada", score: 3 };
    by_name["ada"] = ada;
    let held = by_name["ada"].unwrap();
    held.score += 4;
    ada.score
}

pub fn boxed() -> any {
    let ages: {str: int} = {"ada": 36, "alan": 41};
    let cell: any = ages;
    cell
}

pub fn unboxed() -> int {
    let cell: any = map().with("x", 3).with("y", 4);
    let point: {str: int} = cell;
    point["x"].unwrap() * 10 + point["y"].unwrap()
}

pub fn missing() -> int {
    let ages: {str: int} = {};
    ages["nobody"] += 1;
    0
}

pub fn shown() -> str {
    let ages: {str: int} = {"ada": 36};
    "{ages}"
}
"#;

#[test]
fn a_map_reads_writes_and_keeps_the_order_keys_were_set() {
    let mut rt = run(SCRIPT);
    assert_eq!(
        call(&mut rt, "read", &[Value::Str("ada".into())]),
        Value::Int(37)
    );
    assert_eq!(
        call(&mut rt, "read", &[Value::Str("grace".into())]),
        Value::Int(85)
    );
    assert_eq!(
        call(&mut rt, "read", &[Value::Str("nobody".into())]),
        Value::Int(-1)
    );
    assert_eq!(
        call(&mut rt, "order", &[]),
        Value::Array(vec![
            Value::Str("alan".into()),
            Value::Str("grace".into()),
            Value::Str("ada".into()),
        ])
    );
    assert_eq!(rt.live(), 0, "a map let go of frees its entries");
}

#[test]
fn a_map_counts_as_rusts_does_across_growth_and_removal() {
    let mut rt = run(SCRIPT);
    let words: Vec<String> = (0..400).map(|i| format!("w{}", (i * 37) % 97)).collect();
    let mut first_seen: Vec<String> = vec![];
    let mut counts: HashMap<String, i64> = HashMap::new();
    for word in &words {
        if !counts.contains_key(word) {
            first_seen.push(word.clone());
        }
        *counts.entry(word.clone()).or_default() += 1;
    }
    let want: Vec<Value> = first_seen.iter().map(|w| Value::Int(counts[w])).collect();
    let given = Value::Array(words.into_iter().map(Value::Str).collect());
    assert_eq!(call(&mut rt, "counted", &[given]), Value::Array(want));

    let n = 300;
    let kept: Vec<i64> = (0..n).filter(|i| i % 3 != 0).collect();
    let want = kept.iter().map(|i| i * i).sum::<i64>() + i64::try_from(kept.len()).unwrap();
    assert_eq!(call(&mut rt, "squares", &[Value::Int(n)]), Value::Int(want));
    assert_eq!(rt.live(), 0);
}

#[test]
fn a_map_holds_maps_and_classes_by_reference() {
    let mut rt = run(SCRIPT);
    assert_eq!(call(&mut rt, "nested", &[]), Value::Int(20));
    assert_eq!(call(&mut rt, "players", &[]), Value::Int(7));
    assert_eq!(rt.live(), 0);
}

#[test]
fn a_str_keyed_map_is_an_any_map_and_back() {
    let mut rt = run(SCRIPT);
    let Value::Map(entries) = call(&mut rt, "boxed", &[]) else {
        panic!("not a map");
    };
    assert_eq!(
        entries,
        vec![
            ("ada".to_string(), Value::Int(36)),
            ("alan".to_string(), Value::Int(41)),
        ]
    );
    assert_eq!(call(&mut rt, "unboxed", &[]), Value::Int(34));
    assert_eq!(
        call(&mut rt, "shown", &[]),
        Value::Str(r#"{"ada": 36}"#.into())
    );
    assert_eq!(rt.live(), 0);
}

#[test]
fn adding_to_a_missing_key_traps() {
    let mut rt = run(SCRIPT);
    assert!(rt.call("missing", &[]).is_err());
}

#[test]
fn a_map_type_is_checked_where_it_is_written() {
    assert!(
        compile_error("pub fn f() { let m: {float: int} = {}; }")
            .contains("a map's key is a `str` or an `int`")
    );
    assert!(
        compile_error("pub fn f() { let m: {str: int?} = {}; }")
            .contains("a map's value is never nil")
    );
    assert!(compile_error("pub fn f() { let m = {\"a\": 1}; }").contains("typed by where it goes"));
    assert!(
        compile_error("pub fn f() -> any { let m: {int: int} = {}; let a: any = m; a }")
            .contains("{ int: int }")
    );
}
