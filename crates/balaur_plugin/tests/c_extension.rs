//! An extension written in C, loaded and called from a script.
//!
//! The Rust extension tests next door prove that a separately built library
//! can register itself. These prove the part that generalises: the library
//! here was never touched by a Rust compiler, so nothing about it depends on
//! rustc, on cargo, or on the engine's build tree. Anything that can emit a
//! shared library and a C call can do what this file does -- Odin and Zig
//! included.
#![cfg(feature = "dylib")]

use std::path::{Path, PathBuf};

use balaur_core::{App, AppConfig};
use balaur_plugin::{library_suffix, load_extension, BalaurValue};
use balaur_script::{NodeId, Value};

fn app_in(dir: &Path) -> App {
    App::new(AppConfig {
        script_backend: Some(balaur_script_rune::factory()),
        ..AppConfig::bare(dir.to_path_buf())
    })
    .unwrap()
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Compile the C extension with the system compiler.
///
/// No cargo, no build script, no crate: the closest thing to what someone
/// writing an extension in another language would actually run.
fn compile_c(source: &Path, stem: &str) -> PathBuf {
    let out_dir = repo_root().join("target/c_extensions");
    std::fs::create_dir_all(&out_dir).unwrap();
    let library = out_dir.join(format!("lib{stem}.{}", library_suffix()));

    let compiler = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let result = std::process::Command::new(&compiler)
        .arg("-std=c11")
        .arg("-shared")
        .arg("-fPIC")
        .arg("-I")
        .arg(repo_root().join("crates/balaur_plugin/include"))
        .arg("-o")
        .arg(&library)
        .arg(source)
        .output()
        .unwrap_or_else(|e| panic!("{compiler} should run: {e}"));
    assert!(
        result.status.success(),
        "the C extension did not build:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    library
}

fn counter() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| {
            compile_c(
                &repo_root().join("examples/extension_c_counter/counter.c"),
                "counter",
            )
        })
        .clone()
}

/// The counter loaded into a fresh app, with `probe.rn` attached to a node.
///
/// Rune runs code from files, not strings, so each test's expressions are
/// `pub fn`s of that script and `call` runs one by name. The `Extension`
/// stays with the app because it owns the open library, and every function
/// pointer it registered points into that library.
struct Loaded {
    app: App,
    _library: balaur_plugin::Extension,
    _dir: tempfile::TempDir,
    probe: NodeId,
}

fn loaded(script: &str) -> Loaded {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.toml"), "[project]\nname = \"t\"\n").unwrap();
    std::fs::write(dir.path().join("probe.rn"), script).unwrap();
    let mut app = app_in(dir.path());
    let mut extension = unsafe { load_extension(&counter()) }.expect("the C extension should load");
    assert_eq!(extension.manifest().name, "counter");
    balaur_plugin::load(&mut app, extension.plugin_mut()).unwrap();

    let root = app.engine.root();
    let node = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Probe", root);
    let probe = balaur_core::node_id_of(node);
    let host = app.engine.script_host().unwrap();
    host.attach(probe, "probe.rn").unwrap();
    Loaded {
        app,
        _library: extension,
        _dir: dir,
        probe,
    }
}

impl Loaded {
    /// Run `pub fn name(this)` from `probe.rn` and hand back what it returned.
    fn call(&self, name: &str) -> Value {
        self.app
            .engine
            .script_host()
            .unwrap()
            .call_on(self.probe, name, &[])
            .unwrap_or_else(|| panic!("`{name}` did not run to completion"))
    }
}

fn number(value: Value) -> f64 {
    match value {
        Value::Num(n) => n,
        Value::Int(i) => i as f64,
        other => panic!("expected a number, got {other:?}"),
    }
}

#[test]
fn a_c_extension_declares_itself_into_a_running_app() {
    let probe = loaded("pub fn greet(this) { counter::greet(\"world\") }\n");
    assert_eq!(probe.call("greet"), Value::Str("hello, world".into()));
}

#[test]
fn numbers_cross_the_boundary_in_both_directions() {
    let probe = loaded(
        "pub fn whole(this) { counter::add(2, 3) }\n\
         pub fn fraction(this) { counter::add(0.5, 0.25) }\n",
    );
    let sum = number(probe.call("whole"));
    assert!((sum - 5.0).abs() < f64::EPSILON);
    // Rune hands whole numbers over as integers and fractions as floats; both
    // reach the same C function.
    let mixed = number(probe.call("fraction"));
    assert!((mixed - 0.75).abs() < f64::EPSILON);
}

