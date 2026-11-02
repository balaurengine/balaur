//! The operating system's fonts, through `fontique`: which of its faces draws
//! a character a project's own have no glyph for, and what a picker lists.
//!
//! Reading them takes every font on the machine on macOS, iOS and Android, tens
//! of milliseconds on a warm disk, so a worker does it once the first frame is
//! out ([`read_ahead`]). Nothing else here runs until a text needs a face.

use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};
use unicode_script::{Script, UnicodeScript};

/// One face the operating system has installed, as a picker lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledFace {
    pub family: String,
    /// Its weight and slant in words: `Regular`, `Bold Italic`.
    pub style: String,
    pub path: std::path::PathBuf,
}

/// Whether `c` is drawn by a glyph of its own, so that no face having one is
/// worth asking about: not a control, format, private or unassigned character,
/// not a space the shaper makes from another, not a mark its base's face draws.
pub(crate) fn wants_a_face(c: char) -> bool {
    !matches!(
        c.general_category_group(),
        GeneralCategoryGroup::Other | GeneralCategoryGroup::Separator
    ) && c.script() != Script::Inherited
}

/// Whether the face at `index` of a font file has a glyph for `c`.
pub(crate) fn draws(file: &[u8], index: usize, c: char) -> bool {
    swash::FontRef::from_index(file, index).is_some_and(|font| font.charmap().map(c) != 0)
}

/// The language to ask for a face in: the writing beside a Han character says
/// more than the locale does, since Japanese and Korean draw those in faces of
/// their own, and a face asked for its Han alone would split a word in two.
pub(crate) fn language_of<'a>(text: &str, locale: &'a str) -> &'a str {
    for c in text.chars() {
        match c.script() {
            Script::Hiragana | Script::Katakana => return "ja",
            Script::Hangul => return "ko",
            _ => {}
        }
    }
    locale
}

#[cfg(not(target_family = "wasm"))]
pub use native::installed_faces;
#[cfg(not(target_family = "wasm"))]
pub(crate) use native::{face_for, read_ahead};

/// Every face installed; a browser has no fonts to read.
#[cfg(target_family = "wasm")]
#[must_use]
pub fn installed_faces() -> Vec<InstalledFace> {
    Vec::new()
}

/// A browser has no face files to map.
#[cfg(target_family = "wasm")]
pub(crate) fn face_for(_: char, _: &str) -> Option<crate::fonts::FontFace> {
    None
}

/// A browser has no fonts to read ahead.
#[cfg(target_family = "wasm")]
pub(crate) fn read_ahead() {}

#[cfg(not(target_family = "wasm"))]
mod native {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

    use fontique::{
        Collection, CollectionOptions, FallbackKey, FamilyId, FontStyle, GenericFamily, SourceKind,
    };
    use unicode_properties::{EmojiStatus, UnicodeEmoji};
    use unicode_script::UnicodeScript;

    use super::{InstalledFace, draws};
    use crate::fonts::{FaceBytes, FaceTweak, FontFace};

    /// How many times a character may send the search through every file on
    /// the machine and find nothing: a text full of what no face has would
    /// otherwise pay for that once per character.
    const FRUITLESS_SEARCHES: u32 = 8;

    /// What the operating system answers with, read once for the life of the
    /// process, and the files mapped so far, one mapping each however many
    /// projects and egui hold them.
    struct SystemFonts {
        fonts: Collection,
        files: HashMap<PathBuf, FontFace>,
        fruitless_searches: u32,
    }

