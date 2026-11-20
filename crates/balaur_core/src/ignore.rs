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
//!
//! `tests/` is on every project's list: `balaur test` owns that folder.

use crate::engine::Engine;
use crate::vocabulary::keys;

pub const SETTING: &str = "application/ignore";

/// What no project ships.
const ALWAYS: &str = "tests/**";

/// The patterns in force for a project that lists `declared`.
fn with_always(declared: Option<&Vec<toml::Value>>) -> Vec<String> {
    declared
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(str::to_string))
        .chain([ALWAYS.to_string()])
        .collect()
}

/// The patterns a running project holds to.
pub fn patterns(eng: &Engine) -> Vec<String> {
    with_always(
        crate::settings::get(eng, SETTING)
            .as_ref()
            .and_then(toml::Value::as_array),
    )
}

/// The same list read straight from a `project.toml`, for a tool holding the
/// text rather than a running engine.
pub fn from_manifest(manifest: &str) -> Vec<String> {
    let document = toml::from_str::<toml::Value>(manifest).ok();
    with_always(
        document
            .as_ref()
            .and_then(|document| document.get(keys::APPLICATION))
            .and_then(|table| table.get(keys::IGNORE))
            .and_then(toml::Value::as_array),
    )
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
            .any(|part| glob_matches(pattern, part))
    })
}

/// Whether `pattern` matches `text`, with `*` inside a path segment and `**`
/// across them. Byte-wise: a wildcard spans whatever it spans, and everything
/// else is compared literally.
/// Time is the pattern's length times the text's, however many wildcards.
#[must_use]
pub fn glob_matches(pattern: &str, text: &str) -> bool {
    #[derive(Clone, Copy, PartialEq)]
    enum Token {
        AcrossSegments,
        InSegment,
        Byte(u8),
    }
    let mut tokens = Vec::with_capacity(pattern.len());
    let mut bytes = pattern.bytes().peekable();
    while let Some(byte) = bytes.next() {
        tokens.push(match byte {
            b'*' if bytes.next_if_eq(&b'*').is_some() => Token::AcrossSegments,
            b'*' => Token::InSegment,
            byte => Token::Byte(byte),
        });
    }

    // Transitions only move forward, so one pass in order closes the set.
    let close = |reached: &mut [bool]| {
        for at in 0..tokens.len() {
            if !reached[at] {
                continue;
            }
            match tokens[at] {
                Token::AcrossSegments => {
                    reached[at + 1] = true;
                    // `**/` stands for no directory at all as well as for many.
                    if tokens.get(at + 1) == Some(&Token::Byte(b'/')) {
                        reached[at + 2] = true;
                    }
                }
                Token::InSegment => reached[at + 1] = true,
                Token::Byte(_) => {}
            }
        }
    };

    let mut reached = vec![false; tokens.len() + 1];
    let mut next = reached.clone();
    reached[0] = true;
    close(&mut reached);
    for byte in text.bytes() {
        next.fill(false);
        for (at, token) in tokens.iter().enumerate() {
            if !reached[at] {
                continue;
            }
            match *token {
                Token::AcrossSegments => next[at] = true,
                Token::InSegment if byte != b'/' => next[at] = true,
                Token::Byte(want) if want == byte => next[at + 1] = true,
                _ => {}
            }
        }
        close(&mut next);
        if !next.contains(&true) {
            return false;
        }
        std::mem::swap(&mut reached, &mut next);
    }
    reached[tokens.len()]
}
