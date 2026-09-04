//! The pack's script format: a compiled unit instead of its source.
//!
//! A dev run compiles `.rn` off disk. A shipped pack carries the unit the
//! exporter already built, so startup costs a deserialise rather than a
//! compile, and the source does not ship.
//!
//! What a unit still spells out, checked by `tests/packed.rs`: every function
//! name, private ones included, because rune keeps them as static strings;
//! object field names; and string literals. What it does not: any source
//! text — no expressions, no control flow, no comments, no names of locals.
//! So a reader learns the shape of the API and nothing about the algorithm.
//! That is a long way from shipping the source and a long way from
//! encryption. Treat it as "not casually readable", not as protection.
//!
//! Rune promises no stability for a serialised unit across its own versions.
//! It does not have to: balaur pins one fork commit, and [`FORMAT`] is bumped
//! whenever that pin moves to a rune whose `Unit` changed shape. A pack from
//! any other version is rejected here rather than deserialised into nonsense.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use bincode::Options as _;
use rune::runtime::{Logic, Unit};
use rune::Source;

use crate::inspect::PublicSignature;

const MAGIC: &[u8; 4] = b"BLRU";

/// Bump when the pinned rune fork changes `Unit`'s serialised shape.
const FORMAT: u32 = 1;

/// Serialise a compiled unit and its public signatures for the pack.
///
/// The unit's `Logic` rather than the unit: `Unit` flattens that field, which
/// serialises as a map of unknown length, and a length-prefixed format cannot
/// write one. It also means the debug half cannot reach the file by accident.
///
/// Written as a pair rather than a struct so the writing side can borrow and
/// the reading side can own, without `Logic` having to be `Clone`.
pub(crate) fn encode(unit: &Unit, functions: &[PublicSignature]) -> Result<Vec<u8>> {
    let mut out = Vec::from(*MAGIC);
    out.extend_from_slice(&FORMAT.to_le_bytes());
    bincode::serialize_into(&mut out, &(unit.logic(), functions))?;
    Ok(out)
}

/// Read back what [`encode`] wrote.
pub(crate) fn decode(bytes: &[u8]) -> Result<(Unit, Vec<PublicSignature>)> {
    let Some(rest) = bytes.strip_prefix(MAGIC) else {
        bail!("not a compiled balaur script");
    };
    let (version, rest) = rest
        .split_first_chunk::<4>()
        .ok_or_else(|| anyhow!("compiled script is truncated"))?;
    let version = u32::from_le_bytes(*version);
    if version != FORMAT {
        bail!("compiled script is format {version}, this build reads {FORMAT}");
    }
    let (logic, functions): (Logic, Vec<PublicSignature>) = options(rest.len() as u64)
        .deserialize(rest)
        .map_err(|why| anyhow!("compiled script does not read back: {why}"))?;
    Ok((Unit::from_parts(logic, None)?, functions))
}

/// [`encode`]'s own encoding, bounded by the bytes actually on hand.
///
/// A crafted length prefix would otherwise reserve gigabytes before a single
/// field was validated; nothing genuine decodes to more than it was read from.
fn options(limit: u64) -> impl bincode::Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .with_limit(limit)
}

