//! The `ui` Lua module: panels, containers, and the design system's widget
//! shapes (pill, circle button, field, toggle, slider, code line, modal).
//! Every color arrives per call from the script's token table, so themes are
//! entirely script-defined.

use anyhow::Result;
use balaur_core::{App, Engine};
use balaur_script::{Bindings, CallbackId, Value};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Margin, Sense, Stroke, StrokeKind};

use crate::bridge::{scale, scoped, with_ui};
use crate::theme::{self, parse_hex};
use crate::UiState;

// ---------------------------------------------------------------- opts

/// An options table as passed from script: `{ height = 56, fill = "#20242a" }`.
///
/// Reads the neutral map rather than a Lua table, so widgets name no language.
/// A missing or wrong-typed key falls back to the default: a typo in an options
/// table should not stop the frame.
pub(crate) struct Opts(pub(crate) Option<Value>);

impl Opts {
    fn get(&self, key: &str) -> Option<&Value> {
        match self.0.as_ref()? {
            Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub(crate) fn f32(&self, key: &str, default: f32) -> f32 {
        match self.get(key) {
            Some(Value::Num(n)) => *n as f32,
            Some(Value::Int(i)) => *i as f32,
            _ => default,
        }
    }
    pub(crate) fn boolean(&self, key: &str, default: bool) -> bool {
        matches!(self.get(key), Some(Value::Bool(b)) if *b) || {
            !matches!(self.get(key), Some(Value::Bool(_))) && default
        }
    }
    pub(crate) fn string(&self, key: &str) -> Option<String> {
        match self.get(key) {
            Some(Value::Str(s)) => Some(s.clone()),
            _ => None,
        }
    }
    pub(crate) fn color(&self, key: &str, default: Color32) -> Color32 {
        self.string(key)
            .and_then(|s| parse_hex(&s))
            .unwrap_or(default)
    }
    pub(crate) fn opt_color(&self, key: &str) -> Option<Color32> {
        self.string(key).and_then(|s| parse_hex(&s))
    }
    /// A dimension in design pixels, multiplied by the global UI scale.
    pub(crate) fn px(&self, key: &str, default: f32) -> f32 {
        self.f32(key, default) * scale()
    }
    pub(crate) fn callback(&self, key: &str) -> Option<CallbackId> {
        match self.get(key) {
            Some(Value::Callback(id)) => Some(*id),
            _ => None,
        }
    }
}

/// Scale a literal design dimension.
pub(crate) fn sc(v: f32) -> f32 {
    v * scale()
}

pub(crate) fn pill_radius(h: f32) -> CornerRadius {
    CornerRadius::same((h / 2.0).min(127.0) as u8)
}

pub(crate) fn panel_frame(opts: &Opts) -> egui::Frame {
    let px = opts.px("padding_x", 0.0);
    let py = opts.px("padding_y", 0.0);
    let mut frame = egui::Frame::new().inner_margin(Margin::symmetric(px as i8, py as i8));
    if let Some(fill) = opts.opt_color("fill") {
        frame = frame.fill(fill);
    } else if opts.boolean("transparent", false) {
        frame = frame.fill(Color32::TRANSPARENT);
    }
    frame
}

pub(crate) fn text(
    label: &str,
    size: f32,
    fam: &str,
    color: Option<Color32>,
    strong: bool,
) -> egui::RichText {
    let mut rt = egui::RichText::new(label)
        .size(size)
        .family(theme::family(fam));
    if let Some(color) = color {
        rt = rt.color(color);
    }
    if strong {
        rt = rt.strong();
    }
    rt
}

// ---------------------------------------------------------------- install

pub(crate) fn install(app: &mut App) -> Result<()> {
    let host = app.engine.scripts().expect("script host present");
    let mut module = host.module("ui")?;
    let m: &mut dyn Bindings<Engine> = &mut module;

    crate::widget_bindings::install_theme(m);
    crate::widget_bindings::install_panels(m);
    crate::widget_bindings::install_containers(m);
    crate::widget_bindings::install_text(m);
    crate::widget_bindings::install_buttons(m);
    crate::widget_bindings::install_controls(m);
    crate::widget_bindings::install_text_input(m);
    crate::widget_bindings::install_code(m);
    crate::widget_bindings::install_modal(m);
    crate::widget_bindings::install_widget_layer(m);
    crate::widget_bindings::install_scale(m);
    crate::widget_bindings::install_code_editor(m);
    crate::widget_bindings::install_dropdown_select(m);
    crate::widget_bindings::install_images(m);
    crate::widget_bindings::install_queries(m);

    Ok(())
}

pub(crate) fn text_field(
    engine: &Engine,
    id: &str,
    placeholder: &str,
    opts: &Opts,
) -> anyhow::Result<(String, bool, bool)> {
    let state = engine.resource::<UiState>();
    let mut buffer = {
        let state = state.borrow();
        state.text_buffers.get(id).cloned().unwrap_or_default()
    };
    let autofocus = opts.boolean("autofocus", false) && {
        let mut state = state.borrow_mut();
        state.focused_once.insert(id.to_string())
    };
    let id_owned = id.to_string();
    let result = with_ui(|ui| {
        let size = opts.px("size", 13.0);
        let mut edit = egui::TextEdit::singleline(&mut buffer)
            .id(egui::Id::new(id_owned.clone()))
            .frame(egui::Frame::NONE)
            .hint_text(placeholder)
            .font(FontId::new(size, theme::family("ui")));
        if let Some(color) = opts.opt_color("color") {
            edit = edit.text_color(color);
        }
        let w = opts.px("width", 0.0);
        if w > 0.0 {
            edit = edit.desired_width(w);
        }
        let response = ui.add(edit);
        if autofocus {
            response.request_focus();
        }
        let submitted = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        Ok((response.changed(), submitted))
    })?;
    let (changed, submitted) = result;
    state
        .borrow_mut()
        .text_buffers
        .insert(id.to_string(), buffer.clone());
    Ok((buffer, changed, submitted))
}

// ---------------------------------------------------------------- code editor

pub(crate) struct SyntaxColors {
    key: Color32,
    string: Color32,
    number: Color32,
    comment: Color32,
    ident: Color32,
    builtin: Color32,
    punct: Color32,
}

impl SyntaxColors {
    pub(crate) fn from_opts(opts: &Opts) -> Self {
        Self {
            key: opts.color("k_key", Color32::from_rgb(0xf0, 0xa2, 0x73)),
            string: opts.color("k_str", Color32::from_rgb(0x8f, 0xbc, 0xae)),
            number: opts.color("k_num", Color32::from_rgb(0xff, 0xc7, 0xa8)),
            comment: opts.color("k_com", Color32::from_rgb(0x76, 0x7e, 0x88)),
            ident: opts.color("k_fn", Color32::from_rgb(0xee, 0xf1, 0xf4)),
            builtin: opts.color("k_type", Color32::from_rgb(0xb6, 0xd8, 0xcc)),
            punct: opts.color("k_punc", Color32::from_rgb(0x98, 0xa1, 0xaa)),
        }
    }
}

pub(crate) const LUAU_KEYWORDS: &[&str] = &[
    "local", "function", "end", "if", "then", "else", "elseif", "while", "for", "in", "do",
    "return", "and", "or", "not", "true", "false", "nil", "break", "continue", "repeat", "until",
    "type", "export",
];
pub(crate) const LUAU_BUILTINS: &[&str] = &[
    "engine", "scene", "input", "physics", "render", "audio", "rng", "ui", "log", "fs", "toml",
    "math", "string", "table", "self", "require",
];

/// Line-based Luau highlighting into a LayoutJob (used by the editable code
/// editor's layouter; mirrors the design handoff's tokenizer rules).
pub(crate) fn highlight_luau(
    text_src: &str,
    font: &FontId,
    colors: &SyntaxColors,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let fmt = |color: Color32| egui::TextFormat::simple(font.clone(), color);
    for (i, line) in text_src.split('\n').enumerate() {
        if i > 0 {
            job.append("\n", 0.0, fmt(colors.punct));
        }
        if line.trim_start().starts_with("--") {
            job.append(line, 0.0, fmt(colors.comment));
            continue;
        }
        let bytes = line.as_bytes();
        let mut pos = 0;
        while pos < bytes.len() {
            let rest = &line[pos..];
            // pos < bytes.len(), so rest is non-empty.
            let c = rest.chars().next().unwrap();
            let (token_len, color) = if c.is_whitespace() {
                (
                    rest.chars()
                        .take_while(|c| c.is_whitespace())
                        .map(char::len_utf8)
                        .sum(),
                    colors.ident,
                )
            } else if c == '"' || c == '\'' {
                let quote = c;
                let mut len = c.len_utf8();
                for ch in rest[len..].chars() {
                    len += ch.len_utf8();
                    if ch == quote {
                        break;
                    }
                }
                (len, colors.string)
            } else if c.is_ascii_digit() {
                (
                    rest.chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '.')
                        .map(char::len_utf8)
                        .sum(),
                    colors.number,
                )
            } else if c.is_alphabetic() || c == '_' {
                let len: usize = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .map(char::len_utf8)
                    .sum();
                let word = &rest[..len];
                let color = if LUAU_KEYWORDS.contains(&word) {
                    colors.key
                } else if LUAU_BUILTINS.contains(&word)
                    || word.chars().next().is_some_and(char::is_uppercase)
                {
                    colors.builtin
                } else {
                    colors.ident
                };
                (len, color)
            } else {
                (c.len_utf8(), colors.punct)
            };
            job.append(&rest[..token_len], 0.0, fmt(color));
            pos += token_len;
        }
    }
    job
}

/// An editable, syntax-highlighted code editor with a line-number gutter.
/// The buffer persists per `id` in `UiState`; returns (text, changed).
pub(crate) fn code_editor(
    engine: &Engine,
    id: &str,
    source: &str,
    opts: &Opts,
) -> anyhow::Result<(String, bool)> {
    let state = engine.resource::<UiState>();
    let mut buffer = {
        let cached = state.borrow().text_buffers.get(id).cloned();
        if let Some(b) = cached {
            b
        } else {
            state
                .borrow_mut()
                .text_buffers
                .insert(id.to_string(), source.to_string());
            source.to_string()
        }
    };
    let size = opts.px("size", 12.5);
    let gutter_w = opts.px("gutter_width", 34.0);
    let gutter_color = opts.color("gutter_color", Color32::from_rgb(0x76, 0x7e, 0x88));
    let colors = SyntaxColors::from_opts(opts);
    let font = FontId::new(size, theme::family("mono"));
    let changed = with_ui(|ui| {
        let font = font.clone();
        let row_h = ui
            .painter()
            .layout_no_wrap("0".into(), font.clone(), gutter_color)
            .size()
            .y;
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            // Gutter.
            let n_lines = buffer.split('\n').count().max(1);
            let (gutter_rect, _) =
                ui.allocate_exact_size(vec2(gutter_w, row_h * n_lines as f32), Sense::hover());
            for i in 0..n_lines {
                ui.painter().text(
                    pos2(
                        gutter_rect.max.x - sc(6.0),
                        gutter_rect.min.y + row_h * (i as f32 + 0.5),
                    ),
                    Align2::RIGHT_CENTER,
                    (i + 1).to_string(),
                    FontId::new((size - sc(1.5)).max(8.0), theme::family("mono")),
                    gutter_color,
                );
            }
            ui.add_space(sc(12.0));
            // Editor.
            let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                let mut job = highlight_luau(buf.as_str(), &font, &colors);
                job.wrap.max_width = f32::INFINITY;
                ui.fonts_mut(|f| f.layout_job(job))
            };
            let response = ui.add(
                egui::TextEdit::multiline(&mut buffer)
                    .id(egui::Id::new(id.to_string()))
                    .frame(egui::Frame::NONE)
                    .desired_width(ui.available_width())
                    .layouter(&mut layouter),
            );
            changed = response.changed();
        });
        Ok(changed)
    })?;
    state
        .borrow_mut()
        .text_buffers
        .insert(id.to_string(), buffer.clone());
    Ok((buffer, changed))
}

