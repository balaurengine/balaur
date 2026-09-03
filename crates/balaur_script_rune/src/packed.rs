//! The pack's script format: a compiled unit instead of its source.
//!
//! A dev run compiles `.rn` off disk. A shipped pack carries the unit the
//! exporter already built, so startup costs a deserialise rather than a
//! compile, and the source does not ship. What stays readable in a unit is
//! string literals, object field names and type paths; control flow and
//! function names do not. That is a long way from shipping the source and a
//! long way from encryption — treat it as "not casually readable".
//!
//! Rune promises no stability for a serialised unit across its own versions.
//! It does not have to: balaur pins one fork commit, and [`FORMAT`] is bumped
//! whenever that pin moves to a rune whose `Unit` changed shape. A pack from
//! any other version is rejected here rather than deserialised into nonsense.

use anyhow::{anyhow, bail, Result};
use rune::runtime::Unit;

use crate::inspect::PublicSignature;

const MAGIC: &[u8; 4] = b"BLRU";

/// Bump when the pinned rune fork changes `Unit`'s serialised shape.
const FORMAT: u32 = 1;

/// Serialise a compiled unit and its public signatures for the pack.
///
/// Written as a pair rather than a struct so the writing side can borrow and
/// the reading side can own, without `Unit` having to be `Clone`.
pub(crate) fn encode(unit: &Unit, functions: &[PublicSignature]) -> Result<Vec<u8>> {
    let mut out = Vec::from(*MAGIC);
    out.extend_from_slice(&FORMAT.to_le_bytes());
    bincode::serialize_into(&mut out, &(unit, functions))?;
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
    Ok(bincode::deserialize(rest)?)
}

/// Whether `bytes` look like [`encode`]'s output rather than script source.
pub(crate) fn is_encoded(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
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
