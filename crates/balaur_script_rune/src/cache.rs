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
use rune::item::ComponentRef;
use rune::runtime::debug::{DebugArgs, DebugSignature};
use rune::runtime::{DebugInfo, DebugInst, Logic, Unit};
use rune::{Hash, ItemBuf, Source, Sources};
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
    debug: Option<Spans>,
}

/// Rune's [`DebugInfo`], in a shape a length-prefixed format can write.
///
/// Its maps and the `ItemBuf` inside a signature serialise as sequences of
/// unknown length, which bincode refuses, so each is written as a list and the
/// whole is rebuilt on the way back in.
#[derive(Deserialize)]
struct Spans {
    instructions: Vec<(usize, DebugInst)>,
    functions: Vec<(Hash, Signature)>,
    functions_rev: Vec<(usize, Hash)>,
    idents: Vec<(Hash, String)>,
}

#[derive(Deserialize)]
struct Signature {
    path: Vec<Part>,
    args: DebugArgs,
}

/// One component of an item path.
#[derive(Serialize, Deserialize)]
enum Part {
    Crate(String),
    Str(String),
    Id(usize),
}

/// What [`Spans`] is written from: the unit's own debug info, borrowed.
#[derive(Serialize)]
struct SpansOf<'a> {
    instructions: Vec<(usize, &'a DebugInst)>,
    functions: Vec<(Hash, SignatureOf<'a>)>,
    functions_rev: Vec<(usize, Hash)>,
    idents: Vec<(Hash, &'a str)>,
}

#[derive(Serialize)]
struct SignatureOf<'a> {
    path: Vec<Part>,
    args: &'a DebugArgs,
}

impl<'a> SpansOf<'a> {
    fn of(debug: &'a DebugInfo) -> Self {
        Self {
            instructions: debug.instructions.iter().map(|(ip, i)| (*ip, i)).collect(),
            functions: debug
                .functions
                .iter()
                .map(|(hash, sig)| {
                    (
                        *hash,
                        SignatureOf {
                            path: parts_of(&sig.path),
                            args: &sig.args,
                        },
                    )
                })
                .collect(),
            functions_rev: debug
                .functions_rev
                .iter()
                .map(|(ip, h)| (*ip, *h))
                .collect(),
            idents: debug
                .hash_to_ident
                .iter()
                .map(|(hash, name)| (*hash, &**name))
                .collect(),
        }
    }
}

fn parts_of(item: &ItemBuf) -> Vec<Part> {
    item.iter()
        .map(|component| match component {
            ComponentRef::Crate(name) => Part::Crate(name.to_string()),
            ComponentRef::Str(name) => Part::Str(name.to_string()),
            ComponentRef::Id(id) => Part::Id(id),
        })
        .collect()
}

fn item_of(parts: &[Part]) -> Option<ItemBuf> {
    let mut item = ItemBuf::new();
    for part in parts {
        let component = match part {
            Part::Crate(name) => ComponentRef::Crate(name),
            Part::Str(name) => ComponentRef::Str(name),
            Part::Id(id) => ComponentRef::Id(*id),
        };
        item.push(component).ok()?;
    }
    Some(item)
}

/// Rebuild rune's own debug info from what was written.
fn debug_of(read: Spans) -> Option<DebugInfo> {
    let mut debug = DebugInfo::default();
    for (ip, instruction) in read.instructions {
        debug.instructions.try_insert(ip, instruction).ok()?;
    }
    for (hash, signature) in read.functions {
        let path = item_of(&signature.path)?;
        debug
            .functions
            .try_insert(hash, DebugSignature::new(path, signature.args))
            .ok()?;
    }
    for (ip, hash) in read.functions_rev {
        debug.functions_rev.try_insert(ip, hash).ok()?;
    }
    for (hash, ident) in read.idents {
        let ident = rune::alloc::Box::try_from(ident.as_str()).ok()?;
        debug.hash_to_ident.try_insert(hash, ident).ok()?;
    }
    Some(debug)
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
    if entry.stamp != stamp(host)? {
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
    let debug = match entry.debug {
        Some(spans) => Some(debug_of(spans)?),
        None => None,
    };
    let unit = Unit::from_parts(entry.logic, debug).ok()?;
    Some(Hit { unit, sources })
}

/// Write `unit` back for the next run. A failure is not worth a line in the
/// log: the next run compiles, which is what it would have done anyway.
pub(crate) fn store(host: &RuneHost, key: &str, source: &str, unit: &Unit, sources: &Sources) {
    let (Some(path), Some(origins)) = (file_of(host, key), origins_of(host, source, sources))
    else {
        return;
    };

    let Some(stamp) = stamp(host) else {
        return;
    };
    let mut bytes = Vec::from(*MAGIC);
    bytes.extend_from_slice(&FORMAT.to_le_bytes());
    let debug = unit.debug_info().map(SpansOf::of);
    let entry = &(stamp, &origins, unit.logic(), &debug);
    if bincode::serde::encode_into_std_write(entry, &mut bytes, CONFIG).is_err() {
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
///
/// A pack is cached too. One that ships compiled units never reaches here,
/// and one that ships source is compiled on every boot like a dev run: that
/// is what a browser opens, where the compile is slowest.
fn file_of(host: &RuneHost, key: &str) -> Option<PathBuf> {
    let root = host.state.borrow().project_root.clone();
    let mut hasher = Hasher::new();
    hasher.write_str(&root.to_string_lossy());
    hasher.write_str(key);
    let dir = balaur_core::engine_api::user_data_dir_of(&host.engine).join("units");
    Some(dir.join(format!("{}.unit", hasher.finish())))
}

/// What a cached unit is only valid against: the engine that compiled it, and
/// the addons it compiled with.
///
/// `None` where the engine cannot be told apart from another build of itself:
/// nothing is cached then, rather than risking a unit an older engine wrote.
///
/// A native build is told by its own file, which every rebuild rewrites. A
/// browser has no such file and takes the id a packaged build carries, so a
/// released web build caches and one built straight from source does not.
fn stamp(host: &RuneHost) -> Option<u64> {
    let mut hasher = Hasher::new();
    hasher.write_u64(u64::from(crate::packed::FORMAT));
    match std::env::current_exe().ok() {
        Some(exe) => {
            let built = balaur_core::files::backend(&host.engine).mtime(&exe)?;
            hasher.write_str(&exe.to_string_lossy());
            hasher.write_f64(built);
        }
        None => hasher.write_str(option_env!("BALAUR_BUILD")?),
    }
    crate::mounts::fingerprint(&host.state.borrow().mounts, &mut hasher);
    Some(hasher.finish().0)
}

fn hash_of(text: &str) -> u64 {
    let mut hasher = Hasher::new();
    hasher.write_str(text);
    hasher.finish().0
}
