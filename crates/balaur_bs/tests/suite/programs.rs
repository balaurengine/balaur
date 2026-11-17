// Every `.bs` under `tests/programs/` compiled, validated, and its `// expect:`
// calls run on every engine this build has, which must agree to the bit.
// `BLESS=1` rewrites the answers; `BS_PROGRAMS=x` runs the paths holding `x`.

#[path = "../corpus/mod.rs"]
pub(crate) mod corpus;

use corpus::{Program, build, engines, on_every_core, read_expectations, rewrite, run};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

/// One test for each folder, so the folders run side by side.
macro_rules! folders {
    ($($folder:ident),* $(,)?) => {
        const FOLDERS: &[&str] = &[$(stringify!($folder)),*];
        $(
            mod $folder {
                #[test]
                fn every_program_answers_as_written_on_every_engine() {
                    super::folder(stringify!($folder));
                }
            }
        )*
    };
}

folders!(
    anys, classes, enums, functions, library, loops, maps, matching, memory, optional, refs,
    syntax,
);

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/programs")
}

#[test]
fn every_folder_of_the_corpus_has_its_test() {
    let mut found: Vec<String> = std::fs::read_dir(root())
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() {
                Some(name)
            } else {
                e.path()
                    .extension()
                    .is_some_and(|x| x == "bs")
                    .then(|| format!("{name}, a program outside every folder"))
            }
        })
        .collect();
    found.sort();
    assert_eq!(
        found, FOLDERS,
        "a folder of `tests/programs` is in `folders!`"
    );
}

#[test]
fn a_corpus_file_with_errors_and_calls_is_refused() {
    let src = "pub fn f() -> int { 1 }\n// expect: f() == 1\n// error: 1:1: never\n";
    assert!(read_expectations(src).is_err());
}

#[test]
fn a_trap_names_its_kind_and_never_fuel() {
    let check = |line: &str| corpus::parse_check(line).map(|c| corpus::show_want_line(&c.want));
    assert_eq!(check("f() traps").as_deref(), Ok("traps"));
    assert_eq!(
        check("f() traps unreachable").as_deref(),
        Ok("traps unreachable")
    );
    assert!(check("f() traps fuel").is_err());
    assert!(check("f() traps \"unreachable\"").is_err());
}

/// The programs of one folder: built, then built as native code, then run.
fn folder(folder: &str) {
    let only = std::env::var("BS_PROGRAMS").ok();
    let bless = std::env::var("BLESS").is_ok_and(|b| b == "1");
    let engines = engines();
    // `cargo test -p balaur_bs` has wasmi alone, which agrees with itself.
    if std::env::var_os("CI").is_some() {
        assert!(
            engines.len() >= 3,
            "the corpus ran on {:?} alone",
            engines.iter().map(|(name, _)| *name).collect::<Vec<_>>()
        );
    }
    let mut failures = Vec::new();
    let mut programs = read_all(&root(), folder, only.as_deref(), &mut failures);
    assert!(
        !programs.is_empty() || only.is_some(),
        "no programs under {folder}"
    );
    failures.extend(each(&mut programs, |p, failures| build(p, bless, failures)));
    #[cfg(all(feature = "c", feature = "native"))]
    {
        let wasm: Vec<&[u8]> = programs
            .iter()
            .filter_map(|p| p.compiled.as_ref().map(|c| c.wasm.as_slice()))
            .collect();
        if let Err(e) = corpus::native::load(&wasm) {
            failures.push(format!("native build: {e}"));
        }
    }
    failures.extend(each(&mut programs, |p, failures| {
        run(p, &engines, bless, failures);
    }));
    if bless {
        let mut written = 0;
        for p in &programs {
            let text = rewrite(&p.src, &p.expected);
            if text != p.src {
                std::fs::write(&p.path, text).unwrap();
                written += 1;
            }
        }
        failures.push(format!(
            "BLESS=1 rewrote {written} of {folder}'s programs; a blessed run never passes"
        ));
    }
    assert!(
        failures.is_empty(),
        "{} of {folder} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn read_all(
    root: &Path,
    folder: &str,
    only: Option<&str>,
    failures: &mut Vec<String>,
) -> Vec<Program> {
    let mut programs = Vec::new();
    for path in files(&root.join(folder)) {
        let name = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        if only.is_some_and(|o| !name.contains(o)) {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        match read_expectations(&src) {
            Ok(expected) => programs.push(Program {
                path,
                name,
                src,
                expected,
                compiled: None,
            }),
            Err(e) => failures.push(format!("{name}: {e}")),
        }
    }
    programs
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            out.extend(files(&path));
        } else if path.extension().is_some_and(|x| x == "bs") {
            out.push(path);
        }
    }
    out
}

/// `step` over every program on every core, a panic a failure of its program;
/// the failures in the order the programs came in.
fn each(
    programs: &mut [Program],
    step: impl Fn(&mut Program, &mut Vec<String>) + Sync,
) -> Vec<String> {
    on_every_core(programs, |p| {
        let mut failures = Vec::new();
        if let Err(panic) = catch_unwind(AssertUnwindSafe(|| step(p, &mut failures))) {
            let why = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_default();
            failures.push(format!("{}: panicked: {why}", p.name));
        }
        failures
    })
    .concat()
}