/// Draw a PNG from the project (cached as an egui texture by path).
pub(crate) fn draw_image(engine: &Engine, path: &str, opts: &Opts) -> anyhow::Result<()> {
    let state = engine.resource::<UiState>();
    let cached = state.borrow().textures.get(path).cloned();
    with_ui(|ui| {
        let texture = if let Some(t) = cached {
            t
        } else {
            let full = match engine.try_resource::<balaur_core::project::ProjectRoot>() {
                Some(root) if !std::path::Path::new(path).is_absolute() => {
                    root.borrow().0.join(path)
                }
                _ => std::path::PathBuf::from(path),
            };
            let dynamic = image::open(&full)?;
            let rgba = dynamic.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
            let handle = ui
                .ctx()
                .load_texture(path, color, egui::TextureOptions::LINEAR);
            state
                .borrow_mut()
                .textures
                .insert(path.to_string(), handle.clone());
            handle
        };
        let native = texture.size_vec2();
        let aspect = if native.y > 0.0 {
            native.x / native.y
        } else {
            1.0
        };
        let w = opts.px("width", 0.0);
        let h = opts.px("height", 0.0);
        let size = if w > 0.0 && h > 0.0 {
            vec2(w, h)
        } else if h > 0.0 {
            vec2(h * aspect, h)
        } else if w > 0.0 {
            vec2(w, w / aspect)
        } else {
            native
        };
        let radius = opts.px("radius", 0.0);
        let padding = opts.px("padding", 0.0);
        if let Some(bg) = opts.opt_color("bg") {
            // Backing plate (e.g. white circle behind a logo) with padding.
            let total = size + vec2(padding * 2.0, padding * 2.0);
            let (rect, _) = ui.allocate_exact_size(total, Sense::hover());
            ui.painter().rect_filled(
                rect,
                if radius > 0.0 {
                    pill_radius((radius + padding) * 2.0)
                } else {
                    pill_radius(total.y)
                },
                bg,
            );
            let inner = egui::Rect::from_center_size(rect.center(), size);
            let mut img = egui::Image::new((texture.id(), size));
            if radius > 0.0 {
                img = img.corner_radius(pill_radius(radius * 2.0));
            }
            img.paint_at(ui, inner);
            return Ok(());
        }
        let mut img = egui::Image::new((texture.id(), size));
        if radius > 0.0 {
            img = img.corner_radius(pill_radius(radius * 2.0));
        }
        ui.add(img);
        Ok(())
    })
}

