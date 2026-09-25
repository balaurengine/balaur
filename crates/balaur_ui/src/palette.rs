//! A theme from a handful of sources: seven colours and four sizes, with every
//! other token worked out from them. A token the theme states wins, and the
//! rules read what is stated, so overriding `bg_panel` also moves the text
//! measured against it.
//!
//! Colour arithmetic is in OKLab, where equal steps look equal, and every ink
//! is pushed towards the foreground until it reads at WCAG AA on the three
//! surfaces text lands on.

use toml::Table;

use crate::contrast::{AA, ratio};
use crate::vocabulary::keys as k;

/// The colours a theme states; everything else can be derived from them.
pub const COLOR_SOURCES: &[&str] = &[
    "background",
    "foreground",
    "primary",
    "secondary",
    "success",
    "warning",
    "danger",
];

/// The sizes a theme states, and what each is when it does not.
pub const SIZE_SOURCES: &[(&str, f64)] = &[
    ("font_size", 12.0),
    ("radius", 6.0),
    ("control_height", 24.0),
    ("stroke_width", 1.0),
];

/// The colour families: each takes `_fill`, `_fill_hover`, `_text`, `_bg` and
/// a `text_on_` token.
const FAMILIES: &[&str] = &["primary", "secondary", "success", "warning", "danger"];

/// The hue each family falls back to when the theme names no colour for it.
const FAMILY_HUES: &[(&str, f64)] = &[
    ("primary", 250.0),
    ("secondary", 180.0),
    ("success", 146.0),
    ("warning", 80.0),
    ("danger", 25.0),
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

#[derive(Clone, Copy, Debug)]
struct Lab {
    l: f64,
    a: f64,
    b: f64,
}

fn to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        libm::pow((c + 0.055) / 1.055, 2.4)
    }
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
    let [r, g, b] = [color.r(), color.g(), color.b()].map(|c| to_linear(f64::from(c) / 255.0));
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
    .map(|c| (to_gamma(c.clamp(0.0, 1.0)) * 255.0).round().clamp(0.0, 255.0) as u8);
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
    match (crate::theme::parse_hex(&hex(a)), crate::theme::parse_hex(&hex(b))) {
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
        self.colors.get(name).and_then(toml::Value::as_str).and_then(parse)
    }

    /// State `name` as `value` unless the theme already states it.
    fn put(&mut self, name: &str, value: Lab) {
        if !self.colors.contains_key(name) {
            self.colors.insert(name.to_owned(), toml::Value::String(hex(value)));
        }
    }

    fn at(&self, name: &str) -> Lab {
        self.get(name).unwrap_or(Lab { l: 0.5, a: 0.0, b: 0.0 })
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
        polar(if self.dark { 0.73 } else { 0.48 }, chroma_of.min(0.13), hue_radians)
    }

    /// `x`, walked towards the foreground until it reads at AA on every
    /// surface text lands on.
    fn legible(&self, x: Lab) -> Lab {
        let fg = self.at("foreground");
        let grounds = ["bg_app", "bg_panel", "bg_control"].map(|g| self.at(g));
        let mut t = 0.0;
        loop {
            let candidate = mix(x, fg, t);
            if t >= 1.0 || grounds.iter().all(|g| contrast(candidate, *g) >= AA) {
                return candidate;
            }
            t += 0.02;
        }
    }
}

/// `doc` with its `[colors]` and `[sizes]` filled in, and `dark` stated. A
/// document with no `[colors]` at all is returned as it came: a game theme
/// that only restyles kinds has nothing to derive.
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
    let sizes = derive_sizes(table.get(k::SIZES).and_then(toml::Value::as_table));
    out.insert(k::SIZES.into(), toml::Value::Table(sizes));
    toml::Value::Table(out)
}

