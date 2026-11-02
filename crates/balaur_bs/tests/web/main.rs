// The corpus on the browser's own WebAssembly engine, beside wasmi, in Node:
// `cargo test -p balaur_bs --target wasm32-unknown-unknown --features web
// --test web`, with `wasm-bindgen-test-runner` as the target's runner.
#![cfg(all(target_arch = "wasm32", feature = "web"))]

#[path = "../corpus/mod.rs"]
#[allow(dead_code, reason = "only the native run rewrites the corpus")]
mod corpus;

use corpus::{Expected, Program, build, engines, read_expectations, run};
use std::path::PathBuf;
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen(module = "node:fs")]
extern "C" {
    #[wasm_bindgen(js_name = readFileSync)]
    fn read_file(path: &str, encoding: &str) -> String;
    #[wasm_bindgen(js_name = readdirSync)]
    fn read_dir(path: &str, options: &js_sys::Object) -> js_sys::Array;
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
