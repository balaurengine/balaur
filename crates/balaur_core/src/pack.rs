//! Exported game packs: the whole project (manifest, scenes, scripts and
//! binary assets) in one blob, with every script precompiled by its backend.
//!
//! A pack is what `balaur export` produces and what a shipped game runs
//! from, either as a standalone file (`balaur play game.bpak`) or embedded
//! straight into a release binary with `include_bytes!`. Running from a pack
//! needs no compiler, no source files, and no file watcher.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::Path;

use anyhow::{Context, Result, anyhow};

const MAGIC: &[u8; 5] = b"BPAK\x02";

/// File extensions that ship inside a pack. A game's textures, sounds and
/// fonts have to travel with it; source art and notes do not.
pub const ASSET_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "bmp", "tga", "ogg", "wav", "mp3", "flac", "ttf", "otf", "fnt",
    "glb", "gltf", "bin", "obj",
];

/// How many of the heaviest entries a report names: enough to see where the
/// bytes went, few enough to read at a glance.
const LARGEST_ENTRIES: usize = 10;

/// Directories the engine reads by listing rather than by reference, so
/// nothing in a scene names their contents: `balaur_ui` loads every face under
/// `fonts/` when the UI starts. Stripping one of these would take a project's
/// text away with it.
pub const LOADED_WHOLE: &[&str] = &["fonts/"];

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
        Self::build_with(project_root, compiler, false)
    }

    /// [`build`](Self::build), with a say over what a script entry holds.
    ///
    /// With `keep_sources`, every script is still compiled — an export is a
    /// check — but the pack stores its source text, and the runtime compiles
    /// it again when it loads. That is what a runtime with a different
    /// pointer width than this machine needs: the compiled form writes
    /// `usize` sentinels that do not read back on 32-bit targets, the web
    /// build first among them, until the bytecode format is portable.
    pub fn build_with(
        project_root: &Path,
        compiler: &dyn balaur_script::ScriptCompiler,
        keep_sources: bool,
    ) -> Result<Self> {
        // Through the file backend, not `std::fs`: on a desktop that is the
        // disk, and in a browser it is the project the editor is editing, so
        // `balaur export` to a pack is the one export a tab can finish.
        let fs = crate::files::default_backend();
        let manifest = text(&*fs, &project_root.join("project.toml"))
            .with_context(|| format!("no project.toml in {}", project_root.display()))?;
        let mut pack = Self {
            manifest,
            ..Default::default()
        };
        let mut files = Vec::new();
        collect_files(&*fs, project_root, project_root, &mut files);
        for rel in files {
            let path = project_root.join(&rel);
            match Path::new(&rel).extension().and_then(|e| e.to_str()) {
                Some(ext) if compiler.extensions().contains(&ext) => {
                    let source = text(&*fs, &path)?;
                    let bytes = compiler.compile(&rel, &source)?;
                    pack.scripts.insert(
                        rel,
                        if keep_sources {
                            source.into_bytes()
                        } else {
                            bytes
                        },
                    );
                }
                // `scenes` is the pack's text map: scene documents, asset
                // documents and shader sources all read back through it.
                Some("toml") if rel != "project.toml" => {
                    pack.scenes.insert(rel, text(&*fs, &path)?);
                }
                Some("wesl") => {
                    pack.scenes.insert(rel, text(&*fs, &path)?);
                }
                Some(ext) if ASSET_EXTENSIONS.contains(&ext) => {
                    pack.assets.insert(rel, fs.read(&path)?);
                }
                _ => {}
            }
        }
        Ok(pack)
    }

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

    /// What the pack weighs, section by section, and what nothing in it names.
    #[must_use]
    pub fn report(&self) -> PackReport {
        self.report_with(&[])
    }

    /// [`report`](Self::report), told the `keep` globs an export protects
    /// files with, so the report names what [`strip`](Self::strip) would drop.
    #[must_use]
    pub fn report_with(&self, keep: &[String]) -> PackReport {
        // Every entry is a length-prefixed key and value, every section opens
        // with its count, and an asset carries its hash between the two.
        const LEN: usize = 4;
        const HASH: usize = 64;

        let text_bytes = |entries: &mut dyn Iterator<Item = (usize, usize)>| -> usize {
            LEN + entries.map(|(k, v)| LEN + k + LEN + v).sum::<usize>()
        };
        let sections = vec![
            SectionReport {
                name: "manifest",
                entries: 1,
                bytes: MAGIC.len() + LEN + self.manifest.len(),
            },
            SectionReport {
                name: "scenes",
                entries: self.scenes.len(),
                bytes: text_bytes(&mut self.scenes.iter().map(|(k, v)| (k.len(), v.len()))),
            },
            SectionReport {
                name: "scripts",
                entries: self.scripts.len(),
                bytes: text_bytes(&mut self.scripts.iter().map(|(k, v)| (k.len(), v.len()))),
            },
            SectionReport {
                name: "assets",
                entries: self.assets.len(),
                bytes: LEN
                    + self
                        .assets
                        .iter()
                        .map(|(k, v)| LEN + k.len() + LEN + HASH + LEN + v.len())
                        .sum::<usize>(),
            },
        ];

        let mut by_extension: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for (key, value) in &self.assets {
            let slot = by_extension.entry(extension_of(key)).or_default();
            slot.0 += 1;
            slot.1 += value.len();
        }
        let mut extensions: Vec<ExtensionReport> = by_extension
            .into_iter()
            .map(|(extension, (entries, bytes))| ExtensionReport {
                extension,
                entries,
                bytes,
            })
            .collect();
        extensions.sort_by(|a, b| {
            b.bytes
                .cmp(&a.bytes)
                .then_with(|| a.extension.cmp(&b.extension))
        });

        let mut largest: Vec<EntryReport> = self
            .assets
            .iter()
            .map(|(key, value)| EntryReport {
                key: key.clone(),
                bytes: value.len(),
            })
            .collect();
        largest.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.key.cmp(&b.key)));
        largest.truncate(LARGEST_ENTRIES);

        let unreferenced = self.unreferenced(keep);
        let unreferenced_bytes = unreferenced
            .iter()
            .filter_map(|key| self.assets.get(key))
            .map(Vec::len)
            .sum();
        PackReport {
            total: sections.iter().map(|s| s.bytes).sum(),
            sections,
            extensions,
            largest,
            unreferenced,
            unreferenced_bytes,
        }
    }

    /// Asset keys nothing in the pack names, sorted.
    ///
    /// A reference is any string in a scene document, or any literal in a
    /// script that is text, spelling the key, `key#entry`, a directory above
    /// it, or an `id://` the project's index resolves to it. `keep` holds
    /// globs — `*` inside a path segment, `**` across them — for the paths a
    /// script computes instead of writing.
    ///
    /// A file under [`LOADED_WHOLE`] is never reported: the engine finds those
    /// by listing the directory, so no document names one.
    #[must_use]
    pub fn unreferenced(&self, keep: &[String]) -> Vec<String> {
        let references = self.references();
        self.assets
            .keys()
            .filter(|key| {
                !is_referenced(key, &references)
                    && !LOADED_WHOLE.iter().any(|dir| key.starts_with(dir))
                    && !keep.iter().any(|pattern| glob_matches(pattern, key))
            })
            .cloned()
            .collect()
    }

    /// Drop every asset [`unreferenced`](Self::unreferenced) names, answering
    /// the keys removed.
    pub fn strip(&mut self, keep: &[String]) -> Vec<String> {
        let removed = self.unreferenced(keep);
        for key in &removed {
            self.assets.remove(key);
        }
        removed
    }

    /// Every path the pack's own files name, with any `#entry` cut off.
    fn references(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for (key, text) in &self.scenes {
            // The id index maps every id to its path, so its values would name
            // every indexed asset; it answers `id://` below and nothing else.
            if key == crate::assets::INDEX_PATH {
                continue;
            }
            if let Ok(value) = toml::from_str::<toml::Value>(text) {
                collect_toml_strings(&value, &mut out);
            }
        }
        for bytes in self.scripts.values() {
            // A script may be bytecode; only text can hold a literal to read.
            if let Ok(text) = std::str::from_utf8(bytes) {
                collect_string_literals(text, &mut out);
            }
        }
        if let Some(index) = self
            .scenes
            .get(crate::assets::INDEX_PATH)
            .and_then(|text| crate::asset_index::parse(text).ok())
        {
            let resolved: Vec<String> = out
                .iter()
                .filter_map(|reference| reference.strip_prefix("id://"))
                .filter_map(|id| index.get(id).cloned())
                .collect();
            out.extend(resolved);
        }
        for (key, bytes) in &self.assets {
            if extension_of(key) == "fnt"
                && let Ok(text) = std::str::from_utf8(bytes)
            {
                out.extend(fnt_pages(key, text));
            }
        }
        out
    }
}

