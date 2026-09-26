//! Theme tokens and their translation to `egui::Visuals`.
//!
//! Scripts own the palette: `ui::set_theme(doc)` takes the same document a
//! `widget_theme` file holds, a few source colours and sizes in `[colors]` and
//! `[sizes]` and the looks under `roles`, and [`apply`] turns what it derives
//! into egui's visuals. Fonts are loaded from `<project>/fonts/*.ttf` when
//! present; the named families `heading`, `ui`, `mono` and `icon` always exist.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::vocabulary::{keys as k, tokens as t, words as w};
use balaur_script::Value;
use balaur_text::fonts::{FontFace, system_static_bytes};
use egui::{Color32, CornerRadius, FontFamily, Shadow, Stroke};

#[derive(Clone)]
pub struct ThemeTokens {
    pub dark: bool,
    /// Every token, the stated ones and the ones derived from the sources.
    pub colors: HashMap<String, Color32>,
    pub sizes: HashMap<String, f32>,
    /// Named looks, `input` or `tab`, as the option map a widget would
    /// otherwise have been given at the call site, with every token already
    /// spelled out. Shared, since every widget naming a role reads it.
    pub roles: HashMap<String, Rc<Vec<(String, Value)>>>,
}

impl Default for ThemeTokens {
    fn default() -> Self {
        Self::from_doc(&toml::Value::Table(toml::Table::new()))
    }
}

impl ThemeTokens {
    /// A theme document, completed from its sources, as tokens and roles.
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
        let sizes: HashMap<String, f32> = table(k::SIZES)
            .into_iter()
            .flatten()
            .filter_map(|(name, v)| {
                Some((name.clone(), balaur_core::components::as_f64(v)? as f32))
            })
            .collect();
        let resolve = Resolve {
            colors: table(k::COLORS),
            sizes: &sizes,
        };
        let roles = table(k::ROLES)
            .into_iter()
            .flatten()
            .filter_map(|(name, body)| {
                let entries = body
                    .as_table()?
                    .iter()
                    .filter_map(|(key, v)| {
                        let value = balaur_core::node_api::from_toml(&resolve.spell(v)).ok()?;
                        Some((key.clone(), value))
                    })
                    .collect();
                Some((name.clone(), Rc::new(entries)))
            })
            .collect();
        Self {
            dark: doc
                .get(k::DARK)
                .and_then(toml::Value::as_bool)
                .unwrap_or(true),
            colors,
            sizes,
            roles,
        }
    }

    pub fn color(&self, name: &str, fallback: Color32) -> Color32 {
        self.colors.get(name).copied().unwrap_or(fallback)
    }

    /// A named size; every document has them all, derived where not stated.
    pub fn size(&self, name: &str) -> f32 {
        self.sizes.get(name).copied().unwrap_or(0.0)
    }

    /// The option map a role stands for, empty when nothing declares it.
    pub fn role(&self, name: &str) -> &[(String, Value)] {
        self.roles.get(name).map_or(&[], |role| role.as_slice())
    }
}

/// A role's values with their tokens spelled out: a colour's name as its
/// `#rrggbb`, a size's name as its number, through every state table.
struct Resolve<'a> {
    colors: Option<&'a toml::Table>,
    sizes: &'a HashMap<String, f32>,
}

