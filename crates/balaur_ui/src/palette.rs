//! A theme from a handful of sources: seven colours and four sizes, with every
//! other token worked out from them. A token the theme states wins, and the
//! rules read what is stated, so overriding `bg_panel` also moves the text
//! measured against it.
//!
//! Colour arithmetic is in OKLab, where equal steps look equal, and every ink
//! is pushed towards the foreground until it reads at WCAG AA on the three
//! surfaces text lands on.

use std::collections::HashMap;
use std::sync::LazyLock;

use toml::Table;

use crate::contrast::{AA, ratio, to_linear};
use crate::vocabulary::{keys as k, tokens as t};

/// The colours a theme states; everything else can be derived from them.
pub const COLOR_SOURCES: &[&str] = &[
    t::BACKGROUND,
    t::FOREGROUND,
    t::PRIMARY,
    t::SECONDARY,
    t::SUCCESS,
    t::WARNING,
    t::DANGER,
];

/// The sizes a theme states, and what each is when it does not: a game's
/// sizes, which the editor's own themes state smaller.
pub const SIZE_SOURCES: &[(&str, f64)] = &[
    (t::FONT_SIZE, 16.0),
    (t::RADIUS, 6.0),
    (t::CONTROL_HEIGHT, 32.0),
    (t::STROKE_WIDTH, 1.0),
];

/// Each derived size as a source times a ratio.
const DERIVED_SIZES: &[(&str, &str, f64)] = &[
    (t::FONT_SIZE_SMALL, t::FONT_SIZE, 0.92),
    (t::FONT_SIZE_LARGE, t::FONT_SIZE, 1.2),
    (t::FONT_SIZE_TITLE, t::FONT_SIZE, 1.44),
    (t::RADIUS_SMALL, t::RADIUS, 0.6),
    (t::RADIUS_LARGE, t::RADIUS, 1.4),
    (t::CONTROL_HEIGHT_SMALL, t::CONTROL_HEIGHT, 0.83),
    (t::CONTROL_HEIGHT_LARGE, t::CONTROL_HEIGHT, 1.25),
    (t::CONTROL_HEIGHT_TOUCH, t::CONTROL_HEIGHT, 1.375),
];

/// The colour families, each with the hue it falls back to when the theme
/// names no colour for it. Each takes `_fill`, `_fill_hover`, `_text`, `_bg`
/// and a `text_on_` token.
const FAMILY_HUES: &[(&str, f64)] = &[
    (t::PRIMARY, 250.0),
    (t::SECONDARY, 180.0),
    (t::SUCCESS, 146.0),
    (t::WARNING, 80.0),
    (t::DANGER, 25.0),
];

/// Categories that keep their own hue whatever the brand colour is, so an
/// orange primary cannot make a 3D node read as a bone.
const FIXED_HUES: &[(&str, f64)] = &[
    ("node_2d", 83.0),
    ("node_3d", 249.0),
    ("node_ui", 300.0),
    ("node_physics", 182.0),
    ("node_bone", 51.0),
    ("node_modifier", 315.0),
    ("axis_x", 19.0),
    ("axis_y", 131.0),
    ("axis_z", 254.0),
];

/// The background a theme that states none gets: a dark slate.
const DEFAULT_BACKGROUND: Lab = Lab {
    l: 0.24,
    a: -0.01,
    b: -0.03,
};
/// Below this OKLab lightness a background makes a dark theme: where white
/// and black text read equally.
const DARK_BELOW: f64 = 0.56;
/// The lightness one `contrast` step moves a surface, dark and light.
const STEP_DARK: f64 = 0.05;
const STEP_LIGHT: f64 = 0.03;
/// Surfaces, in steps above the background.
const PANEL_STEPS: f64 = 1.0;
const CONTROL_STEPS: f64 = -0.8;
const CONTROL_HOVER_STEPS: f64 = 2.25;
/// How far a derived colour is mixed towards another.
const BORDER_MIX: f64 = 0.13;
const MUTED_MIX: f64 = 0.30;
const SUBTLE_MIX: f64 = 0.38;
const FILL_HOVER_MIX: f64 = 0.15;
const TINT_MIX: f64 = 0.12;
const GRID_MINOR_MIX: f64 = 0.05;
const GRID_MAJOR_MIX: f64 = 0.15;
const SYNTAX_MIX: f64 = 0.35;
/// The lightness and most chroma an ink takes, dark and light.
const INK_L_DARK: f64 = 0.73;
const INK_L_LIGHT: f64 = 0.48;
const INK_CHROMA: f64 = 0.13;
const FIXED_CHROMA: f64 = 0.12;
/// A family colour the theme leaves out, dark and light.
const FAMILY_L_DARK: f64 = 0.62;
const FAMILY_L_LIGHT: f64 = 0.52;
/// The foreground a theme that states none gets, as OKLab lightness and
/// chroma at the background's hue, dark and light.
const FOREGROUND_DARK: (f64, f64) = (0.93, 0.008);
const FOREGROUND_LIGHT: (f64, f64) = (0.2, 0.01);
/// The two inks a family's fill may carry, whichever reads better on it.
const ON_LIGHT: (f64, f64) = (0.98, 0.005);
const ON_DARK: (f64, f64) = (0.16, 0.01);
/// Constants in both modes: the disc behind the engine's mark, and the
/// rings a touch leaves.
const BRAND_PLATE: &str = "#e9edf2";
const INPUT_RIPPLE: &str = "#ffffff";

