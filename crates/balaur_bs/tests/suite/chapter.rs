// The language chapter's fences that need no engine API, on wasmi and wasmtime.
// `BS_CHAPTER=x` runs the pages whose path holds `x`.

#[path = "../corpus/chapter.rs"]
mod fences;

use crate::programs::corpus::{Program, engines, read_expectations, run};
use balaur_bs::{Modules, Options, errors};
use fences::{is_fragment, module_name, needs_the_engine, pages};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Debug, Default, PartialEq)]
struct Tally {
    ran: usize,
    compiled: usize,
    refused: usize,
}

// The fence as the corpus reads it: `// f(x) == v` is its `// expect:`, with
// nil written the way a call's answer is.
fn as_corpus(code: &str) -> String {
    let mut out = String::new();
    for line in code.lines() {
        let bare = line.trim_start();
        let call = bare.strip_prefix("// ").filter(|rest| {
            let head = rest.split('(').next().unwrap_or("");
            !head.is_empty()
                && head.chars().all(|c| c.is_alphanumeric() || c == '_')
                && rest.contains('(')
                && (rest.contains(") == ") || rest.ends_with(" traps"))
        });
        match call {
            Some(call) => {
                let call = call
                    .replace(" == nil", " == ()")
                    .replace("(nil)", "(())")
                    .replace("(nil,", "((),")
                    .replace(", nil)", ", ())");
                out.push_str("// expect: ");
                out.push_str(&call);
            }
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

// What one fence does, or why it does not do what it says.
fn check(
    code: &str,
    at: &str,
    modules: &HashMap<String, String>,
    tally: &mut Tally,
) -> Result<(), String> {
    let mut src = as_corpus(code);
    if is_fragment(&src) {
        src = format!("pub fn snippet() {{\n{src}}}\n");
    }
    let modules = modules.clone();
    let options = Options {
        modules: Some(Modules(Rc::new(move |_: &str, name: &str| {
            let file = format!("{}.bs", name.replace("::", "/"));
            modules
                .get(&file)
                .map(|source| (file.clone(), source.clone()))
        }))),
        path: "main.bs".into(),
        ..Options::default()
    };
    let expected = read_expectations(&src).map_err(|e| format!("{at}: {e}"))?;
    let found: Vec<String> = errors(&src, &options)
        .iter()
        .map(ToString::to_string)
        .collect();
    if found != expected.errors {
        return Err(format!(
            "{at}: compile errors\n  want: {:?}\n  got:  {found:?}",
            expected.errors
        ));
    }
    if !expected.errors.is_empty() {
        tally.refused += 1;
        return Ok(());
    }
    if expected.checks.is_empty() {
        tally.compiled += 1;
        return Ok(());
    }
    let compiled = balaur_bs::compile(&src, &options).map_err(|e| format!("{at}: {e}"))?;
    let mut program = Program {
        path: PathBuf::from(at),
        name: at.to_string(),
        src,
        expected,
        compiled: Some(compiled),
    };
    let mut failures = vec![];
    // Native code needs one build of the whole set, which the corpus proves.
    let engines: Vec<_> = engines()
        .into_iter()
        .filter(|(name, _)| *name != "native")
        .collect();
    run(&mut program, &engines, false, &mut failures);
    tally.ran += 1;
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

#[test]
fn every_example_in_the_language_chapter_does_what_it_says() {
    let only = std::env::var("BS_CHAPTER").ok();
    let mut tally = Tally::default();
    let mut failures = vec![];
    for page in pages() {
        if only
            .as_ref()
            .is_some_and(|o| !page.name.contains(o.as_str()))
        {
            continue;
        }
        let modules: HashMap<String, String> = page
            .fences
            .iter()
            .filter_map(|(_, code)| Some((module_name(code)?.to_string(), code.clone())))
            .collect();
        for (line, code) in &page.fences {
            if needs_the_engine(code) {
                continue;
            }
            let at = format!("{}:{line}", page.name);
            if let Err(why) = check(code, &at, &modules, &mut tally) {
                failures.push(why);
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of the chapter's examples failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(
        only.is_some() || (tally.ran > 0 && tally.compiled > 0 && tally.refused > 0),
        "the chapter was not read: {tally:?}"
    );
}

#[test]
fn an_example_that_stopped_being_true_fails() {
    let modules = HashMap::new();
    let mut tally = Tally::default();
    let wrong_answer = "pub fn f() -> int { 2 }\n// f() == 3\n";
    assert!(check(wrong_answer, "page", &modules, &mut tally).is_err());
    let no_longer_compiles = "pub fn f() -> int { nil() }\n";
    assert!(check(no_longer_compiles, "page", &modules, &mut tally).is_err());
    let wrong_error = "pub fn f() -> int { true }\n// error: 1:1: not this\n";
    assert!(check(wrong_error, "page", &modules, &mut tally).is_err());
    let right = "pub fn f() -> int { 2 }\n// f() == 2\n";
    assert!(check(right, "page", &modules, &mut tally).is_ok());
}
