//! Exported game packs: the whole project (manifest, scenes, scripts and
//! binary assets) in one blob, with every script precompiled by its backend.
//!
//! A pack is what `balaur export` produces and what a shipped game runs
//! from, either as a standalone file (`balaur play game.bpak`) or embedded
//! straight into a release binary with `include_bytes!`. Running from a pack
//! needs no compiler, no source files, and no file watcher.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

use anyhow::{anyhow, Context, Result};

const MAGIC: &[u8; 5] = b"BPAK\x02";

/// File extensions that ship inside a pack. A game's textures, sounds and
/// fonts have to travel with it; source art and notes do not.
pub const ASSET_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "bmp", "tga", "ogg", "wav", "mp3", "flac", "ttf", "otf", "glb",
    "gltf", "bin", "obj",
];

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
}

impl Pack {
    /// Compile every script in `project_root` and gather scenes into a pack.
    ///
    /// `compiler` decides which files count as scripts and how they compile.
    pub fn build(
        project_root: &Path,
        compiler: &dyn balaur_script::ScriptCompiler,
    ) -> Result<Self> {
        let manifest = std::fs::read_to_string(project_root.join("project.toml"))
            .with_context(|| format!("no project.toml in {}", project_root.display()))?;
        let mut pack = Self {
            manifest,
            ..Default::default()
        };
        let mut files = Vec::new();
        collect_files(project_root, project_root, &mut files)?;
        for rel in files {
            let path = project_root.join(&rel);
            match Path::new(&rel).extension().and_then(|e| e.to_str()) {
                Some(ext) if compiler.extensions().contains(&ext) => {
                    let source = std::fs::read_to_string(&path)?;
                    let bytes = compiler.compile(&rel, &source)?;
                    pack.scripts.insert(rel, bytes);
                }
                Some("toml") if rel != "project.toml" => {
                    pack.scenes.insert(rel, std::fs::read_to_string(&path)?);
                }
                Some(ext) if ASSET_EXTENSIONS.contains(&ext) => {
                    pack.assets.insert(rel, std::fs::read(&path)?);
                }
                _ => {}
            }
        }
        Ok(pack)
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
        Ok(pack)
    }
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
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
