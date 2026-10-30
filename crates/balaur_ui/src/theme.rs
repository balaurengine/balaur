//! Theme tokens and their translation to `egui::Visuals`.
//!
//! Scripts own the palette: `ui::set_theme(doc)` takes the same document a
//! `widget_theme` file holds, a few source colours and sizes in `[colors]` and
//! `[sizes]` and its fonts in `[fonts]`, and [`apply`] turns what it derives
//! into egui's visuals. egui is given a family for every font the shaper has
//! read, so its own text draws in the faces the shaper's does.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::vocabulary::{keys as k, tokens as t};
use balaur_text::FontProject;
use balaur_text::fonts::{FontFace, system_static_bytes};
use egui::{Color32, CornerRadius, FontFamily, Shadow, Stroke};
use std::collections::HashSet;

#[derive(Clone)]
pub struct ThemeTokens {
    pub dark: bool,
    /// Every token, the stated ones and the ones derived from the sources.
    pub colors: HashMap<String, Color32>,
    pub sizes: HashMap<String, f32>,
    /// `[fonts]`: a font by the name a role or a kind writes for it.
    pub fonts: HashMap<String, String>,
}

impl Default for ThemeTokens {
    fn default() -> Self {
        Self::from_doc(&toml::Value::Table(toml::Table::new()))
    }
}

impl ThemeTokens {
    /// A theme document, completed from its sources, as tokens.
    #[must_use]
    pub fn from_doc(doc: &toml::Value) -> Self {
        // An empty `[colors]` is every token derived from the defaults.
        let mut doc = doc.as_table().cloned().unwrap_or_default();
        doc.entry(k::COLORS)
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        let doc = crate::palette::complete(&toml::Value::Table(doc));
        let table = |key: &str| doc.get(key).and_then(toml::Value::as_table);
        let colors: HashMap<String, Color32> = table(k::COLORS)
            .into_iter()
            .flatten()
            .filter_map(|(name, v)| Some((name.clone(), parse_hex(v.as_str()?)?)))
            .collect();
        let fonts: HashMap<String, String> = table(k::FONTS)
            .into_iter()
            .flatten()
            .filter_map(|(name, v)| Some((name.clone(), v.as_str()?.to_string())))
            .collect();
        let sizes: HashMap<String, f32> = table(k::SIZES)
            .into_iter()
            .flatten()
            .filter_map(|(name, v)| {
                Some((name.clone(), balaur_core::components::as_f64(v)? as f32))
            })
            .collect();
        Self {
            dark: doc
                .get(k::DARK)
                .and_then(toml::Value::as_bool)
                .unwrap_or(true),
            colors,
            sizes,
            fonts,
        }
    }

    /// The font `[fonts]` names `name`, or the default font where it names
    /// none.
    pub fn font(&self, name: &str) -> &str {
        self.fonts
            .get(name)
            .or_else(|| self.fonts.get(t::FONT_DEFAULT))
            .map_or("", String::as_str)
    }

    pub fn color(&self, name: &str, fallback: Color32) -> Color32 {
        self.colors.get(name).copied().unwrap_or(fallback)
    }

    /// A named size; every document has them all, derived where not stated.
    pub fn size(&self, name: &str) -> f32 {
        self.sizes.get(name).copied().unwrap_or(0.0)
    }
}

thread_local! {
    /// The theme `ui::set_theme` put in force: what a kind falls back to for
    /// a colour its `widget_theme` does not name.
    static IN_FORCE: RefCell<Rc<ThemeTokens>> = RefCell::new(Rc::new(ThemeTokens::default()));
}

/// A colour of the theme in force, by name.
pub(crate) fn color(name: &str) -> Color32 {
    IN_FORCE.with(|theme| theme.borrow().color(name, Color32::GRAY))
}

