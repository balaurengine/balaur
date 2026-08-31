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

fn app() -> App {
    App::new(AppConfig {
        project_root: PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_luau::factory()),
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

/// Load the counter into a fresh app.
///
/// The `Extension` comes back with the app because it owns the open library,
/// and every function pointer it registered points into that library.
fn loaded() -> (App, balaur_plugin::Extension) {
    let mut app = app();
    let mut extension = unsafe { load_extension(&counter()) }.expect("the C extension should load");
    assert_eq!(extension.manifest().name, "counter");
    balaur_plugin::load(&mut app, extension.plugin_mut()).unwrap();
    (app, extension)
}

#[test]
fn a_c_extension_declares_itself_into_a_running_app() {
    let (app, _library) = loaded();
    let lua = balaur_script_luau::lua_of(&app.engine);
    let greeting: String = lua.load("return counter.greet('world')").eval().unwrap();
    assert_eq!(greeting, "hello, world");
}

#[test]
fn numbers_cross_the_boundary_in_both_directions() {
    let (app, _library) = loaded();
    let lua = balaur_script_luau::lua_of(&app.engine);
    let sum: f64 = lua.load("return counter.add(2, 3)").eval().unwrap();
    assert!((sum - 5.0).abs() < f64::EPSILON);
    // Luau hands whole numbers over as integers and fractions as floats; both
    // reach the same C function.
    let mixed: f64 = lua.load("return counter.add(0.5, 0.25)").eval().unwrap();
    assert!((mixed - 0.75).abs() < f64::EPSILON);
}

/// The `user` pointer is how a C extension keeps state, with no Rust type and
/// no `TypeId` involved -- which is exactly what the Rust extension path
/// cannot do across separate compilations.
#[test]
fn a_c_extension_keeps_its_own_state_across_calls() {
    let (app, _library) = loaded();
    let lua = balaur_script_luau::lua_of(&app.engine);
    let first: i64 = lua.load("return counter.bump()").eval().unwrap();
    let second: i64 = lua.load("return counter.bump()").eval().unwrap();
    assert_eq!(second, first + 1);
}

#[test]
fn lists_cross_the_boundary_in_both_directions() {
    let (app, _library) = loaded();
    let lua = balaur_script_luau::lua_of(&app.engine);
    let out: Vec<f64> = lua.load("return counter.triple(2)").eval().unwrap();
    assert_eq!(out, vec![2.0, 4.0, 6.0]);

    let total: f64 = lua
        .load("return counter.sum_list({1, 2, 3.5})")
        .eval()
        .unwrap();
    assert!((total - 6.5).abs() < f64::EPSILON);
}

#[test]
fn a_constant_registered_from_c_is_readable() {
    let (app, _library) = loaded();
    let lua = balaur_script_luau::lua_of(&app.engine);
    let version: String = lua.load("return counter.VERSION").eval().unwrap();
    assert_eq!(version, "0.1.0");
}

/// A non-zero return becomes a script error, and the string the extension
/// wrote becomes the message. Without that, a failing extension is a silent
/// nil.
#[test]
fn a_failing_c_function_becomes_a_script_error_with_its_message() {
    let (app, _library) = loaded();
    let lua = balaur_script_luau::lua_of(&app.engine);
    let err = lua
        .load("return counter.always_fails()")
        .exec()
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("this function fails on purpose"),
        "the extension's own message did not reach the script: {err}"
    );

    let wrong_types = lua
        .load("return counter.add('a', 'b')")
        .exec()
        .unwrap_err()
        .to_string();
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
