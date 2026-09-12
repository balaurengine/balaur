//! The faces text is shaped with: a project's own `fonts/*.ttf`, the ones the
//! operating system ships, and the face every build carries.

use balaur_core::Engine;
use balaur_core::project::ProjectFiles;

/// Faces the operating system already ships, appended to every chain so a
/// script balaur does not vendor draws instead of tofu. Never bundled: a
/// single CJK weight is larger than the whole editor.
#[cfg(target_os = "macos")]
const SYSTEM_FACES: &[&str] = &[
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    "/System/Library/Fonts/GeezaPro.ttc",
    "/System/Library/Fonts/ArialHB.ttc",
    "/System/Library/Fonts/Kohinoor.ttc",
    "/System/Library/Fonts/ThonburiUI.ttc",
    "/System/Library/Fonts/Apple Symbols.ttf",
    "/System/Library/Fonts/Apple Color Emoji.ttc",
];

#[cfg(target_os = "windows")]
const SYSTEM_FACES: &[&str] = &[
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\YuGothM.ttc",
    "C:\\Windows\\Fonts\\malgun.ttf",
    "C:\\Windows\\Fonts\\msyh.ttc",
    "C:\\Windows\\Fonts\\seguisym.ttf",
    "C:\\Windows\\Fonts\\seguiemj.ttf",
];

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const SYSTEM_FACES: &[&str] = &[
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
    "/usr/share/fonts/noto/NotoColorEmoji.ttf",
];

/// Which chain a bundled file joins, from its name. Explicit, because a
/// guess put every new face in the UI chain and nothing said so.
fn chain_of(stem: &str) -> &'static str {
    let lower = stem.to_lowercase();
    for prefix in ["heading", "ui", "mono", "icons"] {
        if lower.starts_with(&format!("{prefix}-")) {
            return match prefix {
                "heading" => "heading",
                "mono" => "mono",
                "icons" => "icons",
                _ => "ui",
            };
        }
    }
    // Pre-convention names, kept so an existing project's fonts still load.
    if lower.contains("mono") {
        return "mono";
    }
    if lower.contains("caprasimo") || lower.contains("display") {
        return "heading";
    }
    "ui"
}

/// One face the theme found, and which chain it joins.
#[derive(Clone)]
pub struct FontFace {
    pub name: String,
    /// `heading`, `ui`, `mono`, `icons`, or `system` for an OS face.
    pub chain: &'static str,
    pub bytes: std::sync::Arc<Vec<u8>>,
}

/// The faces the operating system ships, whichever of them are present,
/// read once for the life of the process.
///
/// These are the largest files on the machine — one CJK collection runs to
/// tens of megabytes — and `font_faces` is called again for every mesher that
/// wants them, so reading them per call meant a fresh copy of all of it each
/// time. Cloning a face now clones an `Arc`.
fn system_cache() -> &'static [FontFace] {
    static LOADED: std::sync::OnceLock<Vec<FontFace>> = std::sync::OnceLock::new();
    LOADED.get_or_init(|| {
        SYSTEM_FACES
            .iter()
            .filter_map(|path| {
                let bytes = std::fs::read(path).ok()?; // os files: the system's own faces
                Some(FontFace {
                    name: format!("system:{path}"),
                    chain: "system",
                    bytes: std::sync::Arc::new(bytes),
                })
            })
            .collect()
    })
}

/// The faces the operating system ships, whichever of them are present.
pub fn system_faces() -> Vec<FontFace> {
    system_cache().to_vec()
}

/// The cached bytes for an OS face. Borrowed for the life of the process, so
/// egui holds them without a copy of its own.
pub fn system_static_bytes(name: &str) -> Option<&'static [u8]> {
    system_cache()
        .iter()
        .find(|face| face.name == name)
        .map(|face| face.bytes.as_slice())
}

/// Every face, in chain order: a project's own `fonts/*.ttf` first, then the
/// system's. Read once here for both egui and the shaper.
pub fn font_faces(eng: &Engine) -> Vec<FontFace> {
    let mut faces = Vec::new();
    if let Some(files) = eng.try_resource::<ProjectFiles>() {
        let files = files.borrow();
        let mut paths: Vec<String> = files
            .list("fonts")
            .into_iter()
            .filter(|p| {
                matches!(
                    std::path::Path::new(p).extension().and_then(|e| e.to_str()),
                    Some("ttf" | "otf" | "ttc")
                )
            })
            .collect();
        paths.sort();
        for rel in paths {
            let path = std::path::PathBuf::from(&rel);
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(bytes) = files.read(&rel) else {
                continue;
            };
            faces.push(FontFace {
                name: stem.to_string(),
                chain: chain_of(stem),
                bytes: std::sync::Arc::new(bytes),
            });
            tracing::info!("ui: loaded font {stem}");
        }
    }
    let ships_own = !faces.is_empty();
    if wants_system_fonts(eng) {
        faces.extend(system_faces());
    }
    // Behind the OS faces, and only for a project carrying none of its own:
    // wasm reads no system face, and an empty face list aborts the shaper.
    if !ships_own {
        faces.push(fallback_face());
    }
    faces
}

/// The face every build carries, so the shaper is never handed nothing.
fn fallback_face() -> FontFace {
    static LOADED: std::sync::OnceLock<std::sync::Arc<Vec<u8>>> = std::sync::OnceLock::new();
    let bytes = LOADED.get_or_init(|| {
        std::sync::Arc::new(
            include_bytes!("../../../editor/fonts/ui-SourceSans3-Regular.ttf").to_vec(),
        )
    });
    FontFace {
        name: "ui-SourceSans3-Regular".into(),
        chain: "ui",
        bytes: std::sync::Arc::clone(bytes),
    }
}

/// Whether this project wants the operating system's faces appended, from
/// `[ui] system_fonts`. A project that says nothing gets them, so text in a
/// script balaur does not vendor keeps drawing.
fn wants_system_fonts(eng: &Engine) -> bool {
    balaur_core::project::UiSettings::from_settings(eng).system_fonts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A measurement shapes over the project's own faces alone, and wasm reads
    /// no OS face: with no `fonts/` of its own that set was empty and aborted.
    #[test]
    fn a_project_with_no_fonts_of_its_own_still_measures() {
        let faces = font_faces(&Engine::new());
        assert!(
            faces.iter().any(|face| face.chain != "system"),
            "nothing outside the system's faces to shape with"
        );
        let size = crate::TextState::new(&faces, "en-US").measure(&crate::Request {
            text: "measure me".into(),
            size: 24.0,
            weight: 400,
            italic: false,
            width: None,
            align: crate::Align::Start,
            markup: false,
            font: String::new(),
            family: String::new(),
            line_height: 0.0,
            letter_spacing: 0.0,
        });
        assert!(size.x > 0.0, "measured no width");
    }
}
