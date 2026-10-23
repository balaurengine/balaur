//! The `code` kind's editor: syntax tables for Rune, Balaur Script and WESL,
//! the highlighter, the gutter with its diagnostics, and the caret the node
//! says it moved.

use balaur_core::Engine;
use egui::{Align2, Color32, CornerRadius, FontId, Sense, pos2, vec2};

use crate::UiState;
use crate::theme;
use crate::vocabulary::{keys as k, tokens as t, words as w};
use crate::widget::node::Widget;
use crate::widget::theme::WidgetTheme;

/// A `code` node and the theme it is drawn under: what the editor reads.
pub(crate) struct CodeLook<'a> {
    pub(crate) widget: &'a Widget,
    pub(crate) theme: &'a WidgetTheme,
}

impl CodeLook<'_> {
    /// A colour the theme's `[colors]` names under `key`, or the built-in one.
    fn color(&self, key: &str, fallback: Color32) -> Color32 {
        self.theme.token(key).unwrap_or(fallback)
    }

    fn size(&self) -> f32 {
        if self.widget.font_size > 0.0 {
            self.widget.font_size
        } else {
            12.5
        }
    }
}

fn line_numbers(lines: &[u32]) -> Vec<usize> {
    lines
        .iter()
        .filter_map(|line| usize::try_from(*line).ok())
        .collect()
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
    fn of(look: &CodeLook<'_>) -> Self {
        Self {
            key: look.color(k::SYNTAX_KEYWORD, theme::color(t::SYNTAX_KEYWORD)),
            string: look.color(k::SYNTAX_STRING, theme::color(t::SYNTAX_STRING)),
            number: look.color(k::SYNTAX_NUMBER, theme::color(t::SYNTAX_NUMBER)),
            comment: look.color(k::SYNTAX_COMMENT, theme::color(t::SYNTAX_COMMENT)),
            ident: look.color(k::SYNTAX_IDENTIFIER, theme::color(t::SYNTAX_IDENTIFIER)),
            builtin: look.color(k::SYNTAX_TYPE, theme::color(t::SYNTAX_TYPE)),
            punct: look.color(k::SYNTAX_PUNCTUATION, theme::color(t::SYNTAX_PUNCTUATION)),
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
        "random",
        "ui",
        "log",
        "node",
        "fs",
        "toml",
        "this",
        "println",
    ],
};

/// balaur_bs's `.bs` scripts: Rust's look, its own types and built-ins.
const BS: Syntax = Syntax {
    line_comment: "//",
    keywords: &[
        "fn", "let", "if", "else", "while", "for", "in", "loop", "struct", "impl", "pub", "return",
        "break", "continue", "const", "async", "await", "true", "false", "as", "extern", "self",
    ],
    builtins: &[
        "int",
        "float",
        "bool",
        "str",
        "any",
        "Node",
        "Vec2",
        "Vec3",
        "Color",
        "vec2",
        "vec3",
        "color",
        "nil",
        "map",
        "task",
        "engine",
        "scene",
        "input",
        "physics",
        "physics2d",
        "physics3d",
        "render",
        "audio",
        "random",
        "ui",
        "log",
        "http",
        "gamend",
        "events",
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
    // `match` on a `&str` is not const, and three languages do not warrant a
    // table.
    if matches!(language.as_bytes(), b"wesl" | b"wgsl") {
        &WESL
    } else if matches!(language.as_bytes(), b"bs") {
        &BS
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
    fn of(look: &CodeLook<'_>) -> Self {
        Self {
            errors: line_numbers(&look.widget.problems),
            warnings: line_numbers(&look.widget.warnings),
            error_color: look.color(k::PROBLEM_COLOR, theme::color(t::DANGER_TEXT)),
            warning_color: look.color(k::WARNING_COLOR, theme::color(t::WARNING_TEXT)),
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
        let underline = marks
            .color(i + 1)
            .map_or(egui::Stroke::NONE, |color| egui::Stroke::new(1.0, color));
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
    fn of(look: &CodeLook<'_>, size: f32) -> Self {
        let widget = look.widget;
        Self {
            width: if widget.gutter_width > 0.0 {
                widget.gutter_width
            } else {
                34.0
            },
            color: look.color(k::GUTTER_COLOR, theme::color(t::TEXT_SUBTLE)),
            size,
            breakpoints: line_numbers(&widget.breakpoints),
            current_line: usize::try_from(widget.current_line).unwrap_or(0),
            breakpoint_color: look.color(k::BREAKPOINT_COLOR, theme::color(t::DANGER_TEXT)),
            current_fill: look.color(
                k::CURRENT_FILL,
                theme::color(t::WARNING_TEXT).gamma_multiply(0.25),
            ),
            marks: Marks::of(look),
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
        let font = FontId::new((self.size - 1.5).max(8.0), theme::family(w::MONO));
        for i in 0..n_lines {
            let center_y = rect.min.y + row_h * (i as f32 + 0.5);
            if self.breakpoints.contains(&(i + 1)) {
                ui.painter().circle_filled(
                    pos2(rect.min.x + 7.0, center_y),
                    4.0,
                    self.breakpoint_color,
                );
            }
            // On the inner edge, so it reads beside the code and cannot be
            // taken for the breakpoint dot on the outer one.
            if let Some(color) = self.marks.color(i + 1) {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        pos2(rect.max.x - 2.0, center_y - row_h * 0.5),
                        vec2(2.0, row_h),
                    ),
                    CornerRadius::ZERO,
                    color,
                );
            }
            ui.painter().text(
                pos2(rect.max.x - 6.0, center_y),
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

/// An editable, syntax-highlighted code editor with a line-number gutter. The
/// buffer persists per `id` in `UiState`.
///
/// Returns the buffer, whether it changed, the gutter line clicked this frame
/// if any, and the caret: `breakpoints` marks lines, `current_line` highlights
/// one.
pub(crate) fn code_editor(
    eng: &Engine,
    ui: &mut egui::Ui,
    id: &str,
    look: &CodeLook<'_>,
) -> (String, bool, Option<i64>, Option<Caret>) {
    let widget = look.widget;
    let language = widget.source.as_str();
    let syntax = syntax_for(language);
    let state = eng.resource::<UiState>();
    // Taken rather than copied: the buffer holds the whole open file, and it
    // goes back below whether or not the pass drew.
    let mut buffer = state
        .borrow_mut()
        .text_buffers
        .remove(id)
        .unwrap_or_else(|| widget.text.to_string());
    let size = look.size();
    let gutter = Gutter::of(look, size);
    let colors = SyntaxColors::of(look);
    let marks = Marks::of(look);
    let font = FontId::new(size, theme::family(w::MONO));
    let margin = code_margin(widget.padding);
    let (changed, clicked, caret) = {
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
            ui.add_space(12.0);
            // Tokenising a whole file into a `LayoutJob` and hashing every
            // section of it, once a frame, was the dearest thing the editor
            // did; the galley only changes when the text or the look does.
            let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                let want = look_key(buf.as_str(), &font, &colors, &marks, language);
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
            let mut edit = egui::TextEdit::multiline(&mut buffer)
                .id(egui::Id::new(id.to_string()))
                .frame(egui::Frame::NONE)
                .desired_width(ui.available_width())
                .interactive(widget.egui.editable)
                .lock_focus(widget.egui.tab_inserts)
                .cursor_at_end(widget.egui.caret_at_end)
                .layouter(&mut layouter);
            if let Some(margin) = margin {
                edit = edit.margin(margin);
            }
            let output = edit.show(ui);
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
        (changed, clicked, caret)
    };
    state
        .borrow_mut()
        .text_buffers
        .insert(id.to_string(), buffer.clone());
    (buffer, changed, clicked, caret)
}

/// The margin round the code, from `padding`: left, top, right and bottom,
/// where a side under zero is unset; `None` when none is set leaves egui's.
fn code_margin(padding: [f32; 4]) -> Option<egui::Margin> {
    if padding.iter().all(|side| *side < 0.0) {
        return None;
    }
    let side = |px: f32| px.max(0.0).round().min(f32::from(i8::MAX)) as i8;
    let [left, top, right, bottom] = padding;
    Some(egui::Margin {
        left: side(left),
        right: side(right),
        top: side(top),
        bottom: side(bottom),
    })
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
    fn bs_selects_its_own_tokens() {
        let bl = syntax_for("bs");
        assert!(bl.keywords.contains(&"extern") && bl.keywords.contains(&"self"));
        assert!(bl.builtins.contains(&"any") && bl.builtins.contains(&"Node"));
        assert!(!bl.keywords.contains(&"match"), "`.bs` has no `match`");
    }
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
