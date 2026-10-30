//! The faces text is shaped with — a project's own `fonts/`, the ones the
//! operating system ships, and the face every build carries — and the fonts a
//! text names, each a face of those and how it is shaped.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use balaur_core::Engine;
use balaur_core::project::ProjectFiles;
use cosmic_text::{FontSystem, fontdb};

use crate::font::{Font, FontSettings};
use crate::shape::Tweaks;

/// Faces the operating system already ships, behind every project's own so a
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

/// One face file a project draws in.
#[derive(Clone)]
pub struct FontFace {
    /// What egui knows the face by: its file's stem, or `system:<path>`.
    pub name: String,
    /// The file it was read from, as the project names it; empty for the
    /// system's faces and the one every build carries.
    pub path: String,
    /// One of the operating system's: drawn as a fallback, never measured.
    pub system: bool,
    pub bytes: Arc<Vec<u8>>,
    /// What the face's import settings adjust wherever it draws: egui's own
    /// text and everything this crate shapes.
    pub tweak: FaceTweak,
}

/// A project face's `scale`, `y_offset` and `hinting`, from its sidecar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceTweak {
    /// How much larger the face's glyphs and their advances are than the size
    /// asked for; a font's own face sizes the line with them.
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

/// The faces the operating system ships, whichever of them are present,
/// read once for the life of the process.
///
/// These are the largest files on the machine — one CJK collection runs to
/// tens of megabytes — and a project's faces are read again whenever a file
/// under its `fonts/` moves, so reading these per read meant a fresh copy of
/// all of it each time. Cloning a face clones an `Arc`.
fn system_cache() -> &'static [FontFace] {
    static LOADED: std::sync::OnceLock<Vec<FontFace>> = std::sync::OnceLock::new();
    LOADED.get_or_init(|| {
        SYSTEM_FACES
            .iter()
            .filter_map(|path| {
                let bytes = std::fs::read(path).ok()?; // os files: the system's own faces
                Some(FontFace {
                    name: format!("system:{path}"),
                    path: String::new(),
                    system: true,
                    bytes: Arc::new(bytes),
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

/// The face every build carries: the default font where a project's theme
/// names none, and what the shaper falls back to when nothing else draws.
#[must_use]
pub fn bundled_face() -> FontFace {
    static LOADED: std::sync::OnceLock<Arc<Vec<u8>>> = std::sync::OnceLock::new();
    let bytes = LOADED.get_or_init(|| {
        Arc::new(include_bytes!("../../../editor/fonts/SourceSans3-Regular.ttf").to_vec())
    });
    FontFace {
        name: "SourceSans3-Regular".into(),
        path: String::new(),
        system: false,
        bytes: Arc::clone(bytes),
        tweak: FaceTweak::default(),
    }
}

/// One face the operating system has installed, as a picker lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledFace {
    pub family: String,
    /// Its weight and slant in words: `Regular`, `Bold Italic`.
    pub style: String,
    pub path: std::path::PathBuf,
}

/// The name CSS gives a weight.
#[cfg(not(target_family = "wasm"))]
fn weight_name(weight: u16) -> &'static str {
    match weight {
        0..=149 => "Thin",
        150..=249 => "ExtraLight",
        250..=349 => "Light",
        350..=449 => "Regular",
        450..=549 => "Medium",
        550..=649 => "SemiBold",
        650..=749 => "Bold",
        750..=849 => "ExtraBold",
        _ => "Black",
    }
}

/// Every face the operating system has installed, by family and style, one
/// file each: what a picker on a desktop offers to copy into a project, since
/// an exported game cannot count on a face being installed. Empty where
/// there is nothing to read them from: the web.
#[must_use]
pub fn installed_faces() -> Vec<InstalledFace> {
    #[cfg(not(target_family = "wasm"))]
    {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut faces: Vec<InstalledFace> = db
            .faces()
            .filter_map(|info| {
                let fontdb::Source::File(path) = &info.source else {
                    return None;
                };
                let (family, _) = info.families.first()?;
                let slant = if info.style == fontdb::Style::Normal {
                    ""
                } else {
                    " Italic"
                };
                Some(InstalledFace {
                    family: family.clone(),
                    style: format!("{}{slant}", weight_name(info.weight.0)),
                    path: path.clone(),
                })
            })
            .collect();
        faces.sort_by(|a, b| (&a.family, &a.style, &a.path).cmp(&(&b.family, &b.style, &b.path)));
        faces.dedup_by(|a, b| a.family == b.family && a.style == b.style);
        faces
    }
    #[cfg(target_family = "wasm")]
    {
        Vec::new()
    }
}

/// Whose faces a text draws in: the engine's own project's, or those of the
/// project whose content it plays ([`balaur_core::project::ContentRoot`]). A
/// game run plays its own, so the two are one; the editor draws its panels in
/// its own faces and the game it edits in the game's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontProject {
    #[default]
    Host,
    Content,
}

impl FontProject {
    /// The project `entity` draws its text in: the content's when one is
    /// played and the node sits in the subtree playing it.
    #[must_use]
    pub fn of(eng: &Engine, entity: balaur_core::hecs::Entity) -> Self {
        Self::content_if(balaur_core::project::in_content(eng, entity))
    }

    /// The content's when `in_content`, else the host's.
    #[must_use]
    pub fn content_if(in_content: bool) -> Self {
        if in_content {
            Self::Content
        } else {
            Self::Host
        }
    }

    /// Both, the host's first.
    pub const BOTH: [Self; 2] = [Self::Host, Self::Content];

    /// A short word for the project, for a name that has to tell the two
    /// apart: egui's families.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Content => "content",
        }
    }
}

/// The directory a project's faces are read from, as `ProjectFiles` takes it.
fn fonts_directory(eng: &Engine, project: FontProject) -> String {
    match (project, balaur_core::project::content_root(eng)) {
        (FontProject::Content, Some(root)) => root.join("fonts").to_string_lossy().into_owned(),
        _ => "fonts".to_string(),
    }
}

/// The face files under a project's `fonts/`, in path order.
fn face_paths(eng: &Engine, project: FontProject) -> Vec<String> {
    let Some(files) = eng.try_resource::<ProjectFiles>() else {
        return Vec::new();
    };
    let mut paths: Vec<String> = files
        .borrow()
        .list(&fonts_directory(eng, project))
        .into_iter()
        .filter(|path| balaur_core::import::is_face(path))
        .collect();
    paths.sort();
    paths
}

/// What a project's faces were read from: each file's path, when it was
/// written and its import settings' stamp, and the default font its theme
/// names. Read again when it differs.
#[derive(Clone, Default, PartialEq)]
struct FacesRead {
    faces: Vec<(String, Option<f64>, String)>,
    default: String,
}

impl FacesRead {
    fn of(eng: &Engine, project: FontProject) -> Self {
        let Some(files) = eng.try_resource::<ProjectFiles>() else {
            return Self::default();
        };
        Self {
            faces: face_paths(eng, project)
                .into_iter()
                .map(|path| {
                    let written = files.borrow().mtime(&path);
                    let settings = balaur_core::import::resolved(eng, &path).stamp.clone();
                    (path, written, settings)
                })
                .collect(),
            default: default_font(eng, project),
        }
    }
}

/// One face file of a project, read with its import settings.
fn face_at(eng: &Engine, path: &str) -> Option<FontFace> {
    let files = eng.try_resource::<ProjectFiles>()?;
    let stem = std::path::Path::new(path).file_stem()?.to_str()?;
    let bytes = files.borrow().read(path).ok()?;
    let settings = balaur_core::import::resolved(eng, path);
    Some(FontFace {
        name: stem.to_string(),
        path: path.to_string(),
        system: false,
        bytes: Arc::new(bytes),
        tweak: FaceTweak::of(&settings.settings),
    })
}

/// Every face a project draws in, in fallback order: its own `fonts/`, the
/// system's when it wants them, then the face every build carries. Read once
/// here for both egui and the shaper.
pub fn faces_of(eng: &Engine, project: FontProject) -> Vec<FontFace> {
    let mut faces: Vec<FontFace> = face_paths(eng, project)
        .iter()
        .filter_map(|path| face_at(eng, path))
        .collect();
    if wants_system_fonts(eng) {
        faces.extend(system_faces());
    }
    // Last, and always: the default font where no theme names one, and what
    // draws when nothing else does — wasm reads no system face.
    faces.push(bundled_face());
    faces
}

/// Whether this project wants the operating system's faces appended, from
/// `[ui] system_fonts`. A project that says nothing gets them, so text in a
/// script balaur does not vendor keeps drawing.
fn wants_system_fonts(eng: &Engine) -> bool {
    balaur_core::project::UiSettings::from_settings(eng).system_fonts
}

/// The `widget_theme` a project starts its UI from: its `[ui] theme`, against
/// its own root when it is the played content's.
fn project_theme(eng: &Engine, project: FontProject) -> Option<String> {
    let root = match project {
        FontProject::Content => balaur_core::project::content_root(eng),
        FontProject::Host => None,
    };
    let Some(root) = root else {
        let theme = balaur_core::project::UiSettings::from_settings(eng).theme;
        return (!theme.is_empty()).then_some(theme);
    };
    let manifest = balaur_core::files::backend(eng)
        .read(&root.join("project.toml"))
        .ok()?;
    let manifest: toml::Value = toml::from_str(std::str::from_utf8(&manifest).ok()?).ok()?;
    let theme = manifest.get("ui")?.get("theme")?.as_str()?;
    (!theme.is_empty()).then(|| root.join(theme).to_string_lossy().into_owned())
}

/// The theme's key naming its fonts, and the one there naming the default.
pub const THEME_FONTS: &str = "fonts";
pub const DEFAULT_FONT: &str = "default";

/// The font a project's theme names its default, from its `[fonts]`; empty
/// where it names none, which is the face every build carries.
fn default_font(eng: &Engine, project: FontProject) -> String {
    project_theme(eng, project)
        .and_then(|theme| balaur_core::assets::duplicate_definition(eng, &theme).ok())
        .and_then(|theme| {
            theme
                .get(THEME_FONTS)?
                .get(DEFAULT_FONT)?
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// A font as the shaper draws it: the family its face goes by and how it is
/// shaped.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedFont {
    /// The family its face goes by, which a request asks the database for;
    /// weight and style pick among the faces sharing it. Empty for a bitmap
    /// face.
    pub family: String,
    /// The face's import `scale`, which sizes the line as well as its glyphs.
    pub scale: f32,
    /// The face's own weight: what 400, the regular a scene leaves unsaid,
    /// draws in this font, so a font naming a Semibold face draws Semibold.
    pub weight: u16,
    pub settings: FontSettings,
    /// The `.fnt` a bitmap font lays out with; `None` for an outline face.
    pub bitmap: Option<String>,
}

impl LoadedFont {
    /// The weight this font is asked for at `asked`.
    #[must_use]
    pub fn weight_for(&self, asked: u16) -> u16 {
        if asked == 400 { self.weight } else { asked }
    }
}

/// The fonts a project's texts have named, by reference, and the ones named
/// and not read yet. Apart from the font systems, so a shaping borrows both.
#[derive(Default)]
pub(crate) struct FontTable {
    loaded: HashMap<String, Rc<LoadedFont>>,
    /// Asked for and not read yet: the shaper answers with the default, and
    /// the shaper's next look at the files reads them.
    wanted: RefCell<Vec<String>>,
}

impl FontTable {
    /// The font `reference` names, or the default until it is read.
    pub(crate) fn of(&self, reference: &str) -> &LoadedFont {
        if let Some(found) = self.loaded.get(reference) {
            return found;
        }
        let mut wanted = self.wanted.borrow_mut();
        if !wanted.iter().any(|held| held == reference) {
            wanted.push(reference.to_string());
        }
        self.default_font()
    }

    /// The default font, which every table holds.
    pub(crate) fn default_font(&self) -> &LoadedFont {
        static NONE: std::sync::OnceLock<LoadedFont> = std::sync::OnceLock::new();
        self.loaded.get("").map_or_else(
            || {
                NONE.get_or_init(|| LoadedFont {
                    family: String::new(),
                    scale: 1.0,
                    weight: 400,
                    settings: FontSettings::default(),
                    bitmap: None,
                })
            },
            |found| found,
        )
    }
}

/// The faces one project ships and the system's behind them, as the font
/// systems its text is laid out by, and the fonts its texts have named. One
/// database a project, so a game's faces never stand in for the editor's,
/// nor the editor's for the game's.
pub struct ProjectFonts {
    /// Every face, in fallback order: own, system, the bundled one.
    faces: Vec<FontFace>,
    /// Every face, the system's included: what drawing falls back through.
    pub(crate) drawing: FontSystem,
    /// Each face's import settings, by the id `drawing` gave it.
    pub(crate) tweaks: Tweaks,
    /// The face each id of `drawing` came from, as an index into `faces`.
    face_of_id: HashMap<fontdb::ID, usize>,
    pub(crate) table: FontTable,
    /// The rasteriser's cache, keyed by the ids `drawing` gave: one a project.
    pub(crate) swash: cosmic_text::SwashCache,
    /// The project's own faces alone, built the first time something
    /// measures. Separate from `drawing` on purpose: drawing may fall back to
    /// whatever the machine has, and a number that reaches a script may not.
    strict: Option<(FontSystem, Tweaks)>,
    locale: String,
    /// What the faces were read from; empty for faces handed in whole.
    read: FacesRead,
    /// The asset generation `read` was last compared under.
    compared_at: u64,
    /// Bumped whenever a font is read, so egui takes the families again.
    pub(crate) fonts_read: u64,
}

impl ProjectFonts {
    /// Over `faces`, in fallback order, the first of them the default font.
    #[must_use]
    pub fn new(faces: &[FontFace], locale: &str) -> Self {
        let (drawing, tweaks, face_of_id) = system_of(faces, locale);
        let mut fonts = Self {
            faces: faces.to_vec(),
            drawing,
            tweaks,
            face_of_id,
            table: FontTable::default(),
            swash: cosmic_text::SwashCache::new(),
            strict: None,
            locale: locale.to_string(),
            read: FacesRead::default(),
            compared_at: 0,
            fonts_read: 0,
        };
        if let Some(first) = faces.iter().position(|face| !face.system)
            && let Some(default) = fonts.outline(first, FontSettings::default())
        {
            fonts.table.loaded.insert(String::new(), Rc::new(default));
        }
        fonts
    }

    /// A project's faces, read from its `fonts/`, with the default font its
    /// theme names.
    #[must_use]
    pub fn read(eng: &Engine, project: FontProject) -> Self {
        let faces = faces_of(eng, project);
        let mut fonts = Self::new(&faces, &balaur_core::strings::locale(eng));
        fonts.read = FacesRead::of(eng, project);
        fonts.compared_at = balaur_core::assets::generation(eng);
        // Where no theme names one, the face every build carries is the default.
        if let Some(bundled) = fonts
            .faces
            .iter()
            .rposition(|face| face.path.is_empty() && !face.system)
            && let Some(default) = fonts.outline(bundled, FontSettings::default())
        {
            fonts.table.loaded.insert(String::new(), Rc::new(default));
        }
        let named = fonts.read.default.clone();
        if !named.is_empty() {
            match fonts.resolve(eng, &named) {
                Ok(default) => {
                    fonts.table.loaded.insert(String::new(), Rc::new(default));
                }
                Err(err) => tracing::warn!("default font '{named}': {err:#}"),
            }
        }
        fonts
    }

    /// Whether a face was added, removed, rewritten or given new import
    /// settings, or the theme names another default, since these were read.
    /// Looked at only when an asset moved.
    pub(crate) fn moved(&mut self, eng: &Engine, project: FontProject) -> bool {
        let generation = balaur_core::assets::generation(eng);
        if generation == self.compared_at {
            return false;
        }
        self.compared_at = generation;
        if FacesRead::of(eng, project) != self.read {
            return true;
        }
        // A definition file may have been saved: the fonts it names are read
        // again when next asked for. An inline one is filed by its content.
        let before = self.table.loaded.len();
        self.table.loaded.retain(|reference, _| {
            reference.is_empty()
                || reference.starts_with("#!")
                || balaur_core::import::kind_of(reference).is_some()
        });
        if self.table.loaded.len() != before {
            self.fonts_read += 1;
        }
        false
    }

    /// Every face, in fallback order: own, system, the bundled one.
    #[must_use]
    pub fn faces(&self) -> &[FontFace] {
        &self.faces
    }

    /// Every font read so far, by the reference that named it; `""` is the
    /// default.
    pub fn loaded(&self) -> impl Iterator<Item = (&str, &LoadedFont)> {
        self.table
            .loaded
            .iter()
            .map(|(reference, font)| (reference.as_str(), font.as_ref()))
    }

    /// The face `font` draws in at `weight`, upright or italic: CSS font
    /// matching over the faces sharing its family, as the shaper does it.
    /// An index into [`Self::faces`], and whether that face is italic; `None`
    /// for a bitmap font.
    #[must_use]
    pub fn face_for(&self, font: &LoadedFont, weight: u16, italic: bool) -> Option<(usize, bool)> {
        if font.family.is_empty() {
            return None;
        }
        let db = self.drawing.db();
        let id = db.query(&fontdb::Query {
            families: &[fontdb::Family::Name(&font.family)],
            weight: fontdb::Weight(font.weight_for(weight)),
            stretch: font.settings.stretch.cosmic(),
            style: if italic {
                fontdb::Style::Italic
            } else {
                fontdb::Style::Normal
            },
        })?;
        let slanted = db
            .face(id)
            .is_some_and(|info| info.style != fontdb::Style::Normal);
        Some((*self.face_of_id.get(&id)?, slanted))
    }

    /// The system a measurement shapes with: the project's own faces only.
    pub(crate) fn measuring(&mut self) -> (&mut FontSystem, &Tweaks, &FontTable) {
        let (system, tweaks) = self.strict.get_or_insert_with(|| {
            let own: Vec<FontFace> = self
                .faces
                .iter()
                .filter(|face| !face.system)
                .cloned()
                .collect();
            let (system, tweaks, _) = system_of(&own, &self.locale);
            (system, tweaks)
        });
        (system, tweaks, &self.table)
    }

    /// Put face `face` under `reference` with `settings`, as a definition
    /// naming it would: a test's font with no asset layer to read it from.
    #[cfg(test)]
    pub(crate) fn define(&mut self, reference: &str, face: usize, settings: FontSettings) {
        if let Some(font) = self.outline(face, settings) {
            self.table
                .loaded
                .insert(reference.to_string(), Rc::new(font));
        }
    }

    /// The references named and not read yet, taken.
    pub(crate) fn take_wanted(&self) -> Vec<String> {
        std::mem::take(&mut *self.table.wanted.borrow_mut())
    }

    /// Read the font `reference` names into the table. A reference that does
    /// not read is said once and drawn in the default from then on.
    pub(crate) fn load(&mut self, eng: &Engine, reference: &str) -> Rc<LoadedFont> {
        if let Some(found) = self.table.loaded.get(reference) {
            return Rc::clone(found);
        }
        let font = match self.resolve(eng, reference) {
            Ok(font) => Rc::new(font),
            Err(err) => {
                if balaur_core::logbuf::first_time("font", reference) {
                    tracing::warn!("font '{reference}': {err:#}");
                }
                Rc::new(self.table.default_font().clone())
            }
        };
        self.table
            .loaded
            .insert(reference.to_string(), Rc::clone(&font));
        self.fonts_read += 1;
        font
    }

    /// What `reference` draws in: its definition's face, read into both
    /// systems when it is not among the project's own, and its settings.
    fn resolve(&mut self, eng: &Engine, reference: &str) -> anyhow::Result<LoadedFont> {
        let font: Rc<Font> = balaur_core::assets::load_typed(eng, reference)?;
        if font.source.is_empty() {
            return Ok(LoadedFont {
                settings: font.settings.clone(),
                ..self.table.default_font().clone()
            });
        }
        let bitmap = std::path::Path::new(&font.source)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("fnt"));
        if bitmap {
            return Ok(LoadedFont {
                family: String::new(),
                scale: 1.0,
                weight: 400,
                settings: font.settings.clone(),
                bitmap: Some(font.source.clone()),
            });
        }
        let index = match self.faces.iter().position(|face| face.path == font.source) {
            Some(index) => index,
            None => self.add_face(eng, &font.source)?,
        };
        self.outline(index, font.settings.clone())
            .ok_or_else(|| anyhow::anyhow!("'{}' holds no face", font.source))
    }

    /// Read a face that is not under the project's `fonts/` into both
    /// systems, behind the project's own.
    fn add_face(&mut self, eng: &Engine, path: &str) -> anyhow::Result<usize> {
        let face =
            face_at(eng, path).ok_or_else(|| anyhow::anyhow!("no face can be read at '{path}'"))?;
        let ids = self
            .drawing
            .db_mut()
            .load_font_source(fontdb::Source::Binary(shared(&face.bytes)));
        if ids.is_empty() {
            anyhow::bail!("'{path}' is not a face");
        }
        let index = self.faces.len();
        self.tweaks.add(&ids, face.tweak);
        for id in ids {
            self.face_of_id.insert(id, index);
        }
        if let Some((strict, tweaks)) = self.strict.as_mut() {
            let ids = strict
                .db_mut()
                .load_font_source(fontdb::Source::Binary(shared(&face.bytes)));
            tweaks.add(&ids, face.tweak);
        }
        self.faces.push(face);
        Ok(index)
    }

    /// The font face `index` draws as, shaped with `settings`.
    fn outline(&self, index: usize, settings: FontSettings) -> Option<LoadedFont> {
        let face = self.faces.get(index)?;
        let (id, _) = self
            .face_of_id
            .iter()
            .filter(|(_, held)| **held == index)
            .min_by_key(|(id, _)| **id)?;
        let info = self.drawing.db().face(*id)?;
        let (family, _) = info.families.first()?;
        Some(LoadedFont {
            family: family.clone(),
            scale: face.tweak.scale,
            weight: info.weight.0,
            settings,
            bitmap: None,
        })
    }
}

/// A face's bytes as fontdb takes them, shared rather than copied.
fn shared(bytes: &Arc<Vec<u8>>) -> Arc<dyn AsRef<[u8]> + Send + Sync> {
    let shared: Arc<Vec<u8>> = Arc::clone(bytes);
    shared
}

/// The faces, in order, as the fallback list: what the engine means by "the
/// next font" is what cosmic-text asks this for.
struct FaceFallback {
    families: Vec<&'static str>,
}

impl cosmic_text::Fallback for FaceFallback {
    fn common_fallback(&self) -> &[&'static str] {
        &self.families
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        &[]
    }

    fn script_fallback(&self, _: unicode_script::Script, _: &str) -> &[&'static str] {
        &[]
    }
}

/// One font system over `faces`, with those faces as the fallback list, each
/// face's import settings by the id it was given there, and the face each id
/// came from.
fn system_of(faces: &[FontFace], locale: &str) -> (FontSystem, Tweaks, HashMap<fontdb::ID, usize>) {
    let mut db = fontdb::Database::new();
    let mut families: Vec<&'static str> = Vec::new();
    let mut face_of_id = HashMap::new();
    let mut tweaks = Tweaks::default();
    for (index, face) in faces.iter().enumerate() {
        let ids = db.load_font_source(fontdb::Source::Binary(shared(&face.bytes)));
        tweaks.add(&ids, face.tweak);
        for id in ids {
            face_of_id.insert(id, index);
            let Some((name, _)) = db.face(id).and_then(|info| info.families.first()) else {
                continue;
            };
            // The shaper's fallback list wants `'static`; a font set lives
            // as long as the process, so the leak is the family's lifetime.
            let leaked: &'static str = Box::leak(name.clone().into_boxed_str());
            if !families.contains(&leaked) {
                families.push(leaked);
            }
        }
    }
    let system = FontSystem::new_with_locale_and_db_and_fallback(
        locale.to_string(),
        db,
        FaceFallback { families },
    );
    (system, tweaks, face_of_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(name: &str, bytes: &'static [u8]) -> FontFace {
        FontFace {
            name: name.into(),
            path: format!("fonts/{name}.ttf"),
            system: false,
            bytes: Arc::new(bytes.to_vec()),
            tweak: FaceTweak::default(),
        }
    }

    /// A font's weight is picked among the faces of its family, the way the
    /// shaper picks it, and 400 is the font's own face.
    #[test]
    fn a_heavier_weight_picks_the_heavier_face_of_the_family() {
        let fonts = ProjectFonts::new(
            &[
                face(
                    "SourceSans3-Regular",
                    include_bytes!("../../../editor/fonts/SourceSans3-Regular.ttf"),
                ),
                face(
                    "SourceSans3-Semibold",
                    include_bytes!("../../../editor/fonts/SourceSans3-Semibold.ttf"),
                ),
                face(
                    "JetBrainsMono-Regular",
                    include_bytes!("../../../editor/fonts/JetBrainsMono-Regular.ttf"),
                ),
            ],
            "en-US",
        );
        let regular = fonts.table.default_font().clone();
        let semibold = fonts.outline(1, FontSettings::default()).unwrap();
        let mono = fonts.outline(2, FontSettings::default()).unwrap();
        assert_eq!(fonts.face_for(&regular, 400, false), Some((0, false)));
        assert_eq!(fonts.face_for(&regular, 700, false), Some((1, false)));
        assert_eq!(fonts.face_for(&semibold, 300, false), Some((0, false)));
        // 400 is the font's own face, which egui draws it in.
        assert_eq!(fonts.face_for(&semibold, 400, false), Some((1, false)));
        assert_eq!(fonts.face_for(&mono, 700, false), Some((2, false)));
        // No italic face ships, so the upright one answers and says so.
        assert_eq!(fonts.face_for(&regular, 400, true), Some((0, false)));
    }

    /// A desktop lists the faces it has installed, each a file a picker can
    /// copy; the test machines all carry some.
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn a_desktop_lists_its_installed_faces_by_family_and_style() {
        let faces = installed_faces();
        assert!(!faces.is_empty(), "no face installed");
        assert!(faces.iter().all(|face| face.path.is_file()));
        assert!(faces.iter().any(|face| face.style == "Regular"));
    }

    /// A measurement shapes over the project's own faces alone, and wasm reads
    /// no OS face: with no `fonts/` of its own that set was empty and aborted.
    #[test]
    fn a_project_with_no_fonts_of_its_own_still_measures() {
        let faces = faces_of(&Engine::new(), FontProject::Host);
        assert!(
            faces.iter().any(|face| !face.system),
            "nothing outside the system's faces to shape with"
        );
        let size = crate::TextState::new(&faces, "en-US")
            .measure(&crate::Request::new("measure me", 24.0));
        assert!(size.x > 0.0, "measured no width");
    }
}
