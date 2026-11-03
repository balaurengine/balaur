//! The `code` kind's editor: syntax tables for Balaur Script and WESL,
//! the highlighter, the gutter with its diagnostics, and the caret the node
//! says it moved. The text is typed through [`crate::widget::typing`], laid
//! out a line at a time by the shaper.

use balaur_core::Engine;
use balaur_core::hecs::Entity;
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, pos2, vec2};

use crate::UiState;
use crate::theme;
use crate::vocabulary::{keys as k, tokens as t};
use crate::widget::arrange::Pad;
use crate::widget::node::Widget;
use crate::widget::paint::Brush;
use crate::widget::theme::WidgetTheme;
use crate::widget::typing::{Field, Typesetter, Typing};

/// The room between the gutter and the code.
const GUTTER_GAP: f32 = 12.0;

/// A `code` node and the theme it is drawn under: what the editor reads.
pub(crate) struct CodeLook<'a> {
    pub(crate) widget: &'a Widget,
    pub(crate) theme: &'a WidgetTheme,
    /// Whose faces it draws in.
    pub(crate) project: balaur_text::FontProject,
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

    /// What the code draws in: the widget's own font, else the theme's
    /// `mono`, else its default.
    fn font(&self) -> &str {
        if !self.widget.font.is_empty() {
            return self.widget.font.as_str();
        }
        let mono = self.theme.font(t::FONT_MONO);
        if mono.is_empty() {
            self.theme.font(t::FONT_DEFAULT)
        } else {
            mono
        }
    }

    /// The gutter's numbers' family, in that font.
    fn family(&self) -> egui::FontFamily {
        theme::weighted(self.project, self.font(), 400.0, false)
    }

    /// What a line of the code is shaped from.
    fn request(&self) -> balaur_text::RequestRef<'_> {
        balaur_text::RequestRef {
            text: "",
            size: self.size(),
            weight: 400,
            slant: balaur_text::Slant::Normal,
            width: None,
            truncate: false,
            align: balaur_text::Align::Start,
            markup: false,
            font: self.font(),
            project: self.project,
            line_height: 0.0,
            letter_spacing: 0.0,
            options: &balaur_text::PLAIN,
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

/// balaur_bs's `.bs` scripts: Rust's look, its own types and built-ins.
const BS: Syntax = Syntax {
    line_comment: "//",
    keywords: &[
        "fn", "let", "if", "else", "while", "for", "in", "loop", "struct", "impl", "pub",
        "internal", "return", "break", "continue", "const", "async", "await", "true", "false",
        "as", "extern", "self",
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
/// An unknown name falls back to Balaur Script rather than rendering the editor
/// as plain punctuation.
pub(crate) const fn syntax_for(language: &str) -> &'static Syntax {
    // `match` on a `&str` is not const, and two languages do not warrant a
    // table.
    if matches!(language.as_bytes(), b"wesl" | b"wgsl") {
        &WESL
    } else {
        &BS
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

/// One line's colours, by the byte each starts at: the design handoff's
/// tokenizer rules.
pub(crate) fn spans(line: &str, syntax: &Syntax, colors: &SyntaxColors) -> Vec<(u32, Color32)> {
    let at = |pos: usize| u32::try_from(pos).unwrap_or(u32::MAX);
    if line.trim_start().starts_with(syntax.line_comment) {
        return vec![(0, colors.comment)];
    }
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < line.len() {
        let rest = &line[pos..];
        let Some(c) = rest.chars().next() else {
            break;
        };
        let (token_len, color) = if c.is_whitespace() {
            (
                rest.chars()
                    .take_while(|c| c.is_whitespace())
                    .map(char::len_utf8)
                    .sum(),
                colors.ident,
            )
        } else if c == '"' || c == '\'' {
            let mut len = c.len_utf8();
            for ch in rest[len..].chars() {
                len += ch.len_utf8();
                if ch == c {
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
        out.push((at(pos), color));
        pos += token_len;
    }
    out
}

/// The column beside the code: line numbers, breakpoint dots, and the row
/// the debugger is stopped on.
struct Gutter {
    width: f32,
    color: Color32,
    size: f32,
    family: egui::FontFamily,
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
            width: gutter_width(widget),
            color: look.color(k::GUTTER_COLOR, theme::color(t::TEXT_SUBTLE)),
            size,
            family: look.family(),
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

    /// Paint `n_lines` rows from the top of `rect`; returns the row clicked
    /// this frame, if any.
    fn paint(&self, ui: &mut egui::Ui, rect: Rect, n_lines: usize, row_h: f32) -> Option<i64> {
        let response = ui.interact(rect, ui.scope_id().with("gutter"), Sense::click());
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
            Brush::of(ui).fill(row, CornerRadius::ZERO, self.current_fill);
        }
        let font = FontId::new((self.size - 1.5).max(8.0), self.family.clone());
        let clip = ui.clip_rect();
        let first = (((clip.top() - rect.min.y) / row_h).floor().max(0.0) as usize).min(n_lines);
        let last = (((clip.bottom() - rect.min.y) / row_h).ceil().max(0.0) as usize).min(n_lines);
        for i in first..last {
            let center_y = rect.min.y + row_h * (i as f32 + 0.5);
            if self.breakpoints.contains(&(i + 1)) {
                Brush::of(ui).disc(pos2(rect.min.x + 7.0, center_y), 4.0, self.breakpoint_color);
            }
            // On the inner edge, so it reads beside the code and cannot be
            // taken for the breakpoint dot on the outer one.
            if let Some(color) = self.marks.color(i + 1) {
                Brush::of(ui).fill(
                    egui::Rect::from_min_size(
                        pos2(rect.max.x - 2.0, center_y - row_h * 0.5),
                        vec2(2.0, row_h),
                    ),
                    CornerRadius::ZERO,
                    color,
                );
            }
            Brush::of(ui).text(
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

/// What one pass of the editor did.
pub(crate) struct Coded {
    /// The text, when the reader changed it.
    pub(crate) typed: Option<String>,
    /// The gutter row clicked, counted from 1.
    pub(crate) gutter: Option<i64>,
    /// The caret, when it moved.
    pub(crate) caret: Option<Caret>,
    /// The box it drew in.
    pub(crate) used: egui::Vec2,
}

thread_local! {
    /// Where each `code` widget's caret was last said to be, by byte, so
    /// `caret` is emitted when it moves rather than every pass it is drawn.
    static CARETS: std::cell::RefCell<rustc_hash::FxHashMap<u64, usize>> =
        std::cell::RefCell::new(rustc_hash::FxHashMap::default());
}

fn moved_caret(entity: Entity, byte: usize) -> bool {
    CARETS.with(|carets| carets.borrow_mut().insert(entity.to_bits().get(), byte) != Some(byte))
}

/// How wide the column of line numbers is.
fn gutter_width(widget: &Widget) -> f32 {
    if widget.gutter_width > 0.0 {
        widget.gutter_width
    } else {
        34.0
    }
}

/// The field a `code` node keeps, refilled from its `text`.
fn field_of(eng: &Engine, id: egui::Id, widget: &Widget) -> Field {
    let mut field = eng
        .resource::<UiState>()
        .borrow_mut()
        .fields
        .remove(&id)
        .unwrap_or_else(|| Field::new(&widget.text, true, widget.egui.caret_at_end));
    field.refill(&widget.text);
    field
}

/// Let go of the caret of each node freed.
pub(crate) fn forget_freed(world: &balaur_core::hecs::World) {
    CARETS.with(|carets| {
        carets
            .borrow_mut()
            .retain(|bits, _| Entity::from_bits(*bits).is_some_and(|node| world.contains(node)));
    });
}

fn code_id(entity: Entity) -> egui::Id {
    egui::Id::unique(("balaur-code", entity))
}

/// The box a `code` node asks for: its gutter, the gap after it, and its
/// lines laid out as the draw lays them out, in its padding.
pub(crate) fn natural(
    eng: &Engine,
    ctx: &egui::Context,
    entity: Entity,
    look: &CodeLook<'_>,
) -> egui::Vec2 {
    let id = code_id(entity);
    let mut field = field_of(eng, id, look.widget);
    let shown = field.editing.shown(false);
    let typeset = Typesetter::Lines(look.request()).set(eng, ctx, &shown.text, &mut field.kept);
    eng.resource::<UiState>()
        .borrow_mut()
        .fields
        .insert(id, field);
    let margin = code_margin(look.widget.padding).taken();
    vec2(
        gutter_width(look.widget) + GUTTER_GAP + typeset.size.x + margin.x,
        typeset.size.y + margin.y,
    )
}

/// An editable, syntax-highlighted code editor with a line-number gutter, at
/// the top-left of `ui`'s box and as wide as it at least. What it holds is
/// kept per node in `UiState`; a script writing a new `text` refills it.
pub(crate) fn code_editor(
    eng: &Engine,
    ui: &mut egui::Ui,
    entity: Entity,
    look: &CodeLook<'_>,
) -> Coded {
    let widget = look.widget;
    let syntax = syntax_for(widget.source.as_str());
    let gutter = Gutter::of(look, look.size());
    let colors = SyntaxColors::of(look);
    let marks = Marks::of(look);
    let id = code_id(entity);
    let state = eng.resource::<UiState>();
    let mut field = field_of(eng, id, widget);
    let setter = Typesetter::Lines(look.request());
    let shown = field.editing.shown(false);
    let typeset = setter.set(eng, ui.ctx(), &shown.text, &mut field.kept);
    let row = typeset
        .clusters
        .lines
        .first()
        .map_or(0.0, |line| line.height);
    let lines = typeset.blocks.len().max(1);
    let margin = code_margin(widget.padding);
    let origin = ui.max_rect().min;
    let rows = Rect::from_min_size(
        origin + vec2(0.0, margin.top),
        vec2(gutter.width, row * lines as f32),
    );
    let gutter_hit = gutter.paint(ui, rows, lines, row);
    let left = origin.x + gutter.width + GUTTER_GAP;
    let size = vec2(
        (ui.max_rect().right() - left).max(typeset.size.x + margin.taken().x),
        typeset.size.y + margin.taken().y,
    );
    let rect = Rect::from_min_size(pos2(left, origin.y), size);
    let paint_line = |line: &str| spans(line, syntax, &colors);
    let underline = |line: usize| marks.color(line);
    let typed = Typing {
        id,
        multiline: true,
        secret: false,
        editable: widget.egui.editable,
        clip: false,
        frame: false,
        tab_inserts: widget.egui.tab_inserts,
        numeric: false,
        max_chars: 0,
        submit: None,
        align: egui::Align::Min,
        margin,
        color: colors.ident,
        placeholder: None,
        prefix: None,
        suffix: None,
        colors: Some(&paint_line),
        underline: Some(&underline),
    }
    .show(ui, eng, rect, &mut field, &setter);
    let used = vec2(rect.right() - origin.x, rect.height().max(rows.height()));
    ui.advance_cursor_after_rect(Rect::from_min_size(origin, used));
    let caret = typed
        .caret
        .filter(|(_, byte)| moved_caret(entity, *byte))
        .map(|(at, byte)| Caret {
            x: at.left(),
            y: at.bottom(),
            index: field.editing.text()[..byte].chars().count(),
        });
    let coded = Coded {
        typed: typed.changed.then(|| field.editing.text().to_string()),
        gutter: gutter_hit,
        caret,
        used,
    };
    field.owner = Some(entity);
    state.borrow_mut().fields.insert(id, field);
    coded
}

/// The margin round the code, from `padding`: left, top, right and bottom,
/// where a side under zero is none.
fn code_margin(padding: [f32; 4]) -> Pad {
    Pad::of(padding.map(|side| side.max(0.0)))
}

#[cfg(test)]
mod tests {
    use super::{BS, syntax_for};

    #[test]
    fn bs_selects_its_own_tokens() {
        let bl = syntax_for("bs");
        assert!(bl.keywords.contains(&"extern") && bl.keywords.contains(&"self"));
        assert!(bl.builtins.contains(&"any") && bl.builtins.contains(&"Node"));
        assert!(!bl.keywords.contains(&"match"), "`.bs` has no `match`");
    }
    #[test]
    fn shaders_select_their_own_tokens() {
        for name in ["wesl", "wgsl"] {
            let wesl = syntax_for(name);
            assert!(wesl.keywords.contains(&"var"), "{name}");
            assert!(wesl.keywords.contains(&"import"), "{name}");
            assert!(wesl.builtins.contains(&"textureSample"), "{name}");
        }
    }

    /// An unknown or absent language must still highlight something rather
    /// than render the editor as plain punctuation.
    #[test]
    fn an_unknown_language_falls_back_to_balaur_script() {
        assert_eq!(syntax_for("brainfuck").line_comment, BS.line_comment);
        assert_eq!(syntax_for("").line_comment, BS.line_comment);
    }
}