/// One pack section's share of the encoded bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionReport {
    /// `manifest`, `scenes`, `scripts` or `assets`.
    pub name: &'static str,
    /// How many entries the section holds; the manifest is always one.
    pub entries: usize,
    /// What the section occupies in [`Pack::encode`], its keys, length
    /// prefixes and asset hashes counted. The manifest row carries the pack's
    /// five magic bytes too, so the four rows sum to [`PackReport::total`].
    pub bytes: usize,
}

/// What one file extension weighs across the asset section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionReport {
    /// Lowercased and without the dot; empty for an asset that has none.
    pub extension: String,
    pub entries: usize,
    /// The files' own bytes, without the pack's framing around them.
    pub bytes: usize,
}

/// One asset, for the heaviest-first list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryReport {
    pub key: String,
    /// The file's own bytes, without the pack's framing around them.
    pub bytes: usize,
}

/// What a pack weighs and what nothing in it names.
///
/// Counts are exact bytes; [`Display`](std::fmt::Display) is the only place
/// they become KB and MB.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackReport {
    /// The encoded pack's length: what an export writes.
    pub total: usize,
    /// Manifest, scenes, scripts and assets, in that order.
    pub sections: Vec<SectionReport>,
    /// Extensions across the asset section, heaviest first.
    pub extensions: Vec<ExtensionReport>,
    /// The heaviest assets, ten of them at most.
    pub largest: Vec<EntryReport>,
    /// What [`Pack::strip`] would drop, sorted.
    pub unreferenced: Vec<String>,
    /// What those files weigh together.
    pub unreferenced_bytes: usize,
}

