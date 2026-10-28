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

use anyhow::{Context, Result, anyhow, bail};

const MAGIC: &[u8; 5] = b"BPAK\x02";

/// File extensions that ship inside a pack. A game's textures, sounds, fonts
/// and the data it reads have to travel with it; source art and notes do not.
pub const ASSET_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "svg", "bmp", "tga", "ogg", "wav", "mp3", "flac", "ttf", "otf",
    "fnt", "glb", "gltf", "bin", "obj", "json", "csv",
];

/// How many of the heaviest entries a report names: enough to see where the
/// bytes went, few enough to read at a glance.
const LARGEST_ENTRIES: usize = 10;

/// Directories the engine reads without a document naming their contents:
/// `balaur_ui` loads every face under `fonts/` when the UI starts, and a build
/// with no shader linker reads the shaders an export linked under `.balaur/`.
/// Stripping one of these would take a project's text or shaders with it.
pub const LOADED_WHOLE: &[&str] = &["fonts/", ".balaur/"];

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
    /// Compile every script in `project_root` and gather scenes into a pack.
    ///
    /// `compiler` decides which files count as scripts and how they compile.
    /// A script that imports a file the pack leaves out fails the build,
    /// naming the `[application] ignore` pattern that dropped it: the game
    /// would fail on it when it runs.
    ///
    /// Each of `[dependencies]` ships under `deps/<alias>/`, less what its
    /// own `ignore` names, with its documents rewritten to the game's
    /// spelling (a packed run never asks which project owns a document) and
    /// its assets that nothing names dropped, whatever `[export] strip` says:
    /// a game pays only for what it uses of a large dependency. Its own
    /// `[export] include` globs keep what its scripts build.
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
        let ignored = crate::ignore::from_manifest(&pack.manifest);
        let (fs, files, shipped) = gathered(fs, project_root, &pack.manifest, &ignored)?;
        let nested = nested_roots(&files);
        // Every script is compiled before the build fails, so one export
        // names every file that does not compile rather than the first.
        let mut failed: Vec<String> = Vec::new();
        // Each script and the files it links in, checked against what ships
        // once every script is in.
        let mut used: Vec<(String, Vec<String>)> = Vec::new();
        // What every script's source names, read before it becomes bytecode
        // no literal can be read back out of: what the strip under `deps/`
        // keeps.
        let mut literals = BTreeSet::new();
        for rel in files {
            let path = project_root.join(&rel);
            match Path::new(&rel).extension().and_then(|e| e.to_str()) {
                Some(ext) if compiler.extensions().contains(&ext) => {
                    let source = text(&*fs, &path)?;
                    // A nested project's scripts are its own: they resolve
                    // at *its* root, not here. Shipped as source, to copy.
                    if in_nested(&nested, &rel) {
                        pack.scripts.insert(rel, source.into_bytes());
                        continue;
                    }
                    if !shipped.is_empty() {
                        literals.extend(script_references(&rel, &source));
                    }
                    let checked = compiler
                        .compile(&rel, &source)
                        .and_then(|bytes| Ok((bytes, compiler.uses(&rel, &source)?)));
                    let bytes = match checked {
                        Ok((bytes, files)) => {
                            used.push((rel.clone(), files));
                            bytes
                        }
                        Err(err) => {
                            failed.push(format!("{rel}: {err:#}"));
                            continue;
                        }
                    };
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
                    let source = text(&*fs, &path)?;
                    let source = match shipped.iter().find(|dep| dep.holds(&rel)) {
                        Some(dep) => dep.game_spelling(&*fs, &source),
                        None => source,
                    };
                    pack.scenes.insert(rel, source);
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
        if !failed.is_empty() {
            bail!(
                "{} script(s) did not compile:\n{}",
                failed.len(),
                failed.join("\n")
            );
        }
        // A script compiled against the project, where an ignored module is
        // still on disk; the runtime finds its imports in the pack alone.
        let dropped: Vec<String> = used
            .iter()
            .flat_map(|(rel, files)| files.iter().map(move |file| (rel, file)))
            .filter(|(_, file)| !pack.scripts.contains_key(file.as_str()))
            .map(|(rel, file)| {
                let why = crate::ignore::matching(&ignored, file).map_or_else(
                    || "the pack leaves it out".to_string(),
                    |pattern| format!("`[application] ignore` leaves it out (\"{pattern}\")"),
                );
                format!("{rel} uses {file}: {why}")
            })
            .collect();
        if !dropped.is_empty() {
            bail!(
                "{} import(s) of a shipped script are left out of the pack:\n{}",
                dropped.len(),
                dropped.join("\n")
            );
        }
        // After every script compiled: the calls they make.
        pack.api = compiler.api();
        if !shipped.is_empty() {
            pack.finish_dependencies(&shipped, &literals);
        }
        Ok(pack)
    }

    /// Drop each dependency's assets nothing names, bake its
    /// `[import.<kind>]` defaults into the sidecars of what is left, and
    /// write `deps/credits.json` for the game's credits screen.
    fn finish_dependencies(&mut self, shipped: &[Shipped], literals: &BTreeSet<String>) {
        let mut keep = crate::dependencies::strings_at(
            &toml::from_str(&self.manifest).unwrap_or_default(),
            "export",
            "include",
        );
        for dep in shipped {
            keep.extend(
                dep.include
                    .iter()
                    .map(|glob| format!("{}/{glob}", dep.folder)),
            );
        }
        let dir = format!("{}/", crate::dependencies::DIR);
        let references = self.references_with(literals.clone());
        let dropped: Vec<String> = self
            .assets
            .keys()
            .filter(|key| {
                key.starts_with(&dir)
                    && !is_referenced(key, &references)
                    && !keep.iter().any(|pattern| glob_matches(pattern, key))
            })
            .cloned()
            .collect();
        for key in dropped {
            self.assets.remove(&key);
        }
        for dep in shipped {
            let prefix = format!("{}/", dep.folder);
            let keys: Vec<String> = self
                .assets
                .keys()
                .filter(|key| key.starts_with(&prefix))
                .cloned()
                .collect();
            for key in keys {
                let Some(defaults) = crate::import::kind_of(&key)
                    .and_then(|kind| dep.import.get(kind)?.as_table().cloned())
                else {
                    continue;
                };
                let sidecar = crate::import::sidecar_of(&key);
                let mut settings = defaults;
                if let Some(own) = self
                    .scenes
                    .get(&sidecar)
                    .and_then(|text| toml::from_str::<toml::Table>(text).ok())
                {
                    settings.extend(own);
                }
                self.scenes
                    .insert(sidecar, toml::to_string(&settings).unwrap_or_default());
            }
        }
        let credits: Vec<serde_json::Value> = shipped
            .iter()
            .map(|dep| {
                serde_json::json!({
                    "alias": dep.alias,
                    "project": dep.project,
                    "rev": dep.rev,
                    "license": dep.license,
                })
            })
            .collect();
        self.assets.insert(
            format!("{}/{CREDITS}", crate::dependencies::DIR),
            serde_json::to_vec_pretty(&credits).unwrap_or_default(),
        );
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
            SectionReport {
                name: "api",
                entries: self.api.len(),
                bytes: LEN + self.api.iter().map(|line| LEN + line.len()).sum::<usize>(),
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
        self.references_with(BTreeSet::new())
    }

    /// [`references`](Self::references), starting from `out`.
    ///
    /// A dependency's document counts only once something already counted
    /// names it: a dependency ships its showroom scenes, and those keep no
    /// asset of it the game does not reach.
    fn references_with(&self, mut out: BTreeSet<String>) -> BTreeSet<String> {
        // `[application] splash` and `icon` name files no scene or script does.
        if let Ok(value) = toml::from_str::<toml::Value>(&self.manifest) {
            collect_toml_strings(&value, &mut out);
        }
        let mut waiting: Vec<(&String, &String)> = Vec::new();
        for (key, text) in &self.scenes {
            // The id index maps every id to its path, so its values would name
            // every indexed asset; it answers `id://` below and nothing else.
            if key == crate::assets::INDEX_PATH {
                continue;
            }
            if crate::dependencies::split(key).is_some() {
                waiting.push((key, text));
            } else if let Ok(value) = toml::from_str::<toml::Value>(text) {
                collect_toml_strings(&value, &mut out);
            }
        }
        for (key, bytes) in &self.scripts {
            // A script may be bytecode; only text can hold a literal to read.
            if let Ok(text) = std::str::from_utf8(bytes) {
                out.extend(script_references(key, text));
            }
        }
        loop {
            let (reached, rest): (Vec<_>, Vec<_>) = waiting
                .into_iter()
                .partition(|(key, _)| is_referenced(key, &out));
            waiting = rest;
            if reached.is_empty() {
                break;
            }
            for (_, text) in reached {
                if let Ok(value) = toml::from_str::<toml::Value>(text) {
                    collect_toml_strings(&value, &mut out);
                }
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
    /// five magic bytes too, so the five rows sum to [`PackReport::total`].
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

/// The directory prefixes of every project nested inside this one, each with
/// its trailing slash. A `project.toml` below the root is what marks one.
fn nested_roots(files: &[String]) -> Vec<String> {
    files
        .iter()
        .filter_map(|rel| rel.strip_suffix("/project.toml"))
        .map(|dir| format!("{dir}/"))
        .collect()
}

/// Whether `rel` lives inside one of them.
fn in_nested(nested: &[String], rel: &str) -> bool {
    nested.iter().any(|prefix| rel.starts_with(prefix.as_str()))
}

/// The paths a script's literals name, as the game spells them: a
/// dependency's script names its files from its own root.
fn script_references(key: &str, source: &str) -> BTreeSet<String> {
    let mut own = BTreeSet::new();
    collect_string_literals(source, &mut own);
    let Some((alias, _)) = crate::dependencies::split(key) else {
        return own;
    };
    let folder = crate::dependencies::folder(alias);
    own.into_iter()
        .map(|reference| {
            if crate::dependencies::split(&reference).is_some() {
                reference
            } else {
                format!("{folder}/{reference}")
            }
        })
        .collect()
}

/// The backend to read through, every file the pack may hold, and each
/// dependency's own: the game's files less `ignored`, then each dependency's
/// under `deps/<alias>/` less its own `ignore`.
#[allow(clippy::type_complexity)]
fn gathered(
    fs: std::rc::Rc<dyn crate::files::FileBackend>,
    project_root: &Path,
    manifest: &str,
    ignored: &[String],
) -> Result<(
    std::rc::Rc<dyn crate::files::FileBackend>,
    Vec<String>,
    Vec<Shipped>,
)> {
    let dependencies = crate::dependencies::declared(manifest)?;
    let fs = mounted(fs, project_root, &dependencies);
    let mut files = Vec::new();
    let skip = (!dependencies.is_empty()).then_some(crate::dependencies::DIR);
    collect_files(&*fs, project_root, project_root, ignored, skip, &mut files);
    let shipped = shipped_dependencies(&*fs, project_root, &dependencies)?;
    for dep in &shipped {
        // Its documents' `id://` are paths by the time they ship.
        files.extend(
            dep.files
                .iter()
                .filter(|rel| *rel != crate::assets::INDEX_PATH)
                .map(|rel| format!("{}/{rel}", dep.folder)),
        );
    }
    Ok((fs, files, shipped))
}

/// What a pack credits each dependency with: `deps/credits.json`.
pub const CREDITS: &str = "credits.json";

/// The backend a pack reads through: the disk, with each `path` dependency
/// mounted under `deps/<alias>`.
fn mounted(
    fs: std::rc::Rc<dyn crate::files::FileBackend>,
    project_root: &Path,
    dependencies: &[crate::dependencies::Dependency],
) -> std::rc::Rc<dyn crate::files::FileBackend> {
    let mounts: Vec<(std::path::PathBuf, std::path::PathBuf)> = dependencies
        .iter()
        .filter_map(|dep| match &dep.source {
            crate::dependencies::Source::Path(path) => Some((
                project_root.join(dep.folder()),
                crate::files::lexical(&project_root.join(path)),
            )),
            crate::dependencies::Source::Forge { .. } => None,
        })
        .collect();
    if mounts.is_empty() {
        return fs;
    }
    std::rc::Rc::new(crate::dependencies::Mounted::new(fs, mounts))
}

/// One dependency on its way into a pack.
struct Shipped {
    alias: String,
    folder: String,
    root: std::path::PathBuf,
    /// Under its root, less its own `ignore`.
    files: Vec<String>,
    index: BTreeMap<String, String>,
    include: Vec<String>,
    import: toml::Table,
    project: String,
    rev: String,
    license: String,
}

impl Shipped {
    fn holds(&self, rel: &str) -> bool {
        crate::dependencies::split(rel).is_some_and(|(alias, _)| alias == self.alias)
    }

    /// A document of this dependency as the game spells its paths. Text that
    /// does not parse ships as it is, as a game's own does.
    fn game_spelling(&self, fs: &dyn crate::files::FileBackend, source: &str) -> String {
        let Ok(mut value) = toml::from_str::<toml::Value>(source) else {
            return source.to_string();
        };
        let exists = |path: &str| fs.exists(&self.root.join(path));
        crate::document_paths::prefixed_in(&self.folder, &mut value, &exists, &self.index);
        toml::to_string(&value).unwrap_or_else(|_| source.to_string())
    }
}

/// Each dependency's files and own settings, read under the game.
fn shipped_dependencies(
    fs: &dyn crate::files::FileBackend,
    project_root: &Path,
    dependencies: &[crate::dependencies::Dependency],
) -> Result<Vec<Shipped>> {
    let locked: BTreeMap<String, String> = fs
        .read(&project_root.join(crate::dependencies::LOCK))
        .ok()
        .map(|bytes| crate::dependencies::read_lock(&String::from_utf8_lossy(&bytes)))
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .map(|entry| (entry.name, entry.rev))
        .collect();
    let mut out = Vec::new();
    for dep in dependencies {
        let folder = dep.folder();
        let root = project_root.join(&folder);
        let own =
            text(fs, &root.join(crate::project::MANIFEST)).with_context(|| match &dep.source {
                crate::dependencies::Source::Forge { .. } => {
                    format!(
                        "dependency `{}` is not fetched: run `balaur fetch`",
                        dep.alias
                    )
                }
                crate::dependencies::Source::Path(path) => format!(
                    "dependency `{}`: {} is not a project",
                    dep.alias,
                    path.display()
                ),
            })?;
        let table: toml::Table = toml::from_str(&own)
            .with_context(|| format!("dependency `{}`: its project.toml", dep.alias))?;
        let ignore = crate::ignore::from_manifest(&own);
        out.push(Shipped {
            files: crate::dependencies::files_of(fs, &root, &ignore),
            index: crate::dependencies::index_of(fs, &root),
            include: crate::dependencies::strings_at(&table, "export", "include"),
            import: table
                .get("import")
                .and_then(toml::Value::as_table)
                .cloned()
                .unwrap_or_default(),
            project: match &dep.source {
                crate::dependencies::Source::Forge { project, .. } => project.clone(),
                crate::dependencies::Source::Path(_) => String::new(),
            },
            rev: locked.get(&dep.alias).cloned().unwrap_or_default(),
            license: table
                .get("application")
                .and_then(|app| app.get("license"))
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            alias: dep.alias.clone(),
            folder,
            root,
        });
    }
    Ok(out)
}

/// Every file under `dir`, project-relative, less dot files and what
/// `ignored` names. A directory the backend cannot read contributes nothing:
/// the manifest was read first, so a root that is not there has already
/// failed. `skip` is a top-level folder left out whole: `deps/`, whose
/// dependencies are gathered with their own settings.
fn collect_files(
    fs: &dyn crate::files::FileBackend,
    root: &Path,
    dir: &Path,
    ignored: &[String],
    skip: Option<&str>,
    out: &mut Vec<String>,
) {
    for (name, is_dir) in fs.list(dir) {
        if name.starts_with('.') || (dir == root && skip == Some(name.as_str())) {
            continue;
        }
        let path = dir.join(&name);
        let rel = path
            .strip_prefix(root)
            .map(|rel| rel.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if crate::ignore::ignored(ignored, &rel) {
            continue;
        }
        if is_dir {
            collect_files(fs, root, &path, ignored, skip, out);
        } else if !rel.is_empty() {
            out.push(rel);
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
