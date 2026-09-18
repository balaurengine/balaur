//! What one file becomes in a target's pack, without building the pack: the
//! row the editor's Import tab draws per target.
//!
//! The same steps an export takes, on one entry: the variant the target
//! answers to, then the texture step, then the re-encode. A face is shown
//! whole, because subsetting needs every character the whole project holds.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use balaur::Pack;
use balaur::import::{kind_of, kinds, sidecar_of};

use crate::config::{self, ExportConfig};

/// One file as one target ships it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    /// The file whose bytes ship under the name: the file or a variant of it.
    pub source: String,
    /// That file's own bytes, and what ships.
    pub before: usize,
    pub after: usize,
    /// The shipped pixels, zero for a file that is not a picture.
    pub width: u32,
    pub height: u32,
    /// The size the scene measures, when a smaller copy ships.
    pub drawn: Option<(u32, u32)>,
    /// What the picture costs on the GPU once uploaded.
    pub gpu_bytes: u64,
}

/// `rel` in the pack `target` would build from `project`; `None` is this
/// machine's own.
///
/// # Errors
/// If the file cannot be read, or a step the export would run refuses it.
pub fn preview(project: &Path, rel: &str, target: Option<&str>) -> Result<Preview> {
    let manifest = config::manifest_for(project, target)?;
    let export = ExportConfig::from_manifest(&manifest, project)?;
    let source = config::manifest_text(project).unwrap_or_default();
    let tags = config::tags_for(&source, target)?;
    let declared =
        toml::from_str(&source).map_or_else(|_| Vec::new(), |doc| balaur::tags::declared_in(&doc));
    let fs = balaur::files::default_backend();
    let mut pack = Pack::default();
    if let Ok(bytes) = fs.read(&project.join(rel)) {
        pack.assets.insert(rel.to_string(), bytes);
    }
    let (stem, extension) = rel.rsplit_once('.').unwrap_or((rel, ""));
    for tag in tags.0.iter().chain(&declared) {
        let variant = format!("{stem}.{tag}.{extension}");
        if let Ok(bytes) = fs.read(&project.join(&variant)) {
            pack.assets.insert(variant, bytes);
        }
    }
    let sidecar = sidecar_of(rel);
    if let Ok(text) = fs.read(&project.join(&sidecar)).map(String::from_utf8) {
        pack.scenes
            .insert(sidecar.clone(), text.unwrap_or_default());
    }
    let chosen = chosen_source(&pack, rel, &tags);
    crate::variants::apply(&mut pack, &tags, &declared);
    let Some(bytes) = pack.assets.get(rel).cloned() else {
        anyhow::bail!("{rel} does not ship for this target");
    };
    let before = bytes.len();
    let settings = balaur::import::merged(
        &manifest,
        rel,
        pack.scenes.get(&sidecar).map(String::as_str),
    );
    let page = font_page(project, rel);
    let (bytes, drawn) = match crate::size::shipped(rel, &bytes, &settings, &export, &page)? {
        Some(shipped) => (shipped.bytes, shipped.drawn),
        None => (bytes, None),
    };
    let bytes =
        crate::size::smaller(rel, &bytes, &export, &BTreeSet::new(), &settings)?.unwrap_or(bytes);
    let drawn = drawn.or_else(|| recorded_size(pack.scenes.get(&sidecar)));
    let (width, height, gpu_bytes) = if kind_of(rel) == Some(kinds::TEXTURE) {
        let (w, h) = balaur::pixels::size(&bytes, &settings)?;
        let base = u64::from(w) * u64::from(h) * 4;
        let mips = balaur::import::texture::sampling(&settings).mipmaps;
        (w, h, if mips { base + base / 3 } else { base })
    } else {
        (0, 0, 0)
    };
    Ok(Preview {
        source: chosen,
        before,
        after: bytes.len(),
        width,
        height,
        drawn,
        gpu_bytes,
    })
}

/// Which file the fold will take for `rel`: the narrowest variant the target
/// answers to, or the file itself.
fn chosen_source(pack: &Pack, rel: &str, tags: &balaur::tags::Tags) -> String {
    let (stem, extension) = rel.rsplit_once('.').unwrap_or((rel, ""));
    tags.0
        .iter()
        .rev()
        .map(|tag| format!("{stem}.{tag}.{extension}"))
        .find(|variant| pack.assets.contains_key(variant))
        .unwrap_or_else(|| rel.to_string())
}

/// The `size` a folded variant recorded, which the scene measures by.
fn recorded_size(sidecar: Option<&String>) -> Option<(u32, u32)> {
    let table: toml::Table = toml::from_str(sidecar?).ok()?;
    let pair = table.get(balaur::import::keys::SIZE)?.as_array()?;
    let side = |at: usize| u32::try_from(pair.get(at)?.as_integer()?).ok();
    Some((side(0)?, side(1)?))
}

/// The pages the `.fnt` files beside `rel` draw from, which a cap leaves be.
fn font_page(project: &Path, rel: &str) -> BTreeSet<String> {
    let directory = rel.rsplit_once('/').map_or("", |(dir, _)| dir);
    let fs = balaur::files::default_backend();
    let mut fonts = Vec::new();
    for (name, is_dir) in fs.list(&project.join(directory)) {
        if is_dir || !name.to_ascii_lowercase().ends_with(".fnt") {
            continue;
        }
        let path = if directory.is_empty() {
            name
        } else {
            format!("{directory}/{name}")
        };
        if let Ok(bytes) = fs.read(&project.join(&path)) {
            fonts.push((path, bytes));
        }
    }
    crate::textures::font_pages(fonts.iter().map(|(p, b)| (p.as_str(), b.as_slice())))
}

#[cfg(test)]
mod tests {
    use super::preview;

    fn png(width: u32, height: u32) -> Vec<u8> {
        crate::textures::png(image::RgbaImage::new(width, height))
    }

    /// A phone's cap and a web variant each show up in their own target's row.
    #[test]
    fn each_target_sees_its_own_cap_and_variant() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("art")).unwrap();
        std::fs::write(
            root.join("project.toml"),
            "[application]\nname = \"t\"\n\n[override.android.export]\nmax_size = 64\n",
        )
        .unwrap();
        std::fs::write(root.join("art/sky.png"), png(256, 128)).unwrap();
        std::fs::write(root.join("art/sky.web.png"), png(128, 64)).unwrap();

        let android = preview(root, "art/sky.png", Some("android")).unwrap();
        assert_eq!((android.width, android.height), (64, 32));
        assert_eq!(android.drawn, Some((256, 128)));
        assert_eq!(android.source, "art/sky.png");

        let web = preview(root, "art/sky.png", Some("web")).unwrap();
        assert_eq!((web.width, web.height), (128, 64));
        assert_eq!(web.source, "art/sky.web.png");
        assert_eq!(web.drawn, Some((256, 128)));
        assert_eq!(web.gpu_bytes, 128 * 64 * 4);

        let desktop = preview(root, "art/sky.png", Some("linux-x64")).unwrap();
        assert_eq!((desktop.width, desktop.height), (256, 128));
        assert_eq!(desktop.drawn, None);
    }
}