impl std::fmt::Display for PackReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let width = self
            .sections
            .iter()
            .map(|s| s.name.len())
            .chain(self.extensions.iter().map(|e| e.extension.len()))
            .chain(self.largest.iter().map(|e| e.key.len()))
            .max()
            .unwrap_or(0)
            .max("extension".len());
        // The two right-hand columns, and their width joined for the one-column
        // list of entries below them.
        let (count, size) = (8, 10);
        let wide = count + 1 + size;

        writeln!(f, "pack {}", human_bytes(self.total))?;
        writeln!(
            f,
            "{:<width$} {:>count$} {:>size$}",
            "section", "entries", "bytes"
        )?;
        for section in &self.sections {
            let bytes = human_bytes(section.bytes);
            writeln!(
                f,
                "{:<width$} {:>count$} {bytes:>size$}",
                section.name, section.entries
            )?;
        }
        if !self.extensions.is_empty() {
            writeln!(f)?;
            writeln!(
                f,
                "{:<width$} {:>count$} {:>size$}",
                "extension", "entries", "bytes"
            )?;
            for entry in &self.extensions {
                let name = if entry.extension.is_empty() {
                    "(none)"
                } else {
                    entry.extension.as_str()
                };
                let bytes = human_bytes(entry.bytes);
                writeln!(f, "{name:<width$} {:>count$} {bytes:>size$}", entry.entries)?;
            }
        }
        if !self.largest.is_empty() {
            writeln!(f)?;
            writeln!(f, "{:<width$} {:>wide$}", "largest", "bytes")?;
            for entry in &self.largest {
                let bytes = human_bytes(entry.bytes);
                writeln!(f, "{:<width$} {bytes:>wide$}", entry.key)?;
            }
        }
        writeln!(f)?;
        let files = if self.unreferenced.len() == 1 {
            "file"
        } else {
            "files"
        };
        write!(
            f,
            "{} {files} nothing references, {}",
            self.unreferenced.len(),
            human_bytes(self.unreferenced_bytes)
        )
    }
}

/// Bytes as a person reads them, at one decimal place.
fn human_bytes(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    let value = bytes as f64;
    if value < KB {
        format!("{bytes} B")
    } else if value < KB * KB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{:.1} MB", value / (KB * KB))
    }
}

