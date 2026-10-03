//! What a block is shaped with past its face and size: cosmic-text's own
//! settings, each read from the word a scene spells it with.

use egui::Color32;

use crate::vocabulary::words as w;

/// Upright, an italic face, or the upright face slanted. cosmic-text slants
/// a face that has no italic of its own by 14° for either.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Slant {
    #[default]
    Normal,
    Italic,
    Oblique,
}

impl Slant {
    /// The slant a word names; anything else is upright.
    #[must_use]
    pub fn of(word: &str) -> Self {
        match word {
            w::ITALIC => Self::Italic,
            w::OBLIQUE => Self::Oblique,
            _ => Self::Normal,
        }
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Normal => w::NORMAL,
            Self::Italic => w::ITALIC,
            Self::Oblique => w::OBLIQUE,
        }
    }

    pub(crate) fn style(self) -> cosmic_text::Style {
        match self {
            Self::Normal => cosmic_text::Style::Normal,
            Self::Italic => cosmic_text::Style::Italic,
            Self::Oblique => cosmic_text::Style::Oblique,
        }
    }

    /// Whether a face is asked to lean at all.
    #[must_use]
    pub fn leans(self) -> bool {
        self != Self::Normal
    }
}

/// How wide a face is picked, among the faces a family ships; nothing is
/// stretched that no face draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Stretch {
    UltraCondensed,
    ExtraCondensed,
    Condensed,
    SemiCondensed,
    #[default]
    Normal,
    SemiExpanded,
    Expanded,
    ExtraExpanded,
    UltraExpanded,
}

impl Stretch {
    const ALL: [Self; 9] = [
        Self::UltraCondensed,
        Self::ExtraCondensed,
        Self::Condensed,
        Self::SemiCondensed,
        Self::Normal,
        Self::SemiExpanded,
        Self::Expanded,
        Self::ExtraExpanded,
        Self::UltraExpanded,
    ];

    #[must_use]
    pub fn of(word: &str) -> Self {
        w::FONT_STRETCHES
            .iter()
            .position(|known| *known == word)
            .map_or(Self::Normal, |at| Self::ALL[at])
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        w::FONT_STRETCHES[self as usize]
    }

    pub(crate) fn cosmic(self) -> cosmic_text::Stretch {
        use cosmic_text::Stretch as S;
        [
            S::UltraCondensed,
            S::ExtraCondensed,
            S::Condensed,
            S::SemiCondensed,
            S::Normal,
            S::SemiExpanded,
            S::Expanded,
            S::ExtraExpanded,
            S::UltraExpanded,
        ][self as usize]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
}

impl Underline {
    #[must_use]
    pub fn of(word: &str) -> Self {
        match word {
            w::SINGLE => Self::Single,
            w::DOUBLE => Self::Double,
            _ => Self::None,
        }
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::None => w::NONE,
            Self::Single => w::SINGLE,
            Self::Double => w::DOUBLE,
        }
    }
}

/// Where a wrapped line may break.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineBreak {
    /// Between words, and inside a word too long for a line of its own.
    #[default]
    WordOrGlyph,
    Word,
    Glyph,
}

impl LineBreak {
    #[must_use]
    pub fn of(word: &str) -> Self {
        match word {
            w::WORD => Self::Word,
            w::GLYPH => Self::Glyph,
            _ => Self::WordOrGlyph,
        }
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::WordOrGlyph => w::WORD_OR_GLYPH,
            Self::Word => w::WORD,
            Self::Glyph => w::GLYPH,
        }
    }

    pub(crate) fn wrap(self) -> cosmic_text::Wrap {
        match self {
            Self::WordOrGlyph => cosmic_text::Wrap::WordOrGlyph,
            Self::Word => cosmic_text::Wrap::Word,
            Self::Glyph => cosmic_text::Wrap::Glyph,
        }
    }
}

/// Which part of a cut line the ellipsis stands in for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TruncateAt {
    #[default]
    End,
    Start,
    Middle,
}

impl TruncateAt {
    #[must_use]
    pub fn of(word: &str) -> Self {
        match word {
            w::START => Self::Start,
            w::MIDDLE => Self::Middle,
            _ => Self::End,
        }
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::End => w::END,
            Self::Start => w::START,
            Self::Middle => w::MIDDLE,
        }
    }
}

/// The shaper's two strategies: `Simple` is cosmic-text's basic shaping,
/// which neither joins complex scripts nor falls back to another face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Shaping {
    #[default]
    Complex,
    Simple,
}

impl Shaping {
    #[must_use]
    pub fn of(word: &str) -> Self {
        if word == w::SIMPLE {
            Self::Simple
        } else {
            Self::Complex
        }
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Complex => w::COMPLEX,
            Self::Simple => w::SIMPLE,
        }
    }

    pub(crate) fn cosmic(self) -> cosmic_text::Shaping {
        match self {
            Self::Complex => cosmic_text::Shaping::Advanced,
            Self::Simple => cosmic_text::Shaping::Basic,
        }
    }
}

/// Whether swash hints a glyph's outline: `Auto` takes the face's import
/// `hinting`, which hints unless it says `false`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Hinting {
    #[default]
    Auto,
    On,
    Off,
}

impl Hinting {
    #[must_use]
    pub fn of(word: &str) -> Self {
        match word {
            w::ON => Self::On,
            w::OFF => Self::Off,
            _ => Self::Auto,
        }
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Auto => w::AUTO,
            Self::On => w::ON,
            Self::Off => w::OFF,
        }
    }

    /// Whether a glyph of a face whose import says `face` is hinted.
    #[must_use]
    pub fn hints(self, face: Option<bool>) -> bool {
        match self {
            Self::Auto => face != Some(false),
            Self::On => true,
            Self::Off => false,
        }
    }
}

