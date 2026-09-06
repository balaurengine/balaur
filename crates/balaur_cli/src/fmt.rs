//! `balaur fmt`: Rune's own formatter over a project's scripts.
//!
//! Formatting needs no project — the formatter parses a source and lays
//! it out — but the app is booted anyway so a loose file and a project
//! file go through one code path.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use balaur::AppConfig;

/// Format `.rn` files in place, or say which would change under `--check`.
///
/// A directory is walked for every `.rn` under it, so `balaur fmt` on a
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
    // Formatting needs no project: the formatter parses a source and lays it
    // out, so a loose `.rn` file outside any project formats too.
    let root = paths
        .first()
        .filter(|p| p.is_dir())
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    let mut app = balaur::standard_app(AppConfig::export(&root))?;
    app.load_project().ok();
    let host = balaur::rune::rune_of(&app.engine);
    let mut changed = 0;
    for file in &files {
        let source =
            std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
        let key = file.to_string_lossy();
        let formatted = match host.format(&key, &source) {
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

/// Every `.rn` file under `dir`.
fn collect_scripts(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_scripts(&path, out);
        } else if path.extension().is_some_and(|e| e == "rn") {
            out.push(path);
        }
    }
}
