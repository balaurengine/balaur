//! `balaur import`: a model, a sprite, a level or a Godot project brought
//! into a project as the files the editor edits.

mod godot;
mod ldtk;
#[cfg(test)]
mod scene_check;
pub mod shrink;
mod tiled_map;

use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{Context, Result};
use balaur_core::files::{self, FileBackend};
use balaur_core::glb::{Beside, SideReader};

/// What an import wrote: the project-relative paths, and the scene the editor
/// would instantiate for a model.
#[derive(Debug, Default)]
pub struct Imported {
    pub files: Vec<String>,
    pub scene: Option<String>,
    pub note: String,
}

/// Where an import's files go as it makes them.
///
/// One at a time, rather than a list the importer fills and a caller drains:
/// a model names its textures, and collecting those before writing any holds
/// the whole model in memory. It is also the boundary a caller reporting
/// progress wants, and the one a browser task has to yield at.
pub trait Sink {
    /// Take one file, at a path relative to the project.
    ///
    /// # Errors
    /// If the file cannot be written.
    fn put(&mut self, relative: &str, bytes: &[u8]) -> Result<()>;

    /// Every path taken so far, in the order it was written.
    fn written(&self) -> &[String];
}

/// A [`Sink`] writing into a project through the engine's file backend.
///
/// The backend is the thread's, so a desktop writes to disk and a browser tab
/// writes into the memory its project already lives in, with no second path
/// for either.
pub struct ProjectSink {
    root: PathBuf,
    fs: Rc<dyn FileBackend>,
    written: Vec<String>,
}

impl ProjectSink {
    /// A sink writing under `root`.
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            fs: files::default_backend(),
            written: Vec::new(),
        }
    }
}

impl Sink for ProjectSink {
    fn put(&mut self, relative: &str, bytes: &[u8]) -> Result<()> {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            self.fs.mkdir(parent)?;
        }
        self.fs
            .write(&path, bytes)
            .with_context(|| format!("writing {}", path.display()))?;
        self.written.push(relative.to_string());
        Ok(())
    }

    fn written(&self) -> &[String] {
        &self.written
    }
}

/// `balaur import <file>`, printing each path it wrote.
pub fn import_and_report(file: &Path, project: &Path, layers: &[String]) -> Result<()> {
    let imported = import_file(file, project, layers)?;
    for rel in &imported.files {
        println!("wrote {}", project.join(rel).display());
    }
    if !imported.note.is_empty() {
        println!("{}", imported.note);
    }
    Ok(())
}

/// Which importer a file's name routes to.
enum Route {
    Sprite,
    Model,
    /// A `.tmx` or `.ldtk`, which names the files around it.
    Level,
    /// A Godot project or one scene of one, which walks the whole project.
    Godot,
}

/// Where a name is imported by, decided before anything is read: a flag that
/// does not belong is an error about the flag rather than about a file.
fn route(name: &Path, layers: &[String]) -> Result<Route> {
    let extension = extension_of(name);
    Ok(match extension.as_str() {
        "aseprite" | "ase" => Route::Sprite,
        "tmx" | "ldtk" => Route::Level,
        "godot" | "tscn" | "tres" => Route::Godot,
        _ if !layers.is_empty() => {
            anyhow::bail!("--layer picks layers of an .aseprite file; {extension} has none")
        }
        _ => Route::Model,
    })
}

/// `balaur import <file>`: by extension, a model or a sprite.
pub fn import_file(file: &Path, project: &Path, layers: &[String]) -> Result<Imported> {
    match route(file, layers)? {
        // A level and a Godot project are read from where they sit: both
        // walk a directory rather than taking one file.
        Route::Level => import_level(file, &mut ProjectSink::new(project)),
        Route::Godot => import_from_godot(file, project),
        route => {
            let fs = files::default_backend();
            let name = file
                .file_name()
                .and_then(|n| n.to_str())
                .context("the file has no name")?;
            let bytes = fs
                .read(file)
                .with_context(|| format!("reading {}", file.display()))?;
            // Whatever a `.gltf` names beside itself is beside the file it
            // was read from.
            let directory = file.parent().map(Path::to_path_buf).unwrap_or_default();
            let side = |uri: &str| -> Result<Vec<u8>> {
                let path = directory.join(uri);
                fs.read(&path)
                    .with_context(|| format!("reading {}", path.display()))
            };
            let sink = &mut ProjectSink::new(project);
            match route {
                Route::Sprite => import_sprite(name, &bytes, sink, layers),
                _ => import_model(name, &bytes, sink, &side),
            }
        }
    }
}

