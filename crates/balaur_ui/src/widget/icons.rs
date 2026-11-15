//! A widget's icons by name: `icon = "plus"` draws the glyph the theme's icon
//! font calls `plus`, read from the font's own glyph names, so a scene says
//! which icon it shows rather than carrying a private-use character.

use std::rc::Rc;

use balaur_core::Engine;
use smol_str::SmolStr;

use crate::widget::arena::Look;

/// The glyph names of the icon font a look draws its icons in.
pub(crate) struct Icons {
    names: Rc<balaur_text::GlyphNames>,
    font: SmolStr,
}

impl Icons {
    pub(crate) fn of(eng: &Engine, look: &Look) -> Self {
        Self {
            names: balaur_text::glyph_names(eng, look.project, &look.icon_font),
            font: look.icon_font.clone(),
        }
    }

    /// The glyph `name` names, as the text that draws it. A name the font
    /// has no glyph for warns once and draws nothing; empty is no icon.
    pub(crate) fn glyph(&self, name: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        let found = self.names.get(name);
        if found.is_none()
            && balaur_core::logbuf::first_time("icon", &format!("{}:{name}", self.font))
        {
            tracing::warn!(
                "icon '{name}': the icon font '{}' has no glyph by that name",
                self.font
            );
        }
        found.map(String::from)
    }
}
