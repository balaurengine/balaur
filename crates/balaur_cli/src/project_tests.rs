//! `balaur test`: each test script in its own headless app.

use std::path::Path;

use anyhow::{Context, Result};
use balaur::AppConfig;

/// `balaur test`: each test script on its own node in its own headless app,
/// failed by any script error the run logs. The project's main scene loads
/// first, so a test finds the nodes a game would.
pub(crate) fn test_project(path: &Path, frames: u64, filter: Option<&str>) -> Result<()> {
    let tests = test_scripts(path);
    let mut failed = 0usize;
    let mut ran = 0usize;
    for rel in tests {
        if filter.is_some_and(|f| !rel.contains(f)) {
            continue;
        }
        ran += 1;
        balaur::logbuf::clear();
        let outcome = run_test(path, &rel, frames);
        let errors: Vec<String> = balaur::logbuf::recent(500)
            .into_iter()
            .filter(|entry| entry.level == "error")
            .map(|entry| entry.message)
            .collect();
        match (outcome, errors.is_empty()) {
            (Ok(()), true) => println!("test {rel} ... ok"),
            (Ok(()), false) => {
                failed += 1;
                println!("test {rel} ... FAILED");
                for message in errors {
                    println!("    {message}");
                }
            }
            (Err(why), _) => {
                failed += 1;
                println!("test {rel} ... FAILED\n    {why:#}");
            }
        }
    }
    if ran == 0 {
        println!("no tests: put `.rn` files under tests/");
        return Ok(());
    }
    println!("{} passed, {failed} failed", ran - failed);
    if failed > 0 {
        anyhow::bail!("{failed} of {ran} tests failed");
    }
    Ok(())
}

/// Every `.rn` under `tests/`, project-relative and sorted.
pub(crate) fn test_scripts(project_root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut dirs = vec![project_root.join("tests")];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rn")
                && let Ok(rel) = path.strip_prefix(project_root)
            {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

pub(crate) fn run_test(project_root: &Path, rel: &str, frames: u64) -> Result<()> {
    let mut app = balaur::standard_app(AppConfig::export(project_root))?;
    app.load_project()?;
    let root = app.engine.root();
    let node = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Test", root);
    let host = app
        .engine
        .script_host()
        .context("no script backend for the project")?;
    host.attach(balaur::node_id_of(node), rel)?;
    for _ in 0..frames {
        app.tick(balaur::FIXED_DT);
    }
    Ok(())
}