/// `balaur import` for a caller holding the bytes and no path: a file dropped
/// on a browser tab, which has a name and contents and no directory.
///
/// `name` is the file's own name, whose extension picks the importer and whose
/// stem names what is written. `side` answers for the files a `.gltf` names
/// beside itself; a `.glb` is self-contained and never asks.
///
/// # Errors
/// If the bytes are not the format the name claims, or a file cannot be
/// written, or the name is one only [`import_file`] reads.
pub fn import_bytes(
    name: &str,
    bytes: &[u8],
    sink: &mut dyn Sink,
    side: SideReader<'_>,
    layers: &[String],
) -> Result<Imported> {
    match route(Path::new(name), layers)? {
        Route::Sprite => import_sprite(name, bytes, sink, layers),
        Route::Model => import_model(name, bytes, sink, side),
        Route::Level | Route::Godot => anyhow::bail!(
            "{name} names the files around it, so it is imported from the folder it sits in \
             rather than from bytes"
        ),
    }
}

/// A file's extension, lowercased, or empty for one with none.
fn extension_of(file: &Path) -> String {
    file.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// `balaur import level.tmx --project game`: the atlas, a `tileset` per
/// sheet, and a scene rooted at the level, a `tilemap` node per tile layer
/// under it.
fn import_level(file: &Path, sink: &mut dyn Sink) -> Result<Imported> {
    let stem = import_stem(file)?;
    let ldtk = file
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ldtk"));
    let imported = if ldtk {
        crate::ldtk::import(file, &stem)
    } else {
        crate::tiled_map::import(file, &stem)
    }
    .with_context(|| format!("importing {}", file.display()))?;
    let mut out = Imported::default();
    for (rel, data) in &imported.files {
        sink.put(rel, data)?;
        if rel.starts_with("scenes/") {
            out.scene = Some(rel.clone());
        }
    }
    out.files = sink.written().to_vec();
    out.note = format!(
        "imported {} as {} layer{}",
        file.display(),
        imported.layers,
        if imported.layers == 1 { "" } else { "s" }
    );
    Ok(out)
}

/// `balaur import project.godot --project game`: the settings, every scene,
/// the files they name, and a report of what did not carry. A `.tscn` is one
/// scene of it.
fn import_from_godot(file: &Path, project: &Path) -> Result<Imported> {
    let extension = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "godot" => crate::godot::files::import_project(file, project),
        "tscn" => crate::godot::files::import_scene(file, project),
        _ => anyhow::bail!(
            "a .{extension} on its own is not read yet; `balaur import project.godot` converts the \
             resources its scenes use"
        ),
    }
}

/// The name an imported file's outputs share: its stem, lowercased, with
/// spaces and dashes as underscores so it is a bare TOML key.
fn import_stem(file: &Path) -> Result<String> {
    Ok(file
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .context("the file has no name")?
        .to_ascii_lowercase()
        .replace([' ', '-'], "_"))
}

