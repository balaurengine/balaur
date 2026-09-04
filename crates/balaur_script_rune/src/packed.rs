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

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
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
    let (logic, functions): (Logic, Vec<PublicSignature>) = bincode::deserialize(rest)?;
    Ok((Unit::from_parts(logic, None)?, functions))
}

/// Whether `bytes` look like [`encode`]'s output rather than script source.
pub(crate) fn is_encoded(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
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
