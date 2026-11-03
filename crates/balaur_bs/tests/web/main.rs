// The corpus on the browser's own WebAssembly engine, beside wasmi, in Node:
// `cargo test -p balaur_bs --target wasm32-unknown-unknown --features web
// --test web`, with `wasm-bindgen-test-runner` as the target's runner.
#![cfg(all(target_arch = "wasm32", feature = "web"))]

#[path = "../corpus/mod.rs"]
#[allow(dead_code, reason = "only the native run rewrites the corpus")]
mod corpus;

use balaur_bs::runtime::{EngineKind, HostFns, Limits, Object, Runtime, Value};
use balaur_bs::{Modules, Options, Parsed, compile};
use corpus::{Expected, Program, build, engines, read_expectations, run};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen(module = "node:fs")]
extern "C" {
    #[wasm_bindgen(js_name = readFileSync)]
    fn read_file(path: &str, encoding: &str) -> String;
    #[wasm_bindgen(js_name = readdirSync)]
    fn read_dir(path: &str, options: &js_sys::Object) -> js_sys::Array;
}

const EDITOR: &str = "pub class Editor { node: Node, edits: int }
impl Editor {
    pub fn edit() -> int {
        self.edits += 1;
        self.edits
    }
    pub fn spin() -> int {
        let spun = 0;
        while true {
            spun += 1;
        }
        spun
    }
}
";

const PANEL: &str = "use editor;
pub class Panel { node: Node, editor_node: Node }
impl Panel {
    pub fn poke() -> int {
        let found: editor::Editor? = self.editor_node.script;
        found.unwrap().edit()
    }
    pub fn spun() -> int {
        let found: editor::Editor? = self.editor_node.script;
        found.unwrap().spin()
    }
    pub fn note() -> str {
        \"panel {1 + 1} {2.5}\"
    }
}
";

/// The editor's and the panel's units, the instance's own, linked on the
/// browser's engine as a web build's host links them: `(runtime, panel)`.
fn project(direct: bool, profile: bool, fuel: Option<u64>) -> (Runtime, Object) {
    let files: HashMap<&str, &str> = HashMap::from([
        ("editor.bs", EDITOR),
        ("panel.bs", PANEL),
        ("project.bs", ""),
    ]);
    let parsed = Parsed::default();
    let unit = |key: &str| {
        let options = Options {
            modules: Some(Modules(Rc::new(|_: &str, name: &str| {
                let key = format!("{name}.bs");
                find_in(&key).map(|source| (key, source.to_string()))
            }))),
            path: key.into(),
            unit: true,
            parsed: Some(parsed.clone()),
            meter: true,
            direct_imports: direct,
            profile,
            ..Options::default()
        };
        Arc::new(compile(files[key], &options).unwrap_or_else(|e| panic!("{key}: {e}")))
    };
    let held: Rc<RefCell<i32>> = Rc::default();
    let pointer = held.clone();
    let host = HostFns::default().scripts(move |node, file, class| {
        (node == 1 && file == "editor.bs" && class == "Editor").then(|| *pointer.borrow())
    });
    let limits = Limits {
        engine: EngineKind::Browser,
        fuel,
        ..Limits::default()
    };
    let mut rt = Runtime::new(&unit("project.bs"), host, limits).unwrap();
    let editor = unit("editor.bs");
    let at = rt.link("editor.bs", &editor, &[]).unwrap();
    let object = rt
        .in_unit(at, |rt| rt.object("Editor", &[("node", Value::Node(1))]))
        .unwrap();
    *held.borrow_mut() = Runtime::pointer(&object);
    let uses = [("editor.bs".to_string(), editor)];
    let at = rt.link("panel.bs", &unit("panel.bs"), &uses).unwrap();
    let panel = rt
        .in_unit(at, |rt| {
            rt.object(
                "Panel",
                &[("node", Value::Node(2)), ("editor_node", Value::Node(1))],
            )
        })
        .unwrap();
    (rt, panel)
}

fn find_in(key: &str) -> Option<&'static str> {
    match key {
        "editor.bs" => Some(EDITOR),
        "panel.bs" => Some(PANEL),
        _ => None,
    }
}

// Whichever way it reaches the engine, a panel holds the editor as its class,
// and a string it makes crosses to the host from its own unit.
#[wasm_bindgen_test]
fn a_typed_call_links_on_the_browsers_engine() {
    for direct in [true, false] {
        let (mut rt, panel) = project(direct, false, None);
        assert_eq!(rt.call_method(&panel, "poke", &[]), Ok(Value::Int(1)));
        assert_eq!(rt.call_method(&panel, "poke", &[]), Ok(Value::Int(2)));
        assert_eq!(
            rt.call_method(&panel, "note", &[]),
            Ok(Value::Str("panel 2 2.5".into()))
        );
    }
}

#[wasm_bindgen_test]
fn a_typed_call_counts_on_the_browsers_engine() {
    let (mut rt, panel) = project(true, true, None);
    let before = rt.steps().expect("a profiled instance counts");
    assert_eq!(rt.call_method(&panel, "poke", &[]), Ok(Value::Int(1)));
    assert_eq!(rt.steps().unwrap().wrapping_sub(before), 4);
    let mut names: Vec<String> = rt
        .function_costs()
        .into_iter()
        .map(|(name, _, _)| name)
        .collect();
    names.sort();
    assert_eq!(names, ["Editor.edit", "Panel.poke"]);
}

// The page's engine has no fuel: the units' own count stops a runaway one,
// across a link.
#[wasm_bindgen_test]
fn a_runaway_unit_stops_on_its_own_fuel() {
    let (mut rt, panel) = project(true, false, Some(1000));
    let e = rt.call_method(&panel, "spun", &[]).unwrap_err();
    assert!(e.msg.contains("fuel"), "{e}");
    assert_eq!(rt.call_method(&panel, "poke", &[]), Ok(Value::Int(1)));
}

#[wasm_bindgen_test]
fn every_program_answers_on_the_browsers_engine() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/programs");
    let recursive = js_sys::Object::new();
    js_sys::Reflect::set(&recursive, &"recursive".into(), &true.into()).unwrap();
    let mut names: Vec<String> = read_dir(root, &recursive)
        .iter()
        .filter_map(|v| v.as_string())
        .filter(|n| {
            std::path::Path::new(n)
                .extension()
                .is_some_and(|x| x == "bs")
        })
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no programs under {root}");
    let mut failures = Vec::new();
    let engines = engines();
    assert!(
        engines.iter().any(|(name, _)| *name == "browser"),
        "no browser engine in this build"
    );
    for name in names {
        let path = PathBuf::from(format!("{root}/{name}"));
        let src = read_file(&format!("{root}/{name}"), "utf8");
        let expected = read_expectations(&src).unwrap_or_else(|e| {
            failures.push(format!("{name}: {e}"));
            Expected::default()
        });
        let mut p = Program {
            path,
            name,
            src,
            expected,
            compiled: None,
        };
        build(&mut p, false, &mut failures);
        run(&mut p, &engines, false, &mut failures);
    }
    assert!(
        failures.is_empty(),
        "{} of the corpus failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
