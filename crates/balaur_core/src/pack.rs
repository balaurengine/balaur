//! Exported game packs: the whole project (manifest, scenes, scripts and
//! binary assets) in one blob, with every script precompiled by its backend.
//!
//! A pack is what `balaur export` produces and what a shipped game runs
//! from, either as a standalone file (`balaur play game.bpak`) or embedded
//! straight into a release binary with `include_bytes!`. Running from a pack
//! needs no compiler, no source files, and no file watcher.

use std::collections::BTreeMap;
use std::fmt::Write;

use anyhow::{Result, anyhow};

/// What every pack opens with: `BPAK` and the format's version.
pub const MAGIC: &[u8; 5] = b"BPAK\x02";

/// A content hash, so a decoded pack can prove an entry arrived intact and a
/// materialised file can be cached under a name that changes with its bytes.
#[must_use]
pub fn content_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    // Folded rather than mapped into Strings: one allocation, not one per byte.
    hasher.finalize().iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

#[derive(Default, Clone, Debug)]
pub struct Pack {
    /// `project.toml` source.
    pub manifest: String,
    /// Scene sources keyed by project-relative path.
    ///
    /// Ordered, not hashed: `encode` walks these maps, and a pack has to come
    /// out byte-identical on every machine that builds it.
    pub scenes: BTreeMap<String, String>,
    /// Compiled script bytes keyed by project-relative path. The format is
    /// the backend's business; the pack only stores and ships them.
    pub scripts: BTreeMap<String, Vec<u8>>,
    /// Textures, audio, fonts and models keyed by project-relative path.
    /// Without these a shipped game is only a single file when it is silent
    /// and untextured.
    pub assets: BTreeMap<String, Vec<u8>>,
    /// The engine functions the scripts call, one line each with the types
    /// they were checked against, sorted (`ScriptCompiler::api`): the
    /// runtime that loads the pack says which it lacks or types otherwise,
    /// before a script fails on one. Empty when the compiler keeps none.
    pub api: Vec<String>,
}

impl Pack {
    /// The pack as the project tree it was built from: `project.toml`, then
    /// every scene, script and asset under the path it was packed at.
    ///
    /// What a virtual filesystem is seeded with, so a browser can open a
    /// project it fetched as one file.
    #[must_use]
    pub fn entries(&self) -> Vec<(String, Vec<u8>)> {
        let mut out = vec![(
            "project.toml".to_string(),
            self.manifest.clone().into_bytes(),
        )];
        out.extend(
            self.scenes
                .iter()
                .map(|(k, v)| (k.clone(), v.clone().into_bytes())),
        );
        out.extend(self.scripts.iter().map(|(k, v)| (k.clone(), v.clone())));
        out.extend(self.assets.iter().map(|(k, v)| (k.clone(), v.clone())));
        out
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        write_bytes(&mut out, self.manifest.as_bytes());
        write_u32(&mut out, self.scenes.len() as u32);
        for (k, v) in &self.scenes {
            write_bytes(&mut out, k.as_bytes());
            write_bytes(&mut out, v.as_bytes());
        }
        write_u32(&mut out, self.scripts.len() as u32);
        for (k, v) in &self.scripts {
            write_bytes(&mut out, k.as_bytes());
            write_bytes(&mut out, v);
        }
        // Each asset carries its hash, so decode can tell a truncated or
        // altered entry from a good one rather than handing on bad bytes.
        write_u32(&mut out, self.assets.len() as u32);
        for (k, v) in &self.assets {
            write_bytes(&mut out, k.as_bytes());
            write_bytes(&mut out, content_hash(v).as_bytes());
            write_bytes(&mut out, v);
        }
        write_u32(&mut out, self.api.len() as u32);
        for line in &self.api {
            write_bytes(&mut out, line.as_bytes());
        }
        out
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut cursor = data;
        let magic = take(&mut cursor, MAGIC.len())?;
        if magic != MAGIC {
            return Err(anyhow!("not a balaur pack (bad magic)"));
        }
        let manifest = String::from_utf8(read_bytes(&mut cursor)?.to_vec())?;
        let mut pack = Self {
            manifest,
            ..Default::default()
        };
        for _ in 0..read_u32(&mut cursor)? {
            let k = String::from_utf8(read_bytes(&mut cursor)?.to_vec())?;
            let v = String::from_utf8(read_bytes(&mut cursor)?.to_vec())?;
            pack.scenes.insert(k, v);
        }
        for _ in 0..read_u32(&mut cursor)? {
            let k = String::from_utf8(read_bytes(&mut cursor)?.to_vec())?;
            let v = read_bytes(&mut cursor)?.to_vec();
            pack.scripts.insert(k, v);
        }
        for _ in 0..read_u32(&mut cursor)? {
            let k = String::from_utf8(read_bytes(&mut cursor)?.to_vec())?;
            let want = String::from_utf8(read_bytes(&mut cursor)?.to_vec())?;
            let v = read_bytes(&mut cursor)?.to_vec();
            let got = content_hash(&v);
            if got != want {
                return Err(anyhow!(
                    "pack asset '{k}' is corrupt: expected {want}, got {got}"
                ));
            }
            pack.assets.insert(k, v);
        }
        // A pack from before the section ends here.
        if !cursor.is_empty() {
            for _ in 0..read_u32(&mut cursor)? {
                let line = String::from_utf8(read_bytes(&mut cursor)?.to_vec())?;
                pack.api.push(line);
            }
        }
        Ok(pack)
    }
}

fn write_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn write_bytes(out: &mut Vec<u8>, data: &[u8]) {
    write_u32(out, data.len() as u32);
    out.extend_from_slice(data);
}

fn take<'a>(cursor: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
    if cursor.len() < n {
        return Err(anyhow!("truncated pack"));
    }
    let (head, tail) = cursor.split_at(n);
    *cursor = tail;
    Ok(head)
}

fn read_u32(cursor: &mut &[u8]) -> Result<u32> {
    let bytes = take(cursor, 4)?;
    // take() either returned exactly 4 bytes or already returned Err.
    Ok(u32::from_le_bytes(
        bytes.try_into().expect("take(4) yields 4 bytes"),
    ))
}

fn read_bytes<'a>(cursor: &mut &'a [u8]) -> Result<&'a [u8]> {
    let len = read_u32(cursor)? as usize;
    take(cursor, len)
}
