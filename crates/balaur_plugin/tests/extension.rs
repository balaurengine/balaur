#![cfg(feature = "dylib")]

use std::path::{Path, PathBuf};

use balaur_core::{App, AppConfig, Engine};
use balaur_plugin::{library_suffix, load_extension, load_extensions_in, AbiTag, Fingerprint};
use balaur_script::{NodeId, Value};

fn app_in(dir: &Path) -> App {
    App::new(AppConfig {
        script_backend: Some(balaur_script_rune::factory()),
        ..AppConfig::bare(dir.to_path_buf())
    })
    .unwrap()
}

fn app() -> App {
    app_in(Path::new("."))
}

/// Build the out-of-tree extension and return the library cargo produced.
///
/// Built through its own manifest into its own target directory, because that
/// is what an extension is: compiled separately, by someone who does not have
/// the engine's build tree. As a workspace member it shared the host's rlibs,
/// feature resolution and artifacts, which is the one arrangement where the
/// mismatches the fingerprint exists to catch cannot arise.
fn greeter() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let target = root.join("target/extension_greeter");
            let built = std::process::Command::new(env!("CARGO"))
                .current_dir(&root)
                .arg("build")
                .arg("--manifest-path")
                .arg(root.join("examples/extension_greeter/Cargo.toml"))
                .arg("--target-dir")
                .arg(&target)
                .status()
                .expect("cargo should run");
            assert!(built.success(), "the extension did not build");

            let suffix = library_suffix();
            let out = target.join("debug");
            let unix = out.join(format!("libextension_greeter.{suffix}"));
            if unix.exists() {
                unix
            } else {
                out.join(format!("extension_greeter.{suffix}"))
            }
        })
        .clone()
}

/// The greeter loaded into a fresh app, with `probe.rn` attached to a node.
///
/// Rune runs code from files, not strings, so each test's expressions are
/// `pub fn`s of that script and `call` runs one by name.
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
    let mut extension = unsafe { load_extension(&greeter()) }.expect("the extension should load");
    assert_eq!(extension.manifest().name, "greeter");
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

#[test]
fn an_extension_declares_itself_into_a_running_app() {
    let probe = loaded(
        "pub fn greet(this) { greeter::greet(\"world\") }\n\
         pub fn version(this) { greeter::VERSION }\n",
    );
    assert_eq!(probe.call("greet"), Value::Str("hello, world".into()));
    assert_eq!(probe.call("version"), Value::Str("0.1.0".into()));
}

/// What a separately compiled extension cannot do, recorded because it is not
/// obvious and it fails silently.
///
/// Resources are keyed by `TypeId`, and a `TypeId` is a hash over the crate's
/// identity as cargo compiled it -- package, features, profile, and the
/// metadata of everything it depends on. The host and an out-of-tree extension
/// each compile their own `balaur_core`, so `ProjectRoot` is one type to a
/// reader and two different keys to the resource map. Measured: the host
/// computes `TypeId(0x50b788f9...)` and the extension `TypeId(0x10770dfb...)`
/// for the same type.
///
/// So an extension may own state (the test above) but may not reach state the
/// engine owns, and `Engine::resource` would panic with "missing resource"
/// rather than name the cause. This is the ceiling on the Rust-ABI extension
/// path, and the reason a C ABI is the one that generalises: a C extension
/// reaches the engine through function pointers and never computes a `TypeId`
/// at all.
///
/// Asserting today's behaviour on purpose. If this starts failing, resource
/// keying was made stable across compilations and the limitation is gone --
/// delete the test rather than repair it.
#[test]
fn an_extension_cannot_reach_a_resource_the_host_inserted() {
    let probe = loaded("pub fn root(this) { greeter::project_root() }\n");
    assert_eq!(
        probe.call("root"),
        Value::Str("<not visible from the extension>".into()),
        "an out-of-tree extension can now see the host's resources"
    );
}

/// The extension owns state the engine now holds, which is the whole point of
/// loading one rather than calling into it.
#[test]
fn an_extension_owns_a_resource_the_engine_keeps() {
    let probe = loaded(
        "pub fn greetings(this) { greeter::greetings() }\n\
         pub fn greet(this) { greeter::greet(\"again\"); }\n",
    );
    assert_eq!(probe.call("greetings"), Value::Int(0));
    for _ in 0..3 {
        probe.call("greet");
    }
    assert_eq!(
        probe.call("greetings"),
        Value::Int(3),
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
    let found = unsafe { load_extensions_in(dir.path(), &[]) }.unwrap();
    assert!(found.is_empty());
    let absent = unsafe { load_extensions_in(Path::new("/nowhere"), &[]) }.unwrap();
    assert!(absent.is_empty());
}

#[test]
fn a_directory_of_extensions_loads_them_all() {
    let dir = tempfile::tempdir().unwrap();
    let source = greeter();
    let copy = dir.path().join(format!("greeter.{}", library_suffix()));
    std::fs::copy(&source, &copy).unwrap();

    let found = unsafe { load_extensions_in(dir.path(), &[]) }.unwrap();
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

    let found = unsafe { load_extensions_in(dir.path(), &[]) }.unwrap();
    assert_eq!(found[0].manifest().name, "greeter");
}

/// An extension that requires one nothing provides must be refused, naming
/// what is missing — not left half-loaded.
#[test]
fn a_directory_missing_a_requirement_is_refused() {
    use balaur_plugin::{load_order, Manifest};
    let manifests = vec![Manifest::new("greeter", "0.1.0").requiring(&["absent_thing"])];
    let err = load_order(&manifests, &[]).unwrap_err().to_string();
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
