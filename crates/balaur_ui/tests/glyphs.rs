//! Every glyph the editor's scripts draw is in a font the editor bundles.
//! A desktop quietly fills the gaps from system faces; a browser has none,
//! and draws a box where the glyph should be.

use std::path::{Path, PathBuf};

/// Every `.rn` under `dir`, recursively.
fn scripts(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scripts(&path, out);
        } else if path.extension().is_some_and(|e| e == "rn") {
            out.push(path);
        }
    }
}

#[test]
fn editor_scripts_draw_only_bundled_glyphs() {
    let editor = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editor");
    let mut fonts = egui::FontDefinitions::default();
    let mut chain = Vec::new();
    for entry in std::fs::read_dir(editor.join("fonts")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "ttf") {
            continue;
        }
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).unwrap();
        fonts.font_data.insert(
            name.clone(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        chain.push(name);
    }
    assert!(
        !chain.is_empty(),
        "control: no fonts under {}",
        editor.display()
    );
    // The chain every family falls through on the web: the bundle, then the
    // faces egui itself ships.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        chain.extend(fonts.families[&family].iter().cloned());
    }
    let family = egui::FontFamily::Name("bundle".into());
    fonts.families.insert(family.clone(), chain);

    let ctx = egui::Context::default();
    ctx.set_fonts(fonts);
    ctx.begin_pass(egui::RawInput::default());
    let font = egui::FontId::new(12.0, family);
    let mut files = Vec::new();
    scripts(&editor, &mut files);
    let mut missing = Vec::new();
    for path in files {
        // The font smoke test draws scripts the bundle has no face for.
        if path.file_name().is_some_and(|n| n == "selftest.rn") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for c in text.chars().filter(|c| !c.is_ascii()) {
            if !ctx.fonts_mut(|fonts| fonts.has_glyph(&font, c)) {
                let file = path.file_name().unwrap().to_string_lossy().into_owned();
                missing.push(format!("U+{:04X} {c:?} in {file}", c as u32));
            }
        }
    }
    ctx.end_pass().textures_delta.clear();
    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "glyphs no bundled font has; use `icons::` or a character the bundle covers:\n{}",
        missing.join("\n")
    );
}
