//! The words a block's shaping settings take, as `text2d`, `text3d` and the
//! widget spell them, and the keys of the `font` asset, whose schema this
//! crate writes. Each crate that writes a schema keeps its own copy of the
//! keys; the words are read back into the shaper's types here.

/// The `font` and `text_style` assets' rows.
pub mod keys {
    pub const SOURCE: &str = balaur_core::assets::SOURCE;
    pub const FEATURES: &str = "features";
    pub const STRETCH: &str = "stretch";
    pub const SHAPING: &str = "shaping";
    pub const HINTING: &str = "hinting";
    pub const PIXEL_SNAP: &str = "pixel_snap";
    pub const SNAP_ADVANCES: &str = "snap_advances";
    pub const MONOSPACE_WIDTH: &str = "monospace_width";

    // The `text_style` asset's rows.
    pub const LINE_HEIGHT: &str = "line_height";
    pub const LETTER_SPACING: &str = "letter_spacing";
    pub const OUTLINE_SIZE: &str = "outline_size";
    pub const OUTLINE_COLOR: &str = "outline_color";
    pub const SHADOW_COLOR: &str = "shadow_color";
    pub const SHADOW_OFFSET_X: &str = "shadow_offset_x";
    pub const SHADOW_OFFSET_Y: &str = "shadow_offset_y";
    pub const UNDERLINE: &str = "underline";
    pub const UNDERLINE_COLOR: &str = "underline_color";
    pub const OVERLINE: &str = "overline";
    pub const OVERLINE_COLOR: &str = "overline_color";
    pub const STRIKETHROUGH: &str = "strikethrough";
    pub const STRIKETHROUGH_COLOR: &str = "strikethrough_color";
}

/// Words as the `options` of an enum row.
pub(crate) fn options(words: &[&str]) -> String {
    words
        .iter()
        .map(|word| format!("\"{word}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

pub mod words {
    pub const NORMAL: &str = "normal";

    pub const ULTRA_CONDENSED: &str = "ultra_condensed";
    pub const EXTRA_CONDENSED: &str = "extra_condensed";
    pub const CONDENSED: &str = "condensed";
    pub const SEMI_CONDENSED: &str = "semi_condensed";
    pub const SEMI_EXPANDED: &str = "semi_expanded";
    pub const EXPANDED: &str = "expanded";
    pub const EXTRA_EXPANDED: &str = "extra_expanded";
    pub const ULTRA_EXPANDED: &str = "ultra_expanded";
    /// How wide a face is picked, narrowest first: CSS's nine.
    pub const FONT_STRETCHES: &[&str] = &[
        ULTRA_CONDENSED,
        EXTRA_CONDENSED,
        CONDENSED,
        SEMI_CONDENSED,
        NORMAL,
        SEMI_EXPANDED,
        EXPANDED,
        EXTRA_EXPANDED,
        ULTRA_EXPANDED,
    ];

    pub const ITALIC: &str = "italic";
    pub const OBLIQUE: &str = "oblique";
    /// Upright, an italic face, or the upright face slanted.
    pub const FONT_STYLES: &[&str] = &[NORMAL, ITALIC, OBLIQUE];

    pub const NONE: &str = "none";
    pub const SINGLE: &str = "single";
    pub const DOUBLE: &str = "double";
    /// The lines an underline draws.
    pub const UNDERLINES: &[&str] = &[NONE, SINGLE, DOUBLE];

    pub const WORD_OR_GLYPH: &str = "word_or_glyph";
    pub const WORD: &str = "word";
    pub const GLYPH: &str = "glyph";
    /// Where a wrapped line may break.
    pub const LINE_BREAKS: &[&str] = &[WORD_OR_GLYPH, WORD, GLYPH];

    pub const START: &str = "start";
    pub const CENTER: &str = "center";
    pub const END: &str = "end";
    pub const MIDDLE: &str = "middle";
    /// Which part of a cut line the ellipsis replaces.
    pub const TRUNCATE_ATS: &[&str] = &[END, START, MIDDLE];

    pub const LEFT: &str = "left";
    pub const RIGHT: &str = "right";
    pub const JUSTIFY: &str = "justify";
    /// Where lines sit in their block: start and end follow the text's
    /// direction, left and right do not.
    pub const TEXT_ALIGNS: &[&str] = &[START, CENTER, END, LEFT, RIGHT, JUSTIFY];

    pub const COMPLEX: &str = "complex";
    pub const SIMPLE: &str = "simple";
    /// The shaper's two strategies.
    pub const SHAPINGS: &[&str] = &[COMPLEX, SIMPLE];

    pub const AUTO: &str = "auto";
    pub const ON: &str = "on";
    pub const OFF: &str = "off";
    /// Whether glyphs are hinted: `auto` takes the face's import setting.
    pub const HINTINGS: &[&str] = &[AUTO, ON, OFF];
}
