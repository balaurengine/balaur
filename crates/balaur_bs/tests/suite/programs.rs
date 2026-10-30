// Every `.bs` under `tests/programs/` compiled, validated, and its `// expect:`
// calls run on every engine this build has, which must agree to the bit.
// `BLESS=1` rewrites the answers; `BS_PROGRAMS=x` runs the paths holding `x`.

use balaur_bs::runtime::{EngineKind, HostFns, Limits, Runtime, Value};
use balaur_bs::{Compiled, Options, compile, errors};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const EXPECT: &str = "// expect: ";
const ERROR: &str = "// error: ";
const LIVE: &str = "// live: ";

enum Want {
    Value(Value),
    Trap(Option<String>),
}

struct Check {
    line: usize,
    call: String,
    function: String,
    args: Vec<Value>,
    want: Want,
}

/// What a file says about itself: its calls, its compile errors, and how
/// many objects its calls leave alive.
#[derive(Default)]
struct Expected {
    checks: Vec<Check>,
    errors: Vec<String>,
    live: Option<i32>,
}

struct Program {
    path: PathBuf,
    name: String,
    src: String,
    expected: Expected,
    compiled: Option<Compiled>,
}

type Outcome = Result<Value, String>;

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
    for p in &mut programs {
        build(p, bless, &mut failures);
    }
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
    for p in &mut programs {
        run(p, &engines, bless, &mut failures);
    }
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

/// Compiles a program, or checks that it fails as its `// error:` lines say.
fn build(p: &mut Program, bless: bool, failures: &mut Vec<String>) {
    let name = &p.name;
    let found: Vec<String> = errors(&p.src, &Options::default())
        .iter()
        .map(ToString::to_string)
        .collect();
    if !p.expected.errors.is_empty() || !found.is_empty() {
        if bless {
            p.expected.errors = found;
        } else if found != p.expected.errors {
            failures.push(format!(
                "{name}: compile errors\n  want: {:?}\n  got:  {:?}",
                p.expected.errors, found
            ));
        }
        return;
    }
    if p.expected.checks.is_empty() {
        failures.push(format!("{name}: no `// expect:` or `// error:` line"));
        return;
    }
    match compile(&p.src, &Options::default()) {
        Ok(c) => match wasmparser::Validator::new().validate_all(&c.wasm) {
            Ok(_) => p.compiled = Some(c),
            Err(e) => failures.push(format!("{name}: module does not validate: {e}")),
        },
        Err(e) => failures.push(format!("{name}: {e}")),
    }
}

