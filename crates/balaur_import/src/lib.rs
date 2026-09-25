//! `balaur import`: a model, a sprite, a level or a Godot project brought
//! into a project as the files the editor edits.

pub mod atlas;
mod godot;
mod ldtk;
#[cfg(test)]
mod scene_check;
pub mod shrink;
mod tiled_map;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use balaur_core::files;
use balaur_core::glb::{Beside, SideReader};
use balaur_core::task::Progress;

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
/// for either. Asked for at each write rather than held, so a sink can move to
/// the thread an import runs on.
pub struct ProjectSink {
    root: PathBuf,
    written: Vec<String>,
}

impl ProjectSink {
    /// A sink writing under `root`.
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            written: Vec::new(),
        }
    }
}

impl Sink for ProjectSink {
    fn put(&mut self, relative: &str, bytes: &[u8]) -> Result<()> {
        let path = self.root.join(relative);
        let fs = files::default_backend();
        if let Some(parent) = path.parent() {
            fs.mkdir(parent)?;
        }
        fs.write(&path, bytes)
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
    /// An animated `.gif`, packed as `balaur atlas` packs loose frames.
    Gif,
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
        "gif" => Route::Gif,
        "tmx" | "ldtk" => Route::Level,
        "godot" | "tscn" | "tres" => Route::Godot,
        _ if !layers.is_empty() => {
            anyhow::bail!("--layer picks layers of an .aseprite file; {extension} has none")
        }
        _ => Route::Model,
    })
}

/// Extensions an importer reads, as [`import_file`] routes them.
///
/// The list lives here and nowhere else: a second copy in the editor is how a
/// `.tscn` came to be readable by `balaur import` and refused by a drop.
/// `.tres` is not here -- one on its own is a resource a scene names, not a
/// thing to import.
const CLAIMED: &[&str] = &[
    "glb", "gltf", "aseprite", "ase", "gif", "tmx", "ldtk", "godot", "tscn",
];

/// Whether an importer reads this name, by extension.
#[must_use]
pub fn claims(name: &str) -> bool {
    CLAIMED.contains(&extension_of(Path::new(name)).as_str())
}

/// The extensions an importer reads, for a file picker's filter.
#[must_use]
pub fn claimed() -> &'static [&'static str] {
    CLAIMED
}

/// Whether importing this name can be driven a slice at a time.
///
/// A model and a sprite can: [`plan_bytes`] reads one and answers what it will
/// write. A level cannot, because it walks the folder it sits in as it goes,
/// so a caller has to give it one long call. A project walks too, and is
/// [`ProjectWalk`] instead.
#[must_use]
pub fn slices(name: &str) -> bool {
    matches!(
        route(Path::new(name), &[]),
        Ok(Route::Sprite | Route::Gif | Route::Model)
    )
}

/// Whether this name is a whole Godot project, which [`ProjectWalk`] steps.
#[must_use]
pub fn walks(name: &str) -> bool {
    extension_of(Path::new(name)) == "godot"
}

/// A Godot project import in flight, a file at a time.
///
/// The one import that has no plan: what it writes is found by walking the
/// project, and a project holds thousands of files. So it is counted off by
/// the files it reads rather than the ones it writes, and a caller under a
/// tick steps it as it does a [`Plan`].
pub struct ProjectWalk(godot::walk::Walk);

impl ProjectWalk {
    /// Convert the project's settings, and read the lookups every file after
    /// them needs. Nothing else is read here.
    ///
    /// # Errors
    /// If `project.godot` cannot be read, or says something this cannot map.
    pub fn begin(file: &Path, project: &Path) -> Result<Self> {
        Ok(Self(godot::walk::Walk::begin(file, project)?))
    }

    /// How many files it will read, for a caller counting progress.
    #[must_use]
    pub fn files(&self) -> usize {
        self.0.files()
    }

    /// The project-relative path [`ProjectWalk::read_next`] would read now, or
    /// `None` when there are none left.
    #[must_use]
    pub fn peek(&self) -> Option<&str> {
        self.0.peek()
    }

    /// Read one file, convert it, and say whether any are left.
    ///
    /// A scene that will not convert is a note in the report rather than an
    /// error: one bad scene in a thousand does not stop the other files.
    ///
    /// # Errors
    /// If a file cannot be read or written.
    pub fn read_next(&mut self) -> Result<Progress> {
        self.0.step()?;
        Ok(if self.0.peek().is_some() {
            Progress::More
        } else {
            Progress::Done
        })
    }

