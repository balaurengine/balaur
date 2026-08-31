//! Stamp the compiler's identity into the crate.
//!
//! `Fingerprint` refuses a plugin built by a different rustc, because Rust has
//! no stable ABI and loading such a library is undefined behaviour rather than
//! a version error. That refusal is only worth anything if both sides know
//! which rustc built them, and a crate cannot ask at run time — the answer has
//! to be baked in at compile time, separately for the host and for every
//! plugin, which is exactly what a build script is for.
//!
//! Failing here is deliberate. The previous version fell back to the string
//! `"unknown"`, which both sides then agreed on, so the check passed for every
//! mismatched build it existed to refuse. A check that cannot fail is worse
//! than no check, because it is trusted.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=RUSTC");

    // Cargo always sets RUSTC; the fallback is for a bare `rustc --edition`
    // invocation outside cargo, which is not how this crate is built but is
    // cheap to tolerate.
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());

    let output = Command::new(&rustc)
        .arg("-vV")
        .output()
        .unwrap_or_else(|e| panic!("cannot run `{rustc} -vV` to identify the compiler: {e}"));
    assert!(
        output.status.success(),
        "`{rustc} -vV` failed with {}",
        output.status
    );

    let text = String::from_utf8_lossy(&output.stdout);
    let version =
        identity(&text).unwrap_or_else(|| panic!("`{rustc} -vV` printed no release line:\n{text}"));

    println!("cargo:rustc-env=BALAUR_RUSTC_VERSION={version}");
}

/// `release` plus `commit-hash`, which is what actually decides layout.
///
/// The release alone is too coarse (a nightly is not its release number) and
/// the whole `-vV` block is too long for the fixed-size field it has to
/// survive in `AbiTag`. Two builds that agree on both of these agree on the
/// compiler.
fn identity(version_output: &str) -> Option<String> {
    let mut release = None;
    let mut commit = None;
    for line in version_output.lines() {
        if let Some(value) = line.strip_prefix("release: ") {
            release = Some(value.trim());
        } else if let Some(value) = line.strip_prefix("commit-hash: ") {
            commit = Some(value.trim());
        }
    }
    let release = release?;
    // A compiler built from source reports no commit hash; the release on its
    // own is the best available answer there.
    Some(match commit {
        Some(hash) => format!("{release} ({})", &hash[..hash.len().min(9)]),
        None => release.to_string(),
    })
}