/// The `user` pointer is how a C extension keeps state, with no Rust type and
/// no `TypeId` involved -- which is exactly what the Rust extension path
/// cannot do across separate compilations.
#[test]
fn a_c_extension_keeps_its_own_state_across_calls() {
    let probe = loaded("pub fn bump(this) { counter::bump() }\n");
    let Value::Int(first) = probe.call("bump") else {
        panic!("bump returns an integer");
    };
    let Value::Int(second) = probe.call("bump") else {
        panic!("bump returns an integer");
    };
    assert_eq!(second, first + 1);
}

#[test]
fn lists_cross_the_boundary_in_both_directions() {
    let probe = loaded(
        "pub fn tripled(this) { counter::triple(2) }\n\
         pub fn summed(this) { counter::sum_list([1, 2, 3.5]) }\n",
    );
    assert_eq!(
        probe.call("tripled"),
        Value::List(vec![Value::Num(2.0), Value::Num(4.0), Value::Num(6.0)])
    );

    let total = number(probe.call("summed"));
    assert!((total - 6.5).abs() < f64::EPSILON);
}

#[test]
fn a_constant_registered_from_c_is_readable() {
    let probe = loaded("pub fn version(this) { counter::VERSION }\n");
    assert_eq!(probe.call("version"), Value::Str("0.1.0".into()));
}

/// A non-zero return becomes a script error, and the string the extension
/// wrote becomes the message. Without that, a failing extension is a silent
/// nil.
#[test]
fn a_failing_c_function_becomes_a_script_error_with_its_message() {
    let probe = loaded(
        "pub fn failing(this) {\n\
         \x20 let (ok, err) = script::attempt(|| counter::always_fails());\n\
         \x20 if ok { \"no error\" } else { err }\n\
         }\n\
         pub fn wrong_types(this) {\n\
         \x20 let (ok, err) = script::attempt(|| counter::add(\"a\", \"b\"));\n\
         \x20 if ok { \"no error\" } else { err }\n\
         }\n",
    );
    let Value::Str(err) = probe.call("failing") else {
        panic!("the probe returns the error text");
    };
    assert!(
        err.contains("this function fails on purpose"),
        "the extension's own message did not reach the script: {err}"
    );

    let Value::Str(wrong_types) = probe.call("wrong_types") else {
        panic!("the probe returns the error text");
    };
    assert!(
        wrong_types.contains("add expects two numbers"),
        "unhelpful: {wrong_types}"
    );
}

/// The version handshake is the only thing standing between a stale library
/// and a misread struct, so it is checked before any other symbol is called.
#[test]
fn an_extension_built_for_another_abi_version_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("stale.c");
    std::fs::write(
        &source,
        r#"
#include <stdint.h>
uint32_t balaur_extension_abi(void) { return 99u; }
const char *balaur_extension_name(void) { return "stale"; }
const char *balaur_extension_version(void) { return "0.1.0"; }
int32_t balaur_extension_declare(const void *api, void *reg) {
    (void)api; (void)reg; return 0;
}
"#,
    )
    .unwrap();

    let library = compile_c(&source, "stale");
    let err = unsafe { load_extension(&library) }
        .map(|_| ())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("99") && err.contains("abi"),
        "does not name the mismatch: {err}"
    );
}

#[test]
fn a_library_exporting_neither_entry_point_names_both() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("nothing.c");
    std::fs::write(&source, "int unrelated(void) { return 0; }\n").unwrap();

    let library = compile_c(&source, "nothing");
    let err = unsafe { load_extension(&library) }
        .map(|_| ())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("balaur_plugin_abi") && err.contains("balaur_extension_abi"),
        "does not say what was looked for: {err}"
    );
}

/// The other half of the header's `_Static_assert`s: those pin C to the
/// numbers, these pin Rust to them. A layout change now fails on both sides
/// rather than silently reinterpreting memory.
#[test]
fn the_rust_types_match_the_committed_header() {
    use std::mem::size_of;
    assert_eq!(size_of::<BalaurValue>(), 24, "BalaurValue");
    assert_eq!(size_of::<balaur_plugin::BalaurStr>(), 16, "BalaurStr");
    assert_eq!(size_of::<balaur_plugin::BalaurSlice>(), 16, "BalaurSlice");
    assert_eq!(size_of::<balaur_plugin::BalaurEntry>(), 40, "BalaurEntry");
    assert_eq!(
        std::mem::offset_of!(BalaurValue, payload),
        8,
        "BalaurValue payload"
    );
    assert_eq!(
        std::mem::offset_of!(balaur_plugin::BalaurEntry, value),
        16,
        "BalaurEntry value"
    );
}
