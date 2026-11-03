// Random well-typed programs, run on every engine and on an interpreter that
// is the language's meaning in code (PLAN-script-tests §8). `BS_RANDOM_*`.

mod anys;
mod generate;
mod interp;
mod show;
mod shrink;

use balaur_bs::runtime::{EngineKind, HostFns, Limits, Runtime, Value};
use balaur_bs::{Compiled, Options, compile};
use generate::Gen;
use interp::{Stop, answer, to_value};
use show::source;
use std::panic::catch_unwind;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Debug)]
enum T {
    Int,
    Float,
    Bool,
    Str,
    Ints,
    /// `struct Point { x: int, y: int }`: a value, copied where it is bound.
    Point,
    /// `enum Shade { Dark, Lit(int) }`.
    Shade,
    /// `{str: int}`: shared by every name that holds it.
    Ages,
    /// `int?`.
    Maybe,
    /// `any`: a counted cell; a map or a list in one is shared.
    Any,
}

/// What every program declares before its functions.
const DECLARATIONS: &str = "struct Point { x: int, y: int }\nenum Shade { Dark, Lit(int) }\nfn any_map() -> any { {} }\nfn any_nil() -> any { nil }\n";

/// The keys a `{str: int}` literal and a write may name.
const KEYS: [&str; 3] = ["a", "b", "c"];

/// The keys an `any` map is read or written by: a `{str: int}`'s and a
/// boxed Point's.
const ANY_KEYS: [&str; 5] = ["a", "b", "c", "x", "y"];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Lt,
    Le,
    Eq,
    Ne,
    And,
    Or,
}