impl Resolve<'_> {
    fn spell(&self, value: &toml::Value) -> toml::Value {
        match value {
            toml::Value::String(name) => {
                if let Some(hex) = self.colors.and_then(|colors| colors.get(name)) {
                    return hex.clone();
                }
                self.sizes
                    .get(name)
                    .map_or_else(|| value.clone(), |px| toml::Value::Float(f64::from(*px)))
            }
            toml::Value::Table(fields) => toml::Value::Table(
                fields
                    .iter()
                    .map(|(key, v)| (key.clone(), self.spell(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

thread_local! {
    /// The theme in force for `ui::*`: what a call falls back to for a colour
    /// or a size it was not given.
    static IN_FORCE: RefCell<Rc<ThemeTokens>> = RefCell::new(Rc::new(ThemeTokens::default()));
}

/// A size of the theme in force, by name.
pub(crate) fn size(name: &str) -> f32 {
    IN_FORCE.with(|theme| theme.borrow().size(name))
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

    let body = tokens.size(t::FONT_SIZE);
    let small = tokens.size(t::FONT_SIZE_SMALL);
    let title = tokens.size(t::FONT_SIZE_TITLE);
    let sheet = CornerRadius::same(tokens.size(t::RADIUS_LARGE).round() as u8);
    ctx.set_visuals(visuals);
    ctx.all_styles_mut(|style| {
        use egui::{FontId, TextStyle};
        style.text_styles = [
            (TextStyle::Small, FontId::new(small, family(w::UI))),
            (TextStyle::Body, FontId::new(body, family(w::UI))),
            (TextStyle::Button, FontId::new(body, family(w::UI))),
            (TextStyle::Heading, FontId::new(title, family(w::HEADING))),
            (TextStyle::Monospace, FontId::new(body, family(w::MONO))),
        ]
        .into();
        // 4 px base grid; panels/widgets add their own padding.
        style.spacing.item_spacing = egui::vec2(4.0, 4.0);
        style.spacing.button_padding = egui::vec2(12.0, 0.0);
        // The bar floats over the content rather than taking a strip of it,
        // so a scroll's rows are as wide as the sheet's padding leaves them.
        style.spacing.scroll = egui::style::ScrollStyle::floating();
        style.spacing.window_margin = egui::Margin::ZERO;
        style.spacing.menu_margin = egui::Margin::ZERO;
        style.visuals.menu_corner_radius = sheet;
        style.visuals.window_corner_radius = sheet;
    });
}

/// The named family for a widget option value.
pub(crate) fn family(name: &str) -> FontFamily {
    match name {
        w::HEADING => FontFamily::Name(w::HEADING.into()),
        w::MONO => FontFamily::Name(w::MONO.into()),
        w::ICON => FontFamily::Name(w::ICON.into()),
        _ => FontFamily::Name(w::UI.into()),
    }
}

/// Load the four named families into `ctx`. A project's own `fonts/*.ttf`
/// come first, then what the editor bundles, then the system's — so a game
/// can ship the face its language needs without patching the editor.
pub(crate) fn load_fonts(ctx: &egui::Context, faces: &[FontFace]) {
    let mut fonts = egui::FontDefinitions::default();
    let mut heading_chain: Vec<String> = Vec::new();
    let mut ui_chain: Vec<String> = Vec::new();
    let mut mono_chain: Vec<String> = Vec::new();
    let mut icon_chain: Vec<String> = Vec::new();
    let mut system_chain: Vec<String> = Vec::new();

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
            .insert(face.name.clone(), std::sync::Arc::new(data));
        match face.chain {
            w::HEADING => heading_chain.push(face.name.clone()),
            w::MONO => mono_chain.push(face.name.clone()),
            "icon" => icon_chain.push(face.name.clone()),
            "system" => system_chain.push(face.name.clone()),
            _ => ui_chain.push(face.name.clone()),
        }
    }

    // Fall back to the built-ins so the named families always resolve.
    let default_prop = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let default_mono = fonts
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();
    if heading_chain.is_empty() {
        heading_chain.clone_from(&ui_chain);
    }
    // Icons resolve wherever a glyph is written, so they join every chain.
    let text_faces = ui_chain.clone();
    for chain in [&mut heading_chain, &mut ui_chain, &mut mono_chain] {
        chain.extend(icon_chain.iter().cloned());
    }
    // And the other way: an icon table names letters where a Fill face draws
    // a tile — `×` closes a tab — and no icon face has those glyphs.
    icon_chain.extend(text_faces);
    for chain in [
        &mut heading_chain,
        &mut ui_chain,
        &mut mono_chain,
        &mut icon_chain,
    ] {
        chain.extend(system_chain.iter().cloned());
    }
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        if let Some(chain) = fonts.families.get_mut(&family) {
            chain.extend(system_chain.iter().cloned());
        }
    }
    heading_chain.extend(default_prop.clone());
    ui_chain.extend(default_prop.clone());
    icon_chain.extend(default_prop);
    mono_chain.extend(default_mono);

    fonts
        .families
        .insert(FontFamily::Name(w::HEADING.into()), heading_chain);
    fonts
        .families
        .insert(FontFamily::Name(w::UI.into()), ui_chain);
    fonts
        .families
        .insert(FontFamily::Name(w::MONO.into()), mono_chain);
    fonts
        .families
        .insert(FontFamily::Name("icon".into()), icon_chain);
    ctx.set_fonts(fonts);
}