/// Whether `bytes` look like [`encode`]'s output rather than script source.
pub(crate) fn is_encoded(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Every file a compiled unit was read from, as project-relative keys.
///
/// A file pulled in by `mod name;` is folded into its root's unit and is a key
/// nowhere else; this is the only record that a save of it belongs to that
/// root.
pub(crate) fn source_keys(root: &Path, sources: &rune::Sources) -> Vec<String> {
    let mut keys = Vec::new();
    for index in 0.. {
        let Some(source) = sources.get(rune::SourceId::new(index)) else {
            break;
        };
        let Some(path) = source.path() else { continue };
        let key = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let key = key.trim_start_matches("./").to_string();
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

/// Every `.rn` under `root` that another one pulls in with `mod name;`.
///
/// Only a root is compiled for a pack: a submodule compiled on its own fails
/// on the `super::` items it names, and one that does compile ships twice —
/// once on its own and once folded into every root that reaches it.
pub(crate) fn module_files(root: &Path) -> BTreeSet<String> {
    let mut files = Vec::new();
    collect_scripts(root, root, &mut files);
    let mut found = BTreeSet::new();
    for rel in &files {
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        let dir = Path::new(rel)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        for name in declared_mods(&text) {
            for candidate in [
                dir.join(&name).with_extension("rn"),
                dir.join(&name).join("mod.rn"),
            ] {
                let key = candidate.to_string_lossy().replace('\\', "/");
                if files.contains(&key) {
                    found.insert(key);
                }
            }
        }
    }
    found
}

/// The modules a source pulls in from a file. The braced `mod name { .. }`
/// declares one inline and loads nothing.
fn declared_mods(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in source.lines() {
        let rest = line.trim_start();
        let rest = rest.strip_prefix("pub ").unwrap_or(rest).trim_start();
        let Some(rest) = rest.strip_prefix("mod ") else {
            continue;
        };
        let Some(name) = rest.trim().strip_suffix(';') else {
            continue;
        };
        let name = name.trim();
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            out.push(name.to_string());
        }
    }
    out
}

/// Every `.rn` under `dir`, as project-relative keys, in a fixed order.
fn collect_scripts(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        })
        .collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_scripts(root, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rn") {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

/// `mod name;` in a packed script: `name.rn` or `name/mod.rn` beside the
/// requesting file, looked up in the pack.
pub(crate) struct PackSourceLoader {
    pub(crate) scripts: std::collections::BTreeMap<String, Vec<u8>>,
}

impl rune::compile::SourceLoader for PackSourceLoader {
    fn load(
        &mut self,
        root: &Path,
        item: &rune::Item,
        span: &dyn rune::ast::Spanned,
    ) -> rune::compile::Result<Source> {
        let not_found = |path: PathBuf| {
            rune::compile::Error::msg(
                span,
                format!("module {} is not in the pack", path.display()),
            )
        };
        let mut base = root.to_path_buf();
        base.pop();
        for component in item {
            match component {
                rune::item::ComponentRef::Str(name) => base.push(name),
                _ => return Err(not_found(base)),
            }
        }
        for candidate in [base.join("mod.rn"), base.with_extension("rn")] {
            let key = candidate.to_string_lossy().replace('\\', "/");
            if let Some(bytes) = self.scripts.get(&key) {
                // A compiled pack has already folded its modules into each
                // unit, so nothing should be compiling against one here.
                if is_encoded(bytes) {
                    return Err(rune::compile::Error::msg(
                        span,
                        format!("module {key} is compiled, not source"),
                    ));
                }
                let text = String::from_utf8_lossy(bytes);
                return Ok(Source::with_path(key.as_str(), text.as_ref(), &candidate)?);
            }
        }
        Err(not_found(base))
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, is_encoded, FORMAT, MAGIC};

    #[test]
    fn source_is_not_mistaken_for_a_unit() {
        assert!(!is_encoded(b"pub fn update(this, dt) {}\n"));
        assert!(!is_encoded(b""));
        assert!(is_encoded(MAGIC));
    }

    #[test]
    fn a_foreign_format_is_refused_rather_than_read() {
        let mut bytes = Vec::from(*MAGIC);
        bytes.extend_from_slice(&(FORMAT + 1).to_le_bytes());
        bytes.extend_from_slice(&[0; 32]);
        let err = decode(&bytes).unwrap_err().to_string();
        assert!(err.contains("format"), "{err}");
    }

    #[test]
    fn truncated_input_is_refused() {
        let mut bytes = Vec::from(*MAGIC);
        bytes.push(0);
        assert!(decode(&bytes).is_err());
    }
}