/// `#rrggbb` or `#rrggbbaa` to Color32.
pub(crate) fn parse_hex(hex: &str) -> Option<Color32> {
    let hex = hex.strip_prefix('#')?;
    let parse = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        6 => Some(Color32::from_rgb(parse(0)?, parse(2)?, parse(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(
            parse(0)?,
            parse(2)?,
            parse(4)?,
            parse(6)?,
        )),
        _ => None,
    }
}

pub(crate) fn apply(tokens: &ThemeTokens, ctx: &egui::Context) {
    IN_FORCE.with(|theme| *theme.borrow_mut() = Rc::new(tokens.clone()));
    let c = |name: &str| tokens.color(name, Color32::GRAY);
    let panel = c(t::BG_PANEL);
    let control = c(t::BG_CONTROL);
    let hover = c(t::BG_CONTROL_HOVER);
    let border = c(t::BORDER_DEFAULT);
    let text = c(t::TEXT_DEFAULT);
    let muted = c(t::TEXT_MUTED);
    let primary = c(t::PRIMARY_TEXT);
    let primary_bg = c(t::PRIMARY_BG);
    let line = |color| Stroke::new(tokens.size(t::STROKE_WIDTH), color);

    let mut visuals = if tokens.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = panel;
    visuals.window_fill = panel;
    visuals.window_stroke = line(border);
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.extreme_bg_color = control;
    visuals.faint_bg_color = hover;
    visuals.code_bg_color = control;
    visuals.override_text_color = Some(text);
    visuals.selection.bg_fill = primary_bg;
    visuals.selection.stroke = line(primary);
    visuals.hyperlink_color = primary;

    // The seams between regions come from the noninteractive stroke.
    visuals.widgets.noninteractive.bg_fill = panel;
    visuals.widgets.noninteractive.weak_bg_fill = panel;
    visuals.widgets.noninteractive.bg_stroke = line(border);
    visuals.widgets.noninteractive.fg_stroke = line(text);
    visuals.widgets.inactive.bg_fill = control;
    visuals.widgets.inactive.weak_bg_fill = control;
    visuals.widgets.inactive.bg_stroke = line(border);
    visuals.widgets.inactive.fg_stroke = line(muted);
    visuals.widgets.hovered.bg_fill = hover;
    visuals.widgets.hovered.weak_bg_fill = hover;
    visuals.widgets.hovered.bg_stroke = line(border);
    visuals.widgets.hovered.fg_stroke = line(text);
    visuals.widgets.active.bg_fill = hover;
    visuals.widgets.active.weak_bg_fill = hover;
    visuals.widgets.active.bg_stroke = line(primary);
    visuals.widgets.active.fg_stroke = line(text);
    // A control no role rounds takes the theme's small radius, not egui's.
    let control_radius = CornerRadius::same(tokens.size(t::RADIUS_SMALL).round() as u8);
    for state in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        state.corner_radius = control_radius;
    }

    let sheet = CornerRadius::same(tokens.size(t::RADIUS_LARGE).round() as u8);
    ctx.set_visuals(visuals);
    text_styles(ctx, tokens);
    ctx.all_styles_mut(|style| {
        // 4 px base grid; panels/widgets add their own padding.
        style.spacing.item_spacing = egui::vec2(4.0, 4.0);
        style.spacing.button_padding = BUTTON_PADDING;
        // The bar floats over the content rather than taking a strip of it,
        // so a scroll's rows are as wide as the sheet's padding leaves them.
        style.spacing.scroll = egui::style::ScrollStyle::floating();
        style.spacing.window_margin = egui::Margin::ZERO;
        style.spacing.menu_margin = egui::Margin::ZERO;
        style.visuals.menu_corner_radius = sheet;
        style.visuals.window_corner_radius = sheet;
    });
}