    fn system_fonts() -> MutexGuard<'static, SystemFonts> {
        static READ: OnceLock<Mutex<SystemFonts>> = OnceLock::new();
        READ.get_or_init(|| {
            Mutex::new(SystemFonts {
                fonts: Collection::new(CollectionOptions {
                    shared: false,
                    system_fonts: true,
                }),
                files: HashMap::new(),
                fruitless_searches: 0,
            })
        })
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
    }

    /// The system's face for `c`, in `language` where its script cares.
    pub(crate) fn face_for(c: char, language: &str) -> Option<FontFace> {
        system_fonts().face_for(c, language)
    }

    /// A letter of each script most text is written in: Han, kana, Hangul,
    /// Arabic, Hebrew, Devanagari, Bengali, Tamil and Thai. The operating system
    /// takes milliseconds to answer for a script the first time.
    const ASKED_AHEAD: [char; 10] = ['今', 'あ', 'ア', '한', 'ع', 'ש', 'ह', 'ব', 'த', 'ก'];

    /// Read the system's fonts on a worker, and have it ask for the usual
    /// scripts. Nothing waits on it and it reports nothing: a character asking
    /// meanwhile waits in [`system_fonts`] and gets the same face, so what a
    /// frame draws never depends on when this finishes.
    pub(crate) fn read_ahead() {
        drop(balaur_core::task::compute(|| {
            for c in ASKED_AHEAD {
                // A lock per question, so a frame asking waits for one at most.
                system_fonts().preferred(c, "");
            }
        }));
    }

    /// Whether `c` draws as an emoji unless a text says otherwise; `☀` draws
    /// as a symbol and only falls to the emoji face where no symbol has it.
    fn presents_as_emoji(c: char) -> bool {
        matches!(
            c.emoji_status(),
            EmojiStatus::EmojiPresentation
                | EmojiStatus::EmojiPresentationAndModifierBase
                | EmojiStatus::EmojiPresentationAndEmojiComponent
                | EmojiStatus::EmojiPresentationAndModifierAndEmojiComponent
        )
    }

    /// A leading dot is Apple's mark for a family no application may ask for.
    /// Among them is `.LastResort`, which claims every character and draws each
    /// as a box: taken up once, no later character would ask for a real face.
    fn for_applications(family: &str) -> bool {
        !family.starts_with('.')
    }

    impl SystemFonts {
        fn face_for(&mut self, c: char, language: &str) -> Option<FontFace> {
            let emoji: Vec<FamilyId> = self.fonts.generic_families(GenericFamily::Emoji).collect();
            if presents_as_emoji(c)
                && let Some(face) = self.first_drawing(&emoji, c)
            {
                return Some(face);
            }
            let preferred = self.preferred(c, language);
            if let Some(face) = self.first_drawing(&preferred, c) {
                return Some(face);
            }
            if self.fruitless_searches < FRUITLESS_SEARCHES {
                let mut every = self.families();
                every.retain(|family| !emoji.contains(family));
                if let Some(face) = self.first_drawing(&every, c) {
                    return Some(face);
                }
                self.fruitless_searches += 1;
            }
            self.first_drawing(&emoji, c)
        }

        /// The family the system draws `c`'s script in, then its everyday
        /// faces: a symbol belongs to no script, and these hold most of them.
        fn preferred(&mut self, c: char, language: &str) -> Vec<FamilyId> {
            let mut families = Vec::new();
            if let Ok(script) = <[u8; 4]>::try_from(c.script().short_name().as_bytes()) {
                let script = fontique::Script::from_bytes(script);
                families.extend(
                    self.fonts
                        .fallback_families(FallbackKey::from((script, language))),
                );
            }
            for everyday in [
                GenericFamily::SystemUi,
                GenericFamily::SansSerif,
                GenericFamily::Math,
            ] {
                families.extend(self.fonts.generic_families(everyday));
            }
            families
        }

        /// Every family an application may ask for, by name, so a character
        /// lands in the same face each run.
        fn families(&mut self) -> Vec<FamilyId> {
            let ids: Vec<FamilyId> = self.fonts.family_ids().collect();
            let mut named: Vec<(String, FamilyId)> = ids
                .into_iter()
                .filter_map(|id| Some((self.fonts.family_name(id)?.to_string(), id)))
                .filter(|(name, _)| for_applications(name))
                .collect();
            named.sort();
            named.into_iter().map(|(_, id)| id).collect()
        }

        /// The first of `families` with a glyph for `c`, as its file.
        fn first_drawing(&mut self, families: &[FamilyId], c: char) -> Option<FontFace> {
            families.iter().find_map(|id| {
                let family = self
                    .fonts
                    .family(*id)
                    .filter(|family| for_applications(family.name()))?;
                let font = family.default_font()?;
                let SourceKind::Path(path) = font.source().kind() else {
                    return None;
                };
                let index = font.index() as usize;
                if let Some(held) = self.files.get(&**path) {
                    return draws((*held.bytes).as_ref(), index, c).then(|| held.clone());
                }
                let bytes = mapped(path)?;
                if !draws((*bytes).as_ref(), index, c) {
                    return None;
                }
                let face = FontFace {
                    name: format!("system:{}", path.display()),
                    path: String::new(),
                    system: true,
                    bytes,
                    tweak: FaceTweak::default(),
                };
                tracing::debug!("{c:?} draws in {} from {}", family.name(), path.display());
                self.files.insert(path.to_path_buf(), face.clone());
                Some(face)
            })
        }
    }

    /// One of the system's font files, mapped rather than read: they are the
    /// largest files on the machine, the emoji collection alone two hundred
    /// megabytes, and a page loads only when a glyph on it is drawn.
    fn mapped(path: &Path) -> Option<FaceBytes> {
        let file = std::fs::File::open(path).ok()?; // os files: the system's own faces
        // SAFETY: the operating system's own face, under its fonts directory,
        // which nothing rewrites while a process holds it open.
        let map = unsafe { memmap2::Mmap::map(&file) }.ok()?;
        Some(Arc::new(map))
    }

    /// The name CSS gives a weight.
    fn weight_name(weight: f32) -> &'static str {
        match weight as u16 {
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
    /// file each: what a picker on a desktop offers to copy into a project,
    /// since an exported game cannot count on a face being installed.
    #[must_use]
    pub fn installed_faces() -> Vec<InstalledFace> {
        let mut system = system_fonts();
        let mut faces = Vec::new();
        for id in system.families() {
            let Some(family) = system.fonts.family(id) else {
                continue;
            };
            for font in family.fonts() {
                let SourceKind::Path(path) = font.source().kind() else {
                    continue;
                };
                let slant = if font.style() == FontStyle::Normal {
                    ""
                } else {
                    " Italic"
                };
                faces.push(InstalledFace {
                    family: family.name().to_string(),
                    style: format!("{}{slant}", weight_name(font.weight().value())),
                    path: path.to_path_buf(),
                });
            }
        }
        faces.sort_by(|a, b| (&a.family, &a.style, &a.path).cmp(&(&b.family, &b.style, &b.path)));
        faces.dedup_by(|a, b| a.family == b.family && a.style == b.style);
        faces
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn asking_while_the_worker_reads_waits_for_it_and_gets_the_same_face() {
        read_ahead();
        let during = face_for('日', "ja").map(|face| face.name);
        read_ahead();
        let after = face_for('日', "ja").map(|face| face.name);
        assert_eq!(during, after);
    }

    #[test]
    fn han_beside_kana_or_hangul_asks_in_that_language_and_alone_in_the_locale() {
        assert_eq!(language_of("日本語のテキスト", "en-US"), "ja");
        assert_eq!(language_of("한글 漢字", "en-US"), "ko");
        assert_eq!(language_of("漢字", "zh-TW"), "zh-TW");
    }

    #[test]
    fn only_a_character_with_a_glyph_of_its_own_wants_a_face() {
        for wants in ['日', 'ש', '→', '😀'] {
            assert!(wants_a_face(wants), "{wants:?}");
        }
        // A control, a joiner, a variation selector, a private and an
        // unassigned character, and a space.
        for not in [
            '\u{7}', '\u{200d}', '\u{fe0f}', '\u{e000}', '\u{378}', '\u{3000}',
        ] {
            assert!(!wants_a_face(not), "{not:?}");
        }
    }
}