    /// The shim, the report, and what the whole walk wrote.
    ///
    /// # Errors
    /// If the report cannot be written.
    pub fn finish(self) -> Result<Imported> {
        self.0.finish()
    }
}

/// `balaur import <file>`: by extension, a model or a sprite.
pub fn import_file(file: &Path, project: &Path, layers: &[String]) -> Result<Imported> {
    match route(file, layers)? {
        // A level and a Godot project are read from where they sit: both
        // walk a directory rather than taking one file.
        Route::Level => import_level(file, &mut ProjectSink::new(project)),
        Route::Godot => import_from_godot(file, project),
        // A model or a sprite: read here, and routed again by name inside.
        _ => {
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
            import_bytes(name, &bytes, &mut ProjectSink::new(project), &side, layers)
        }
    }
}

/// One file an import is going to write, and where its bytes come from.
enum Output {
    /// Bytes the import made or already holds: a converted document, a
    /// sidecar, or a texture the model carried inside itself.
    Held { path: String, bytes: Vec<u8> },
    /// A file the source named beside itself, read when it is written and not
    /// before. A model's textures are this, which is why a plan of them costs
    /// their names rather than their bytes.
    Beside { path: String, uri: String },
}

impl Output {
    fn path(&self) -> &str {
        match self {
            Self::Held { path, .. } | Self::Beside { path, .. } => path,
        }
    }
}

/// An import read and understood, with nothing written yet.
///
/// The parse is one piece of work and each file after it is another, so a
/// caller that must not block — a browser tab, or an editor keeping its frame
/// — drives [`Plan::write_next`] a slice at a time rather than handing the
/// whole import one call.
pub struct Plan {
    outputs: Vec<Output>,
    next: usize,
    scene: Option<String>,
    note: String,
}

impl Plan {
    /// How many files it will write, for a caller counting progress.
    #[must_use]
    pub fn outputs(&self) -> usize {
        self.outputs.len()
    }

    /// The scene the editor would instantiate, for a model.
    #[must_use]
    pub fn scene(&self) -> Option<&str> {
        self.scene.as_deref()
    }

    /// A line for the log, for a caller that drove the writing itself and so
    /// never got an [`Imported`] back.
    #[must_use]
    pub fn note(&self) -> &str {
        &self.note
    }

    /// The path of the file [`Plan::write_next`] would write now, or `None`
    /// when there are none left.
    #[must_use]
    pub fn peek(&self) -> Option<&str> {
        self.outputs.get(self.next).map(Output::path)
    }

    /// Write one file, and say whether any are left.
    ///
    /// `side` answers for what the source named beside itself, as it did for
    /// the plan. Bytes are read here rather than held, so a model's textures
    /// cross one at a time however many it names.
    ///
    /// # Errors
    /// If the file cannot be read or written.
    pub fn write_next(&mut self, sink: &mut dyn Sink, side: SideReader<'_>) -> Result<Progress> {
        let Some(output) = self.outputs.get(self.next) else {
            return Ok(Progress::Done);
        };
        self.next += 1;
        match output {
            Output::Held { path, bytes } => sink.put(path, bytes)?,
            Output::Beside { path, uri } => {
                let bytes = side(uri).with_context(|| format!("the source names '{uri}'"))?;
                sink.put(path, &bytes)?;
            }
        }
        Ok(if self.next < self.outputs.len() {
            Progress::More
        } else {
            Progress::Done
        })
    }

    /// Write everything left, and answer as an import does.
    ///
    /// # Errors
    /// If any file cannot be read or written.
    pub fn write_all(mut self, sink: &mut dyn Sink, side: SideReader<'_>) -> Result<Imported> {
        while self.write_next(sink, side)? == Progress::More {}
        Ok(Imported {
            files: sink.written().to_vec(),
            scene: self.scene,
            note: self.note,
        })
    }
}

