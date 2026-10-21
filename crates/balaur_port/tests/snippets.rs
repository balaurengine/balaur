// The translator on snippets in the editor's style: what each Rune idiom
// becomes, and what is flagged rather than guessed.

use balaur_port::{Api, Input, MARK, port_files, port_source};

fn port(src: &str) -> String {
    port_source(src).bs
}

/// The output after its two-line header, for exact comparisons.
fn body(src: &str) -> String {
    let out = port(src);
    out.lines()
        .skip(3)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn has(out: &str, want: &str) {
    assert!(out.contains(want), "expected\n{want}\nin\n{out}");
}

fn lacks(out: &str, unwanted: &str) {
    assert!(
        !out.contains(unwanted),
        "did not expect\n{unwanted}\nin\n{out}"
    );
}

fn files(list: &[(&str, &str)]) -> Vec<balaur_port::Ported> {
    let inputs: Vec<Input> = list
        .iter()
        .map(|(path, src)| Input {
            path: (*path).to_string(),
            source: (*src).to_string(),
            key: None,
        })
        .collect();
    port_files(&inputs, Api::default())
}

#[test]
fn option_is_an_optional_type_and_nil() {
    let out = body(
        r#"
fn find(items, want) {
    for i in 0..items.len() {
        if items[i] == want {
            return Some(i);
        }
    }
    None
}

pub fn first(items) {
    match find(items, "a") {
        Some(i) => i,
        None => -1,
    }
}
"#,
    );
    has(&out, "fn find(items: [str], want: str) -> int? {");
    has(&out, "return i;");
    has(&out, "    nil\n}");
    has(&out, "let matched1 = find(items, \"a\");");
    has(
        &out,
        "if matched1.is_some() {\n        let i = matched1.unwrap();\n        i\n    } else {\n        -1\n    }",
    );
}

#[test]
fn a_match_that_leaves_on_none_is_a_check_then_the_value() {
    let out = body(
        r"
fn top(stack) {
    let last = match stack.pop() {
        Some(last) => last,
        None => return 0,
    };
    last + 1
}

pub fn run() {
    top([1, 2]);
}
",
    );
    has(
        &out,
        "let last_or = stack.pop();\n    if last_or.is_none() {\n        return 0;\n    }\n    let last = last_or.unwrap();",
    );
}

#[test]
fn a_nil_check_that_returns_unwraps_what_follows() {
    let out = body(
        r"
pub fn bump(S) {
    let d = S.check;
    if !(d is Object) {
        return;
    }
    d.waited = d.waited + 1;
}

pub fn fresh() {
    let S = #{ check: None };
    S.check = #{ waited: 0 };
    bump(S);
}
",
    );
    has(
        &out,
        "if d.is_none() {\n        return;\n    }\n    let d = d.unwrap();",
    );
    // The write goes to a copy: flagged, not silently wrong.
    has(
        &out,
        &format!("// {MARK}: `d` is a copy in .bs (structs are values)"),
    );
}

#[test]
fn object_literals_are_structs_named_where_they_sit() {
    let out = body(
        r#"
pub fn reset(S) {
    S.history = #{ past: [], saved: 0, key: None };
}

pub fn fresh_state() {
    let S = #{ zoom: 100.0, tool: "select", history: () };
    reset(S);
    S.history.past.push("first");
    S.history.key = "drag";
    S
}
"#,
    );
    has(
        &out,
        "pub class State {\n    zoom: float,\n    tool: str,\n    history: History?,\n}",
    );
    has(
        &out,
        "pub struct History {\n    past: [str],\n    saved: int,\n    key: str?,\n}",
    );
    has(
        &out,
        "S.history = History { past: [], saved: 0, key: nil };",
    );
    has(&out, "pub fn fresh_state() -> State {");
}

#[test]
fn a_literal_missing_fields_gets_their_zero_values() {
    let out = body(
        r#"
pub fn rows() {
    let out = [];
    out.push(#{ label: "a", height: 20.0 });
    out.push(#{ label: "b", full: true });
    out
}
"#,
    );
    has(
        &out,
        "pub struct Row {\n    label: str,\n    height: float,\n    full: bool,\n}",
    );
    has(
        &out,
        "out.push(Row { label: \"a\", height: 20.0, full: false });",
    );
    has(
        &out,
        "out.push(Row { label: \"b\", height: 0.0, full: true });",
    );
}

#[test]
fn an_object_filled_by_computed_keys_is_an_any_map() {
    let out = body(
        r#"
pub fn remember(S, key, value) {
    S.seen[key] = value;
}

pub fn fresh() {
    let S = #{ seen: #{} };
    remember(S, "k", 1);
    S
}
"#,
    );
    has(&out, "seen: any,");
    has(&out, "S.seen.set(key, value);");
    has(&out, "State { seen: map() }");
}

#[test]
fn an_object_handed_to_the_engine_is_built_as_a_map() {
    let out = body(
        r#"
pub fn hide(id) {
    let found = scene::get_node(id);
    found.patch_component("widget", #{ visible: false, text: id });
}
"#,
    );
    has(
        &out,
        "found.patch_component(\"widget\", map().with(\"visible\", false).with(\"text\", id));",
    );
    lacks(&out, "struct");
}

#[test]
fn match_on_strings_is_an_if_chain() {
    let out = port(
        r#"
pub fn key_label(k) {
    match k {
        "Space" => "␣",
        "Enter" | "Return" => "⏎",
        other => other,
    }
}
"#,
    );
    // Rune has no `|` patterns: the file does not parse, and says so.
    has(&out, "did not parse as Rune");
    let out = body(
        r#"
pub fn key_label(k) {
    match k {
        "Space" => "␣",
        "Enter" => "⏎",
        other => other,
    }
}
"#,
    );
    has(
        &out,
        "if k == \"Space\" {\n        \"␣\"\n    } else if k == \"Enter\" {\n        \"⏎\"\n    } else {\n        let other = k;\n        other\n    }",
    );
}

#[test]
fn format_and_template_strings_interpolate() {
    let out = body(
        r#"
pub fn say(run, n) {
    log::info(`suite: done ${run} after ${n} frames`);
    let a = format!("{} of {}", n, 10);
    let b = format!("{:.2}", 1.5);
    let c = "braces {} stay";
    a + b + c
}
"#,
    );
    has(&out, r#"log::info("suite: done {run} after {n} frames");"#);
    has(&out, r#"let a = "{n} of {10}";"#);
    has(
        &out,
        &format!("// {MARK}: `{{:.2}}`: .bs interpolation has no format specs"),
    );
    has(&out, r#"let c = "braces {{}} stay";"#);
}

#[test]
fn a_closure_handed_to_an_iterator_is_a_loop_in_a_helper() {
    let out = body(
        r#"
pub fn joined(held, topic) {
    held.iter().any(|name| name == topic)
}

pub fn run() {
    joined(["a"], "b");
}
"#,
    );
    has(
        &out,
        "pub fn joined(held: [str], topic: str) -> bool {\n    joined_any_0(topic, held)\n}",
    );
    has(
        &out,
        "fn joined_any_0(topic: str, items: [str]) -> bool {\n    for name in items {\n        if name == topic {\n            return true;\n        }\n    }\n    false\n}",
    );
    lacks(&out, MARK);
}

#[test]
fn a_capturing_closure_is_a_named_function_and_flagged() {
    let p = port_source(
        r#"
pub fn chip(label, on) {
    #{ label: label, on: on }
}

pub fn row(url) {
    chip("Open", || { engine::open_url(url); })
}

pub fn run() {
    row("https://balaurengine.org");
}
"#,
    );
    has(&p.bs, "chip(\"Open\", row_on_0)");
    has(
        &p.bs,
        "fn row_on_0(url: str) {\n    engine::open_url(url);\n}",
    );
    has(
        &p.bs,
        &format!(
            "// {MARK}: the closure read `url`: `row_on_0` takes them first, but a fn value cannot carry them"
        ),
    );
    has(
        &p.bs,
        "pub struct Chip {\n    label: str,\n    on: fn(),\n}",
    );
    assert!(
        p.report
            .flags
            .iter()
            .any(|f| f.kind == "capture" && f.line == 7)
    );
}

#[test]
fn a_script_is_a_struct_with_its_hooks_as_methods() {
    let out = body(
        r"
pub fn exports() {
    #{ speed: 2.0, clockwise: true }
}

pub fn init(this) {
    this.angle = 0.0;
    this.direction = if this.clockwise { 1.0 } else { -1.0 };
}

pub fn update(this, dt) {
    this.angle += dt * this.speed * this.direction;
    spin(this);
}

fn spin(this) {
    this.node.transform.rotation_euler = [0.0, this.angle, 0.0];
}
",
    );
    has(
        &out,
        "pub struct Snippet {\n    pub speed: float = 2.0,\n    pub clockwise: bool = true,\n    node: Node,\n    angle: float,\n    direction: float,\n}",
    );
    has(
        &out,
        "impl Snippet {\n    pub fn init() {\n        self.angle = 0.0;\n        self.direction = if self.clockwise { 1.0 } else { -1.0 };\n    }",
    );
    has(
        &out,
        "    pub fn update(dt: float) {\n        self.angle += dt * self.speed * self.direction;\n        self.spin();\n    }",
    );
    lacks(&out, "exports");
}

#[test]
fn enumerate_is_an_index_loop_and_tuples_are_structs() {
    let out = body(
        r"
fn span(xs) {
    let lo = 0.0;
    let hi = 0.0;
    for (i, x) in xs.iter().enumerate() {
        if i == 0 || x < lo { lo = x; }
        if x > hi { hi = x; }
    }
    (lo, hi)
}

pub fn width(xs) {
    let (lo, hi) = span(xs);
    hi - lo
}

pub fn run() {
    width([1.0, 2.0]);
}
",
    );
    has(&out, "pub struct Span {\n    _0: float,\n    _1: float,\n}");
    has(&out, "for i in 0..xs.len() {\n        let x = xs[i];");
    has(&out, "Span { _0: lo, _1: hi }");
    has(
        &out,
        "let lo_hi_1 = span(xs);\n    let lo = lo_hi_1._0;\n    let hi = lo_hi_1._1;",
    );
}

#[test]
fn a_numeric_helper_keeps_ints_int() {
    let ported = files(&[
        (
            "scripts/util.rn",
            "pub fn max(a, b) {\n    if a > b { a } else { b }\n}\n",
        ),
        (
            "scripts/sel.rn",
            "use crate::util;\n\npub fn widest(w) {\n    util::max(w, 0.5)\n}\n\npub fn last(n) {\n    let i = util::max(n - 1, 0);\n    i\n}\n\npub fn run() {\n    widest(2.0);\n    last(3);\n}\n",
        ),
    ]);
    let sel = &ported[1].bs;
    has(sel, "use util;");
    has(sel, "pub fn last(n: int) -> int {");
    has(sel, "pub fn widest(w: float) -> float {");
    has(&ported[0].bs, "pub fn max(a: float, b: float) -> float {");
}

#[test]
fn a_bare_module_path_is_the_engine_unless_used() {
    let ported = files(&[
        ("scripts/settings.rn", "pub fn panel() {\n    1\n}\n"),
        (
            "scripts/a.rn",
            "pub fn read() {\n    settings::get(\"ui/scale\")\n}\n",
        ),
    ]);
    // `settings::get` is the engine's: no `use`, no editor function named so.
    let a = &ported[1];
    has(&a.bs, "settings::get(\"ui/scale\")");
    lacks(&a.bs, "use settings;");
    assert!(a.report.flags.is_empty(), "{:?}", a.report.flags);
}

#[test]
fn rune_that_does_not_parse_is_kept_as_comments_and_counted() {
    let p = port_source("pub fn broken( {\n    1\n}\n");
    assert_eq!(p.report.code_lines, 3);
    assert_eq!(p.report.flagged_lines, 3);
    has(&p.bs, "// pub fn broken( {");
}

#[test]
fn comments_are_carried_and_block_comments_become_line_comments() {
    let out = port(
        r"// The file's own description.

/* A note
   over two lines. */
pub fn f() {
    // Why one.
    1
}
",
    );
    has(&out, "// The file's own description.");
    has(&out, "// A note\n// over two lines.");
    has(&out, "    // Why one.\n    1");
    lacks(&out, "/*");
}

#[test]
fn the_report_counts_flagged_lines_once() {
    let p = port_source(
        r#"
pub fn f(x) {
    let a = format!("{:.1} {:.2}", x, x);
    let b = x.split_once("=");
    a
}

pub fn g() {
    f(1.5);
}
"#,
    );
    assert_eq!(p.report.code_lines, 8);
    // Two flags on one line, one on another.
    assert_eq!(p.report.flagged_lines, 2);
    assert_eq!(p.report.clean_lines(), 6);
}

#[test]
fn a_dynamic_const_indexed_by_key_is_a_map() {
    let out = port(
        r#"
const ROWS = #{ texture: [#{ key: "filter" }], audio: [#{ key: "rate" }] };
pub fn rows(kind) {
    ROWS.get(kind).unwrap_or([])
}
"#,
    );
    lacks(&out, "struct Rows");
    has(&out, "fn ROWS() -> any {");
    has(&out, "map()");
}

#[test]
fn a_closure_reading_only_the_state_is_a_callback_given_it() {
    let out = port(
        "pub fn rows(S) {
    let row = #{ name: \"a\", run: || S.count += 1 };
    let run = row.run;
    run();
}
",
    );
    has(&out, "fn rows_run_0(S: &mut State)");
    has(&out, "run: fn(&mut State)");
    has(&out, "run(S);");
    lacks(&out, "a fn value cannot carry them");
}

#[test]
fn script_attempt_is_dropped_and_its_call_runs() {
    let out = port(
        "pub fn save(path) {
    let (ok, why) = script::attempt(|| fs::read(path));
    if !ok { log::warn(why); }
}
",
    );
    has(&out, "let why = fs::read(path);");
    has(&out, "let ok = true;");
    lacks(&out, "script::attempt");
}
