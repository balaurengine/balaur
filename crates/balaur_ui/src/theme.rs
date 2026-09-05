//! Theme tokens and their translation to `egui::Visuals`.
//!
//! Scripts own the palette: `ui.set_theme{ bg = "#17191c", ... }` stores a
//! token table which [`apply`] turns into a complete `Visuals` (flat fills,
//! 1 px strokes, no shadows). Fonts are loaded from `<project>/fonts/*.ttf`
//! when present; three named families — `heading`, `ui`, `mono` — always
//! exist so scripts can reference them regardless.

use std::collections::HashMap;

use balaur_core::Engine;
use balaur_core::project::ProjectFiles;
use egui::{Color32, CornerRadius, FontFamily, Shadow, Stroke};
use crate::vocabulary::words as w;

#[derive(Clone)]
pub struct ThemeTokens {
    pub dark: bool,
    pub colors: HashMap<String, Color32>,
    /// Named looks — `field`, `tab`, `heading` — as the option map a widget
    /// would otherwise have been given at the call site. Colour entries are
    /// already resolved from token name to `#rrggbb`.
    pub roles: HashMap<String, Vec<(String, balaur_script::Value)>>,
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
        self.roles.get(name).map_or(&[], Vec::as_slice)
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
pub(crate) fn family(name: &str) -> FontFamily {
    match name {
        w::HEADING => FontFamily::Name(w::HEADING.into()),
        w::MONO => FontFamily::Name(w::MONO.into()),
        w::ICON => FontFamily::Name(w::ICON.into()),
        _ => FontFamily::Name(w::UI.into()),
    }
}

/// Load the three named families into `ctx`. When `<project>/fonts` ships TTFs
/// (Caprasimo, Figtree, JetBrains Mono per the design), they take priority;
/// otherwise the families alias egui's built-in fonts so everything still
/// renders.
/// Faces the operating system already ships, appended to every chain so a
/// script balaur does not vendor draws instead of tofu. Never bundled: a
/// single CJK weight is larger than the whole editor.
#[cfg(target_os = "macos")]
const SYSTEM_FACES: &[&str] = &[
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    "/System/Library/Fonts/GeezaPro.ttc",
    "/System/Library/Fonts/ArialHB.ttc",
    "/System/Library/Fonts/Kohinoor.ttc",
    "/System/Library/Fonts/ThonburiUI.ttc",
    "/System/Library/Fonts/Apple Symbols.ttf",
];

#[cfg(target_os = "windows")]
const SYSTEM_FACES: &[&str] = &[
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\YuGothM.ttc",
    "C:\\Windows\\Fonts\\malgun.ttf",
    "C:\\Windows\\Fonts\\msyh.ttc",
    "C:\\Windows\\Fonts\\seguisym.ttf",
];

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const SYSTEM_FACES: &[&str] = &[
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf",
];

/// Which chain a bundled file joins, from its name. Explicit, because a
/// guess put every new face in the UI chain and nothing said so.
fn chain_of(stem: &str) -> &'static str {
    let lower = stem.to_lowercase();
    for prefix in ["heading", "ui", "mono", "icons"] {
        if lower.starts_with(&format!("{prefix}-")) {
            return match prefix {
                "heading" => "heading",
                "mono" => "mono",
                "icons" => "icons",
                _ => "ui",
            };
        }
    }
    // Pre-convention names, kept so an existing project's fonts still load.
    if lower.contains("mono") {
        return "mono";
    }
    if lower.contains("caprasimo") || lower.contains("display") {
        return "heading";
    }
    "ui"
}

/// One face the theme found, and which chain it joins.
pub(crate) struct FontFace {
    pub(crate) name: String,
    /// `heading`, `ui`, `mono`, `icons`, or `system` for an OS face.
    pub(crate) chain: &'static str,
    pub(crate) bytes: std::sync::Arc<Vec<u8>>,
}

/// The faces the operating system ships, whichever of them are present.
pub(crate) fn system_faces() -> Vec<FontFace> {
    SYSTEM_FACES
        .iter()
        .filter_map(|path| {
            let bytes = std::fs::read(path).ok()?;
            Some(FontFace {
                name: format!("system:{path}"),
                chain: "system",
                bytes: std::sync::Arc::new(bytes),
            })
        })
        .collect()
}

/// Every face, in chain order: a project's own `fonts/*.ttf` first, then the
/// system's. Read once here for both egui and the shaper.
pub(crate) fn font_faces(eng: &Engine) -> Vec<FontFace> {
    let mut faces = Vec::new();
    if let Some(files) = eng.try_resource::<ProjectFiles>() {
        let files = files.borrow();
        let mut paths: Vec<String> = files
            .list("fonts")
            .into_iter()
            .filter(|p| {
                matches!(
                    std::path::Path::new(p).extension().and_then(|e| e.to_str()),
                    Some("ttf" | "otf" | "ttc")
                )
            })
            .collect();
        paths.sort();
        for rel in paths {
            let path = std::path::PathBuf::from(&rel);
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(bytes) = files.read(&rel) else {
                continue;
            };
            faces.push(FontFace {
                name: stem.to_string(),
                chain: chain_of(stem),
                bytes: std::sync::Arc::new(bytes),
            });
            tracing::info!("ui: loaded font {stem}");
        }
    }
    faces.extend(system_faces());
    faces
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
        fonts.font_data.insert(
            face.name.clone(),
            std::sync::Arc::new(egui::FontData::from_owned((*face.bytes).clone())),
        );
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
    for chain in [&mut heading_chain, &mut ui_chain, &mut mono_chain] {
        chain.extend(icon_chain.iter().cloned());
    }
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
    ui_chain.extend(default_prop);
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