#[derive(Clone, Copy, Debug)]
struct Lab {
    l: f64,
    a: f64,
    b: f64,
}

fn to_gamma(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * libm::pow(c, 1.0 / 2.4) - 0.055
    }
}

fn parse(hex: &str) -> Option<Lab> {
    let color = crate::theme::parse_hex(hex)?;
    let [r, g, b] = [color.r(), color.g(), color.b()].map(to_linear);
    let l = libm::cbrt(0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b);
    let m = libm::cbrt(0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b);
    let s = libm::cbrt(0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b);
    Some(Lab {
        l: 0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        a: 1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        b: 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    })
}

fn hex(lab: Lab) -> String {
    let l = lab.l + 0.396_337_777_4 * lab.a + 0.215_803_757_3 * lab.b;
    let m = lab.l - 0.105_561_345_8 * lab.a - 0.063_854_172_8 * lab.b;
    let s = lab.l - 0.089_484_177_5 * lab.a - 1.291_485_548_0 * lab.b;
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    let rgb = [
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s,
    ]
    .map(|c| {
        (to_gamma(c.clamp(0.0, 1.0)) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8
    });
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

fn mix(a: Lab, b: Lab, t: f64) -> Lab {
    Lab {
        l: a.l + (b.l - a.l) * t,
        a: a.a + (b.a - a.a) * t,
        b: a.b + (b.b - a.b) * t,
    }
}

fn chroma(lab: Lab) -> f64 {
    libm::sqrt(lab.a * lab.a + lab.b * lab.b)
}

fn hue(lab: Lab) -> f64 {
    libm::atan2(lab.b, lab.a)
}

fn polar(l: f64, c: f64, h_radians: f64) -> Lab {
    Lab {
        l,
        a: c * libm::cos(h_radians),
        b: c * libm::sin(h_radians),
    }
}

/// The contrast between two OKLab colours, through the sRGB they print as.
fn contrast(a: Lab, b: Lab) -> f64 {
    match (
        crate::theme::parse_hex(&hex(a)),
        crate::theme::parse_hex(&hex(b)),
    ) {
        (Some(a), Some(b)) => ratio(a, b),
        _ => 1.0,
    }
}

/// The colours a theme works in once its sources are known.
struct Work {
    colors: Table,
    dark: bool,
    step: f64,
}

impl Work {
    fn get(&self, name: &str) -> Option<Lab> {
        self.colors
            .get(name)
            .and_then(toml::Value::as_str)
            .and_then(parse)
    }

    /// State `name` as `value` unless the theme already states it.
    fn put(&mut self, name: &str, value: Lab) {
        if !self.colors.contains_key(name) {
            self.colors
                .insert(name.to_owned(), toml::Value::String(hex(value)));
        }
    }

    fn at(&self, name: &str) -> Lab {
        self.get(name).unwrap_or(Lab {
            l: 0.5,
            a: 0.0,
            b: 0.0,
        })
    }

    /// A surface `n` contrast steps above the background: lighter in both
    /// modes, which is the order elevation reads in.
    fn lift(&self, base: Lab, n: f64) -> Lab {
        Lab {
            l: (base.l + n * self.step).clamp(0.0, 1.0),
            ..base
        }
    }

    /// The lightness an ink sits at in this mode, at a colour's own hue.
    fn ink(&self, hue_radians: f64, chroma_of: f64) -> Lab {
        let l = if self.dark { INK_L_DARK } else { INK_L_LIGHT };
        polar(l, chroma_of.min(INK_CHROMA), hue_radians)
    }

    /// `fill` at its own hue, taken away from `ink` until `ink` reads at AA
    /// on it and on its hover: a mid-tone brand colour carries no text as is.
    fn carrying(&self, fill: Lab, ink: Lab) -> Lab {
        let fg = self.at(t::FOREGROUND);
        let step = if ink.l > fill.l { -0.01 } else { 0.01 };
        let mut at = fill;
        while (0.0..=1.0).contains(&at.l) {
            let hover = mix(at, fg, FILL_HOVER_MIX);
            if contrast(ink, at) >= AA && contrast(ink, hover) >= AA {
                return at;
            }
            at.l += step;
        }
        fill
    }

    /// `x`, walked towards the foreground until it reads at AA on every
    /// surface text lands on, a hovered control among them.
    fn legible(&self, x: Lab) -> Lab {
        let fg = self.at(t::FOREGROUND);
        let grounds =
            [t::BG_APP, t::BG_PANEL, t::BG_CONTROL, t::BG_CONTROL_HOVER].map(|g| self.at(g));
        let mut mix_by = 0.0;
        loop {
            let candidate = mix(x, fg, mix_by);
            if mix_by >= 1.0 || grounds.iter().all(|g| contrast(candidate, *g) >= AA) {
                return candidate;
            }
            mix_by += 0.02;
        }
    }
}

/// `doc` with its `[colors]` and `[sizes]` filled in, and `dark` stated. A
/// document with no `[colors]` at all keeps none: a game theme that only
/// restyles kinds has nothing to derive.
#[must_use]
pub fn complete(doc: &toml::Value) -> toml::Value {
    let Some(table) = doc.as_table() else {
        return doc.clone();
    };
    let mut out = table.clone();
    if let Some(colors) = table.get(k::COLORS).and_then(toml::Value::as_table) {
        let stated_dark = table.get(k::DARK).and_then(toml::Value::as_bool);
        let (colors, dark) = derive_colors(colors, stated_dark);
        out.insert(k::COLORS.into(), toml::Value::Table(colors));
        out.insert(k::DARK.into(), toml::Value::Boolean(dark));
    }
    let sizes = sizes(table.get(k::SIZES).and_then(toml::Value::as_table));
    out.insert(k::SIZES.into(), toml::Value::Table(sizes));
    toml::Value::Table(out)
}

fn derive_colors(stated: &Table, stated_dark: Option<bool>) -> (Table, bool) {
    let mut work = Work {
        colors: stated.clone(),
        dark: true,
        step: STEP_DARK,
    };
    let background = work.get(t::BACKGROUND).unwrap_or(DEFAULT_BACKGROUND);
    work.dark = stated_dark.unwrap_or(background.l < DARK_BELOW);
    work.step = work
        .colors
        .get(t::CONTRAST)
        .and_then(balaur_core::components::as_f64)
        .unwrap_or(if work.dark { STEP_DARK } else { STEP_LIGHT });
    work.colors.remove(t::CONTRAST);
    work.put(t::BACKGROUND, background);
    let (l, c) = if work.dark {
        FOREGROUND_DARK
    } else {
        FOREGROUND_LIGHT
    };
    let foreground = polar(l, c, hue(background));
    work.put(t::FOREGROUND, foreground);
    let family_l = if work.dark {
        FAMILY_L_DARK
    } else {
        FAMILY_L_LIGHT
    };
    for (family, degrees) in FAMILY_HUES {
        work.put(family, polar(family_l, INK_CHROMA, degrees.to_radians()));
    }
    let bg = work.at(t::BACKGROUND);
    work.put(t::BG_APP, bg);
    work.put(t::BG_PANEL, work.lift(bg, PANEL_STEPS));
    work.put(t::BG_CONTROL, work.lift(bg, CONTROL_STEPS));
    // From the control as stated, so a theme naming its own control keeps
    // the hover a step above it, and never so far that the text stops reading.
    let control = work.at(t::BG_CONTROL);
    let fg = work.at(t::FOREGROUND);
    let mut steps = CONTROL_HOVER_STEPS - CONTROL_STEPS;
    while steps > 0.0 && contrast(fg, work.lift(control, steps)) < AA {
        steps -= 0.25;
    }
    work.put(t::BG_CONTROL_HOVER, work.lift(control, steps.max(0.0)));
    let panel = work.at(t::BG_PANEL);
    work.put(t::BORDER_DEFAULT, mix(panel, fg, BORDER_MIX));
    work.put(t::TEXT_DEFAULT, fg);
    work.put(t::TEXT_MUTED, work.legible(mix(fg, panel, MUTED_MIX)));
    work.put(t::TEXT_SUBTLE, work.legible(mix(fg, panel, SUBTLE_MIX)));
    let near_white = polar(ON_LIGHT.0, ON_LIGHT.1, hue(bg));
    let near_black = polar(ON_DARK.0, ON_DARK.1, hue(bg));
    for (family, _) in FAMILY_HUES {
        let own = work.at(family);
        work.put(
            &t::of(family, t::TEXT),
            work.legible(work.ink(hue(own), chroma(own))),
        );
        // The ink on the fill is also the ink on the family text a hovered
        // primary action fills with, so that is where it is picked.
        let text = work.at(&t::of(family, t::TEXT));
        let on = match (contrast(near_white, text) >= AA, contrast(near_black, text) >= AA) {
            (true, false) => near_white,
            (false, true) => near_black,
            _ if contrast(near_white, own) >= contrast(near_black, own) => near_white,
            _ => near_black,
        };
        work.put(&t::on(family), on);
        let on = work.at(&t::on(family));
        work.put(&t::of(family, t::FILL), work.carrying(own, on));
        let fill = work.at(&t::of(family, t::FILL));
        work.put(&t::of(family, t::FILL_HOVER), mix(fill, fg, FILL_HOVER_MIX));
        // A tint light enough that the text and the family's own ink still
        // read on it.
        let mut tint = TINT_MIX;
        while tint > 0.0
            && (contrast(fg, mix(panel, own, tint)) < AA || contrast(text, mix(panel, own, tint)) < AA)
        {
            tint -= 0.01;
        }
        work.put(&t::of(family, t::BG), mix(panel, own, tint.max(0.0)));
    }
    work.put(t::GRID_MINOR, mix(bg, fg, GRID_MINOR_MIX));
    work.put(t::GRID_MAJOR, mix(bg, fg, GRID_MAJOR_MIX));
    let primary_text = work.at(t::PRIMARY_TEXT);
    let secondary_text = work.at(t::SECONDARY_TEXT);
    work.put(t::SYNTAX_KEYWORD, primary_text);
    work.put(t::SYNTAX_STRING, secondary_text);
    work.put(t::SYNTAX_COMMENT, work.at(t::TEXT_SUBTLE));
    work.put(t::SYNTAX_IDENTIFIER, fg);
    work.put(t::SYNTAX_PUNCTUATION, work.at(t::TEXT_MUTED));
    work.put(
        t::SYNTAX_NUMBER,
        work.legible(mix(primary_text, fg, SYNTAX_MIX)),
    );
    work.put(
        t::SYNTAX_TYPE,
        work.legible(mix(secondary_text, fg, SYNTAX_MIX)),
    );
    work.put(t::NODE_DEFAULT, work.at(t::TEXT_SUBTLE));
    for (name, degrees) in FIXED_HUES {
        work.put(
            name,
            work.legible(work.ink(degrees.to_radians(), FIXED_CHROMA)),
        );
    }
    work.put(t::BRAND_PLATE, parse(BRAND_PLATE).unwrap_or(fg));
    work.put(t::INPUT_RIPPLE, parse(INPUT_RIPPLE).unwrap_or(fg));
    (work.colors, work.dark)
}

/// The four size sources and the sizes derived from them, a stated one
/// winning.
#[must_use]
pub fn sizes(stated: Option<&Table>) -> Table {
    let mut sizes = stated.cloned().unwrap_or_default();
    let number =
        |sizes: &Table, name: &str| sizes.get(name).and_then(balaur_core::components::as_f64);
    for (name, fallback) in SIZE_SOURCES {
        let value = number(&sizes, name).unwrap_or(*fallback);
        sizes.insert((*name).to_owned(), toml::Value::Float(value));
    }
    for (name, source, ratio) in DERIVED_SIZES {
        let value = number(&sizes, source).unwrap_or_default() * ratio;
        sizes
            .entry((*name).to_owned())
            .or_insert(toml::Value::Float(value.round()));
    }
    sizes
}

static DEFAULT_SIZES: LazyLock<HashMap<String, f32>> = LazyLock::new(|| {
    sizes(None)
        .into_iter()
        .filter_map(|(name, v)| Some((name, balaur_core::components::as_f64(&v)? as f32)))
        .collect()
});

/// A size as a theme that states none has it, or 0 for a name no theme has.
#[must_use]
pub fn default_size(name: &str) -> f32 {
    DEFAULT_SIZES.get(name).copied().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::{complete, hex, parse};
    use crate::contrast::{AA, ratio};

    fn colors(doc: &str) -> toml::Table {
        let value: toml::Value = toml::from_str(doc).unwrap();
        complete(&value)["colors"].as_table().unwrap().clone()
    }

    #[test]
    fn a_colour_survives_the_trip_through_oklab() {
        for sample in [
            "#151f2a", "#e6e9ee", "#4287cc", "#a4392b", "#000000", "#ffffff",
        ] {
            assert_eq!(hex(parse(sample).unwrap()), sample);
        }
    }

    #[test]
    fn seven_colours_make_a_whole_theme_whose_inks_read_at_aa() {
        for background in ["#151f2a", "#ebe9e3", "#000000", "#ffffff", "#3a2f4f"] {
            let done = colors(&format!(
                "[colors]\nbackground = \"{background}\"\nprimary = \"#4287cc\"\n"
            ));
            let inks: Vec<&String> = done
                .keys()
                .filter(|name| {
                    name.starts_with("text_") && !name.starts_with("text_on_")
                        || name.ends_with("_text")
                        || name.starts_with("syntax_")
                        || name.starts_with("node_")
                        || name.starts_with("axis_")
                })
                .collect();
            assert!(inks.len() > 20, "the inks were derived: {inks:?}");
            for ink in inks {
                for ground in ["bg_app", "bg_panel", "bg_control", "bg_control_hover"] {
                    let r = ratio(
                        crate::theme::parse_hex(done[ink.as_str()].as_str().unwrap()).unwrap(),
                        crate::theme::parse_hex(done[ground].as_str().unwrap()).unwrap(),
                    );
                    assert!(r >= AA, "{background}: {ink} on {ground} is {r:.2}");
                }
            }
        }
    }

    #[test]
    fn a_stated_token_wins_and_moves_what_is_measured_against_it() {
        let done = colors("[colors]\nbackground = \"#151f2a\"\nbg_panel = \"#000000\"\n");
        assert_eq!(done["bg_panel"].as_str(), Some("#000000"));
        assert!(done.contains_key("text_muted"));
    }

    fn reads(done: &toml::Table, ink: &str, ground: &str) -> f64 {
        ratio(
            crate::theme::parse_hex(done[ink].as_str().unwrap()).unwrap(),
            crate::theme::parse_hex(done[ground].as_str().unwrap()).unwrap(),
        )
    }

    #[test]
    fn a_mid_tone_brand_colour_is_deepened_until_its_ink_reads() {
        let done = colors("[colors]\nbackground = \"#fafafa\"\nprimary = \"#4078f2\"\n");
        for fill in ["primary_fill", "primary_fill_hover", "primary_text"] {
            let r = reads(&done, "text_on_primary", fill);
            assert!(r >= AA, "text_on_primary on {fill} is {r:.2}");
        }
        assert_ne!(done["primary_fill"].as_str(), Some("#4078f2"));
    }

    #[test]
    fn a_stated_control_keeps_a_hover_the_text_reads_on() {
        let done = colors(
            "[colors]\nbackground = \"#21252b\"\nbg_control = \"#3e4451\"\nforeground = \"#abb2bf\"\n",
        );
        assert_ne!(done["bg_control_hover"], done["bg_control"], "the hover still shows");
        for ink in ["text_default", "text_muted", "text_subtle"] {
            let r = reads(&done, ink, "bg_control_hover");
            assert!(r >= AA, "{ink} on the hover is {r:.2}");
        }
    }

    #[test]
    fn a_tint_stays_light_enough_for_the_text_on_it() {
        let done = colors(
            "[colors]\nbackground = \"#002b36\"\nbg_panel = \"#073642\"\nforeground = \"#93a1a1\"\nprimary = \"#268bd2\"\n",
        );
        for ink in ["text_default", "primary_text"] {
            let r = reads(&done, ink, "primary_bg");
            assert!(r >= AA, "{ink} on primary_bg is {r:.2}");
        }
    }

    #[test]
    fn a_light_background_makes_a_light_theme() {
        let value: toml::Value = toml::from_str("[colors]\nbackground = \"#ebe9e3\"\n").unwrap();
        assert_eq!(complete(&value)["dark"].as_bool(), Some(false));
    }
}
