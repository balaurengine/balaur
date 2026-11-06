//! What one operation costs a script, held to a ceiling (PLAN-balaur-script
//! §9 step 8). Counted, not timed, so CI can gate on it: the fuel wasmi
//! charges is the same on every machine, and every call out of the module is
//! counted where it leaves. None of these touches the engine, so none may
//! leave.

use balaur_bs::runtime::{EngineKind, HostFns, Limits, Runtime, Value};
use balaur_bs::{Options, compile};

const RUNS: i64 = 200;
const FUEL: u64 = 1 << 40;

const DECLS: &str = "struct Item { weight: float, name: str, tags: [str] }";

const SETUP: &str = r#"
    let greeting = "hello world";
    let padded = "  padded  ";
    let wide = "Ünïcödé Straße";
    let scores: any = { "a": 1, "b": 2, "c": 3, "d": 4, "e": 5, "f": 6, "g": 7, "h": 8 };
    let tags: [any] = ["a", "b", "c"];
    let letters: any = tags;
    let item = Item { weight: 1.5, name: "item", tags: ["a", "b"] };
    let boxed: any = item;
    let typed: {str: int} = {"a": 1, "b": 2, "c": 3, "d": 4, "e": 5, "f": 6, "g": 7, "h": 8};
"#;

// The operation, and its ceiling in fuel each time it runs: what it spent on
// 2026-11-06, and a quarter more.
const CASES: &[(&str, u64)] = &[
    (r#"if greeting.contains("wor") { total += 1; }"#, 290),
    (r#"total += greeting.find("wor");"#, 280),
    (
        r#"if greeting.starts_with("hel") && greeting.ends_with("rld") { total += 1; }"#,
        680,
    ),
    ("total += greeting.slice(2, 7).len();", 710),
    ("total += greeting.to_upper().len();", 3450),
    ("total += wide.to_lower().len();", 32970),
    ("total += padded.trim().len();", 2000),
    (r#"total += greeting.replace("o", "0").len();"#, 5510),
    (r#"total += greeting.split(" ").len();"#, 2100),
    ("total += wide.chars().len();", 7490),
    (r#"total += "-12345".parse_int();"#, 1510),
    (r#"total += scores["e"].as_int();"#, 340),
    (r#"scores["e"] = step;"#, 490),
    (r#"if scores.has("h") { total += scores.len(); }"#, 370),
    ("total += scores.keys().len();", 2000),
    (r#"total += typed["e"].or(0);"#, 300),
    (r#"typed["e"] = step;"#, 360),
    (r#"if typed.has("h") { total += typed.len(); }"#, 270),
    ("total += letters[1].as_str().len();", 210),
    ("if letters == letters { total += 1; }", 1930),
    ("total += scores.text().len();", 24030),
    ("let cell: any = item; total += cell.len();", 2950),
    ("let back: Item = boxed; total += back.tags.len();", 2050),
];

// What `op`, run `RUNS` times, spent beyond an empty loop, and the calls out.
fn cost(op: &str) -> (u64, u64) {
    let src = format!(
        "{DECLS}\n\npub fn run(count: int) -> int {{\n{SETUP}\n    let total = 0;\n    for step in 0..count {{\n        {op}\n    }}\n    total\n}}\n"
    );
    let compiled =
        compile(&src, &Options::default()).unwrap_or_else(|error| panic!("{error}\n{src}"));
    let limits = Limits {
        fuel: Some(FUEL),
        engine: EngineKind::Wasmi,
        ..Limits::default()
    };
    let mut rt = Runtime::new(&compiled, HostFns::default(), limits)
        .unwrap_or_else(|error| panic!("{error}"));
    let before = rt.host_calls();
    rt.call("run", &[Value::Int(RUNS)])
        .unwrap_or_else(|error| panic!("{op}: {error}"));
    let spent = FUEL - rt.fuel_left().expect("wasmi meters fuel");
    (spent, rt.host_calls() - before)
}

// What each operation spends, to set a ceiling from and for the plan's tables:
// `cargo test -p balaur_bs --test suite every_cost -- --ignored --nocapture`.
#[test]
#[ignore = "prints; the gate is the test below"]
fn every_cost() {
    let (empty, _) = cost("total += step;");
    for (op, ceiling) in CASES {
        let (spent, calls) = cost(op);
        let each = spent.saturating_sub(empty) / RUNS.unsigned_abs();
        println!("{each:>7} of {ceiling:>6}, {calls} out: {op}");
    }
}

#[test]
fn no_operation_leaves_the_module_or_passes_its_budget() {
    let (empty, _) = cost("total += step;");
    let mut over = vec![];
    for (op, ceiling) in CASES {
        let (spent, calls) = cost(op);
        let each = spent.saturating_sub(empty) / RUNS.unsigned_abs();
        if calls > 0 || each > *ceiling {
            over.push(format!(
                "{op}: {each} fuel (ceiling {ceiling}), {calls} calls out"
            ));
        }
    }
    assert!(over.is_empty(), "{over:#?}");
}