/// egui's own text styles in the theme's fonts: `default` for body text,
/// `heading` and `mono` where the theme names them.
fn text_styles(ctx: &egui::Context, tokens: &ThemeTokens) {
    use egui::{FontId, TextStyle};
    let body = tokens.size(t::FONT_SIZE);
    let small = tokens.size(t::FONT_SIZE_SMALL);
    let title = tokens.size(t::FONT_SIZE_TITLE);
    let family = |name: &str| weighted(FontProject::Host, tokens.font(name), 400.0, false);
    let mono = if tokens.fonts.contains_key(t::FONT_MONO) {
        family(t::FONT_MONO)
    } else {
        FontFamily::Monospace
    };
    let styles: std::collections::BTreeMap<TextStyle, FontId> = [
        (
            TextStyle::Small,
            FontId::new(small, family(t::FONT_DEFAULT)),
        ),
        (TextStyle::Body, FontId::new(body, family(t::FONT_DEFAULT))),
        (
            TextStyle::Button,
            FontId::new(body, family(t::FONT_DEFAULT)),
        ),
        (
            TextStyle::Heading,
            FontId::new(title, family(t::FONT_HEADING)),
        ),
        (TextStyle::Monospace, FontId::new(body, mono)),
    ]
    .into();
    ctx.all_styles_mut(|style| style.text_styles.clone_from(&styles));
}

/// The air either side of a button's caption, and above and below it, where
/// neither the button nor its theme states any: egui's and the widget layer's.
pub(crate) const BUTTON_PADDING: egui::Vec2 = egui::vec2(12.0, 0.0);

/// The weights a font's families are registered at: CSS's nine.
const WEIGHTS: std::ops::RangeInclusive<u16> = 1..=9;

/// One font as egui names it: whose faces, which font, at what weight and
/// slant.
type FontKey = (FontProject, String, u16, bool);

thread_local! {
    /// The families egui has taken. One given to `set_fonts` is taken at the
    /// start of the next pass, and a family egui has not taken is a panic, not
    /// a fallback.
    static BOUND: RefCell<HashSet<FontKey>> = RefCell::new(HashSet::new());
    /// The families given this pass, taken at the start of the next.
    static GIVEN: RefCell<Option<HashSet<FontKey>>> = const { RefCell::new(None) };
    /// The fonts a widget asked for that egui has no family for yet: the next
    /// pass has the shaper read them, and egui takes their families after.
    static WANTED: RefCell<HashSet<(FontProject, String)>> = RefCell::new(HashSet::new());
    /// The fonts and weights whose italic has no face of its own, so egui
    /// slants the upright one. Written where the fonts are given.
    static SLANTED: RefCell<HashSet<FontKey>> = RefCell::new(HashSet::new());
}

/// A weight as a family is registered at: the nearest hundred, 100 to 900.
fn weight_step(weight: f32) -> u16 {
    ((weight / 100.0).round().clamp(1.0, 9.0) as u16) * 100
}

/// The name egui knows a font's family by at a weight and a slant.
fn family_name((project, reference, weight, italic): &FontKey) -> String {
    let slant = if *italic { " italic" } else { "" };
    format!("{}|{reference}|{weight}{slant}", project.word())
}

/// The family egui draws `reference` in at `weight`, upright or italic: the
/// face the shaper would pick for it, ahead of the project's others. A font
/// egui has no family for yet draws in the project's default, and is asked
/// for.
pub(crate) fn weighted(
    project: FontProject,
    reference: &str,
    weight: f32,
    italic: bool,
) -> FontFamily {
    let key = (project, reference.to_string(), weight_step(weight), italic);
    let bound = BOUND.with(|bound| bound.borrow().contains(&key));
    if bound {
        return FontFamily::Name(family_name(&key).into());
    }
    if !reference.is_empty() {
        WANTED.with(|wanted| {
            wanted.borrow_mut().insert((project, reference.to_string()));
        });
    }
    let default = (project, String::new(), key.2, italic);
    if BOUND.with(|bound| bound.borrow().contains(&default)) {
        FontFamily::Name(family_name(&default).into())
    } else {
        FontFamily::Proportional
    }
}

/// Whether egui has to slant `reference`'s glyphs itself to draw it italic at
/// `weight`: the project ships no italic face for it.
pub(crate) fn slanted(project: FontProject, reference: &str, weight: f32, italic: bool) -> bool {
    italic
        && SLANTED.with(|slanted| {
            slanted
                .borrow()
                .contains(&(project, reference.to_string(), weight_step(weight), true))
        })
}

