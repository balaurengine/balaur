#![cfg(feature = "dylib")]

use std::path::{Path, PathBuf};

use balaur_core::{App, AppConfig, Engine};
use balaur_plugin::{library_suffix, load_extension, load_extensions_in, AbiTag, Fingerprint};

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

/// Build the out-of-tree extension and return the library cargo produced.
///
/// Built here rather than assumed present: nothing makes cargo compile a
/// cdylib before a test that only reads it off disk.
fn greeter() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let built = std::process::Command::new(env!("CARGO"))
                .current_dir(&root)
                .args(["build", "-p", "extension_greeter"])
                .status()
                .expect("cargo should run");
            assert!(built.success(), "the extension did not build");

            let target = root.join("target/debug");
            let unix = target.join(format!("libextension_greeter.{}", library_suffix()));
            if unix.exists() {
                unix
            } else {
                target.join(format!("extension_greeter.{}", library_suffix()))
            }
        })
        .clone()
}

#[test]
fn an_extension_declares_itself_into_a_running_app() {
    let mut app = app();
    let mut extension = unsafe { load_extension(&greeter()) }.expect("the extension should load");
    assert_eq!(extension.manifest().name, "greeter");

    balaur_plugin::load(&mut app, extension.plugin_mut()).unwrap();

    let lua = balaur_script_luau::lua_of(&app.engine);
    let greeting: String = lua.load("return greeter.greet('world')").eval().unwrap();
    assert_eq!(greeting, "hello, world");
    let version: String = lua.load("return greeter.VERSION").eval().unwrap();
    assert_eq!(version, "0.1.0");
}

/// The extension owns state the engine now holds, which is the whole point of
/// loading one rather than calling into it.
#[test]
fn an_extension_owns_a_resource_the_engine_keeps() {
    let mut app = app();
    let mut extension = unsafe { load_extension(&greeter()) }.unwrap();
    balaur_plugin::load(&mut app, extension.plugin_mut()).unwrap();

    let lua = balaur_script_luau::lua_of(&app.engine);
    assert_eq!(
        lua.load("return greeter.greetings()")
            .eval::<i64>()
            .unwrap(),
        0
    );
    for _ in 0..3 {
        lua.load("greeter.greet('again')").exec().unwrap();
    }
    assert_eq!(
        lua.load("return greeter.greetings()")
            .eval::<i64>()
            .unwrap(),
        3,
        "the extension's own state did not survive the calls"
    );
}

#[test]
fn a_file_that_is_not_an_extension_is_refused_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let fake = dir.path().join(format!("notreally.{}", library_suffix()));
    std::fs::write(&fake, b"not a library").unwrap();
    let Err(err) = (unsafe { load_extension(&fake) }) else {
        panic!("a text file loaded as an extension");
    };
    assert!(
        format!("{err:#}").contains("notreally"),
        "unhelpful: {err:#}"
    );
}

#[test]
fn a_missing_file_is_an_error_rather_than_a_panic() {
    let missing = Path::new("/nowhere/at/all.dylib");
    assert!(unsafe { load_extension(missing) }.is_err());
}

#[test]
fn an_empty_directory_yields_no_extensions() {
    let dir = tempfile::tempdir().unwrap();
    let found = unsafe { load_extensions_in(dir.path()) }.unwrap();
    assert!(found.is_empty());
    let absent = unsafe { load_extensions_in(Path::new("/nowhere")) }.unwrap();
    assert!(absent.is_empty());
}

#[test]
fn a_directory_of_extensions_loads_them_all() {
    let dir = tempfile::tempdir().unwrap();
    let source = greeter();
    let copy = dir.path().join(format!("greeter.{}", library_suffix()));
    std::fs::copy(&source, &copy).unwrap();

    let found = unsafe { load_extensions_in(dir.path()) }.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].manifest().name, "greeter");
    assert_eq!(found[0].path(), copy);
}

/// The tag is what stops a library built by another compiler from being
/// called at all, so it has to survive the round trip through fixed-size
/// fields unchanged.
#[test]
fn the_abi_tag_round_trips_through_its_c_representation() {
    let tag = AbiTag::current();
    assert_eq!(tag.fingerprint(), Fingerprint::current());
    assert!(Fingerprint::current()
        .differences(&tag.fingerprint())
        .is_empty());
}

#[test]
fn the_library_suffix_matches_this_platform() {
    let expected = if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };
    assert_eq!(library_suffix(), expected);
}

#[test]
fn a_loaded_extension_can_be_ticked_like_any_plugin() {
    let mut app = app();
    let mut extension = unsafe { load_extension(&greeter()) }.unwrap();
    balaur_plugin::load(&mut app, extension.plugin_mut()).unwrap();
    for _ in 0..5 {
        app.tick(1.0 / 60.0);
    }
    let _ = &app.engine as &Engine;
}

/// Two copies of the same library are two extensions with the same name, so
/// ordering has something to disagree about. Renaming the file must not change
/// what the manifest says.
#[test]
fn the_file_name_does_not_decide_the_plugin_name() {
    let dir = tempfile::tempdir().unwrap();
    let renamed = dir
        .path()
        .join(format!("zzz_last_alphabetically.{}", library_suffix()));
    std::fs::copy(greeter(), &renamed).unwrap();

    let found = unsafe { load_extensions_in(dir.path()) }.unwrap();
    assert_eq!(found[0].manifest().name, "greeter");
}

/// An extension that requires one nothing provides must be refused, naming
/// what is missing — not left half-loaded.
#[test]
fn a_directory_missing_a_requirement_is_refused() {
    use balaur_plugin::{load_order, Manifest};
    let manifests = vec![Manifest::new("greeter", "0.1.0").requiring(&["absent_thing"])];
    let err = load_order(&manifests).unwrap_err().to_string();
    assert!(err.contains("absent_thing"), "unhelpful: {err}");
}

/// The check that makes loading foreign code defensible, tested as the
/// decision rather than end to end: macOS kills a process that opens a library
/// whose bytes were edited, so a tampered file is killed by the loader long
/// before this code could refuse it.
#[test]
fn a_build_for_another_engine_or_compiler_is_refused() {
    use balaur_plugin::refuse_mismatch;
    let at = Path::new("/somewhere/thing.dylib");
    assert!(refuse_mismatch(at, &Fingerprint::current()).is_ok());

    for (field, value) in [("balaur", "9.9.9"), ("rustc", "1.0.0")] {
        let mut theirs = Fingerprint::current();
        if field == "balaur" {
            theirs.engine = value.into();
        } else {
            theirs.rustc = value.into();
        }
        let err = refuse_mismatch(at, &theirs).unwrap_err().to_string();
        assert!(err.contains(value), "does not name what differs: {err}");
        assert!(err.contains("thing.dylib"), "does not name the file: {err}");
    }

    let mut theirs = Fingerprint::current();
    theirs.registry_abi += 1;
    assert!(
        refuse_mismatch(at, &theirs).is_err(),
        "a stale registry abi was accepted"
    );
}
