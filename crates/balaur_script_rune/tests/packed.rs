//! The pack ships compiled units, not `.rn` source.
//!
//! Two claims are made for that: a packed project starts without compiling,
//! and the source does not travel with the game. Both are checked here, on a
//! pack built the way the exporter builds one.

use balaur_core::{App, AppConfig, Pack};
use balaur_script::ScriptCompiler;

const SOURCE: &str = "\
pub fn exports() { #{ speed: 3.0 } }\n\
pub fn init(this) { this.hits = 0; }\n\
pub fn update(this, dt) { this.hits = private_ritual(this.hits); }\n\
fn private_ritual(n) { n + 1 }\n";

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.toml"), "[project]\nname = \"t\"\n").unwrap();
    std::fs::write(dir.path().join("s.rn"), SOURCE).unwrap();
    dir
}

fn app_in(dir: &std::path::Path, pack: Option<Pack>) -> App {
    App::new(AppConfig {
        pack,
        script_backend: Some(balaur_script_rune::factory()),
        ..AppConfig::bare(dir.to_path_buf())
    })
    .unwrap()
}

/// Build a pack through the host, the way the exporter does.
fn pack_of(dir: &std::path::Path) -> Pack {
    let app = app_in(dir, None);
    let host = app.engine.script_host().unwrap();
    let compiler = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .unwrap();
    Pack::build(dir, compiler as &dyn ScriptCompiler).unwrap()
}

#[test]
fn a_packed_script_is_a_unit_not_its_source() {
    let dir = project();
    let pack = pack_of(dir.path());
    let bytes = pack.scripts.get("s.rn").expect("s.rn is in the pack");

    assert_ne!(
        bytes.as_slice(),
        SOURCE.as_bytes(),
        "the source was shipped"
    );

    let text = String::from_utf8_lossy(bytes);
    // No source text at all: no syntax, no expressions, no control flow.
    for fragment in ["pub fn", "n + 1", "if ", "let "] {
        assert!(
            !text.contains(fragment),
            "{fragment:?} is readable in the packed script"
        );
    }
}

/// What survives, asserted rather than left implied so nobody mistakes this
/// for encryption: field names, string literals, and every function name —
/// private ones included, because rune keeps them as static strings whether
/// or not the debug info is dropped.
///
/// If this test starts failing because a name no longer appears, that is an
/// improvement, not a break; the doc in `src/packed.rs` should change with it.
#[test]
fn a_packed_script_still_carries_every_name() {
    let dir = project();
    let pack = pack_of(dir.path());
    let text = String::from_utf8_lossy(pack.scripts.get("s.rn").unwrap()).into_owned();
    for fragment in ["speed", "hits", "update", "init", "private_ritual"] {
        assert!(
            text.contains(fragment),
            "{fragment:?} was expected to survive"
        );
    }
}

#[test]
fn a_packed_script_runs_and_keeps_its_exported_defaults() {
    let dir = project();
    let pack = pack_of(dir.path());
    // The sources go away: nothing may fall back to reading them.
    drop(dir);

    let elsewhere = tempfile::tempdir().unwrap();
    let app = app_in(elsewhere.path(), Some(pack));
    let host = app.engine.script_host().unwrap();
    let root = app.engine.root();
    let node = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "n", root);
    host.attach(balaur_core::node_id_of(node), "s.rn").unwrap();

    host.update(1.0 / 60.0);
    host.update(1.0 / 60.0);

    let saved = host.save_state();
    let (_, state) = saved.first().expect("one instance");
    let balaur_script::Value::Map(fields) = state else {
        panic!("expected a map, got {state:?}");
    };
    let get = |name: &str| fields.iter().find(|(k, _)| k == name).map(|(_, v)| v);
    assert_eq!(get("hits"), Some(&balaur_script::Value::Int(2)));
    assert_eq!(get("speed"), Some(&balaur_script::Value::Num(3.0)));
}
