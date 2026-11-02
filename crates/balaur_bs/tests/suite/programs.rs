// Every `.bs` under `tests/programs/` compiled, validated, and its `// expect:`
// calls run on every engine this build has, which must agree to the bit.
// `BLESS=1` rewrites the answers; `BS_PROGRAMS=x` runs the paths holding `x`.

#[path = "../corpus/mod.rs"]
pub(crate) mod corpus;

#[cfg(all(feature = "c", feature = "native"))]
use balaur_bs::Compiled;
use corpus::{Program, build, engines, read_expectations, rewrite, run};
use std::path::{Path, PathBuf};

#[test]
fn every_program_answers_as_written_on_every_engine() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/programs");
    let only = std::env::var("BS_PROGRAMS").ok();
    let bless = std::env::var_os("BLESS").is_some();
    let mut failures = Vec::new();
    let mut programs = read_all(&root, only.as_deref(), &mut failures);
    assert!(
        !programs.is_empty() || only.is_some(),
        "no programs under {}",
        root.display()
    );
    failures.extend(on_every_core(&mut programs, |p, failures| {
        build(p, bless, failures);
    }));
    #[cfg(all(feature = "c", feature = "native"))]
    {
        let built: Vec<&Compiled> = programs
            .iter()
            .filter_map(|p| p.compiled.as_ref())
            .collect();
        if let Err(e) = native::load(&built) {
            failures.push(format!("native build: {e}"));
        }
    }
    let engines = engines();
    failures.extend(on_every_core(&mut programs, |p, failures| {
        run(p, &engines, bless, failures);
    }));
    if bless {
        for p in &programs {
            let written = rewrite(&p.src, &p.expected);
            if written != p.src {
                std::fs::write(&p.path, written).unwrap();
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of the corpus failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn read_all(root: &Path, only: Option<&str>, failures: &mut Vec<String>) -> Vec<Program> {
    let mut programs = Vec::new();
    for path in files(root) {
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

#[cfg(all(feature = "c", feature = "native"))]
mod native {
    use balaur_bs::Compiled;

    /// Every program as one native library, loaded, as an iOS export builds it.
    pub(super) fn load(programs: &[&Compiled]) -> Result<(), String> {
        let wasm: Vec<&[u8]> = programs.iter().map(|c| c.wasm.as_slice()).collect();
        let c = balaur_bs::c::translate(&wasm)?;
        let dir = std::env::temp_dir().join(format!("balaur-programs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let source = dir.join("programs.c");
        std::fs::write(&source, c).map_err(|e| e.to_string())?;
        let lib = dir.join("programs.lib");
        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
        let out = std::process::Command::new(cc)
            .args(["-O2", "-ffp-contract=off", "-fPIC", "-shared", "-w", "-o"])
            .arg(&lib)
            .arg(&source)
            .output()
            .map_err(|e| format!("no C compiler: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).into_owned());
        }
        let loaded = balaur_bs::runtime::load_native(&lib)?;
        if loaded == programs.len() {
            Ok(())
        } else {
            Err(format!("loaded {loaded} of {} modules", programs.len()))
        }
    }
}

/// `each` over the programs, a share of them on every core, and the failures
/// in the order the programs came in.
fn on_every_core(
    programs: &mut [Program],
    each: impl Fn(&mut Program, &mut Vec<String>) + Sync,
) -> Vec<String> {
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    let share = programs.len().div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        let each = &each;
        let started: Vec<_> = programs
            .chunks_mut(share)
            .map(|part| {
                scope.spawn(move || {
                    let mut failures = Vec::new();
                    for p in part {
                        each(p, &mut failures);
                    }
                    failures
                })
            })
            .collect();
        started
            .into_iter()
            .flat_map(|one| one.join().expect("a program's run panicked"))
            .collect()
    })
}
