//! Theme tokens and their translation to `egui::Visuals`.
//!
//! Scripts own the palette: `ui.set_theme{ bg = "#17191c", ... }` stores a
//! token table which [`apply`] turns into a complete `Visuals` (flat fills,
//! 1 px strokes, no shadows). Fonts are loaded from `<project>/fonts/*.ttf`
//! when present; three named families — `heading`, `ui`, `mono` — always
//! exist so scripts can reference them regardless.

use std::collections::HashMap;

use balaur_core::project::ProjectRoot;
use balaur_core::Engine;
use egui::{Color32, CornerRadius, FontFamily, Shadow, Stroke};

#[derive(Clone)]
pub struct ThemeTokens {
    pub dark: bool,
    pub colors: HashMap<String, Color32>,
}

impl Default for ThemeTokens {
    fn default() -> Self {
        ThemeTokens {
            dark: true,
            colors: HashMap::new(),
        }
    }
}

impl ThemeTokens {
    pub fn color(&self, name: &str, fallback: Color32) -> Color32 {
        self.colors.get(name).copied().unwrap_or(fallback)
    }
}

/// `#rrggbb` or `#rrggbbaa` to Color32.
pub fn parse_hex(hex: &str) -> Option<Color32> {
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

pub fn apply(tokens: &ThemeTokens, ctx: &egui::Context) {
    let c = |name: &str, fb: Color32| tokens.color(name, fb);
    let panel = c("panel", Color32::from_rgb(0x20, 0x24, 0x2a));
    let sunken = c("sunken", Color32::from_rgb(0x10, 0x12, 0x15));
    let raised = c("raised", Color32::from_rgb(0x2b, 0x30, 0x37));
    let line = c("line", Color32::from_rgb(0x34, 0x3a, 0x42));
    let line_soft = c("line_soft", Color32::from_rgb(0x2b, 0x30, 0x37));
    let text = c("text", Color32::from_rgb(0xee, 0xf1, 0xf4));
    let dim = c("dim", Color32::from_rgb(0xb0, 0xb8, 0xc0));
    let accent = c("accent", Color32::from_rgb(0xf0, 0xa2, 0x73));
    let accent_soft = c("accent_soft", Color32::from_rgb(0x3d, 0x24, 0x15));
    let code_bg = c("code_bg", Color32::from_rgb(0x13, 0x15, 0x18));

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
    visuals.code_bg_color = code_bg;
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
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, line_soft);
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
pub fn family(name: &str) -> FontFamily {
    match name {
        "heading" => FontFamily::Name("heading".into()),
        "mono" => FontFamily::Name("mono".into()),
        _ => FontFamily::Name("ui".into()),
    }
}

/// Register the three named families. When `<project>/fonts` ships TTFs
/// (Caprasimo, Figtree, JetBrains Mono per the design), they take priority;
/// otherwise the families alias egui's built-in fonts so everything still
/// renders.
pub fn install_fonts(engine: &Engine, ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut heading_chain: Vec<String> = Vec::new();
    let mut ui_chain: Vec<String> = Vec::new();
    let mut mono_chain: Vec<String> = Vec::new();

    if let Some(root) = engine.try_resource::<ProjectRoot>() {
        let dir = root.borrow().0.join("fonts");
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut files: Vec<_> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    matches!(
                        p.extension().and_then(|e| e.to_str()),
                        Some("ttf") | Some("otf")
                    )
                })
                .collect();
            files.sort();
            for path in files {
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                let name = stem.to_string();
                fonts.font_data.insert(
                    name.clone(),
                    std::sync::Arc::new(egui::FontData::from_owned(bytes)),
                );
                let lower = name.to_lowercase();
                if lower.contains("mono") {
                    mono_chain.push(name);
                } else if lower.contains("caprasimo") || lower.contains("display") {
                    heading_chain.push(name);
                } else {
                    ui_chain.push(name);
                }
                log::info!("ui: loaded font {stem}");
            }
        }
    }

    // On macOS, add Apple Symbols (loaded from the system, not bundled) as a
    // fallback so geometric icon glyphs used by tools/editors resolve.
    #[cfg(target_os = "macos")]
    {
        let path = "/System/Library/Fonts/Apple Symbols.ttf";
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                "apple-symbols".into(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            for chain in [&mut heading_chain, &mut ui_chain, &mut mono_chain] {
                chain.push("apple-symbols".into());
            }
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                if let Some(chain) = fonts.families.get_mut(&family) {
                    chain.push("apple-symbols".into());
                }
            }
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
        heading_chain = ui_chain.clone();
    }
    heading_chain.extend(default_prop.clone());
    ui_chain.extend(default_prop);
    mono_chain.extend(default_mono);

    fonts
        .families
        .insert(FontFamily::Name("heading".into()), heading_chain);
    fonts.families.insert(FontFamily::Name("ui".into()), ui_chain);
    fonts
        .families
        .insert(FontFamily::Name("mono".into()), mono_chain);
    ctx.set_fonts(fonts);
}