/// `balaur import walk.aseprite --project game`: `art/walk.webp`,
/// `sheets/walk.toml` and, with tags or more than one frame,
/// `animations/walk.toml`.
fn import_sprite(
    name: &str,
    bytes: &[u8],
    sink: &mut dyn Sink,
    layers: &[String],
) -> Result<Imported> {
    let stem = import_stem(Path::new(name))?;
    let texture = format!("art/{stem}.webp");
    let imported = balaur_render::aseprite::import(bytes, &stem, &texture, layers)
        .with_context(|| format!("importing {name}"))?;
    sink.put(&texture, &imported.page)?;
    sink.put(&format!("sheets/{stem}.toml"), imported.sheet.as_bytes())?;
    if let Some(clips) = imported.clips {
        sink.put(&format!("animations/{stem}.toml"), clips.as_bytes())?;
    }
    Ok(Imported {
        files: sink.written().to_vec(),
        scene: None,
        note: format!(
            "{} frames on a {}x{} page",
            imported.frames, imported.width, imported.height
        ),
    })
}

/// `balaur import model.glb --project game`: `models/model.glb` (and the
/// files a `.gltf` names beside itself), `scenes/model.toml` and, with
/// animations, `animations/model.toml`.
fn import_model(
    name: &str,
    bytes: &[u8],
    sink: &mut dyn Sink,
    side: SideReader<'_>,
) -> Result<Imported> {
    let file = Path::new(name);
    let stem = import_stem(file)?;
    let extension = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("glb")
        .to_ascii_lowercase();
    let model_file = format!("{stem}.{extension}");
    let imported = balaur::glb::import(bytes, &model_file, side)?;
    let mut out = Imported::default();
    sink.put(&format!("models/{model_file}"), bytes)?;
    for (name, beside) in &imported.files {
        // A file the model named is read here and nowhere else, so only one
        // of a model's textures is in memory at a time.
        match beside {
            Beside::Bytes(data) => sink.put(&format!("models/{name}"), data)?,
            Beside::Named(uri) => {
                let data = side(uri).with_context(|| format!("the model names '{uri}'"))?;
                sink.put(&format!("models/{name}"), &data)?;
            }
        }
    }
    // The shader the generated materials draw with, at its own path rather
    // than under `models/`.
    for (rel, text) in &imported.documents {
        sink.put(rel, text.as_bytes())?;
    }
    let scene_rel = format!("scenes/{stem}.toml");
    sink.put(&scene_rel, imported.scene_toml()?.as_bytes())?;
    out.scene = Some(scene_rel);
    if let Some(clips) = imported.clips_toml()? {
        sink.put(&format!("animations/{stem}.toml"), clips.as_bytes())?;
    }
    out.files = sink.written().to_vec();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::import_file;
    use std::path::Path;

    const ASEPRITE_FIXTURE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../balaur_render/tests/fixtures/walk.aseprite"
    );

    const LDTK: &str = r#"{
      "defs": { "tilesets": [
        { "uid": 1, "identifier": "Blocks", "relPath": "blocks.png",
          "tileGridSize": 16, "__cWid": 4 }
      ]},
      "levels": [
        { "identifier": "Cave", "layerInstances": [
          { "__identifier": "Walls", "__type": "Tiles", "__cWid": 1, "__cHei": 1,
            "__gridSize": 16, "__tilesetDefUid": 1,
            "gridTiles": [ { "px": [0, 0], "f": 0, "t": 3 } ]}
        ]}
      ]
    }"#;

    /// The importer's three files, parsed by the parsers the engine loads
    /// them with: the sheet as a `sprite_sheet`, every clip as a clip.
    #[test]
    fn importing_a_sprite_writes_a_page_a_sheet_and_a_clip_per_tag() {
        let project = tempfile::tempdir().unwrap();
        import_file(Path::new(ASEPRITE_FIXTURE), project.path(), &[]).unwrap();
        let page = std::fs::read(project.path().join("art/walk.webp")).unwrap();
        assert_eq!(&page[..4], b"RIFF", "the atlas page is a WebP");
        assert_eq!(&page[8..12], b"WEBP");
        let sheet: toml::Value = toml::from_str(
            &std::fs::read_to_string(project.path().join("sheets/walk.toml")).unwrap(),
        )
        .unwrap();
        let sheet = balaur_render::SpriteSheet::parse(&sheet).unwrap();
        assert_eq!(sheet.texture, "art/walk.webp");
        assert_eq!(sheet.frames.len(), 3);
        let clips: toml::Value = toml::from_str(
            &std::fs::read_to_string(project.path().join("animations/walk.toml")).unwrap(),
        )
        .unwrap();
        let clips = clips["clips"].as_table().unwrap();
        assert_eq!(clips.len(), 3);
        for (name, body) in clips {
            let clip = balaur::animation::clip::parse(body)
                .unwrap_or_else(|e| panic!("clip {name} does not parse: {e:#}"));
            assert_eq!(clip.tracks.len(), 1, "clip {name}");
        }
    }

    /// A level goes to the importer its extension names, and what lands on
    /// disk is a scene the engine loads: one root, the layers under it.
    #[test]
    fn a_ldtk_level_is_routed_by_extension_and_lands_as_a_scene_that_loads() {
        let project = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("cave.ldtk"), LDTK).unwrap();
        std::fs::write(source.path().join("blocks.png"), b"not really a png").unwrap();
        let out = import_file(&source.path().join("cave.ldtk"), project.path(), &[]).unwrap();
        assert_eq!(out.scene.as_deref(), Some("scenes/cave.toml"));
        assert!(out.note.contains("1 layer"), "unhelpful: {}", out.note);
        let scene = std::fs::read_to_string(project.path().join("scenes/cave.toml")).unwrap();
        assert_eq!(
            crate::scene_check::loaded(&scene),
            vec![("cave".to_string(), vec!["Walls".to_string()])]
        );
    }

    /// Bytes and a path are the same import: the drop a browser tab takes and
    /// the command a desktop runs write the same files.
    #[test]
    fn bytes_and_a_path_import_the_same_files() {
        let from_path = tempfile::tempdir().unwrap();
        let by_path = import_file(Path::new(ASEPRITE_FIXTURE), from_path.path(), &[]).unwrap();

        let from_bytes = tempfile::tempdir().unwrap();
        let bytes = std::fs::read(ASEPRITE_FIXTURE).unwrap();
        let mut sink = crate::ProjectSink::new(from_bytes.path());
        let by_bytes = crate::import_bytes(
            "walk.aseprite",
            &bytes,
            &mut sink,
            &balaur_core::glb::no_side_files,
            &[],
        )
        .unwrap();

        assert_eq!(by_path.files, by_bytes.files);
        assert_eq!(by_path.note, by_bytes.note);
        assert!(!by_path.files.is_empty());
        for rel in &by_path.files {
            assert_eq!(
                std::fs::read(from_path.path().join(rel)).unwrap(),
                std::fs::read(from_bytes.path().join(rel)).unwrap(),
                "{rel} differs"
            );
        }
    }

    /// The importer reads and writes through the engine's file backend, which
    /// is what lets a browser tab import into the project it holds in memory.
    #[test]
    fn an_import_reads_and_writes_the_backend_and_not_the_disk() {
        let fs = std::rc::Rc::new(balaur_core::files::MemoryFs::new());
        let bytes = std::fs::read(ASEPRITE_FIXTURE).unwrap();
        fs.seed(Path::new("/source"), [("walk.aseprite".to_string(), bytes)]);
        balaur_core::files::set_default(fs.clone());

        let imported =
            import_file(Path::new("/source/walk.aseprite"), Path::new("/game"), &[]).unwrap();

        let held = fs.snapshot();
        assert!(!imported.files.is_empty());
        for rel in &imported.files {
            assert!(
                held.contains_key(&format!("/game/{rel}")),
                "{rel} is not in the backend; it has {:?}",
                held.keys().collect::<Vec<_>>()
            );
            assert!(
                !Path::new("/game").join(rel).exists(),
                "{rel} reached the disk"
            );
        }
    }

    #[test]
    fn a_layer_flag_on_a_model_is_refused() {
        let project = tempfile::tempdir().unwrap();
        let error = import_file(Path::new("hero.glb"), project.path(), &["a".to_string()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("--layer"), "unhelpful: {error}");
    }
}
