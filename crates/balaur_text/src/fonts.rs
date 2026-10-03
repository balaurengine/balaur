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
    for prefix in ["heading", "ui", "mono", "icon"] {
        if lower.starts_with(&format!("{prefix}-")) {
            return match prefix {
                "heading" => "heading",
                "mono" => "mono",
                "icon" => "icon",
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
    /// `heading`, `ui`, `mono`, `icon`, or `system` for an OS face.
    pub chain: &'static str,
    pub bytes: std::sync::Arc<Vec<u8>>,
    /// What the face's import settings adjust wherever it draws: egui's own
    /// text and everything this crate shapes.
    pub tweak: FaceTweak,
}

/// A project face's `scale`, `y_offset` and `hinting`, from its sidecar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceTweak {
    /// How much larger the face's glyphs and their advances are than the size
    /// asked for; the chain's first face sizes the line with them.
    pub scale: f32,
    /// A fraction of the size the face draws at, positive downward. Moves the
    /// glyphs, never the layout.
    pub y_offset: f32,
    /// `None` hints, as egui and swash both do unless told otherwise.
    pub hinting: Option<bool>,
    /// Off draws every glyph pixel fully on or off, for a pixel face.
    pub antialias: bool,
}

impl Default for FaceTweak {
    fn default() -> Self {
        Self {
            scale: 1.0,
            y_offset: 0.0,
            hinting: None,
            antialias: true,
        }
    }
}

impl FaceTweak {
    fn of(settings: &toml::Table) -> Self {
        use balaur_core::import::{keys, number};
        Self {
            scale: (number(settings, keys::SCALE, 1.0) as f32).clamp(0.1, 10.0),
            y_offset: (number(settings, keys::Y_OFFSET, 0.0) as f32).clamp(-1.0, 1.0),
            hinting: settings.get(keys::HINTING).and_then(toml::Value::as_bool),
            antialias: settings
                .get(keys::ANTIALIAS)
                .and_then(toml::Value::as_bool)
                .unwrap_or(true),
        }
    }
}

/// The chain a face's `font_family` names, or `None` for a word that names none.
fn family_of(settings: &toml::Table) -> Option<&'static str> {
    use balaur_core::import::{keys, word, words};
    match word(settings, keys::FONT_FAMILY, "") {
        words::UI => Some("ui"),
        words::HEADING => Some("heading"),
        words::MONO => Some("mono"),
        words::ICON => Some("icon"),
        _ => None,
    }
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
                    tweak: FaceTweak::default(),
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
            // The sidecar's `family` beats the name's prefix.
            let settings = balaur_core::import::resolved(eng, &rel);
            faces.push(FontFace {
                name: stem.to_string(),
                chain: family_of(&settings.settings).unwrap_or_else(|| chain_of(stem)),
                bytes: std::sync::Arc::new(bytes),
                tweak: FaceTweak::of(&settings.settings),
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
        tweak: FaceTweak::default(),
    }
}

/// Whether this project wants the operating system's faces appended, from
/// `[ui] system_fonts`. A project that says nothing gets them, so text in a
/// script balaur does not vendor keeps drawing.
fn wants_system_fonts(eng: &Engine) -> bool {
    balaur_core::project::UiSettings::from_settings(eng).system_fonts
}

/// The chain a request that names none shapes with.
pub(crate) const UI_CHAIN: &str = "ui";

/// The weight a chain is asked for at `asked`: 400, the regular a scene
/// leaves unsaid, is the chain's own first face's weight, so `heading` draws
/// in the Semibold it opens with wherever it is drawn.
#[must_use]
pub fn chain_weight(asked: u16, chain_face: Option<u16>) -> u16 {
    match (asked, chain_face) {
        (400, Some(own)) => own,
        _ => asked,
    }
}

/// The face the shaper draws a chain in at a weight and a slant, for a
/// drawer that picks faces by name rather than by query: egui.
pub struct FaceMatcher {
    db: cosmic_text::fontdb::Database,
    /// The face each loaded id came from, as an index into the faces given.
    faces: std::collections::HashMap<cosmic_text::fontdb::ID, usize>,
    /// The family and weight of each chain's first face: what the shaper
    /// asks for.
    chains: std::collections::HashMap<&'static str, (String, u16)>,
}

impl FaceMatcher {
    /// Over `faces` less the system's, which share no family with a chain.
    pub fn new(faces: &[FontFace]) -> Self {
        use cosmic_text::fontdb;
        let mut db = fontdb::Database::new();
        let mut by_id = std::collections::HashMap::new();
        let mut chains = std::collections::HashMap::new();
        for (index, face) in faces.iter().enumerate() {
            if face.chain == "system" {
                continue;
            }
            let shared: std::sync::Arc<Vec<u8>> = std::sync::Arc::clone(&face.bytes);
            let data: std::sync::Arc<dyn AsRef<[u8]> + Send + Sync> = shared;
            for id in db.load_font_source(fontdb::Source::Binary(data)) {
                by_id.insert(id, index);
                if let Some(info) = db.face(id)
                    && let Some((family, _)) = info.families.first()
                {
                    chains
                        .entry(face.chain)
                        .or_insert_with(|| (family.clone(), info.weight.0));
                }
            }
        }
        Self {
            db,
            faces: by_id,
            chains,
        }
    }

    /// The face `chain` draws in at `weight`, upright or italic: CSS font
    /// matching over the faces sharing its first face's family, as the shaper
    /// does it. An index into the faces, and whether that face is italic;
    /// `None` for a chain with no face of its own.
    pub fn pick(&self, chain: &str, weight: u16, italic: bool) -> Option<(usize, bool)> {
        use cosmic_text::fontdb;
        let (family, own) = self.chains.get(chain)?;
        let id = self.db.query(&fontdb::Query {
            families: &[fontdb::Family::Name(family)],
            weight: fontdb::Weight(chain_weight(weight, Some(*own))),
            stretch: fontdb::Stretch::Normal,
            style: if italic {
                fontdb::Style::Italic
            } else {
                fontdb::Style::Normal
            },
        })?;
        let slanted = self
            .db
            .face(id)
            .is_some_and(|info| info.style != fontdb::Style::Normal);
        Some((*self.faces.get(&id)?, slanted))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled(name: &str, chain: &'static str, bytes: &'static [u8]) -> FontFace {
        FontFace {
            name: name.into(),
            chain,
            bytes: std::sync::Arc::new(bytes.to_vec()),
            tweak: FaceTweak::default(),
        }
    }

    /// A chain's weight is picked among the faces of its first face's family,
    /// whichever chain they joined, the way the shaper picks it.
    #[test]
    fn a_heavier_weight_picks_the_heavier_face_of_the_family() {
        let faces = [
            bundled(
                "ui-SourceSans3-Regular",
                "ui",
                include_bytes!("../../../editor/fonts/ui-SourceSans3-Regular.ttf"),
            ),
            bundled(
                "heading-SourceSans3-Semibold",
                "heading",
                include_bytes!("../../../editor/fonts/heading-SourceSans3-Semibold.ttf"),
            ),
            bundled(
                "mono-JetBrainsMono-Regular",
                "mono",
                include_bytes!("../../../editor/fonts/mono-JetBrainsMono-Regular.ttf"),
            ),
        ];
        let matcher = FaceMatcher::new(&faces);
        assert_eq!(matcher.pick("ui", 400, false), Some((0, false)));
        assert_eq!(matcher.pick("ui", 700, false), Some((1, false)));
        assert_eq!(matcher.pick("heading", 300, false), Some((0, false)));
        // 400 is the chain's own first face, which egui draws the chain in.
        assert_eq!(matcher.pick("heading", 400, false), Some((1, false)));
        assert_eq!(matcher.pick("mono", 700, false), Some((2, false)));
        // No italic face ships, so the upright one answers and says so.
        assert_eq!(matcher.pick("ui", 400, true), Some((0, false)));
        assert_eq!(matcher.pick("icon", 700, false), None);
    }

    /// A measurement shapes over the project's own faces alone, and wasm reads
    /// no OS face: with no `fonts/` of its own that set was empty and aborted.
    #[test]
    fn a_project_with_no_fonts_of_its_own_still_measures() {
        let faces = font_faces(&Engine::new());
        assert!(
            faces.iter().any(|face| face.chain != "system"),
            "nothing outside the system's faces to shape with"
        );
        let size = crate::TextState::new(&faces, "en-US")
            .measure(&crate::Request::new("measure me", 24.0));
        assert!(size.x > 0.0, "measured no width");
    }
}
