//! `balaur import`: a model, a sprite, a level or a Godot project brought
//! into a project as the files the editor edits.

mod godot;
mod ldtk;
#[cfg(test)]
mod scene_check;
mod tiled_map;

use std::path::Path;

use anyhow::{Context, Result};

/// What an import wrote: the project-relative paths, and the scene the editor
/// would instantiate for a model.
#[derive(Debug, Default)]
pub struct Imported {
    pub files: Vec<String>,
    pub scene: Option<String>,
    pub note: String,
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

/// `balaur import <file>`: by extension, a model or a sprite.
pub fn import_file(file: &Path, project: &Path, layers: &[String]) -> Result<Imported> {
    let extension = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "aseprite" | "ase" => import_sprite(file, project, layers),
        "tmx" | "ldtk" => import_level(file, project),
        "godot" | "tscn" | "tres" => import_from_godot(file, project),
        _ if !layers.is_empty() => {
            anyhow::bail!("--layer picks layers of an .aseprite file; {extension} has none")
        }
        _ => import_model(file, project),
    }
}

/// `balaur import level.tmx --project game`: the atlas, a `tileset` per
/// sheet, and a scene rooted at the level, a `tilemap` node per tile layer
/// under it.
fn import_level(file: &Path, project: &Path) -> Result<Imported> {
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
        let path = project.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, data)?;
        out.files.push(rel.clone());
        if rel.starts_with("scenes/") {
            out.scene = Some(rel.clone());
        }
    }
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

/// `balaur import walk.aseprite --project game`: `art/walk.png`,
/// `sheets/walk.toml` and, with tags or more than one frame,
/// `animations/walk.toml`.
fn import_sprite(file: &Path, project: &Path, layers: &[String]) -> Result<Imported> {
    let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    let stem = import_stem(file)?;
    let texture = format!("art/{stem}.png");
    let imported = balaur_render::aseprite::import(&bytes, &stem, &texture, layers)
        .with_context(|| format!("importing {}", file.display()))?;
    let mut written = vec![
        (texture, imported.png),
        (format!("sheets/{stem}.toml"), imported.sheet.into_bytes()),
    ];
    if let Some(clips) = imported.clips {
        written.push((format!("animations/{stem}.toml"), clips.into_bytes()));
    }
    let mut out = Imported::default();
    for (rel, data) in written {
        let path = project.join(&rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, data)?;
        out.files.push(rel);
    }
    out.note = format!(
        "{} frames on a {}x{} page",
        imported.frames, imported.width, imported.height
    );
    Ok(out)
}

/// `balaur import model.glb --project game`: `models/model.glb` (and the
/// files a `.gltf` names beside itself), `scenes/model.toml` and, with
/// animations, `animations/model.toml`.
fn import_model(file: &Path, project: &Path) -> Result<Imported> {
    let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    let stem = import_stem(file)?;
    let extension = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("glb")
        .to_ascii_lowercase();
    let model_file = format!("{stem}.{extension}");
    let directory = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let side = |uri: &str| -> Result<Vec<u8>> {
        let path = directory.join(uri);
        std::fs::read(&path).with_context(|| format!("reading {}", path.display()))
    };
    let imported = balaur::glb::import(&bytes, &model_file, &side)?;
    let models = project.join("models");
    std::fs::create_dir_all(&models)?;
    std::fs::create_dir_all(project.join("scenes"))?;
    let mut out = Imported::default();
    std::fs::write(models.join(&model_file), &bytes)?;
    out.files.push(format!("models/{model_file}"));
    for (name, data) in &imported.files {
        let path = models.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, data)?;
        out.files.push(format!("models/{name}"));
    }
    let scene_rel = format!("scenes/{stem}.toml");
    std::fs::write(project.join(&scene_rel), imported.scene_toml()?)?;
    out.files.push(scene_rel.clone());
    out.scene = Some(scene_rel);
    if let Some(clips) = imported.clips_toml()? {
        std::fs::create_dir_all(project.join("animations"))?;
        let library = format!("animations/{stem}.toml");
        std::fs::write(project.join(&library), clips)?;
        out.files.push(library);
    }
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
        let png = std::fs::read(project.path().join("art/walk.png")).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let sheet: toml::Value = toml::from_str(
            &std::fs::read_to_string(project.path().join("sheets/walk.toml")).unwrap(),
        )
        .unwrap();
        let sheet = balaur_render::SpriteSheet::parse(&sheet).unwrap();
        assert_eq!(sheet.texture, "art/walk.png");
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

    #[test]
    fn a_layer_flag_on_a_model_is_refused() {
        let project = tempfile::tempdir().unwrap();
        let error = import_file(Path::new("hero.glb"), project.path(), &["a".to_string()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("--layer"), "unhelpful: {error}");
    }
}
