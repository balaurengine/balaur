// What one operation costs a script, held to a ceiling (PLAN-balaur-script
// §9 step 8). Counted, not timed, so CI can gate on it: the fuel wasmi
// charges is the same on every machine, and every call out of the module is
// counted where it leaves. None of these touches the engine, so none may
// leave.

use balaur_bs::runtime::{EngineKind, HostFns, Limits, Runtime, Value};
use balaur_bs::{Options, compile};

const RUNS: i64 = 200;
const FUEL: u64 = 1 << 40;

const DECLS: &str = "struct Item { weight: float, name: str, tags: [str] }";

const SETUP: &str = r#"
    let greeting = "hello world";
    let padded = "  padded  ";
    let wide = "Ünïcödé Straße";
    let scores = map().with("a", 1).with("b", 2).with("c", 3).with("d", 4).with("e", 5).with("f", 6).with("g", 7).with("h", 8);
    let tags: [any] = ["a", "b", "c"];
    let letters: any = tags;
    let item = Item { weight: 1.5, name: "item", tags: ["a", "b"] };
    let boxed: any = item;
    let typed: {str: int} = {"a": 1, "b": 2, "c": 3, "d": 4, "e": 5, "f": 6, "g": 7, "h": 8};
"#;

// The operation, and its ceiling in fuel each time it runs: what it spent on
// 2026-10-27, and a quarter more.
const CASES: &[(&str, u64)] = &[
    (r#"if greeting.contains("wor") { total += 1; }"#, 3000),
    (r#"total += greeting.find("wor");"#, 3000),
    (
        r#"if greeting.starts_with("hel") && greeting.ends_with("rld") { total += 1; }"#,
        2000,
    ),
    ("total += greeting.slice(2, 7).len();", 1100),
    ("total += greeting.to_upper().len();", 4700),
    ("total += wide.to_lower().len();", 55000),
    ("total += padded.trim().len();", 3400),
    (r#"total += greeting.replace("o", "0").len();"#, 10200),
    (r#"total += greeting.split(" ").len();"#, 5900),
    ("total += wide.chars().len();", 10800),
    (r#"total += "-12345".parse_int();"#, 2400),
    (r#"total += scores.get("e").as_int();"#, 1550),
    (r#"scores.set("e", step);"#, 1600),
    (r#"if scores.has("h") { total += scores.len(); }"#, 1950),
    ("total += scores.keys().len();", 2200),
    (
        r#"let wider = scores.with("e", step); total += wider.len();"#,
        8200,
    ),
    (r#"total += typed["e"].or(0);"#, 700),
    (r#"typed["e"] = step;"#, 760),
    (r#"if typed.has("h") { total += typed.len(); }"#, 680),
    ("total += letters.at(1).as_str().len();", 560),
    ("if letters == letters { total += 1; }", 3800),
    ("total += scores.text().len();", 28600),
    ("let cell: any = item; total += cell.len();", 3200),
    ("let back: Item = boxed; total += back.tags.len();", 5400),
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