fn derive_colors(stated: &Table, stated_dark: Option<bool>) -> (Table, bool) {
    let mut work = Work {
        colors: stated.clone(),
        dark: true,
        step: 0.05,
    };
    let background = work.get("background").unwrap_or(Lab { l: 0.24, a: -0.01, b: -0.03 });
    work.dark = stated_dark.unwrap_or(background.l < 0.56);
    work.step = work
        .colors
        .get("contrast")
        .and_then(balaur_core::components::as_f64)
        .unwrap_or(if work.dark { 0.05 } else { 0.03 });
    work.colors.remove("contrast");
    work.put("background", background);
    let foreground = if work.dark {
        polar(0.93, 0.008, hue(background))
    } else {
        polar(0.2, 0.01, hue(background))
    };
    work.put("foreground", foreground);
    for (family, degrees) in FAMILY_HUES {
        let own = polar(if work.dark { 0.62 } else { 0.52 }, 0.13, degrees.to_radians());
        work.put(family, own);
    }
    let fg = work.at("foreground");
    let bg = work.at("background");
    work.put("bg_app", bg);
    work.put("bg_panel", work.lift(bg, 1.0));
    work.put("bg_control", work.lift(bg, -0.8));
    work.put("bg_control_hover", work.lift(bg, 2.25));
    let panel = work.at("bg_panel");
    work.put("border_default", mix(panel, fg, 0.13));
    work.put("text_default", fg);
    work.put("text_muted", work.legible(mix(fg, panel, 0.30)));
    work.put("text_subtle", work.legible(mix(fg, panel, 0.38)));
    let near_white = polar(0.98, 0.005, hue(bg));
    let near_black = polar(0.16, 0.01, hue(bg));
    for family in FAMILIES {
        let own = work.at(family);
        work.put(&format!("{family}_fill"), own);
        let fill = work.at(&format!("{family}_fill"));
        work.put(&format!("{family}_fill_hover"), mix(fill, fg, 0.15));
        work.put(&format!("{family}_text"), work.legible(work.ink(hue(own), chroma(own))));
        work.put(&format!("{family}_bg"), mix(panel, own, 0.12));
        let on = if contrast(near_white, fill) >= contrast(near_black, fill) {
            near_white
        } else {
            near_black
        };
        work.put(&format!("text_on_{family}"), on);
    }
    work.put("grid_minor", mix(bg, fg, 0.05));
    work.put("grid_major", mix(bg, fg, 0.15));
    let primary_text = work.at("primary_text");
    let secondary_text = work.at("secondary_text");
    work.put("syntax_keyword", primary_text);
    work.put("syntax_string", secondary_text);
    work.put("syntax_comment", work.at("text_subtle"));
    work.put("syntax_identifier", fg);
    work.put("syntax_punctuation", work.at("text_muted"));
    work.put("syntax_number", work.legible(mix(primary_text, fg, 0.35)));
    work.put("syntax_type", work.legible(mix(secondary_text, fg, 0.35)));
    work.put("node_default", work.at("text_subtle"));
    for (name, degrees) in FIXED_HUES {
        work.put(name, work.legible(work.ink(degrees.to_radians(), 0.12)));
    }
    work.put("brand_plate", parse("#e9edf2").unwrap_or(fg));
    work.put("input_ripple", parse("#ffffff").unwrap_or(fg));
    (work.colors, work.dark)
}

fn derive_sizes(stated: Option<&Table>) -> Table {
    let mut sizes = stated.cloned().unwrap_or_default();
    let number = |sizes: &Table, name: &str, fallback: f64| {
        sizes
            .get(name)
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(fallback)
    };
    for (name, fallback) in SIZE_SOURCES {
        let value = number(&sizes, name, *fallback);
        sizes.entry((*name).to_owned()).or_insert(toml::Value::Float(value));
    }
    let font = number(&sizes, "font_size", 12.0);
    let radius = number(&sizes, "radius", 6.0);
    let control = number(&sizes, "control_height", 24.0);
    let derived = [
        ("font_size_small", font * 0.92),
        ("font_size_large", font * 1.2),
        ("font_size_title", font * 1.44),
        ("radius_small", radius * 0.6),
        ("radius_large", radius * 1.4),
        ("control_height_small", control * 0.83),
        ("control_height_large", control * 1.25),
        ("control_height_touch", control * 1.375),
    ];
    for (name, value) in derived {
        sizes
            .entry(name.to_owned())
            .or_insert(toml::Value::Float(value.round()));
    }
    sizes
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
        for sample in ["#151f2a", "#e6e9ee", "#4287cc", "#a4392b", "#000000", "#ffffff"] {
            assert_eq!(hex(parse(sample).unwrap()), sample);
        }
    }

    #[test]
    fn seven_colours_make_a_whole_theme_whose_inks_read_at_aa() {
        for background in ["#151f2a", "#ebe9e3", "#000000", "#ffffff", "#3a2f4f"] {
            let done = colors(&format!("[colors]\nbackground = \"{background}\"\nprimary = \"#4287cc\"\n"));
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
                for ground in ["bg_app", "bg_panel", "bg_control"] {
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

    #[test]
    fn a_light_background_makes_a_light_theme() {
        let value: toml::Value = toml::from_str("[colors]\nbackground = \"#ebe9e3\"\n").unwrap();
        assert_eq!(complete(&value)["dark"].as_bool(), Some(false));
    }
}
