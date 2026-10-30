// `{str: V}` and `{int: V}`: built in, typed, a hash table in the module,
// held to Rust's own map and to the order keys were first set.

use super::{call, run};
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
