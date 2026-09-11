//! The code editor widget: syntax tables for Rune and WESL, the highlighter,
//! the gutter with its diagnostics, and the caret a script reads back.

use balaur_core::Engine;
use balaur_script::Value;
use egui::{Align2, Color32, CornerRadius, FontId, Sense, pos2, vec2};

use crate::UiState;
use crate::bridge::with_ui;
use crate::immediate::{Opts, sc};
use crate::theme;
use crate::vocabulary::{keys as k, words as w};

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
            key: opts.color(k::K_KEY, Color32::from_rgb(0xf0, 0xa2, 0x73)),
            string: opts.color(k::K_STR, Color32::from_rgb(0x8f, 0xbc, 0xae)),
            number: opts.color(k::K_NUM, Color32::from_rgb(0xff, 0xc7, 0xa8)),
            comment: opts.color(k::K_COM, Color32::from_rgb(0x76, 0x7e, 0x88)),
            ident: opts.color(k::K_FN, Color32::from_rgb(0xee, 0xf1, 0xf4)),
            builtin: opts.color(k::K_TYPE, Color32::from_rgb(0xb6, 0xd8, 0xcc)),
            punct: opts.color(k::K_PUNC, Color32::from_rgb(0x98, 0xa1, 0xaa)),
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
            errors: opts.lines(k::PROBLEMS),
            warnings: opts.lines(k::WARNINGS),
            error_color: opts.color(k::PROBLEM_COLOR, Color32::from_rgb(0xe0, 0x4a, 0x4a)),
            warning_color: opts.color(k::WARNING_COLOR, Color32::from_rgb(0xe0, 0xb0, 0x4a)),
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
            width: opts.px(k::GUTTER_WIDTH, 34.0),
            color: opts.color(k::GUTTER_COLOR, Color32::from_rgb(0x76, 0x7e, 0x88)),
            size,
            breakpoints: opts.lines(k::BREAKPOINTS),
            current_line: opts.f32(k::CURRENT_LINE, 0.0).max(0.0) as usize,
            breakpoint_color: opts.color(k::BREAKPOINT_COLOR, Color32::from_rgb(0xe0, 0x4a, 0x4a)),
            current_fill: opts.color(
                k::CURRENT_FILL,
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
        let font = FontId::new((self.size - sc(1.5)).max(8.0), theme::family(w::MONO));
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

/// Where the text cursor sits: screen position of its bottom-left corner, and
/// its character index into the buffer. `None` when the editor is not focused.
pub(crate) struct Caret {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) index: usize,
}

/// Returns the buffer, whether it changed, the gutter line clicked this frame
/// if any, and the caret: `breakpoints` marks lines, `current_line` highlights
/// one.
/// A `code` widget's values as the options `code_editor` reads, so the node
/// and the script call reach the same editor.
pub(crate) fn code_opts(widget: &crate::widget::node::Widget, scale: f32) -> Opts {
    let size = if widget.font_size > 0.0 {
        widget.font_size
    } else {
        12.5
    };
    Opts::plain(Some(Value::Map(vec![
        (k::SIZE.into(), Value::Num(f64::from(size * scale))),
        (k::LANGUAGE.into(), Value::Str(widget.source.to_string())),
    ])))
}

pub(crate) fn code_editor(
    eng: &Engine,
    id: &str,
    source: &str,
    opts: &Opts,
) -> anyhow::Result<(String, bool, Option<i64>, Option<Caret>)> {
    // `language` overrides; otherwise highlight whatever the project is
    // written in, so an editor shows Rune as Rune.
    let language = opts.string(k::LANGUAGE).unwrap_or_else(|| {
        eng.try_resource::<balaur_core::project::ProjectManifest>()
            .map_or_else(|| "rune".to_string(), |m| m.borrow().language.clone())
    });
    let syntax = syntax_for(&language);
    let state = eng.resource::<UiState>();
    // Taken rather than copied: the buffer holds the whole open file, and it
    // goes back below whether or not the pass drew.
    let mut buffer = state
        .borrow_mut()
        .text_buffers
        .remove(id)
        .unwrap_or_else(|| source.to_string());
    let size = opts.px(k::SIZE, 12.5);
    let gutter = Gutter::from_opts(opts, size);
    let colors = SyntaxColors::from_opts(opts);
    let marks = Marks::from_opts(opts);
    let font = FontId::new(size, theme::family(w::MONO));
    let (changed, clicked, caret) = with_ui(|ui| {
        let font = font.clone();
        let row_h = ui
            .painter()
            .layout_no_wrap("0".into(), font.clone(), gutter.color)
            .size()
            .y;
        let mut changed = false;
        let mut clicked = None;
        let mut caret = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let n_lines = buffer.split('\n').count().max(1);
            clicked = gutter.paint(ui, n_lines, row_h);
            ui.add_space(sc(12.0));
            // Tokenising a whole file into a `LayoutJob` and hashing every
            // section of it, once a frame, was the dearest thing the editor
            // did; the galley only changes when the text or the look does.
            let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                let want = look_key(buf.as_str(), &font, &colors, &marks, &language);
                if let Some(hit) = state
                    .borrow()
                    .code_galleys
                    .get(id)
                    .and_then(|(had, galley)| (*had == want).then(|| std::sync::Arc::clone(galley)))
                {
                    return hit;
                }
                let mut job = highlight(buf.as_str(), syntax, &font, &colors, &marks);
                job.wrap.max_width = f32::INFINITY;
                let galley = ui.fonts_mut(|f| f.layout_job(job));
                state
                    .borrow_mut()
                    .code_galleys
                    .insert(id.to_string(), (want, std::sync::Arc::clone(&galley)));
                galley
            };
            // `show` rather than `add`: a popup has to open under the caret,
            // and only the output carries the galley it sits in.
            let output = egui::TextEdit::multiline(&mut buffer)
                .id(egui::Id::new(id.to_string()))
                .frame(egui::Frame::NONE)
                .desired_width(ui.available_width())
                .layouter(&mut layouter)
                .show(ui);
            changed = output.response.changed();
            if let Some(range) = output.cursor_range {
                let at = range.primary;
                let rect = output.galley.pos_from_cursor(at);
                caret = Some(Caret {
                    x: output.galley_pos.x + rect.left(),
                    y: output.galley_pos.y + rect.bottom(),
                    index: at.index.into(),
                });
            }
        });
        Ok((changed, clicked, caret))
    })?;
    state
        .borrow_mut()
        .text_buffers
        .insert(id.to_string(), buffer.clone());
    Ok((buffer, changed, clicked, caret))
}

/// What the code editor's galley was laid out from: the text and everything
/// about its look. A hit means re-highlighting would produce the same picture.
fn look_key(
    text: &str,
    font: &FontId,
    colors: &SyntaxColors,
    marks: &Marks,
    language: &str,
) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    language.hash(&mut hasher);
    font.size.to_bits().hash(&mut hasher);
    for color in [
        colors.key,
        colors.string,
        colors.number,
        colors.comment,
        colors.ident,
        colors.builtin,
        colors.punct,
        marks.error_color,
        marks.warning_color,
    ] {
        color.to_array().hash(&mut hasher);
    }
    marks.errors.hash(&mut hasher);
    marks.warnings.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::{RUNE, syntax_for};
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
        assert_eq!(syntax_for("brainfuck").line_comment, RUNE.line_comment);
        assert_eq!(syntax_for("").line_comment, RUNE.line_comment);
    }
}