/// Runs a program's calls on every engine, holds them to each other and to
/// the file, and with `bless` takes the first engine's answers as the file's.
fn run(p: &mut Program, engines: &[(&str, EngineKind)], bless: bool, failures: &mut Vec<String>) {
    let name = &p.name;
    let Some(compiled) = &p.compiled else {
        return;
    };
    let mut runs: Vec<(&str, Vec<Outcome>)> = Vec::new();
    for &(engine_name, engine) in engines {
        match run_all(compiled, engine, &p.expected.checks) {
            Ok((outcomes, live)) => {
                if let Some(want) = p.expected.live
                    && live != want
                {
                    failures.push(format!(
                        "{name}: {engine_name} left {live} objects alive, want {want}"
                    ));
                }
                runs.push((engine_name, outcomes));
            }
            Err(e) => failures.push(format!("{name}: {engine_name}: {e}")),
        }
    }
    let Some((first_name, first)) = runs.first() else {
        return;
    };
    for (i, check) in p.expected.checks.iter().enumerate() {
        let at = format!("{name}:{} `{}`", check.line + 1, check.call);
        for (engine_name, outcomes) in &runs[1..] {
            if !same_outcome(&outcomes[i], &first[i]) {
                failures.push(format!(
                    "{at}: {engine_name} gave {}, {first_name} gave {}",
                    show_outcome(&outcomes[i]),
                    show_outcome(&first[i])
                ));
            }
        }
        let met = match (&check.want, &first[i]) {
            (Want::Value(want), Ok(got)) => same(want, got),
            (Want::Trap(None), Err(_)) => true,
            (Want::Trap(Some(text)), Err(msg)) => msg.contains(text.as_str()),
            _ => false,
        };
        if !met && !bless {
            failures.push(format!(
                "{at}: want {}, {first_name} gave {}",
                show_want(&check.want),
                show_outcome(&first[i])
            ));
        }
    }
    if bless {
        for (check, outcome) in p.expected.checks.iter_mut().zip(first) {
            check.want = match (outcome, &check.want) {
                (Ok(v), _) => Want::Value(v.clone()),
                (Err(_), Want::Trap(text)) => Want::Trap(text.clone()),
                (Err(_), Want::Value(_)) => Want::Trap(None),
            };
        }
    }
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

fn run_all(
    compiled: &Compiled,
    engine: EngineKind,
    checks: &[Check],
) -> Result<(Vec<Outcome>, i32), String> {
    let limits = Limits {
        engine,
        ..Limits::default()
    };
    let mut rt = Runtime::new(compiled, HostFns::default(), limits).map_err(|e| e.to_string())?;
    let outcomes = checks
        .iter()
        .map(|c| rt.call(&c.function, &c.args).map_err(|e| e.to_string()))
        .collect();
    Ok((outcomes, rt.live()))
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

fn read_expectations(src: &str) -> Result<Expected, String> {
    let mut expected = Expected::default();
    for (line, text) in src.lines().enumerate() {
        let text = text.trim_start();
        let at = |e: String| format!("line {}: {e}", line + 1);
        if let Some(rest) = text.strip_prefix(ERROR) {
            expected.errors.push(rest.to_string());
        } else if let Some(rest) = text.strip_prefix(LIVE) {
            expected.live = Some(rest.trim().parse().map_err(|e| at(format!("{e}")))?);
        } else if let Some(rest) = text.strip_prefix(EXPECT) {
            let mut check = parse_check(rest).map_err(at)?;
            check.line = line;
            expected.checks.push(check);
        }
    }
    Ok(expected)
}

fn parse_check(text: &str) -> Result<Check, String> {
    let (call, want) = if let Some((call, value)) = text.split_once(" == ") {
        (call.trim(), Want::Value(Parser::all(value.trim())?))
    } else if let Some((call, rest)) = text.split_once(" traps") {
        let rest = rest.trim();
        let message = if rest.is_empty() {
            None
        } else {
            match Parser::all(rest)? {
                Value::Str(s) => Some(s),
                _ => return Err("a trap's message is a string".into()),
            }
        };
        (call.trim(), Want::Trap(message))
    } else {
        return Err("want `f(args) == value` or `f(args) traps`".into());
    };
    let open = call.find('(').ok_or("a call is `f(args)`")?;
    let inner = call
        .strip_suffix(')')
        .ok_or("a call ends with `)`")?
        .get(open + 1..)
        .unwrap_or("");
    let Value::Array(args) = Parser::all(&format!("[{inner}]"))? else {
        unreachable!("a bracketed list parses as an array")
    };
    Ok(Check {
        line: 0,
        call: call.to_string(),
        function: call[..open].trim().to_string(),
        args,
        want,
    })
}

fn rewrite(src: &str, expected: &Expected) -> String {
    let mut out = String::new();
    let mut next = expected.checks.iter().peekable();
    for (line, text) in src.lines().enumerate() {
        let indent = &text[..text.len() - text.trim_start().len()];
        if text.trim_start().starts_with(ERROR) {
            continue;
        }
        if let Some(check) = next.next_if(|c| c.line == line) {
            let _ = writeln!(
                out,
                "{indent}{EXPECT}{} {}",
                check.call,
                show_want_line(&check.want)
            );
        } else {
            out.push_str(text);
            out.push('\n');
        }
    }
    for e in &expected.errors {
        let _ = writeln!(out, "{ERROR}{e}");
    }
    out
}

fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Float(x), Value::Float(y)) => x.to_bits() == y.to_bits(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b))
        }
        (Value::Struct(n, x), Value::Struct(m, y)) => n == m && same_fields(x, y),
        (Value::Map(x), Value::Map(y)) => same_fields(x, y),
        _ => a == b,
    }
}

fn same_fields(x: &[(String, Value)], y: &[(String, Value)]) -> bool {
    x.len() == y.len() && x.iter().zip(y).all(|((k, a), (l, b))| k == l && same(a, b))
}

fn same_outcome(a: &Outcome, b: &Outcome) -> bool {
    match (a, b) {
        (Ok(x), Ok(y)) => same(x, y),
        (Err(_), Err(_)) => true,
        _ => false,
    }
}

fn show_outcome(o: &Outcome) -> String {
    match o {
        Ok(v) => show(v),
        Err(e) => format!("a trap ({e})"),
    }
}

fn show_want(w: &Want) -> String {
    match w {
        Want::Value(v) => show(v),
        Want::Trap(None) => "a trap".into(),
        Want::Trap(Some(t)) => format!("a trap with {t:?}"),
    }
}

fn show_want_line(w: &Want) -> String {
    match w {
        Want::Value(v) => format!("== {}", show(v)),
        Want::Trap(None) => "traps".into(),
        Want::Trap(Some(t)) => format!("traps {t:?}"),
    }
}

