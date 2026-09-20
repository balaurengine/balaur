//! Compiled units kept between runs, so a dev boot recompiles only what moved.
//!
//! A pack ships its units (see [`crate::packed`]); a dev run compiles the
//! source every time it starts, and the editor is 32,000 lines of Rune of its
//! own. Compiling them is most of what an editor launch costs, and they change
//! only when somebody edits the editor.
//!
//! What makes a hit safe is the stamp and the source hashes. Every file the
//! unit was compiled from is hashed, in the order rune gave it a `SourceId`,
//! so a span in a cached unit points at the file it pointed at when the unit
//! was built. The stamp covers the engine binary and the addons mounted into
//! the compile context, which together decide what a source is allowed to
//! name. Anything else is a miss, and a miss costs the compile it would have
//! cost anyway.

use std::path::{Path, PathBuf};

use balaur_core::digest::Hasher;
use rune::runtime::{DebugInfo, Logic, Unit};
use rune::{Source, Sources};
use serde::{Deserialize, Serialize};

use crate::RuneHost;
use crate::inspect::with_constants;
use crate::packed::{CONFIG, DECODE_LIMIT};

const MAGIC: &[u8; 4] = b"BLRC";

/// Bump when [`Entry`] changes shape.
const FORMAT: u32 = 1;

/// What a cache file holds. Written as a tuple, so the writing side can hand
/// over the unit's own halves rather than cloning them.
#[derive(Deserialize)]
struct Entry {
    stamp: u64,
    sources: Vec<Origin>,
    logic: Logic,
    /// A dev unit keeps its debug info, so a cached one has to carry it:
    /// without it a breakpoint has no line and a runtime error has no span.
    debug: Option<DebugInfo>,
}

/// One source a unit was compiled from, at the `SourceId` its position stands
/// for.
#[derive(Serialize, Deserialize)]
struct Origin {
    name: String,
    path: PathBuf,
    hash: u64,
}

/// A unit read back, with the sources rebuilt around it.
pub(crate) struct Hit {
    pub(crate) unit: Unit,
    pub(crate) sources: Sources,
}

/// The cached unit for `key`, or `None` when there is not a valid one.
///
/// `source` is the root's text as it was read, before [`with_constants`].
pub(crate) fn load(host: &RuneHost, key: &str, source: &str) -> Option<Hit> {
    // Before the stamp: the mounts it covers are discovered with the context,
    // and an empty set here would never match the set a compile stored.
    host.context().ok()?;
    let path = file_of(host, key)?;
    let bytes = balaur_core::files::backend(&host.engine).read(&path).ok()?;
    let rest = bytes.strip_prefix(MAGIC)?;
    let (version, rest) = rest.split_first_chunk::<4>()?;
    if u32::from_le_bytes(*version) != FORMAT {
        return None;
    }
    let config = CONFIG.with_limit::<DECODE_LIMIT>();
    let (entry, _): (Entry, usize) = bincode::serde::decode_from_slice(rest, config).ok()?;
    if entry.stamp != stamp(host) {
        return None;
    }
    let mut sources = Sources::new();
    for (id, origin) in entry.sources.iter().enumerate() {
        let text = text_at(host, id, &origin.path, source)?;
        if hash_of(&text) != origin.hash {
            return None;
        }
        sources
            .insert(Source::with_path(&origin.name, &text, &origin.path).ok()?)
            .ok()?;
    }
    let unit = Unit::from_parts(entry.logic, entry.debug).ok()?;
    Some(Hit { unit, sources })
}

/// Write `unit` back for the next run. A failure is not worth a line in the
/// log: the next run compiles, which is what it would have done anyway.
pub(crate) fn store(host: &RuneHost, key: &str, source: &str, unit: &Unit, sources: &Sources) {
    let (Some(path), Some(origins)) = (file_of(host, key), origins_of(host, source, sources))
    else {
        return;
    };

    let mut bytes = Vec::from(*MAGIC);
    bytes.extend_from_slice(&FORMAT.to_le_bytes());
    let entry = (stamp(host), &origins, unit.logic(), unit.debug_info());
    if bincode::serde::encode_into_std_write(&entry, &mut bytes, CONFIG).is_err() {
        return;
    }
    let fs = balaur_core::files::backend(&host.engine);
    if let Some(dir) = path.parent()
        && fs.mkdir(dir).is_ok()
    {
        let _ = fs.write(&path, &bytes);
    }
}

/// What the unit was compiled from, in `SourceId` order. `None` when a source
/// has no path behind it, since nothing could check that one for changes.
fn origins_of(host: &RuneHost, source: &str, sources: &Sources) -> Option<Vec<Origin>> {
    let mut origins = Vec::new();
    for index in 0.. {
        let Some(found) = sources.get(rune::SourceId::new(index)) else {
            break;
        };
        let path = found.path()?;
        let text = text_at(host, origins.len(), path, source)?;
        origins.push(Origin {
            name: found.name().to_string(),
            path: path.to_path_buf(),
            hash: hash_of(&text),
        });
    }
    Some(origins)
}

/// The text an id stands for: the root as the compiler saw it, or the file on
/// disk for a `mod` the compiler loaded beside it.
fn text_at(host: &RuneHost, id: usize, path: &Path, source: &str) -> Option<String> {
    if id == 0 {
        return Some(with_constants(source).into_owned());
    }
    let bytes = balaur_core::files::backend(&host.engine).read(path).ok()?;
    String::from_utf8(bytes).ok()
}

/// Where `key`'s unit is kept: beside the project's own data, one file per
/// script, so a rewrite replaces it rather than adding to a pile.
fn file_of(host: &RuneHost, key: &str) -> Option<PathBuf> {
    let root = {
        let state = host.state.borrow();
        if state.pack.is_some() {
            return None;
        }
        state.project_root.clone()
    };
    let mut hasher = Hasher::new();
    hasher.write_str(&root.to_string_lossy());
    hasher.write_str(key);
    let dir = balaur_core::engine_api::user_data_dir_of(&host.engine).join("units");
    Some(dir.join(format!("{}.unit", hasher.finish())))
}

/// What a cached unit is only valid against: the engine that compiled it, and
/// the addons it compiled with.
fn stamp(host: &RuneHost) -> u64 {
    let mut hasher = Hasher::new();
    hasher.write_u64(u64::from(crate::packed::FORMAT));
    if let Ok(exe) = std::env::current_exe()
        && let Ok(meta) = std::fs::metadata(&exe)
    {
        hasher.write_str(&exe.to_string_lossy());
        hasher.write_u64(meta.len());
        if let Ok(modified) = meta.modified()
            && let Ok(since) = modified.duration_since(std::time::UNIX_EPOCH)
        {
            hasher.write_u64(u64::try_from(since.as_nanos()).unwrap_or(u64::MAX));
        }
    }
    crate::mounts::fingerprint(&host.state.borrow().mounts, &mut hasher);
    hasher.finish().0
}

fn hash_of(text: &str) -> u64 {
    let mut hasher = Hasher::new();
    hasher.write_str(text);
    hasher.finish().0
}