/// One OpenType feature: its four-letter tag and the value it is set to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Feature {
    pub tag: [u8; 4],
    pub value: u32,
}

impl Feature {
    /// `"smcp"` turns a feature on, `"liga=0"` off, `"salt=2"` picks an
    /// alternate. `None` for a tag that is not four ASCII characters.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (tag, value) = match text.trim().split_once('=') {
            Some((tag, value)) => (tag.trim(), value.trim().parse().ok()?),
            None => (text.trim(), 1),
        };
        let tag: [u8; 4] = tag.as_bytes().try_into().ok()?;
        tag.iter()
            .all(u8::is_ascii_graphic)
            .then_some(Self { tag, value })
    }
}

/// The lines drawn with the glyphs, and their colours: `None` takes the
/// colour of the glyphs under them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Decoration {
    pub underline: Underline,
    pub underline_color: Option<Color32>,
    pub strikethrough: bool,
    pub strikethrough_color: Option<Color32>,
    pub overline: bool,
    pub overline_color: Option<Color32>,
}

/// Every shaping setting past the face and size, as one value a request
/// borrows: [`PLAIN`] is what a block asks for when it says nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub stretch: Stretch,
    /// A family by name, ahead of the chain; empty shapes with the chain.
    pub font_name: String,
    pub features: Vec<Feature>,
    pub decoration: Decoration,
    pub line_break: LineBreak,
    pub truncate_at: TruncateAt,
    /// The most lines a block keeps; 0 keeps every line.
    pub max_lines: u32,
    /// The height lines stop at, in the request's pixels.
    pub max_height: Option<f32>,
    pub shaping: Shaping,
    /// Round each advance to a whole pixel while laying out.
    pub snap_advances: bool,
    pub hinting: Hinting,
    /// Rasterise on the pixel grid with no subpixel offset, for a pixel face.
    pub pixel_snap: bool,
    /// Resize a monospace face's glyphs to this advance, in pixels.
    pub monospace_width: Option<f32>,
    /// Spaces between tab stops.
    pub tab_width: u16,
}

pub static PLAIN: Options = Options {
    stretch: Stretch::Normal,
    font_name: String::new(),
    features: Vec::new(),
    decoration: Decoration {
        underline: Underline::None,
        underline_color: None,
        strikethrough: false,
        strikethrough_color: None,
        overline: false,
        overline_color: None,
    },
    line_break: LineBreak::WordOrGlyph,
    truncate_at: TruncateAt::End,
    max_lines: 0,
    max_height: None,
    shaping: Shaping::Complex,
    snap_advances: false,
    hinting: Hinting::Auto,
    pixel_snap: false,
    monospace_width: None,
    tab_width: 8,
};

impl Default for Options {
    fn default() -> Self {
        PLAIN.clone()
    }
}

impl Options {
    /// Everything that changes the picture, fed to a cache key.
    pub(crate) fn hash_into(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash as _;
        let colour = |c: Option<Color32>| c.map(|c| c.to_array());
        let d = &self.decoration;
        (self.stretch, &self.font_name, &self.features).hash(hasher);
        (d.underline, colour(d.underline_color), d.strikethrough).hash(hasher);
        (
            colour(d.strikethrough_color),
            d.overline,
            colour(d.overline_color),
        )
            .hash(hasher);
        (self.line_break, self.truncate_at, self.max_lines).hash(hasher);
        self.max_height.map(f32::to_bits).hash(hasher);
        (
            self.shaping,
            self.snap_advances,
            self.hinting,
            self.pixel_snap,
        )
            .hash(hasher);
        self.monospace_width.map(f32::to_bits).hash(hasher);
        self.tab_width.hash(hasher);
    }

    /// The features as cosmic-text takes them.
    pub(crate) fn font_features(&self) -> cosmic_text::FontFeatures {
        let mut features = cosmic_text::FontFeatures::new();
        for feature in &self.features {
            features.set(cosmic_text::FeatureTag::new(&feature.tag), feature.value);
        }
        features
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_feature_reads_its_tag_and_an_optional_value() {
        assert_eq!(
            Feature::parse("smcp"),
            Some(Feature {
                tag: *b"smcp",
                value: 1
            })
        );
        assert_eq!(
            Feature::parse("liga=0"),
            Some(Feature {
                tag: *b"liga",
                value: 0
            })
        );
        assert_eq!(Feature::parse("toolong"), None);
        assert_eq!(Feature::parse("ss01=x"), None);
    }

    #[test]
    fn every_word_reads_back_as_itself() {
        for word in w::FONT_STRETCHES {
            assert_eq!(Stretch::of(word).word(), *word);
        }
        for word in w::FONT_STYLES {
            assert_eq!(Slant::of(word).word(), *word);
        }
        for word in w::UNDERLINES {
            assert_eq!(Underline::of(word).word(), *word);
        }
        for word in w::LINE_BREAKS {
            assert_eq!(LineBreak::of(word).word(), *word);
        }
        for word in w::TRUNCATE_ATS {
            assert_eq!(TruncateAt::of(word).word(), *word);
        }
        for word in w::SHAPINGS {
            assert_eq!(Shaping::of(word).word(), *word);
        }
        for word in w::HINTINGS {
            assert_eq!(Hinting::of(word).word(), *word);
        }
    }

    #[test]
    fn auto_hinting_follows_the_face_and_a_word_overrides_it() {
        assert!(Hinting::Auto.hints(None));
        assert!(!Hinting::Auto.hints(Some(false)));
        assert!(Hinting::On.hints(Some(false)));
        assert!(!Hinting::Off.hints(None));
    }
}
