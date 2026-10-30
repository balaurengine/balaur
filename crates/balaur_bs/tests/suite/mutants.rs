// The corpus, broken on purpose: each program cut, spliced and sprinkled with
// tokens by a seeded generator, then read, formatted, analysed, compiled and
// run under fuel. Nothing may panic. `BS_FUZZ_SEED` and `BS_FUZZ_MUTANTS`.

use balaur_bs::runtime::{EngineKind, HostFns, Limits, Runtime, Value};
use balaur_bs::{HostApi, Options, Ty, analysis, compile, errors, fmt};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

const TOKENS: &[&str] = &[
    "{",
    "}",
    "(",
    ")",
    "[",
    "]",
    ";",
    ",",
    ".",
    "?",
    ":",
    "::",
    "->",
    "=",
    "==",
    "+",
    "-",
    "*",
    "/",
    "%",
    "&mut ",
    "!",
    "\"",
    "{x}",
    "nil",
    "let ",
    "fn ",
    "pub ",
    "class ",
    "struct ",
    "impl ",
    "return ",
    "async ",
    "await ",
    "for ",
    "in ",
    "0..",
    "while ",
    "loop ",
    "break;",
    "continue;",
    "if ",
    "else ",
    "as ",
    "0",
    "-1",
    "1.5",
    "9223372036854775807",
    "true",
    "int",
    "float",
    "str",
    "any",
    "[int]",
    "{str: int}",
    "Node",
    "self.",
    "map()",
    "use x;",
];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % (n.max(1) as u64)).unwrap_or(0)
    }
}

fn boundary(s: &str, at: usize) -> usize {
    let mut at = at.min(s.len());
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn mutate(src: &str, rng: &mut Rng) -> String {
    let mut s = src.to_string();
    for _ in 0..=rng.below(3) {
        let a = boundary(&s, rng.below(s.len() + 1));
        let b = boundary(&s, a + rng.below(24));
        match rng.below(5) {
            0 => s.replace_range(a..b, ""),
            1 => s.insert_str(a, TOKENS[rng.below(TOKENS.len())]),
            2 => {
                let piece = s[a..b].to_string();
                let at = boundary(&s, rng.below(s.len() + 1));
                s.insert_str(at, &piece);
            }
            3 => s.truncate(a),
            _ => {
                let mut lines: Vec<&str> = s.lines().collect();
                if lines.len() > 1 {
                    let (i, j) = (rng.below(lines.len()), rng.below(lines.len()));
                    lines.swap(i, j);
                }
                s = lines.join("\n");
            }
        }
    }
    s
}

fn zero(ty: &Ty) -> Option<Value> {
    Some(match ty {
        Ty::Int => Value::Int(0),
        Ty::Float => Value::Float(0.0),
        Ty::Bool => Value::Bool(false),
        Ty::Str => Value::Str(String::new()),
        Ty::Node => Value::Node(0),
        Ty::Void | Ty::Any | Ty::Optional(_) => Value::Void,
        Ty::Array(_) => Value::Array(Vec::new()),
        Ty::Map(..) => Value::Map(Vec::new()),
        Ty::Struct(_) | Ty::Fn(..) => return None,
    })
}

/// What one mutant goes through. A panic anywhere in it is the failure.
fn exercise(src: &str) -> Result<(), String> {
    let _ = errors(src, &Options::default());
    let _ = fmt::format(src);
    let _ = analysis::analyze(src, &HostApi::default());
    let Ok(c) = compile(src, &Options::default()) else {
        return Ok(());
    };
    wasmparser::Validator::new()
        .validate_all(&c.wasm)
        .map_err(|e| format!("the module does not validate: {e}"))?;
    let limits = Limits {
        fuel: Some(1_000_000),
        memory_bytes: Some(64 << 20),
        engine: EngineKind::Wasmi,
    };
    let Ok(mut rt) = Runtime::new(&c, HostFns::default(), limits) else {
        return Ok(());
    };
    for f in &c.meta.fns {
        if !f.public || f.is_async || f.owner.is_some() {
            continue;
        }
        if let Some(args) = f.params.iter().map(zero).collect::<Option<Vec<_>>>() {
            let _ = rt.call(&f.key, &args);
        }
    }
    Ok(())
}

fn corpus(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(corpus(&path));
        } else if path.extension().is_some_and(|x| x == "bs") {
            out.push(path);
        }
    }
    out.sort();
    out
}

#[test]
fn a_broken_program_is_an_error_never_a_panic() {
    let seed: u64 = std::env::var("BS_FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5eed_ba1a_u64);
    let per_program: usize = std::env::var("BS_FUZZ_MUTANTS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(12);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/programs");
    let mut failures = Vec::new();
    for (n, path) in corpus(&root).iter().enumerate() {
        let src = std::fs::read_to_string(path).unwrap();
        let mut rng = Rng(seed ^ (n as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15));
        for _ in 0..per_program {
            let mutant = mutate(&src, &mut rng);
            let outcome = catch_unwind(AssertUnwindSafe(|| exercise(&mutant)));
            let problem = match outcome {
                Ok(Ok(())) => continue,
                Ok(Err(e)) => e,
                Err(panic) => panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
                    .unwrap_or_else(|| "a panic".into()),
            };
            failures.push(format!(
                "{} (seed {seed}): {problem}\n--- mutant ---\n{mutant}\n---",
                path.strip_prefix(&root).unwrap_or(path).display()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mutants panicked or wrote a bad module:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
