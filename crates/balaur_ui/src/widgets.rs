//! The `ui` module: panels, containers, and the design system's widget
//! shapes (pill, circle button, field, toggle, slider, code line, modal).
//! Every color arrives per call from the script's token table, so themes are
//! entirely script-defined.

use anyhow::Result;
use balaur_core::Engine;
use balaur_plugin::Registry;
use balaur_script::{Bindings, CallbackId, Value};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Margin, Sense, Stroke, StrokeKind};

use crate::bridge::{scale, scoped, with_ui};
use crate::theme::{self, parse_hex};
use crate::UiState;

/// An options table as passed from script: `{ height = 56, fill = "#20242a" }`.
///
/// Reads the neutral map rather than one language's table type, so widgets
/// name no language.
/// A missing or wrong-typed key falls back to the default: a typo in an options
/// table should not stop the frame.
pub(crate) struct Opts(pub(crate) Option<Value>);

impl Opts {
    /// The caller's options over the named role's. A call site then says only
    /// what it changes, and the look lives in the theme asset.
    pub(crate) fn with_roles(opts: Option<Value>) -> Self {
        let Some(Value::Map(given)) = opts.as_ref() else {
            return Self(opts);
        };
        let Some(Value::Str(name)) = given.iter().find(|(k, _)| k == "role").map(|(_, v)| v) else {
            return Self(opts);
        };
        let defaults = crate::bridge::role(name);
        if defaults.is_empty() {
            return Self(opts);
        }
        // The caller's entries first: `get` takes the first match.
        let mut merged = given.clone();
        for (key, value) in defaults {
            if !merged.iter().any(|(k, _)| *k == key) {
                merged.push((key, value));
            }
        }
        Self(Some(Value::Map(merged)))
    }

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
    /// A dimension in design pixels when the caller gave one, so a window can
    /// tell "put it here" apart from "wherever you left it".
    pub(crate) fn opt_px(&self, key: &str) -> Option<f32> {
        match self.get(key) {
            Some(Value::Num(n)) => Some(*n as f32 * scale()),
            Some(Value::Int(i)) => Some(*i as f32 * scale()),
            _ => None,
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
    /// A colour as four unit floats, defaulting to opaque white: the shape a
    /// schema's `color` property already stores.
    pub(crate) fn unit_rgba(&self, key: &str) -> [f32; 4] {
        let mut out = [1.0f32; 4];
        if let Some(Value::List(items)) = self.get(key) {
            for (slot, item) in out.iter_mut().zip(items) {
                *slot = match item {
                    Value::Num(n) => *n as f32,
                    Value::Int(i) => *i as f32,
                    _ => *slot,
                }
                .clamp(0.0, 1.0);
            }
        }
        out
    }
    pub(crate) fn callback(&self, key: &str) -> Option<CallbackId> {
        match self.get(key) {
            Some(Value::Callback(id)) => Some(*id),
            _ => None,
        }
    }
    /// A list of line numbers, for gutter markers.
    pub(crate) fn lines(&self, key: &str) -> Vec<usize> {
        match self.get(key) {
            Some(Value::List(items)) => items
                .iter()
                .filter_map(|v| match v {
                    Value::Int(i) => usize::try_from(*i).ok(),
                    Value::Num(n) if *n >= 1.0 => Some(*n as usize),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
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

/// Screen anchors for the `widget` scene component, so a script says
/// `ui.ANCHOR_TOP_LEFT` rather than spelling the string.
pub const ANCHORS: &[(&str, &str)] = &[
    ("ANCHOR_TOP_LEFT", "top_left"),
    ("ANCHOR_TOP_RIGHT", "top_right"),
    ("ANCHOR_BOTTOM_LEFT", "bottom_left"),
    ("ANCHOR_BOTTOM_RIGHT", "bottom_right"),
    ("ANCHOR_CENTER", "center"),
];

/// Widget kinds the layer draws.
pub const WIDGET_KINDS: &[(&str, &str)] = &[
    ("WIDGET_LABEL", "label"),
    ("WIDGET_BUTTON", "button"),
    ("WIDGET_PANEL", "panel"),
    ("WIDGET_ROW", "row"),
    ("WIDGET_COLUMN", "column"),
    ("WIDGET_SCROLL", "scroll"),
    ("WIDGET_TAB", "tab"),
    ("WIDGET_DRAW", "draw"),
];

/// Font families the theme registers.
pub const FONTS: &[(&str, &str)] = &[("FONT_MONO", "mono"), ("FONT_HEADING", "heading")];

/// Keyboard modifiers accepted by shortcut bindings.
pub const MODIFIERS: &[(&str, &str)] = &[
    ("MOD_CMD", "cmd"),
    ("MOD_CTRL", "ctrl"),
    ("MOD_ALT", "alt"),
    ("MOD_SHIFT", "shift"),
];

/// Declare `ui.*`. Takes the `Registry` rather than a module because it opens
/// the module itself: through the registry, so a scriptless app builds the
/// plugin instead of panicking.
pub(crate) fn install_ui_api(reg: &mut Registry<'_>) -> Result<()> {
    let mut m = reg.script_module("ui")?;
    let m: &mut dyn Bindings<Engine> = &mut *m;

    m.module_doc(
        "Immediate-mode UI, redrawn from a script's `draw_ui` every frame: \
         panels, layout containers and the design system's widget shapes. \
         HUD elements that live in the scene tree are the `widget` component \
         instead.",
    );

    for (name, value) in ANCHORS
        .iter()
        .chain(WIDGET_KINDS)
        .chain(FONTS)
        .chain(MODIFIERS)
    {
        m.constant(name, balaur_script::Value::Str((*value).to_string()));
    }
    crate::widget_bindings::install_theme(m);
    crate::widget_bindings::install_panels(m);
    crate::widget_bindings::install_containers(m);
    crate::widget_bindings::install_text(m);
    crate::widget_bindings::install_buttons(m);
    crate::widget_bindings::install_controls(m);
    crate::widget_bindings::install_text_input(m);
    crate::widget_bindings::install_code(m);
    crate::widget_bindings::install_modal(m);
    crate::widget_bindings::install_window(m);
    crate::widget_bindings::install_widget_layer(m);
    crate::widget_bindings::install_scale(m);
    crate::widget_bindings::install_code_editor(m);
    crate::widget_bindings::install_dropdown_select(m);
    crate::widget_bindings::install_images(m);
    crate::widget_bindings::install_queries(m);

    Ok(())
}

pub(crate) fn text_field(
    eng: &Engine,
    id: &str,
    placeholder: &str,
    opts: &Opts,
) -> anyhow::Result<(String, bool, bool)> {
    let state = eng.resource::<UiState>();
    // `value` makes the field show what it edits: the buffer is seeded from it
    // and re-seeded whenever the source changes underneath. Without it the
    // buffer is the only truth, which is what a search box wants.
    let seed = opts.string("value");
    let mut buffer = {
        let mut state = state.borrow_mut();
        if let Some(value) = seed {
            if state.text_seeds.get(id) != Some(&value) {
                state.text_seeds.insert(id.to_string(), value.clone());
                state.text_buffers.insert(id.to_string(), value);
            }
        }
        state.text_buffers.get(id).cloned().unwrap_or_default()
    };
    let autofocus = opts.boolean("autofocus", false) && {
        let mut state = state.borrow_mut();
        state.focused_once.insert(id.to_string())
    };
    let id_owned = id.to_string();
    let result = with_ui(|ui| {
        let size = opts.px("size", 13.0);
        let family = opts
            .string("font")
            .map_or_else(|| theme::family("ui"), |name| theme::family(&name));
        let mut edit = egui::TextEdit::singleline(&mut buffer)
            .id(egui::Id::new(id_owned.clone()))
            .frame(egui::Frame::NONE)
            .hint_text(placeholder)
            .font(FontId::new(size, family));
        if let Some(color) = opts.opt_color("color") {
            edit = edit.text_color(color);
        }
        // A `height` asks for the pill shell every other inspector control
        // wears; its padding comes out of the width the caller asked for.
        let h = opts.px("height", 0.0);
        let pad = if h > 0.0 { sc(11.0) } else { 0.0 };
        let w = opts.px("width", 0.0);
        if w > 0.0 {
            edit = edit.desired_width((w - pad * 2.0).max(sc(8.0)));
        }
        let response = if h > 0.0 {
            let radius = opts.px("radius", 0.0);
            let corner = if radius > 0.0 {
                pill_radius(radius * 2.0)
            } else {
                pill_radius(sc(5.0) * 2.0)
            };
            egui::Frame::new()
                .fill(opts.color("fill", Color32::TRANSPARENT))
                .stroke(Stroke::new(1.0, opts.color("stroke", Color32::TRANSPARENT)))
                .corner_radius(corner)
                .inner_margin(Margin::symmetric(pad as i8, 0))
                .show(ui, |ui| {
                    ui.set_min_height(h);
                    ui.add(edit)
                })
                .inner
        } else {
            ui.add(edit)
        };
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

/// What the highlighter needs to know about one language. The tokenizer is
/// shared; only these differ.
pub(crate) struct Syntax {
    line_comment: &'static str,
    keywords: &'static [&'static str],
    /// Every script module (`docs/generated/script-api.md`) plus the
    /// language's own globals.
    builtins: &'static [&'static str],
}

const RUNE: Syntax = Syntax {
    line_comment: "//",
    keywords: &[
        "fn", "let", "if", "else", "while", "for", "in", "loop", "match", "struct", "impl", "pub",
        "return", "break", "continue", "const", "async", "await", "use", "mod", "true", "false",
        "not", "is", "as", "select", "yield",
    ],
    builtins: &[
        "engine",
        "scene",
        "input",
        "physics",
        "physics2d",
        "render",
        "audio",
        "rng",
        "ui",
        "log",
        "node",
        "fs",
        "toml",
        "this",
        "println",
    ],
};

/// Shaders: WGSL plus what WESL adds to it (`import`, the `@if` family).
const WESL: Syntax = Syntax {
    line_comment: "//",
    keywords: &[
        "fn",
        "let",
        "var",
        "const",
        "if",
        "else",
        "switch",
        "case",
        "default",
        "loop",
        "for",
        "while",
        "break",
        "continue",
        "return",
        "discard",
        "struct",
        "alias",
        "override",
        "const_assert",
        "true",
        "false",
        "enable",
        "requires",
        "diagnostic",
        "import",
        "as",
    ],
    builtins: &[
        "f32",
        "i32",
        "u32",
        "bool",
        "vec2",
        "vec3",
        "vec4",
        "vec2f",
        "vec3f",
        "vec4f",
        "mat2x2",
        "mat3x3",
        "mat4x4",
        "array",
        "atomic",
        "ptr",
        "sampler",
        "texture_2d",
        "texture_cube",
        "textureSample",
        "textureLoad",
        "normalize",
        "length",
        "distance",
        "dot",
        "cross",
        "mix",
        "clamp",
        "min",
        "max",
        "abs",
        "sign",
        "floor",
        "ceil",
        "fract",
        "step",
        "smoothstep",
        "select",
        "sin",
        "cos",
        "tan",
        "pow",
        "exp",
        "log",
        "sqrt",
        "inverseSqrt",
    ],
};

/// The highlighter for a language name.
///
/// An unknown name falls back to Rune rather than rendering the editor as
/// plain punctuation.
pub(crate) const fn syntax_for(language: &str) -> &'static Syntax {
    // `match` on a `&str` is not const, and two languages do not warrant a
    // table.
    if matches!(language.as_bytes(), b"wesl" | b"wgsl") {
        &WESL
    } else {
        &RUNE
    }
}

/// The lines a checker flagged, underlined in the text and dotted in the
/// gutter. Two flat lists rather than rows: a line is the whole anchor the
/// editor has, and severity is which list it is in.
pub(crate) struct Marks {
    errors: Vec<usize>,
    warnings: Vec<usize>,
    error_color: Color32,
    warning_color: Color32,
}

impl Marks {
    fn from_opts(opts: &Opts) -> Self {
        Self {
            errors: opts.lines("problems"),
            warnings: opts.lines("warnings"),
            error_color: opts.color("problem_color", Color32::from_rgb(0xe0, 0x4a, 0x4a)),
            warning_color: opts.color("warning_color", Color32::from_rgb(0xe0, 0xb0, 0x4a)),
        }
    }

    /// The colour a 1-based line is flagged in; an error outranks a warning.
    fn color(&self, line: usize) -> Option<Color32> {
        if self.errors.contains(&line) {
            Some(self.error_color)
        } else if self.warnings.contains(&line) {
            Some(self.warning_color)
        } else {
            None
        }
    }
}

/// Line-based highlighting into a LayoutJob (used by the editable code
/// editor's layouter; mirrors the design handoff's tokenizer rules).
pub(crate) fn highlight(
    text_src: &str,
    syntax: &Syntax,
    font: &FontId,
    colors: &SyntaxColors,
    marks: &Marks,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let fmt = |color: Color32, underline: egui::Stroke| egui::TextFormat {
        font_id: font.clone(),
        color,
        underline,
        ..Default::default()
    };
    for (i, line) in text_src.split('\n').enumerate() {
        if i > 0 {
            job.append("\n", 0.0, fmt(colors.punct, egui::Stroke::NONE));
        }
        let underline = marks.color(i + 1).map_or(egui::Stroke::NONE, |color| {
            egui::Stroke::new(sc(1.0), color)
        });
        if line.trim_start().starts_with(syntax.line_comment) {
            job.append(line, 0.0, fmt(colors.comment, underline));
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
                let color = if syntax.keywords.contains(&word) {
                    colors.key
                } else if syntax.builtins.contains(&word)
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
            job.append(&rest[..token_len], 0.0, fmt(color, underline));
            pos += token_len;
        }
    }
    job
}

/// An editable, syntax-highlighted code editor with a line-number gutter.
/// The buffer persists per `id` in `UiState`; returns (text, changed).
/// The column beside the code: line numbers, breakpoint dots, and the row
/// the debugger is stopped on.
struct Gutter {
    width: f32,
    color: Color32,
    size: f32,
    breakpoints: Vec<usize>,
    current_line: usize,
    breakpoint_color: Color32,
    current_fill: Color32,
    marks: Marks,
}

impl Gutter {
    fn from_opts(opts: &Opts, size: f32) -> Self {
        Self {
            width: opts.px("gutter_width", 34.0),
            color: opts.color("gutter_color", Color32::from_rgb(0x76, 0x7e, 0x88)),
            size,
            breakpoints: opts.lines("breakpoints"),
            current_line: opts.f32("current_line", 0.0).max(0.0) as usize,
            breakpoint_color: opts.color("breakpoint_color", Color32::from_rgb(0xe0, 0x4a, 0x4a)),
            current_fill: opts.color(
                "current_fill",
                Color32::from_rgba_unmultiplied(0xe0, 0xb0, 0x4a, 0x40),
            ),
            marks: Marks::from_opts(opts),
        }
    }

    /// Paint `n_lines` rows; returns the row clicked this frame, if any.
    fn paint(&self, ui: &mut egui::Ui, n_lines: usize, row_h: f32) -> Option<i64> {
        let (rect, response) =
            ui.allocate_exact_size(vec2(self.width, row_h * n_lines as f32), Sense::click());
        let clicked = response
            .interact_pointer_pos()
            .filter(|_| response.clicked())
            .map(|pos| ((pos.y - rect.min.y) / row_h).floor().max(0.0) as usize + 1)
            .filter(|line| *line <= n_lines)
            .and_then(|line| i64::try_from(line).ok());
        if self.current_line > 0 && self.current_line <= n_lines {
            let top = rect.min.y + row_h * (self.current_line - 1) as f32;
            let row = egui::Rect::from_min_max(
                pos2(rect.min.x, top),
                pos2(ui.max_rect().right(), top + row_h),
            );
            ui.painter()
                .rect_filled(row, CornerRadius::ZERO, self.current_fill);
        }
        let font = FontId::new((self.size - sc(1.5)).max(8.0), theme::family("mono"));
        for i in 0..n_lines {
            let center_y = rect.min.y + row_h * (i as f32 + 0.5);
            if self.breakpoints.contains(&(i + 1)) {
                ui.painter().circle_filled(
                    pos2(rect.min.x + sc(7.0), center_y),
                    sc(4.0),
                    self.breakpoint_color,
                );
            }
            // On the inner edge, so it reads beside the code and cannot be
            // taken for the breakpoint dot on the outer one.
            if let Some(color) = self.marks.color(i + 1) {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        pos2(rect.max.x - sc(2.0), center_y - row_h * 0.5),
                        vec2(sc(2.0), row_h),
                    ),
                    CornerRadius::ZERO,
                    color,
                );
            }
            ui.painter().text(
                pos2(rect.max.x - sc(6.0), center_y),
                Align2::RIGHT_CENTER,
                (i + 1).to_string(),
                font.clone(),
                self.color,
            );
        }
        clicked
    }
}

/// Returns the buffer, whether it changed, and the gutter line clicked this
/// frame, if any: `breakpoints` marks lines, `current_line` highlights one.
pub(crate) fn code_editor(
    eng: &Engine,
    id: &str,
    source: &str,
    opts: &Opts,
) -> anyhow::Result<(String, bool, Option<i64>)> {
    // `language` overrides; otherwise highlight whatever the project is
    // written in, so an editor shows Rune as Rune.
    let language = opts.string("language").unwrap_or_else(|| {
        eng.try_resource::<balaur_core::project::ProjectManifest>()
            .map_or_else(|| "rune".to_string(), |m| m.borrow().language.clone())
    });
    let syntax = syntax_for(&language);
    let state = eng.resource::<UiState>();
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
    let gutter = Gutter::from_opts(opts, size);
    let colors = SyntaxColors::from_opts(opts);
    let marks = Marks::from_opts(opts);
    let font = FontId::new(size, theme::family("mono"));
    let (changed, clicked) = with_ui(|ui| {
        let font = font.clone();
        let row_h = ui
            .painter()
            .layout_no_wrap("0".into(), font.clone(), gutter.color)
            .size()
            .y;
        let mut changed = false;
        let mut clicked = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let n_lines = buffer.split('\n').count().max(1);
            clicked = gutter.paint(ui, n_lines, row_h);
            ui.add_space(sc(12.0));
            let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                let mut job = highlight(buf.as_str(), syntax, &font, &colors, &marks);
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
        Ok((changed, clicked))
    })?;
    state
        .borrow_mut()
        .text_buffers
        .insert(id.to_string(), buffer.clone());
    Ok((buffer, changed, clicked))
}

/// Draw a PNG from the project (cached as an egui texture by path).
pub(crate) fn draw_image(eng: &Engine, path: &str, opts: &Opts) -> anyhow::Result<()> {
    let state = eng.resource::<UiState>();
    let cached = state.borrow().textures.get(path).cloned();
    with_ui(|ui| {
        let texture = if let Some(t) = cached {
            t
        } else {
            // Through ProjectFiles, so an image in a packed game loads from
            // the pack rather than from a file that is not shipped.
            let bytes = eng
                .resource::<balaur_core::project::ProjectFiles>()
                .borrow()
                .read(path)?;
            let dynamic = image::load_from_memory(&bytes)?;
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
    eng: &Engine,
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
    // Tiles by default, like every other pill; `round` opts back in.
    let asked = opts.px("radius", 0.0);
    let corner = if asked > 0.0 {
        pill_radius(asked * 2.0)
    } else if opts.boolean("round", false) {
        pill_radius(h)
    } else {
        pill_radius(sc(5.0) * 2.0)
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, corner, fill);
    }
    if let Some(stroke) = opts.opt_color("stroke") {
        ui.painter().rect(
            rect,
            corner,
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
            let _ = scoped(eng, ui, menu);
        });
    }
    Ok(response.clicked())
}

#[cfg(test)]
mod tests {
    use super::{syntax_for, RUNE};

    #[test]
    fn rune_selects_its_own_tokens() {
        let rune = syntax_for("rune");
        assert_eq!(rune.line_comment, "//");
        assert!(rune.keywords.contains(&"fn"));
        assert!(rune.builtins.contains(&"this"));
    }

    #[test]
    fn shaders_select_their_own_tokens() {
        for name in ["wesl", "wgsl"] {
            let wesl = syntax_for(name);
            assert!(wesl.keywords.contains(&"var"), "{name}");
            assert!(wesl.keywords.contains(&"import"), "{name}");
            assert!(wesl.builtins.contains(&"textureSample"), "{name}");
            // Rune's `let` is WGSL's too, but `this` is not a shader word.
            assert!(!wesl.builtins.contains(&"this"), "{name}");
        }
    }

    /// An unknown or absent language must still highlight something rather
    /// than render the editor as plain punctuation.
    #[test]
    fn an_unknown_language_falls_back_to_rune() {
        assert_eq!(syntax_for("luau").line_comment, RUNE.line_comment);
        assert_eq!(syntax_for("").line_comment, RUNE.line_comment);
    }
}
