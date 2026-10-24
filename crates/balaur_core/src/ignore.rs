//! `application/ignore`: the project files that are not the game's.
//!
//! Work in progress beside the art it came from, a scratch folder, an
//! exporter's leftovers. One list in `project.toml` rather than a marker file
//! per folder, so it is visible, versioned and in the place the rest of the
//! project's shape is declared:
//!
//! ```toml
//! [application]
//! ignore = ["art/wip/**", "**/*.blend1"]
//! ```
//!
//! A pattern matches a project-relative path, `*` inside a segment and `**`
//! across them, and a directory it names takes everything under it. The
//! asset index skips what it matches and a pack leaves it out, so it never
//! reaches a shipped game. A shipped script that `use`s a file it matches
//! fails the export, which names the pattern, rather than the game.

use crate::engine::Engine;

pub const SETTING: &str = "application/ignore";

/// The patterns a running project declares.
pub fn patterns(eng: &Engine) -> Vec<String> {
    crate::settings::get(eng, SETTING)
        .and_then(|value| value.as_array().cloned())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The same list read straight from a `project.toml`, for a tool holding the
/// text rather than a running engine.
pub fn from_manifest(manifest: &str) -> Vec<String> {
    let Ok(document) = toml::from_str::<toml::Value>(manifest) else {
        return Vec::new();
    };
    document
        .get("application")
        .and_then(|table| table.get("ignore"))
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether a project-relative path is ignored: the path itself, or a
/// directory above it, matching any pattern.
#[must_use]
pub fn ignored(patterns: &[String], rel: &str) -> bool {
    matching(patterns, rel).is_some()
}

/// The pattern that ignores a project-relative path, if one does: what to
/// name when an ignored file turns out to be wanted.
#[must_use]
pub fn matching<'a>(patterns: &'a [String], rel: &str) -> Option<&'a str> {
    if patterns.is_empty() {
        return None;
    }
    let rel = rel.trim_start_matches('/');
    patterns.iter().map(String::as_str).find(|pattern| {
        let pattern = pattern.trim_end_matches('/');
        std::iter::once(rel)
            .chain(rel.match_indices('/').map(|(at, _)| &rel[..at]))
            .any(|part| crate::pack::glob_matches(pattern, part))
    })
}