/// Read a file and work out what importing it writes, writing nothing.
///
/// `name` is the file's own name, whose extension picks the importer and whose
/// stem names what is written. `side` answers for the files a `.gltf` names
/// beside itself; a `.glb` is self-contained and never asks.
///
/// # Errors
/// If the bytes are not the format the name claims, or the name is one only
/// [`import_file`] reads.
pub fn plan_bytes(
    name: &str,
    bytes: &[u8],
    side: SideReader<'_>,
    layers: &[String],
) -> Result<Plan> {
    match route(Path::new(name), layers)? {
        Route::Sprite => plan_sprite(name, bytes, layers),
        Route::Gif => {
            let stem = import_stem(Path::new(name))?;
            let frames =
                atlas::frames_of_gif(bytes, &stem).with_context(|| format!("importing {name}"))?;
            plan_sheet(&stem, &frames, false)
        }
        Route::Model => plan_model(name, bytes, side),
        Route::Level | Route::Godot => anyhow::bail!(
            "{name} names the files around it, so it is imported from the folder it sits in \
             rather than from bytes"
        ),
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
    plan_bytes(name, bytes, side, layers)?.write_all(sink, side)
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
        "godot" => crate::godot::walk::import_project(file, project),
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
fn plan_sprite(name: &str, bytes: &[u8], layers: &[String]) -> Result<Plan> {
    let stem = import_stem(Path::new(name))?;
    let texture = format!("art/{stem}.webp");
    let imported = balaur_render::aseprite::import(bytes, &stem, &texture, layers)
        .with_context(|| format!("importing {name}"))?;
    // Pixel art: sampled nearest, so each texel stays a square, and left at
    // its size by `balaur shrink`, which reads the same key.
    let sampling = format!(
        "# Written by `balaur import` for a sprite editor's pixels.\n{} = \"nearest\"\n",
        balaur_core::import::keys::FILTER
    );
    // The page is one image the reader composited, so it is held either way.
    let mut outputs = vec![
        Output::Held {
            path: balaur_core::import::sidecar_of(&texture),
            bytes: sampling.into_bytes(),
        },
        Output::Held {
            path: texture,
            bytes: imported.page,
        },
        Output::Held {
            path: format!("sheets/{stem}.toml"),
            bytes: imported.sheet.into_bytes(),
        },
    ];
    if let Some(clips) = imported.clips {
        outputs.push(Output::Held {
            path: format!("animations/{stem}.toml"),
            bytes: clips.into_bytes(),
        });
    }
    Ok(Plan {
        outputs,
        next: 0,
        scene: None,
        note: format!(
            "{} frames on a {}x{} page",
            imported.frames, imported.width, imported.height
        ),
    })
}

/// A packed sheet's files: `art/<stem>.webp`, `sheets/<stem>.toml` and, for a
/// run of frames, `animations/<stem>.toml`. `nearest` marks the page as pixel
/// art, as every frame it was packed from was.
fn plan_sheet(stem: &str, frames: &[atlas::Frame], nearest: bool) -> Result<Plan> {
    let texture = format!("art/{stem}.webp");
    let packed = atlas::pack(frames, stem, &texture)?;
    let mut outputs = Vec::new();
    if nearest {
        outputs.push(Output::Held {
            path: balaur_core::import::sidecar_of(&texture),
            bytes: format!(
                "# Written by `balaur atlas`: its frames were pixel art.\n{} = \"{}\"\n",
                balaur_core::import::keys::FILTER,
                balaur_core::import::words::NEAREST
            )
            .into_bytes(),
        });
    }
    outputs.push(Output::Held {
        path: texture,
        bytes: packed.page,
    });
    outputs.push(Output::Held {
        path: format!("sheets/{stem}.toml"),
        bytes: packed.sheet.into_bytes(),
    });
    if let Some(clips) = packed.clips {
        outputs.push(Output::Held {
            path: format!("animations/{stem}.toml"),
            bytes: clips.into_bytes(),
        });
    }
    Ok(Plan {
        outputs,
        next: 0,
        scene: None,
        note: format!(
            "{} frames on a {}x{} page",
            packed.frames, packed.width, packed.height
        ),
    })
}

/// `balaur atlas <folder or files> --name hero`: every image packed onto one
/// page with a sheet naming each, written into `project`. A frame shows for
/// `milliseconds`; the page is pixel art when each file's settings say so.
///
/// # Errors
/// If an input does not read, the frames do not fit one page, or a file
/// cannot be written.
pub fn atlas_into(
    inputs: &[PathBuf],
    project: &Path,
    name: &str,
    milliseconds: u32,
) -> Result<Imported> {
    let stem = import_stem(Path::new(name))?;
    let files = atlas::files_from(inputs)?;
    let frames = atlas::frames_from(&files, milliseconds)?;
    let source = std::fs::read_to_string(project.join("project.toml")).unwrap_or_default();
    let manifest: toml::Table = toml::from_str(&source).unwrap_or_default();
    let nearest = !files.is_empty()
        && files.iter().all(|file| {
            let rel = file
                .strip_prefix(project)
                .unwrap_or(file)
                .to_string_lossy()
                .replace('\\', "/");
            let own =
                std::fs::read_to_string(project.join(balaur_core::import::sidecar_of(&rel))).ok();
            let settings = balaur_core::import::merged(&manifest, &rel, own.as_deref());
            balaur_core::import::texture::is_pixel_art(&settings)
        });
    let side =
        |uri: &str| -> Result<Vec<u8>> { anyhow::bail!("an atlas names no file beside it: {uri}") };
    plan_sheet(&stem, &frames, nearest)?.write_all(&mut ProjectSink::new(project), &side)
}

/// `balaur import model.glb --project game`: `models/model.glb` (and the
/// files a `.gltf` names beside itself), `scenes/model.toml` and, with
/// animations, `animations/model.toml`.
fn plan_model(name: &str, bytes: &[u8], side: SideReader<'_>) -> Result<Plan> {
    let file = Path::new(name);
    let stem = import_stem(file)?;
    let extension = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("glb")
        .to_ascii_lowercase();
    let model_file = format!("{stem}.{extension}");
    let imported = balaur::glb::import(bytes, &model_file, side)?;
    let scene_toml = imported.scene_toml()?;
    let clips_toml = imported.clips_toml()?;
    // The source first, so the bytes the parse needed are written and dropped
    // before its textures start crossing.
    let mut outputs = vec![Output::Held {
        path: format!("models/{model_file}"),
        bytes: bytes.to_vec(),
    }];
    for (name, beside) in imported.files {
        let path = format!("models/{name}");
        outputs.push(match beside {
            Beside::Bytes(bytes) => Output::Held { path, bytes },
            Beside::Named(uri) => Output::Beside { path, uri },
        });
    }
    // The shader the generated materials draw with, at its own path rather
    // than under `models/`.
    for (path, text) in imported.documents {
        outputs.push(Output::Held {
            path,
            bytes: text.into_bytes(),
        });
    }
    let scene = format!("scenes/{stem}.toml");
    outputs.push(Output::Held {
        path: scene.clone(),
        bytes: scene_toml.into_bytes(),
    });
    if let Some(clips) = clips_toml {
        outputs.push(Output::Held {
            path: format!("animations/{stem}.toml"),
            bytes: clips.into_bytes(),
        });
    }
    Ok(Plan {
        outputs,
        next: 0,
        scene: Some(scene),
        note: String::new(),
    })
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
        let sampling =
            std::fs::read_to_string(project.path().join("art/walk.webp.import.toml")).unwrap();
        assert!(
            sampling.contains("nearest"),
            "pixel art samples nearest: {sampling}"
        );
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

    /// A plan writes one file per slice, which is what lets a caller keep its
    /// frame: the same files land as `import_bytes` writes in one call.
    #[test]
    fn a_plan_writes_one_file_per_slice() {
        use crate::Sink as _;
        use balaur_core::task::Progress;

        let bytes = std::fs::read(ASEPRITE_FIXTURE).unwrap();
        let written = tempfile::tempdir().unwrap();
        let mut plan = crate::plan_bytes(
            "walk.aseprite",
            &bytes,
            &balaur_core::glb::no_side_files,
            &[],
        )
        .unwrap();
        assert_eq!(
            plan.outputs(),
            4,
            "a page, its sampling, a sheet and a clip library"
        );

        let mut sink = crate::ProjectSink::new(written.path());
        let mut slices = 0;
        loop {
            // What it is about to write is known before it writes it, which is
            // the name a progress report carries.
            let next = plan.peek().map(str::to_string);
            let progress = plan
                .write_next(&mut sink, &balaur_core::glb::no_side_files)
                .unwrap();
            if let Some(path) = next {
                slices += 1;
                assert!(
                    written.path().join(&path).exists(),
                    "{path} was announced and not written"
                );
            }
            if progress == Progress::Done {
                break;
            }
        }
        assert_eq!(slices, 4, "one slice per file");

        // The same import in one call, for comparison.
        let whole = tempfile::tempdir().unwrap();
        let at_once = import_file(Path::new(ASEPRITE_FIXTURE), whole.path(), &[]).unwrap();
        assert_eq!(at_once.files, sink.written());
        for rel in &at_once.files {
            assert_eq!(
                std::fs::read(whole.path().join(rel)).unwrap(),
                std::fs::read(written.path().join(rel)).unwrap(),
                "{rel} differs"
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
