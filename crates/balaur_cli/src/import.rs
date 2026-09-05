//! `balaur import`: a model or a sprite brought into a project as the files
//! the editor edits.

use std::path::Path;

use anyhow::{Context, Result};

/// `balaur import <file>`: by extension, a model or a sprite.
pub(crate) fn import_file(file: &Path, project: &Path, layers: &[String]) -> Result<()> {
    let extension = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "aseprite" | "ase" => import_sprite(file, project, layers),
        _ if !layers.is_empty() => {
            anyhow::bail!("--layer picks layers of an .aseprite file; {extension} has none")
        }
        _ => import_model(file, project),
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
fn import_sprite(file: &Path, project: &Path, layers: &[String]) -> Result<()> {
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
    for (rel, data) in written {
        let path = project.join(&rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, data)?;
        println!("wrote {}", path.display());
    }
    println!(
        "{} frames on a {}x{} page",
        imported.frames, imported.width, imported.height
    );
    Ok(())
}

/// `balaur import model.glb --project game`: `models/model.glb` (and the
/// files a `.gltf` names beside itself), `scenes/model.toml` and, with
/// animations, `animations/model.toml`.
fn import_model(file: &Path, project: &Path) -> Result<()> {
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
    let model = models.join(&model_file);
    std::fs::write(&model, &bytes)?;
    println!("wrote {}", model.display());
    for (name, data) in &imported.files {
        let path = models.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, data)?;
        println!("wrote {}", path.display());
    }
    let scene = project.join("scenes").join(format!("{stem}.toml"));
    std::fs::write(&scene, imported.scene_toml()?)?;
    println!("wrote {}", scene.display());
    if let Some(clips) = imported.clips_toml()? {
        std::fs::create_dir_all(project.join("animations"))?;
        let library = project.join("animations").join(format!("{stem}.toml"));
        std::fs::write(&library, clips)?;
        println!("wrote {}", library.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::import_file;
    use std::path::Path;

    const ASEPRITE_FIXTURE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../balaur_render/tests/fixtures/walk.aseprite"
    );

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

    #[test]
    fn a_layer_flag_on_a_model_is_refused() {
        let project = tempfile::tempdir().unwrap();
        let error = import_file(Path::new("hero.glb"), project.path(), &["a".to_string()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("--layer"), "unhelpful: {error}");
    }
}