/// A key's extension, lowercased and without the dot.
fn extension_of(key: &str) -> String {
    Path::new(key)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Whether anything in `references` names `key`: the path itself, or a
/// directory holding it. `#entry` was cut off when the reference was gathered.
fn is_referenced(key: &str, references: &BTreeSet<String>) -> bool {
    references.contains(key)
        || key
            .match_indices('/')
            .any(|(at, _)| references.contains(&key[..at]))
}

/// Keep one reference, without whatever `#entry` follows it.
fn insert_reference(out: &mut BTreeSet<String>, value: &str) {
    let path = value.split_once('#').map_or(value, |(head, _)| head);
    let path = path.trim_end_matches('/');
    if !path.is_empty() {
        out.insert(path.to_string());
    }
}

/// Every string value in a parsed document, however deep. Keys are names, not
/// references, so only values are read.
fn collect_toml_strings(value: &toml::Value, out: &mut BTreeSet<String>) {
    match value {
        toml::Value::String(text) => insert_reference(out, text),
        toml::Value::Array(items) => {
            for item in items {
                collect_toml_strings(item, out);
            }
        }
        toml::Value::Table(table) => {
            for item in table.values() {
                collect_toml_strings(item, out);
            }
        }
        _ => {}
    }
}

/// Every double-quoted literal in a script's source. Textual on purpose: a
/// path in a script is a value it computes, and no type says which.
fn collect_string_literals(source: &str, out: &mut BTreeSet<String>) {
    let mut chars = source.chars();
    while let Some(opening) = chars.next() {
        if opening != '"' {
            continue;
        }
        let mut literal = String::new();
        loop {
            match chars.next() {
                Some('\\') => {
                    chars.next();
                }
                Some('"') | None => break,
                Some(character) => literal.push(character),
            }
        }
        insert_reference(out, &literal);
    }
}

/// The page images an AngelCode descriptor names, project-relative: the page
/// sits beside the descriptor, as the tool that wrote it left it.
fn fnt_pages(key: &str, source: &str) -> Vec<String> {
    let directory = key.rsplit_once('/').map_or("", |(head, _)| head);
    let mut out = Vec::new();
    for line in source.lines() {
        for field in ["file=", "page="] {
            for value in descriptor_values(line, field) {
                out.push(if directory.is_empty() {
                    value
                } else {
                    format!("{directory}/{value}")
                });
            }
        }
    }
    out
}

/// What one `name=` field carries on a descriptor line, quoted or bare.
fn descriptor_values(line: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find(name) {
        rest = &rest[at + name.len()..];
        let value = if let Some(tail) = rest.strip_prefix('"') {
            &tail[..tail.find('"').unwrap_or(tail.len())]
        } else {
            &rest[..rest.find(char::is_whitespace).unwrap_or(rest.len())]
        };
        if !value.is_empty() {
            out.push(value.to_string());
        }
    }
    out
}

/// Whether `pattern` matches `text`, with `*` inside a path segment and `**`
/// across them. Byte-wise: a wildcard spans whatever it spans, and everything
/// else is compared literally.
pub fn glob_matches(pattern: &str, text: &str) -> bool {
    fn matches(pattern: &[u8], text: &[u8]) -> bool {
        match pattern.first() {
            None => text.is_empty(),
            Some(b'*') if pattern.get(1) == Some(&b'*') => {
                let rest = &pattern[2..];
                // `**/` stands for no directory at all as well as for many.
                if rest.first() == Some(&b'/') && matches(&rest[1..], text) {
                    return true;
                }
                (0..=text.len()).any(|at| matches(rest, &text[at..]))
            }
            Some(b'*') => {
                let segment = text.iter().position(|&b| b == b'/').unwrap_or(text.len());
                (0..=segment).any(|at| matches(&pattern[1..], &text[at..]))
            }
            Some(&byte) => text.first() == Some(&byte) && matches(&pattern[1..], &text[1..]),
        }
    }
    matches(pattern.as_bytes(), text.as_bytes())
}

/// One file's text, wherever the backend keeps it.
fn text(fs: &dyn crate::files::FileBackend, path: &Path) -> Result<String> {
    String::from_utf8(fs.read(path)?).with_context(|| format!("'{}' is not text", path.display()))
}

/// Every file under `dir`, project-relative. A directory the backend cannot
/// read contributes nothing: the manifest was read first, so a root that is
/// not there has already failed.
fn collect_files(
    fs: &dyn crate::files::FileBackend,
    root: &Path,
    dir: &Path,
    out: &mut Vec<String>,
) {
    for (name, is_dir) in fs.list(dir) {
        if name.starts_with('.') {
            continue;
        }
        let path = dir.join(&name);
        if is_dir {
            collect_files(fs, root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
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