impl Op {
    fn text(self) -> &'static str {
        match self {
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "*",
            Op::Div => "/",
            Op::Rem => "%",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Eq => "==",
            Op::Ne => "!=",
            Op::And => "&&",
            Op::Or => "||",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum E {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Var(String),
    Bin(Op, Box<E>, Box<E>),
    Neg(Box<E>),
    Not(Box<E>),
    If(Box<E>, Box<E>, Box<E>),
    Call(String, Vec<E>),
    Len(String),
    /// `xs[((i % xs.len()) + xs.len()) % xs.len()]`: in range unless empty.
    At(String, Box<E>),
    StrLen(Box<E>),
    /// `"a{n}b{flag}"`: text and variables.
    Interp(Vec<(String, String)>),
    Array(Vec<E>),
    /// `Point { x: .., y: .. }`.
    Point(Box<E>, Box<E>),
    /// `p.x` or `p.y` of a variable.
    Field(String, &'static str),
    Dark,
    Lit(Box<E>),
    /// `match s { Shade::Dark => .., Shade::Lit(n) => .. }`, `n` an int in the
    /// second arm.
    Match(String, Box<E>, String, Box<E>),
    /// `{"a": .., "b": ..}`, each key once.
    Ages(Vec<(&'static str, E)>),
    /// `m["k"].or(..)`.
    Get(String, &'static str, Box<E>),
    /// `m.len()`.
    Count(String),
    Nil,
    /// `o.or(..)`.
    Or(String, Box<E>),
    /// `r.name(args)` of an `any`.
    Method(Box<E>, &'static str, Vec<E>),
    /// `r[k]` of an `any`: `get` for a str key, `at` for an int.
    Index(Box<E>, Box<E>),
    /// `any_nil()`: an `any` holding nothing, which `DECLARATIONS` builds.
    NilAny,
    /// `any_map()`: a new, empty `any` map, which `DECLARATIONS` builds.
    MapAny,
}

#[derive(Clone, Debug, PartialEq)]
enum S {
    Let(String, E),
    Set(String, E),
    Push(String, E),
    SetAt(String, E, E),
    If(E, Vec<S>, Vec<S>),
    For(String, i64, Vec<S>),
    While(String, i64, Vec<S>),
    /// `let n: T = e;`, for a literal that takes its type from where it goes
    /// and for a value converted into or out of an `any`.
    LetTyped(String, T, E),
    /// `p.x = e;`
    SetField(String, &'static str, E),
    /// `m["k"] = e;`
    SetKey(String, &'static str, E),
    /// `r[k] = e;` on an `any`: `set`, or `put` for an int key.
    Put(E, E),
    /// `e;`, a method that changes its `any`.
    Do(E),
    /// `for x in r { .. }`, `r` an `any`: `x` is each of a list's elements,
    /// as many as it held when the loop began.
    Each(String, E, Vec<S>),
}

#[derive(Clone, Debug, PartialEq)]
struct Param {
    name: String,
    t: T,
    /// `&mut`: the caller's variable, copied in and stored back after.
    by_ref: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Func {
    name: String,
    params: Vec<Param>,
    ret: T,
    body: Vec<S>,
    tail: E,
}

/// How an engine and the interpreter part on a program.
#[derive(Clone, PartialEq, Debug)]
enum Fault {
    /// The compiler refuses it, with this message less its position.
    NoCompile(String),
    CompilerPanics,
    /// The engine will not load the module.
    Refused,
    Differs,
    EngineTraps,
    EngineAnswers,
}

const SEED: u64 = 0x0bad_5eed;

/// How many of a run's programs also run as native code, one library
/// (`BS_RANDOM_NATIVE`).
const NATIVE: usize = 16;

/// How many programs a shrink may try before it settles.
const SHRINK_BUDGET: usize = 4000;

fn program_seed(seed: u64, n: u64) -> u64 {
    seed ^ (n + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Float(x), Value::Float(y)) => x.to_bits() == y.to_bits(),
        _ => a == b,
    }
}

fn engines() -> Vec<(&'static str, EngineKind)> {
    vec![
        #[cfg(feature = "wasmi")]
        ("wasmi", EngineKind::Wasmi),
        #[cfg(all(
            feature = "wasmtime",
            not(target_arch = "wasm32"),
            not(target_os = "ios")
        ))]
        ("wasmtime", EngineKind::Wasmtime),
        #[cfg(all(feature = "c", feature = "native"))]
        ("native", EngineKind::Native),
    ]
}

fn compiled(src: &str) -> Result<Compiled, Fault> {
    match catch_unwind(|| compile(src, &Options::default())) {
        Ok(Ok(c)) => Ok(c),
        Ok(Err(e)) => {
            let text = e.to_string();
            let message = text.split_once(": ").map_or(text.as_str(), |(_, m)| m);
            Err(Fault::NoCompile(message.to_string()))
        }
        Err(_) => Err(Fault::CompilerPanics),
    }
}

/// `main()` on one engine: its answer, or what stopped it; `Err(None)` when
/// the engine would not load the module.
fn run_main(c: &Compiled, engine: EngineKind) -> Result<Value, Option<String>> {
    let limits = Limits {
        engine,
        ..Limits::default()
    };
    let mut rt = Runtime::new(c, HostFns::default(), limits).map_err(|_| None)?;
    rt.call("main", &[]).map_err(|e| Some(e.to_string()))
}

fn fault(want: &Result<interp::V, Stop>, got: &Result<Value, Option<String>>) -> Option<Fault> {
    match (want, got) {
        (_, Err(None)) => Some(Fault::Refused),
        (Ok(w), Ok(g)) if !to_value(w).is_ok_and(|w| same(&w, g)) => Some(Fault::Differs),
        (Ok(_), Err(Some(_))) => Some(Fault::EngineTraps),
        (Err(Stop::Trap), Ok(_)) => Some(Fault::EngineAnswers),
        _ => None,
    }
}

/// How `engine` and the interpreter part on `funcs`, built afresh; `None`
/// when they agree or the interpreter cannot read the program.
fn verdict(funcs: &[Func], engine: EngineKind, mul_adds: bool) -> Option<Fault> {
    let want = answer(funcs, mul_adds);
    if matches!(want, Err(Stop::Bad(_))) {
        return None;
    }
    let c = match compiled(&source(funcs)) {
        Ok(c) => c,
        Err(f) => return Some(f),
    };
    if prepare(engine, &c).is_err() {
        return Some(Fault::Refused);
    }
    fault(&want, &run_main(&c, engine))
}

/// What `engine` needs before it runs `c`: native code, its own build.
#[cfg(all(feature = "c", feature = "native"))]
fn prepare(engine: EngineKind, c: &Compiled) -> Result<(), String> {
    if engine == EngineKind::Native {
        native_build(&[c])
    } else {
        Ok(())
    }
}

#[cfg(not(all(feature = "c", feature = "native")))]
fn prepare(_: EngineKind, _: &Compiled) -> Result<(), String> {
    Ok(())
}

/// `modules` as native code, loaded: a library of at most 16 each, built
/// side by side, since clang takes minutes over one C file of a thousand.
#[cfg(all(feature = "c", feature = "native"))]
fn native_build(modules: &[&Compiled]) -> Result<(), String> {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static BUILT: AtomicUsize = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let chunks: Vec<Vec<&[u8]>> = modules
        .chunks(modules.len().div_ceil(workers).clamp(1, 16))
        .map(|chunk| chunk.iter().map(|c| c.wasm.as_slice()).collect())
        .collect();
    let next = AtomicUsize::new(0);
    let libs = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(chunk) = chunks.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let built = native_library(chunk, BUILT.fetch_add(1, Ordering::Relaxed));
                    libs.lock().unwrap().push(built);
                }
            });
        }
    });
    for lib in libs.into_inner().unwrap() {
        balaur_bs::runtime::load_native(&lib?)?;
    }
    Ok(())
}

