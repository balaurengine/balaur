//! `balaur fmt`: a project's scripts laid out the house's way, by
//! `balaur_bs`'s formatter, which needs nothing but the source.

use std::path::PathBuf;

use anyhow::{Context as _, Result};

/// Format `.bs` files in place, or say which would change under `--check`.
///
/// A directory is walked for every script under it, so `balaur fmt` on a
/// project formats the project. Exits 1 under `--check` when one would change,
/// which is what CI reads.
pub(crate) fn run(paths: &[PathBuf], check: bool) -> Result<()> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            collect_scripts(path, &mut files);
        } else {
            files.push(path.clone());
        }
    }
    files.sort();
    files.dedup();
    let mut changed = 0;
    for file in &files {
        let source =
            std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
        let formatted = match format(&source) {
            Ok(text) => text,
            Err(err) => {
                println!("{}: {err}", file.display());
                continue;
            }
        };
        if formatted == source {
            continue;
        }
        changed += 1;
        if check {
            println!("{}", file.display());
        } else {
            std::fs::write(file, &formatted)
                .with_context(|| format!("writing {}", file.display()))?;
        }
    }
    if check && changed > 0 {
        println!("{changed} of {} files would change", files.len());
        std::process::exit(1);
    }
    println!(
        "{} {} of {} files",
        if check { "would format" } else { "formatted" },
        changed,
        files.len()
    );
    Ok(())
}

#[cfg(any(feature = "bs", feature = "bs-jit", feature = "bs-web"))]
fn format(source: &str) -> Result<String, String> {
    balaur::script_bs::fmt::format(source).map_err(|e| e.to_string())
}

#[cfg(not(any(feature = "bs", feature = "bs-jit", feature = "bs-web")))]
fn format(_: &str) -> Result<String, String> {
    Err("this build has no Balaur Script".into())
}

/// Every `.bs` file under `dir`.
fn collect_scripts(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_scripts(&path, out);
        } else if path.extension().is_some_and(|e| e == "bs") {
            out.push(path);
        }
    }
}