/// A left-aligned pill row (tree rows, list rows, menu items): custom paint
/// so the icon and label hug the left edge instead of egui's centered
/// button text. Supports fill/stroke, a colored leading icon, an optional
/// trailing glyph on the right, tooltip and a context menu.
// Returns Result so the widget helpers share one signature at the binding site.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn left_pill(
    engine: &Engine,
    ui: &mut egui::Ui,
    label: &str,
    opts: &Opts,
) -> anyhow::Result<bool> {
    let h = opts.px("height", 27.0);
    let w = {
        let w = opts.px("min_width", 0.0);
        if w > 0.0 {
            w
        } else {
            ui.available_width().max(sc(40.0))
        }
    };
    let (rect, mut response) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let hovered = response.hovered();
    let mut fill = opts.color("fill", Color32::TRANSPARENT);
    if hovered && fill == Color32::TRANSPARENT {
        if let Some(hover) = opts.opt_color("hover_fill") {
            fill = hover;
        }
    }
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, pill_radius(h), fill);
    }
    if let Some(stroke) = opts.opt_color("stroke") {
        ui.painter().rect(
            rect,
            pill_radius(h),
            Color32::TRANSPARENT,
            Stroke::new(1.0, stroke),
            StrokeKind::Inside,
        );
    }
    let fam = opts.string("font").unwrap_or_else(|| "ui".into());
    let size = opts.px("size", 12.0);
    let color = opts.color("color", Color32::WHITE);
    let mut x = rect.min.x + sc(10.0);
    if let Some(icon) = opts.string("icon") {
        let icon_color = opts.opt_color("icon_color").unwrap_or(color);
        let galley = ui.painter().layout_no_wrap(
            icon,
            FontId::new(opts.px("icon_size", 12.0), theme::family(&fam)),
            icon_color,
        );
        let y = rect.center().y - galley.size().y / 2.0;
        ui.painter().galley(pos2(x, y), galley, icon_color);
        x += sc(7.0) + opts.px("icon_size", 12.0);
    }
    let mut font = FontId::new(size, theme::family(&fam));
    if opts.boolean("strong", false) {
        font = FontId::new(
            size,
            theme::family(if fam == "ui" { "heading" } else { &fam }),
        );
    }
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, color);
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter().galley(pos2(x, y), galley, color);
    if let Some(trailing) = opts.string("trailing") {
        let t_color = opts.opt_color("trailing_color").unwrap_or(color);
        let galley = ui.painter().layout_no_wrap(
            trailing,
            FontId::new(opts.px("trailing_size", 11.0), theme::family(&fam)),
            t_color,
        );
        let ty = rect.center().y - galley.size().y / 2.0;
        ui.painter().galley(
            pos2(rect.max.x - sc(11.0) - galley.size().x, ty),
            galley,
            t_color,
        );
    }
    if let Some(tip) = opts.string("tooltip") {
        response = response.on_hover_text(tip);
    }
    if let Some(menu) = opts.callback("menu") {
        response.context_menu(|ui| {
            let _ = scoped(engine, ui, menu);
        });
    }
    Ok(response.clicked())
}