/// Take the families given last pass, and hand the shaper the fonts a widget
/// asked egui for: what the start of every pass does.
pub(crate) fn take_fonts(eng: &balaur_core::Engine, ctx: &egui::Context) {
    if let Some(given) = GIVEN.with(|given| given.borrow_mut().take()) {
        BOUND.with(|bound| *bound.borrow_mut() = given);
        // egui's own text styles name families, which may only now exist.
        let tokens = IN_FORCE.with(|theme| Rc::clone(&theme.borrow()));
        text_styles(ctx, &tokens);
    }
    let wanted: Vec<(FontProject, String)> =
        WANTED.with(|wanted| wanted.borrow_mut().drain().collect());
    for (project, reference) in wanted {
        balaur_text::load_font(eng, project, &reference);
    }
}

/// Every face of each project's under a name of its own, with its import
/// settings as egui's tweak.
fn face_data(fonts: &mut egui::FontDefinitions, project: FontProject, faces: &[FontFace]) {
    for face in faces {
        // An OS face is cached for the process, so egui borrows those bytes;
        // a project's own are small and reload, so they keep a copy.
        let data = match system_static_bytes(&face.name) {
            Some(bytes) => egui::FontData::from_static(bytes),
            None => egui::FontData::from_owned((*face.bytes).clone()),
        }
        .tweak(egui::FontTweak {
            scale: face.tweak.scale,
            y_offset_factor: face.tweak.y_offset,
            hinting: face.tweak.hinting,
            ..egui::FontTweak::default()
        });
        fonts
            .font_data
            .insert(face_name(project, face), std::sync::Arc::new(data));
    }
}

/// What egui knows one project's face by.
fn face_name(project: FontProject, face: &FontFace) -> String {
    format!("{}|{}", project.word(), face.name)
}

/// Give egui a family for every font the shaper has read, in either project,
/// at every weight and slant: the face the shaper would pick first, then the
/// project's every face in order, then egui's own. Taken at the start of the
/// next pass.
pub(crate) fn load_fonts(ctx: &egui::Context, state: &balaur_text::TextState) {
    let mut fonts = egui::FontDefinitions::default();
    let egui_own = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut given = HashSet::new();
    let mut slanted = HashSet::new();
    for (project, held) in state.projects() {
        let faces = held.faces();
        face_data(&mut fonts, project, faces);
        let every: Vec<String> = faces.iter().map(|face| face_name(project, face)).collect();
        let default = held
            .loaded()
            .find(|(reference, _)| reference.is_empty())
            .map(|(_, font)| font.clone());
        for (reference, font) in held.loaded() {
            for weight in WEIGHTS.map(|step| step * 100) {
                for italic in [false, true] {
                    // A bitmap font draws its glyphs itself; egui's text in it
                    // takes the default font's face.
                    let picked = held
                        .face_for(font, weight, italic)
                        .or_else(|| held.face_for(default.as_ref()?, weight, italic));
                    let mut list = every.clone();
                    if let Some((at, _)) = picked {
                        let first = list.remove(at);
                        list.insert(0, first);
                    }
                    list.extend(egui_own.iter().cloned());
                    let key = (project, reference.to_string(), weight, italic);
                    if italic && !picked.is_some_and(|(_, real)| real) {
                        slanted.insert(key.clone());
                    }
                    fonts
                        .families
                        .insert(FontFamily::Name(family_name(&key).into()), list);
                    given.insert(key);
                }
            }
        }
        // Without egui's bundled faces (`fallback-fonts`), the engine's own
        // faces stand in for its base families.
        if project == FontProject::Host && egui_own.is_empty() {
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                fonts.families.insert(family, every.clone());
            }
        }
    }
    SLANTED.with(|held| *held.borrow_mut() = slanted);
    GIVEN.with(|held| *held.borrow_mut() = Some(given));
    ctx.set_fonts(fonts);
}
