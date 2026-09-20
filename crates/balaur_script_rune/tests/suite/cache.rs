//! Compiled units kept between runs: what a second run reuses, and what it
//! must not.
//!
//! The claim is that a boot whose sources have not moved does not compile
//! them, and that a change to any file the unit was built from does. Each run
//! happens on a thread of its own, since the boot table a run files is its
//! thread's.

use std::path::Path;

use balaur_core::{App, AppConfig};

/// What one fresh app made of the project: the boot table it filed, and the
/// number its script left behind.
struct Run {
    boot: String,
    out: Option<f64>,
}

fn project(name: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!("[application]\nname = \"{name}\"\n"),
    )
    .unwrap();
    for (file, body) in files {
        std::fs::write(dir.path().join(file), body).unwrap();
    }
    dir
}

fn run(dir: &Path) -> Run {
    let root = dir.to_path_buf();
    std::thread::spawn(move || {
        let app = App::new(AppConfig {
            script_backend: Some(balaur_script_rune::factory()),
            ..AppConfig::bare(root)
        })
        .unwrap();
        let node = {
            let engine_root = app.engine.root();
            balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "User", engine_root)
        };
        let host = app
            .engine
            .script_host()
            .unwrap()
            .as_any()
            .downcast_ref::<balaur_script_rune::RuneHost>()
            .expect("the app is running Rune")
            .clone();
        host.attach(node, "s.rn").unwrap();
        Run {
            boot: balaur_core::timings::boot_report(),
            out: host.number_field(node, "out"),
        }
    })
    .join()
    .unwrap()
}

/// A name nothing else in the suite shares: a cached unit is keyed by the
/// project it belongs to, and the projects here are real user data.
fn unique(prefix: &str) -> String {
    format!("{prefix}_{}", std::process::id())
}

#[test]
fn a_second_run_reads_the_unit_back_instead_of_compiling_it() {
    let dir = project(
        &unique("cache_reuse"),
        &[("s.rn", "pub fn init(this) { this.out = 1.0; }\n")],
    );
    let first = run(dir.path());
    assert!(
        first.boot.contains("scripts/compiled"),
        "nothing compiled on the first run:\n{}",
        first.boot
    );
    let second = run(dir.path());
    assert!(
        second.boot.contains("scripts/cached"),
        "the second run did not read the unit back:\n{}",
        second.boot
    );
    assert!(
        !second.boot.contains("scripts/compiled"),
        "the second run compiled anyway:\n{}",
        second.boot
    );
    assert_eq!(second.out, Some(1.0));
}

#[test]
fn editing_the_script_is_a_miss() {
    let dir = project(
        &unique("cache_edit"),
        &[("s.rn", "pub fn init(this) { this.out = 1.0; }\n")],
    );
    assert_eq!(run(dir.path()).out, Some(1.0));
    std::fs::write(
        dir.path().join("s.rn"),
        "pub fn init(this) { this.out = 2.0; }\n",
    )
    .unwrap();
    let second = run(dir.path());
    assert_eq!(second.out, Some(2.0), "a stale unit ran");
    assert!(
        second.boot.contains("scripts/compiled"),
        "the edit was not compiled:\n{}",
        second.boot
    );
}

#[test]
fn editing_a_module_the_script_pulls_in_is_a_miss() {
    let dir = project(
        &unique("cache_module"),
        &[
            (
                "s.rn",
                "mod part;\npub fn init(this) { this.out = part::value(); }\n",
            ),
            ("part.rn", "pub fn value() { 1.0 }\n"),
        ],
    );
    assert_eq!(run(dir.path()).out, Some(1.0));
    std::fs::write(dir.path().join("part.rn"), "pub fn value() { 2.0 }\n").unwrap();
    let second = run(dir.path());
    assert_eq!(second.out, Some(2.0), "a stale unit ran");
    assert!(
        second.boot.contains("scripts/compiled"),
        "the edited module was not compiled:\n{}",
        second.boot
    );
}