/// A value as a corpus file writes it: Balaur Script's literals, a struct as
/// `Name { field: value }`, a map as `{ "key": value }`, nothing as `()`.
fn show(v: &Value) -> String {
    match v {
        Value::Void => "()".into(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) if f.is_nan() => "NaN".into(),
        Value::Float(f) => format!("{f:?}"),
        Value::Bool(b) => b.to_string(),
        Value::Node(n) => format!("node({n})"),
        Value::Str(s) => format!("{s:?}"),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(show).collect::<Vec<_>>().join(", ")
        ),
        Value::Struct(name, fields) if fields.is_empty() => format!("{name} {{}}"),
        Value::Struct(name, fields) => format!(
            "{name} {{ {} }}",
            fields
                .iter()
                .map(|(k, v)| format!("{k}: {}", show(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Map(pairs) if pairs.is_empty() => "{}".into(),
        Value::Map(pairs) => format!(
            "{{ {} }}",
            pairs
                .iter()
                .map(|(k, v)| format!("{k:?}: {}", show(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

struct Parser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl Parser<'_> {
    fn all(text: &str) -> Result<Value, String> {
        let mut p = Parser {
            chars: text.chars().peekable(),
        };
        let v = p.value()?;
        p.space();
        match p.chars.next() {
            None => Ok(v),
            Some(c) => Err(format!("`{c}` after a value in `{text}`")),
        }
    }

    fn space(&mut self) {
        while self.chars.next_if(|c| c.is_whitespace()).is_some() {}
    }

    fn eat(&mut self, c: char) -> bool {
        self.space();
        self.chars.next_if_eq(&c).is_some()
    }

    fn word(&mut self) -> String {
        self.space();
        let mut w = String::new();
        while let Some(c) = self
            .chars
            .next_if(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-' | '+'))
        {
            w.push(c);
        }
        w
    }

    fn value(&mut self) -> Result<Value, String> {
        self.space();
        match self.chars.peek().copied() {
            Some('"') => self.string().map(Value::Str),
            Some('[') => {
                self.chars.next();
                self.list(']').map(Value::Array)
            }
            Some('{') => {
                self.chars.next();
                self.pairs(true).map(Value::Map)
            }
            Some('(') => {
                self.chars.next();
                if self.eat(')') {
                    Ok(Value::Void)
                } else {
                    Err("`()` is the only bracketed value".into())
                }
            }
            Some(_) => self.scalar(),
            None => Err("a value is missing".into()),
        }
    }

    fn scalar(&mut self) -> Result<Value, String> {
        let w = self.word();
        match w.as_str() {
            "" => Err(format!(
                "no value at `{}`",
                self.chars.clone().collect::<String>()
            )),
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            "NaN" => Ok(Value::Float(f64::NAN)),
            "inf" => Ok(Value::Float(f64::INFINITY)),
            "-inf" => Ok(Value::Float(f64::NEG_INFINITY)),
            "node" => {
                if !self.eat('(') {
                    return Err("`node(n)`".into());
                }
                let n = self.word().parse().map_err(|e| format!("node: {e}"))?;
                if self.eat(')') {
                    Ok(Value::Node(n))
                } else {
                    Err("`node(n)`".into())
                }
            }
            _ if w.starts_with(|c: char| c.is_ascii_digit() || c == '-') => {
                if w.contains(['.', 'e', 'E']) {
                    w.parse()
                        .map(Value::Float)
                        .map_err(|e| format!("`{w}`: {e}"))
                } else {
                    w.parse().map(Value::Int).map_err(|e| format!("`{w}`: {e}"))
                }
            }
            _ => {
                if !self.eat('{') {
                    return Err(format!("`{w}` is not a value"));
                }
                let fields = self.pairs(false)?;
                Ok(Value::Struct(w, fields))
            }
        }
    }

    fn list(&mut self, close: char) -> Result<Vec<Value>, String> {
        let mut items = Vec::new();
        if self.eat(close) {
            return Ok(items);
        }
        loop {
            items.push(self.value()?);
            if self.eat(close) {
                return Ok(items);
            }
            if !self.eat(',') {
                return Err(format!("`,` or `{close}` after a list item"));
            }
        }
    }

    fn pairs(&mut self, quoted: bool) -> Result<Vec<(String, Value)>, String> {
        let mut pairs = Vec::new();
        if self.eat('}') {
            return Ok(pairs);
        }
        loop {
            self.space();
            let key = if quoted { self.string()? } else { self.word() };
            if !self.eat(':') {
                return Err(format!("`:` after `{key}`"));
            }
            pairs.push((key, self.value()?));
            if self.eat('}') {
                return Ok(pairs);
            }
            if !self.eat(',') {
                return Err("`,` or `}` after a field".into());
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.space();
        if self.chars.next() != Some('"') {
            return Err("a string opens with `\"`".into());
        }
        let mut s = String::new();
        loop {
            match self.chars.next().ok_or("a string is not closed")? {
                '"' => return Ok(s),
                '\\' => match self.chars.next().ok_or("a string ends in `\\`")? {
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    'r' => s.push('\r'),
                    '0' => s.push('\0'),
                    'u' => {
                        if self.chars.next() != Some('{') {
                            return Err("`\\u{..}`".into());
                        }
                        let hex: String =
                            std::iter::from_fn(|| self.chars.next_if(|c| *c != '}')).collect();
                        self.chars.next();
                        let code = u32::from_str_radix(&hex, 16).map_err(|e| e.to_string())?;
                        s.push(char::from_u32(code).ok_or("not a char")?);
                    }
                    other => s.push(other),
                },
                c => s.push(c),
            }
        }
    }
}
