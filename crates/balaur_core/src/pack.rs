//! Exported game packs: the whole project (manifest, scenes, scripts) in one
//! binary blob, with every script precompiled by its backend.
//!
//! A pack is what `balaur export` produces and what a shipped game runs
//! from, either as a standalone file (`balaur play game.bpak`) or embedded
//! straight into a release binary with `include_bytes!`. Running from a pack
//! needs no compiler, no source files, and no file watcher.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Context, Result};

const MAGIC: &[u8; 5] = b"BPAK\x01";

#[derive(Default, Clone, Debug)]
pub struct Pack {
    /// `project.toml` source.
    pub manifest: String,
    /// Scene sources keyed by project-relative path.
    pub scenes: HashMap<String, String>,
    /// Compiled script bytes keyed by project-relative path. The format is
    /// the backend's business; the pack only stores and ships them.
    pub scripts: HashMap<String, Vec<u8>>,
}

impl Pack {
    /// Compile every script in `project_root` and gather scenes into a pack.
    ///
    /// `scripts` decides which files count as scripts and how they compile.
    pub fn build(project_root: &Path, scripts: &dyn balaur_script::ScriptCompiler) -> Result<Self> {
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
                Some(ext) if scripts.extensions().contains(&ext) => {
                    let source = std::fs::read_to_string(&path)?;
                    let compiled = scripts.compile(&rel, &source)?;
                    pack.scripts.insert(rel, compiled);
                }
                Some("toml") if rel != "project.toml" => {
                    pack.scenes.insert(rel, std::fs::read_to_string(&path)?);
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