// A library's path is new every time: a path already opened opens the
// library first loaded from it.
#[cfg(all(feature = "c", feature = "native"))]
fn native_library(wasm: &[&[u8]], n: usize) -> Result<PathBuf, String> {
    let c = balaur_bs::c::translate(wasm)?;
    let dir = std::env::temp_dir().join(format!("balaur-random-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let source = dir.join("random.c");
    std::fs::write(&source, c).map_err(|e| e.to_string())?;
    let lib = dir.join("random.lib");
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let out = std::process::Command::new(cc)
        .args(["-O2", "-ffp-contract=off", "-fPIC", "-shared", "-w", "-o"])
        .arg(&lib)
        .arg(&source)
        .output()
        .map_err(|e| format!("no C compiler: {e}"))?;
    if out.status.success() {
        Ok(lib)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// A failing program shrunk, printed and saved into the corpus; the report.
fn found(seed: u64, n: u64, funcs: &[Func], fault: &Fault, engine: (&str, EngineKind)) -> String {
    let small = shrink::shrink(
        funcs,
        &mut |f| verdict(f, engine.1, false).as_ref() == Some(fault),
        SHRINK_BUDGET,
    );
    let src = source(&small);
    let want = answer(&small, false);
    let what = match compiled(&src) {
        Err(f) => format!("{f:?}"),
        Ok(c) => match prepare(engine.1, &c).map(|()| run_main(&c, engine.1)) {
            Ok(Ok(v)) => format!("{} gave {}", engine.0, super::programs::corpus::show(&v)),
            Ok(Err(e)) => format!("{} stopped: {e:?}", engine.0),
            Err(e) => format!("no native build: {e}"),
        },
    };
    let expect = match want.as_ref().map(to_value) {
        Ok(Ok(v)) => format!("== {}", super::programs::corpus::show(&v)),
        _ => "traps".into(),
    };
    let file = format!("seed_{seed}_program_{n}.bs");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/programs/random");
    let text = format!(
        "// Random program {n} of seed {seed}, shrunk: {what}.\n\n{src}\n// expect: main() {expect}\n"
    );
    let saved = std::fs::create_dir_all(&path)
        .and_then(|()| std::fs::write(path.join(&file), &text))
        .map_or_else(
            |e| format!("not saved: {e}"),
            |()| format!("saved as random/{file}"),
        );
    format!(
        "program {n} (seed {seed}) on {}: {fault:?}; {what}, the interpreter {:?}; {saved}\n{src}",
        engine.0,
        want.as_ref().map(to_value)
    )
}

#[test]
fn random_programs_answer_as_the_reference_interpreter_does() {
    let seed: u64 = std::env::var("BS_RANDOM_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(SEED);
    let count: u64 = std::env::var("BS_RANDOM_PROGRAMS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let show = std::env::var_os("BS_RANDOM_SHOW").is_some();
    // Every program runs on wasmi and wasmtime, and the first of them as
    // native code too: a program's C carries the `any` prelude, so building
    // all of them natively would cost every push ten seconds.
    let native: usize = std::env::var("BS_RANDOM_NATIVE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(NATIVE);
    let mut failures = Vec::new();
    let mut answered = 0;
    // Every program made and compiled first, so native code is a C build of
    // many programs at a time rather than one a program.
    let mut made = Vec::new();
    for n in 0..count {
        let funcs = Gen::new(program_seed(seed, n)).program();
        let src = source(&funcs);
        let want = answer(&funcs, false);
        if show {
            eprintln!("program {n}: {:?}\n{src}", want.as_ref().map(to_value));
        }
        match &want {
            Ok(_) => answered += 1,
            Err(Stop::Bad(why)) => {
                failures.push(format!(
                    "program {n} (seed {seed}): the interpreter cannot read it: {why}\n{src}"
                ));
                continue;
            }
            Err(Stop::Trap) => {}
        }
        match compiled(&src) {
            Ok(c) => made.push((n, funcs, want, c)),
            Err(f) => {
                let first = engines()[0];
                failures.push(found(seed, n, &funcs, &f, first));
            }
        }
    }
    #[cfg(all(feature = "c", feature = "native"))]
    native_build(
        &made
            .iter()
            .take(native)
            .map(|(.., c)| c)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    for (at, (n, funcs, want, c)) in made.iter().enumerate() {
        for engine in engines() {
            if engine.0 == "native" && at >= native {
                continue;
            }
            if let Some(f) = fault(want, &run_main(c, engine.1)) {
                failures.push(found(seed, *n, funcs, &f, engine));
                break;
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} disagreements:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // A generator whose programs mostly trap would agree while testing little.
    assert!(
        answered * 10 >= count * 7,
        "only {answered} of {count} programs gave a value"
    );
}

#[cfg(feature = "wasmi")]
#[test]
fn a_disagreement_shrinks_to_the_lines_that_show_it() {
    // An interpreter whose int `*` adds parts from every engine wherever a
    // product counts; the shrink keeps the product and little else.
    let broken = |f: &[Func]| verdict(f, EngineKind::Wasmi, true) == Some(Fault::Differs);
    let funcs = (0..200)
        .map(|n| Gen::new(program_seed(SEED, n)).program())
        .find(|f| source(f).lines().count() >= 30 && broken(f))
        .expect("a program where a product counts");
    let small = shrink::shrink(&funcs, &mut |f| broken(f), SHRINK_BUDGET);
    let (before, after) = (source(&funcs), source(&small));
    assert!(broken(&small), "the shrunk program agrees:\n{after}");
    assert!(after.contains(" * "), "the product went:\n{after}");
    assert!(
        after.lines().count() <= 11 && after.lines().count() * 3 <= before.lines().count(),
        "not shrunk:\n{before}\n->\n{after}"
    );
}
