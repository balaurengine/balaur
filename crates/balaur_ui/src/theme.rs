//! Theme tokens and their translation to `egui::Visuals`.
//!
//! Scripts own the palette: `ui.set_theme{ bg = "#17191c", ... }` stores a
//! token table which [`apply`] turns into a complete `Visuals` (flat fills,
//! 1 px strokes, no shadows). Fonts are loaded from `<project>/fonts/*.ttf`
//! when present; three named families — `heading`, `ui`, `mono` — always
//! exist so scripts can reference them regardless.

use std::collections::HashMap;

use crate::vocabulary::words as w;
use balaur_text::fonts::{FontFace, system_static_bytes};
use egui::{Color32, CornerRadius, FontFamily, Shadow, Stroke};

#[derive(Clone)]
pub struct ThemeTokens {
    pub dark: bool,
    pub colors: HashMap<String, Color32>,
    /// Named looks — `field`, `tab`, `heading` — as the option map a widget
    /// would otherwise have been given at the call site. Colour entries are
    /// already resolved from token name to `#rrggbb`. Shared rather than
    /// owned: every widget naming a role would otherwise copy the table.
    pub roles: HashMap<String, std::rc::Rc<Vec<(String, balaur_script::Value)>>>,
}

impl Default for ThemeTokens {
    fn default() -> Self {
        Self {
            dark: true,
            colors: HashMap::new(),
            roles: HashMap::new(),
        }
    }
}

impl ThemeTokens {
    pub fn color(&self, name: &str, fallback: Color32) -> Color32 {
        self.colors.get(name).copied().unwrap_or(fallback)
    }

    /// The option map a role stands for, empty when nothing declares it.
    pub fn role(&self, name: &str) -> &[(String, balaur_script::Value)] {
        self.roles.get(name).map_or(&[], |role| role.as_slice())
    }
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
    let c = |name: &str, fb: Color32| tokens.color(name, fb);
    let panel = c("panel", Color32::from_rgb(0x20, 0x24, 0x2a));
    let sunken = c("sunken", Color32::from_rgb(0x10, 0x12, 0x15));
    let raised = c("raised", Color32::from_rgb(0x2b, 0x30, 0x37));
    let line = c("line", Color32::from_rgb(0x34, 0x3a, 0x42));
    let text = c("text", Color32::from_rgb(0xee, 0xf1, 0xf4));
    let dim = c("dim", Color32::from_rgb(0xb0, 0xb8, 0xc0));
    let accent = c("accent", Color32::from_rgb(0xf0, 0xa2, 0x73));
    let accent_soft = c("accent_soft", Color32::from_rgb(0x3d, 0x24, 0x15));

    let mut visuals = if tokens.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = panel;
    visuals.window_fill = panel;
    visuals.window_stroke = Stroke::new(1.0, line);
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.extreme_bg_color = sunken;
    visuals.faint_bg_color = raised;
    visuals.code_bg_color = sunken;
    visuals.override_text_color = Some(text);
    visuals.selection.bg_fill = accent_soft;
    visuals.selection.stroke = Stroke::new(1.0, accent);
    visuals.hyperlink_color = accent;

    // The 1 px seams between regions come from the noninteractive stroke.
    visuals.widgets.noninteractive.bg_fill = panel;
    visuals.widgets.noninteractive.weak_bg_fill = panel;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, line);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, text);
    visuals.widgets.inactive.bg_fill = sunken;
    visuals.widgets.inactive.weak_bg_fill = sunken;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, line);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, dim);
    visuals.widgets.hovered.bg_fill = raised;
    visuals.widgets.hovered.weak_bg_fill = raised;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, line);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, text);
    visuals.widgets.active.bg_fill = raised;
    visuals.widgets.active.weak_bg_fill = raised;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, text);

    ctx.set_visuals(visuals);
    ctx.all_styles_mut(|style| {
        // 4 px base grid; panels/widgets add their own padding.
        style.spacing.item_spacing = egui::vec2(4.0, 4.0);
        style.spacing.button_padding = egui::vec2(12.0, 0.0);
        style.spacing.window_margin = egui::Margin::ZERO;
        style.spacing.menu_margin = egui::Margin::ZERO;
        style.visuals.menu_corner_radius = CornerRadius::same(16);
        style.visuals.window_corner_radius = CornerRadius::same(16);
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
        };
        fonts
            .font_data
            .insert(face.name.clone(), std::sync::Arc::new(data));
        match face.chain {
            "heading" => heading_chain.push(face.name.clone()),
            "mono" => mono_chain.push(face.name.clone()),
            "icons" => icon_chain.push(face.name.clone()),
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
        .insert(FontFamily::Name("heading".into()), heading_chain);
    fonts
        .families
        .insert(FontFamily::Name("ui".into()), ui_chain);
    fonts
        .families
        .insert(FontFamily::Name("mono".into()), mono_chain);
    fonts
        .families
        .insert(FontFamily::Name("icon".into()), icon_chain);
    ctx.set_fonts(fonts);
}
