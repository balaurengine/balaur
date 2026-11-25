//! The shims' plain-Java logic, compiled and run on the JVM of the machine
//! the tests run on: the template's dex is the same source, compiled for
//! Android by scripts/package_runtime.sh.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The shim sources under `java/` and their tests under `tests/java/`.
fn sources(names: &[&str]) -> Vec<PathBuf> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    names
        .iter()
        .map(|name| {
            let shim = crate_dir.join("java/org/balaur/platform").join(name);
            if shim.is_file() {
                shim
            } else {
                crate_dir.join("tests/java/org/balaur/platform").join(name)
            }
        })
        .collect()
}

/// A JDK's tool, from `JAVA_HOME` or the path; `None` where none is installed.
fn jdk(tool: &str) -> Option<PathBuf> {
    let home = std::env::var_os("JAVA_HOME")
        .map(|home| PathBuf::from(home).join("bin").join(tool))
        .filter(|path| {
            path.with_extension(std::env::consts::EXE_EXTENSION)
                .is_file()
        });
    let found = home.unwrap_or_else(|| PathBuf::from(tool));
    Command::new(&found)
        .arg("-version")
        .output()
        .is_ok_and(|ran| ran.status.success())
        .then_some(found)
}

/// Compile `files` as the template does (`-Werror -Xlint:all`) and run `main`.
fn run_java(files: &[&str], main: &str) {
    let (Some(javac), Some(java)) = (jdk("javac"), jdk("java")) else {
        eprintln!("skipped: no JDK");
        return;
    };
    let classes = tempfile::tempdir().unwrap();
    let compiled = Command::new(javac)
        .args(["--release", "11", "-Werror", "-Xlint:all", "-d"])
        .arg(classes.path())
        .args(sources(files))
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "javac: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let ran = Command::new(java)
        .arg("-cp")
        .arg(classes.path())
        .arg(main)
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{main}: {}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
}

// The chooser's pick is a broadcast and its result an activity callback, in
// no set order: a pick answers `shared` whichever lands first.
#[test]
fn a_share_answers_its_pick_whether_it_lands_before_or_after_the_chooser_shuts() {
    run_java(
        &["ShareAnswer.java", "ShareAnswerTest.java"],
        "org.balaur.platform.ShareAnswerTest",
    );
}
