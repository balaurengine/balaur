//! The `ui` module: panels, containers, and the design system's widget
//! shapes (pill, circle button, field, toggle, slider, code line, modal).
//! Every color arrives per call from the script's token table, so themes are
//! entirely script-defined. The code editor is [`code`].

pub(crate) mod bindings;
pub(crate) mod code;
pub(crate) mod layout;
pub(crate) mod rects;

use anyhow::Result;
use balaur_core::Engine;
use balaur_plugin::Registry;
use balaur_script::{Bindings, CallbackId, Value};
use egui::{Color32, CornerRadius, FontId, Margin, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::UiState;
use crate::bridge::with_ui;
use crate::theme::{self, parse_hex};
use crate::vocabulary::{keys as k, states as st, tokens as t, weights, words as w};

thread_local! {
    /// Where the last `ui.button` landed. An immediate control has no node to
    /// ask, so the one that drew it leaves its box here for `ui.button_rect`.
    static PILL_RECT: std::cell::Cell<Option<egui::Rect>> = const { std::cell::Cell::new(None) };
}

pub(crate) fn note_pill(rect: egui::Rect) {
    PILL_RECT.with(|cell| cell.set(Some(rect)));
}

pub(crate) fn last_pill() -> Option<egui::Rect> {
    PILL_RECT.with(std::cell::Cell::get)
}

/// An options table as passed from script: `{ height = 56, fill = "#20242a" }`.
///
/// Reads the neutral map rather than one language's table type, so widgets
/// name no language.
/// A missing or wrong-typed key falls back to the default: a typo in an options
/// table should not stop the frame.
pub(crate) struct Opts(
    pub(crate) Option<Value>,
    /// The named role's own options, read where the caller said nothing.
    Option<std::rc::Rc<Vec<(String, Value)>>>,
    /// Which of the role's state tables paints over the rest: `hover` while
    /// the pointer is on the control, `active` while it is held.
    &'static str,
    /// Whether the control is on, so its role's `checked` table paints first.
    bool,
);

/// A sub-table of a role's entries, by name.
fn table_in<'a>(entries: &'a [(String, Value)], name: &str) -> Option<&'a [(String, Value)]> {
    match entries.iter().find(|(k, _)| k == name).map(|(_, v)| v) {
        Some(Value::Map(inner)) => Some(inner.as_slice()),
        _ => None,
    }
}

fn entry<'a>(entries: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// What a role's `hover` or `active` table may set. Paint only: a control
/// that resized under the pointer would move whatever sits beside it.
const STATE_KEYS: &[&str] = &[
    k::TEXT_COLOR,
    k::FILL,
    k::ICON_COLOR,
    k::STROKE,
    k::TRAILING_COLOR,
];

/// Every key any widget reads, `role` included.
///
/// One list rather than one per widget: what this catches is a *typo*
/// (`colour`, `witdh`), and a misspelling is in no widget's vocabulary. A
/// real key on the wrong widget still passes, which is the price of not
/// making every call site declare its own set.
const KNOWN_KEYS: &[&str] = &[
    k::AUTOFOCUS,
    k::AXIS,
    k::BREAKPOINT_COLOR,
    k::BREAKPOINTS,
    k::CHECKED,
    k::CLOSABLE,
    k::COLLAPSIBLE,
    k::CORNER_RADIUS,
    k::CURRENT_FILL,
    k::CURRENT_LINE,
    k::DASHED,
    k::DECIMALS,
    k::DIAMETER,
    k::DISABLED,
    k::FILL,
    k::FONT_FAMILY,
    k::FONT_SIZE,
    k::FONT_WEIGHT,
    k::GUTTER_COLOR,
    k::GUTTER_WIDTH,
    k::HEIGHT,
    k::HIGHLIGHT,
    k::ICON,
    k::ICON_COLOR,
    k::ICON_FILL,
    k::ICON_SIZE,
    k::INTERACTIVE,
    k::KEEP_OPEN,
    k::KNOB,
    k::LANGUAGE,
    k::LINE_HEIGHT,
    k::MAX,
    k::MAX_HEIGHT,
    k::MAX_WIDTH,
    k::MENU,
    k::MENU_CLICK,
    k::MIN,
    k::MIN_WIDTH,
    k::OFFSET,
    k::PADDING,
    k::PADDING_X,
    k::PADDING_Y,
    k::PREFIX,
    k::PREFIX_COLOR,
    k::PROBLEM_COLOR,
    k::PROBLEMS,
    k::RAIL,
    k::RESIZABLE,
    k::ROLE,
    k::ROW_HEIGHT,
    k::SCRIM,
    k::SEPARATOR,
    k::SPEED,
    k::STICK_TO_BOTTOM,
    k::STROKE,
    k::STROKE_WIDTH,
    k::SUFFIX,
    k::SYNTAX_COMMENT,
    k::SYNTAX_IDENTIFIER,
    k::SYNTAX_KEYWORD,
    k::SYNTAX_NUMBER,
    k::SYNTAX_PUNCTUATION,
    k::SYNTAX_STRING,
    k::SYNTAX_TYPE,
    k::TEXT_ALIGN,
    k::TEXT_COLOR,
    k::TIGHT,
    k::TITLE,
    k::TOOLTIP,
    k::TOP,
    k::TRACK,
    k::TRAILING,
    k::TRAILING_COLOR,
    k::TRAILING_SIZE,
    k::TRANSPARENT,
    k::TRUNCATE,
    k::VALUE,
    k::WARNING_COLOR,
    k::WARNINGS,
    k::WIDTH,
    k::WRAP,
    k::X,
    k::Y,
];

/// The state tables an options map may carry, beside its keys.
const STATE_TABLES: &[&str] = &[st::HOVER, st::ACTIVE];

/// Report an option key no widget reads, once per name.
///
/// A typo used to do nothing at all: the value fell back to its default and
/// the call looked as though it had been honoured.
///
/// Runs on every options table of every widget of every frame, so the hit
/// path is a binary search and nothing else: `KNOWN_KEYS` is sorted, checked
/// by the test below, and the miss path is the only one that allocates.
fn warn_unknown(entries: &[(String, Value)]) {
    for (key, _) in entries {
        if KNOWN_KEYS.binary_search(&key.as_str()).is_ok() || STATE_TABLES.contains(&key.as_str()) {
            continue;
        }
        if balaur_core::logbuf::first_time("ui option", key) {
            tracing::warn!("ui: no widget reads the option `{key}`");
        }
    }
}

impl Opts {
    /// The caller's options over the named role's. A call site then says only
    /// what it changes, and the look lives in the theme asset.
    pub(crate) fn with_roles(opts: Option<Value>) -> Self {
        let Some(Value::Map(given)) = opts.as_ref() else {
            return Self(opts, None, "", false);
        };
        warn_unknown(given);
        let role = match given.iter().find(|(k, _)| k == k::ROLE).map(|(_, v)| v) {
            Some(Value::Str(name)) => crate::bridge::role(name),
            _ => None,
        };
        let checked = matches!(
            given.iter().find(|(k, _)| k == k::CHECKED).map(|(_, v)| v),
            Some(Value::Bool(true))
        );
        Self(opts, role, "", checked)
    }

    /// Options as given, with no role behind them.
    pub(crate) fn plain(opts: Option<Value>) -> Self {
        Self(opts, None, "", false)
    }

    /// The same options for a control that is on, or off.
    pub(crate) fn with_checked(&self, on: bool) -> Self {
        Self(self.0.clone(), self.1.clone(), self.2, on)
    }

    /// Whether the text is bold: a `font_weight` of 600 or more.
    pub(crate) fn bold(&self) -> bool {
        self.f32(k::FONT_WEIGHT, weights::REGULAR) >= weights::BOLD_FROM
    }

    /// Whether `corner_radius` asks for a pill: as round as the box is short.
    pub(crate) fn is_pill(&self) -> bool {
        self.str(k::CORNER_RADIUS) == Some(w::FULL)
    }

    /// The same options with the role's `hover` or `active` table painting
    /// over them. Held falls back to hovered, the way a theme's own styles do.
    pub(crate) fn in_state(&self, hovered: bool, held: bool) -> Self {
        let state = if held {
            st::ACTIVE
        } else if hovered {
            st::HOVER
        } else {
            ""
        };
        Self(self.0.clone(), self.1.clone(), state, self.3)
    }

    /// What the state tables say about `key`, for the paint keys a state may
    /// set: the caller's own tables first, then the role's. A control that is
    /// on reads its `checked` table first, and the pointer's table inside it
    /// before the one beside it. `active` falls through to `hover`.
    fn state(&self, key: &str) -> Option<&Value> {
        if !STATE_KEYS.contains(&key) {
            return None;
        }
        let given = match self.0.as_ref() {
            Some(Value::Map(entries)) => Some(entries.as_slice()),
            _ => None,
        };
        let role = self.1.as_ref().map(|role| role.as_slice());
        [given, role]
            .into_iter()
            .flatten()
            .find_map(|entries| self.state_in(entries, key))
    }

    fn state_in<'a>(&self, entries: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
        if self.3
            && let Some(checked) = table_in(entries, k::CHECKED)
            && let Some(found) = self
                .pointer_in(checked, key)
                .or_else(|| entry(checked, key))
        {
            return Some(found);
        }
        self.pointer_in(entries, key)
    }

    /// The pointer's state table inside `entries`, for `key`.
    fn pointer_in<'a>(&self, entries: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
        if self.2.is_empty() {
            return None;
        }
        entry(table_in(entries, self.2)?, key).or_else(|| {
            (self.2 == st::ACTIVE)
                .then(|| table_in(entries, st::HOVER).and_then(|t| entry(t, key)))
                .flatten()
        })
    }

    /// Whether the caller or the role has a `checked` table for a control
    /// that is on.
    pub(crate) fn dresses_checked(&self) -> bool {
        let given = match self.0.as_ref() {
            Some(Value::Map(entries)) => Some(entries.as_slice()),
            _ => None,
        };
        let role = self.1.as_ref().map(|role| role.as_slice());
        [given, role]
            .into_iter()
            .flatten()
            .any(|entries| table_in(entries, k::CHECKED).is_some())
    }

    /// Whether the caller or the role dresses the state the control is in.
    /// One that does not takes [`wash`] instead, so every control answers the
    /// pointer.
    pub(crate) fn dressed(&self) -> bool {
        let given = match self.0.as_ref() {
            Some(Value::Map(entries)) => Some(entries.as_slice()),
            _ => None,
        };
        let role = self.1.as_ref().map(|role| role.as_slice());
        !self.2.is_empty()
            && [given, role].into_iter().flatten().any(|entries| {
                entries
                    .iter()
                    .any(|(k, v)| (k == self.2 || k == st::HOVER) && matches!(v, Value::Map(_)))
            })
    }

    /// The state table if one paints this key, then what the caller said, and
    /// failing both what the role it named says.
    fn get(&self, key: &str) -> Option<&Value> {
        if let Some(found) = self.state(key) {
            return Some(found);
        }
        let given = match self.0.as_ref() {
            Some(Value::Map(entries)) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        };
        given.or_else(|| {
            let role = self.1.as_ref()?;
            role.iter().find(|(k, _)| k == key).map(|(_, v)| v)
        })
    }
    /// A number, or the name of one of the theme's sizes.
    pub(crate) fn f32(&self, key: &str, default: f32) -> f32 {
        match self.get(key) {
            Some(Value::Num(n)) => *n as f32,
            Some(Value::Int(i)) => *i as f32,
            Some(Value::Str(name)) if name != w::FULL => theme::size(name),
            _ => default,
        }
    }
    /// A dimension in design pixels when the caller gave one, so a window can
    /// tell "put it here" apart from "wherever you left it".
    pub(crate) fn opt_px(&self, key: &str) -> Option<f32> {
        match self.get(key) {
            Some(Value::Num(n)) => Some(*n as f32),
            Some(Value::Int(i)) => Some(*i as f32),
            Some(Value::Str(name)) if name != w::FULL => Some(theme::size(name)),
            _ => None,
        }
    }
    pub(crate) fn boolean(&self, key: &str, default: bool) -> bool {
        matches!(self.get(key), Some(Value::Bool(b)) if *b) || {
            !matches!(self.get(key), Some(Value::Bool(_))) && default
        }
    }
    /// The value as it sits in the options table. What every reader that only
    /// looks at the text should take: a widget reads a handful of colours a
    /// call, and copying each one to parse six hex digits off it was most of
    /// what a pass allocated.
    pub(crate) fn str(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Value::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }
    /// An owned copy, for the callers that keep the text past the call.
    pub(crate) fn string(&self, key: &str) -> Option<String> {
        self.str(key).map(ToOwned::to_owned)
    }
    pub(crate) fn color(&self, key: &str, default: Color32) -> Color32 {
        self.opt_color(key).unwrap_or(default)
    }
    pub(crate) fn opt_color(&self, key: &str) -> Option<Color32> {
        parse_hex(self.str(key)?)
    }
    /// A dimension in design pixels, which egui's zoom turns into screen
    /// pixels for the whole pass.
    /// The outline a caller or its role asks for, in `stroke`'s colour and
    /// `stroke_width`'s design pixels, which default to one.
    pub(crate) fn stroke_or(&self, fallback: Color32) -> Stroke {
        Stroke::new(
            self.px(k::STROKE_WIDTH, theme::size(t::STROKE_WIDTH)),
            self.color(k::STROKE, fallback),
        )
    }
    pub(crate) fn opt_stroke(&self) -> Option<Stroke> {
        self.opt_color(k::STROKE).map(|color| {
            Stroke::new(
                self.px(k::STROKE_WIDTH, theme::size(t::STROKE_WIDTH)),
                color,
            )
        })
    }
    pub(crate) fn px(&self, key: &str, default: f32) -> f32 {
        self.f32(key, default)
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
    /// Four raw numbers — x, y, width, height. Source pixels into an image,
    /// so the UI scale never touches them: they index the file, not the
    /// screen.
    pub(crate) fn rect(&self, key: &str) -> Option<[f32; 4]> {
        let Some(Value::List(items)) = self.get(key) else {
            return None;
        };
        if items.len() < 4 {
            return None;
        }
        let mut out = [0.0f32; 4];
        for (slot, item) in out.iter_mut().zip(items) {
            *slot = match item {
                Value::Num(n) => *n as f32,
                Value::Int(i) => *i as f32,
                _ => return None,
            };
        }
        Some(out)
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

/// The corner a filled shape gets when it asks for none.
///
/// Nothing in the editor is meant to have a square corner, so the fallback
/// rounds rather than not. It is the radius `image_button` already chose, so
/// an image and the button around it agree instead of one being a plate with
/// a square picture on it.
pub(crate) const DEFAULT_RADIUS: f32 = 3.0;

pub(crate) fn pill_radius(h: f32) -> CornerRadius {
    CornerRadius::same((h / 2.0).min(127.0) as u8)
}

pub(crate) fn panel_frame(opts: &Opts) -> egui::Frame {
    let px = opts.px(k::PADDING_X, 0.0);
    let py = opts.px(k::PADDING_Y, 0.0);
    let mut frame = egui::Frame::new().inner_margin(Margin::symmetric(px as i8, py as i8));
    if let Some(fill) = opts.opt_color(k::FILL) {
        frame = frame.fill(fill);
    } else if opts.boolean(k::TRANSPARENT, false) {
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
    ("ANCHOR_TOP_LEFT", w::TOP_LEFT),
    ("ANCHOR_TOP_RIGHT", w::TOP_RIGHT),
    ("ANCHOR_BOTTOM_LEFT", w::BOTTOM_LEFT),
    ("ANCHOR_BOTTOM_RIGHT", w::BOTTOM_RIGHT),
    ("ANCHOR_CENTER", w::CENTER),
    ("ANCHOR_CENTER_LEFT", w::CENTER_LEFT),
    ("ANCHOR_CENTER_RIGHT", w::CENTER_RIGHT),
    ("ANCHOR_CENTER_TOP", w::CENTER_TOP),
    ("ANCHOR_CENTER_BOTTOM", w::CENTER_BOTTOM),
    ("ANCHOR_FILL", w::FILL),
    ("ANCHOR_FILL_TOP", w::FILL_TOP),
    ("ANCHOR_FILL_BOTTOM", w::FILL_BOTTOM),
    ("ANCHOR_FILL_LEFT", w::FILL_LEFT),
    ("ANCHOR_FILL_RIGHT", w::FILL_RIGHT),
    ("ANCHOR_FILL_ACROSS", w::FILL_ACROSS),
    ("ANCHOR_FILL_DOWN", w::FILL_DOWN),
];

/// Widget kinds the layer draws.
pub const WIDGET_KINDS: &[(&str, &str)] = &[
    ("WIDGET_LABEL", w::LABEL),
    ("WIDGET_BUTTON", w::BUTTON),
    ("WIDGET_PANEL", w::PANEL),
    ("WIDGET_ROW", w::ROW),
    ("WIDGET_COLUMN", w::COLUMN),
    ("WIDGET_SCROLL", w::SCROLL),
    ("WIDGET_TABS", w::TABS),
    ("WIDGET_DRAW", w::DRAW),
    ("WIDGET_IMAGE", w::IMAGE),
    ("WIDGET_TEXT_FIELD", w::TEXT_FIELD),
    ("WIDGET_TEXT_AREA", w::TEXT_AREA),
    ("WIDGET_CHECKBOX", w::CHECKBOX),
    ("WIDGET_SWITCH", w::SWITCH),
    ("WIDGET_COLOR_PICKER", w::COLOR_PICKER),
    ("WIDGET_DROPDOWN", w::DROPDOWN),
    ("WIDGET_MENU", w::MENU),
    ("WIDGET_LIST", w::LIST),
    ("WIDGET_TREE", w::TREE),
    ("WIDGET_TABLE", w::TABLE),
    ("WIDGET_SLIDER", w::SLIDER),
    ("WIDGET_NUMBER_FIELD", w::NUMBER_FIELD),
    ("WIDGET_PROGRESS_BAR", w::PROGRESS_BAR),
    ("WIDGET_GRID", w::GRID),
    ("WIDGET_FLOW", w::FLOW),
    ("WIDGET_FOLD", w::FOLD),
    ("WIDGET_DIALOG", w::DIALOG),
    ("WIDGET_TOAST", w::TOAST),
    ("WIDGET_WINDOW", w::WINDOW),
    ("WIDGET_SEPARATOR", w::SEPARATOR),
    ("WIDGET_CODE", w::CODE),
    ("WIDGET_STACK", w::STACK),
];

/// Where a container puts its children, and where text sits in its width.
pub const ALIGNS: &[(&str, &str)] = &[
    ("ALIGN_START", w::START),
    ("ALIGN_CENTER", w::CENTER),
    ("ALIGN_END", w::END),
];

/// Slant, for `font_style`.
pub const FONT_STYLES: &[(&str, &str)] = &[
    ("FONT_STYLE_NORMAL", w::NORMAL),
    ("FONT_STYLE_ITALIC", w::ITALIC),
];

/// Font families the theme registers.
pub const FONTS: &[(&str, &str)] = &[("FONT_MONO", w::MONO), ("FONT_HEADING", w::HEADING)];

/// The screen classes, as `ui.width_class` and `ui.height_class` answer them
/// and as a widget's own class table names them. The words are the engine's
/// and a project cannot add to them: a theme, a scene and an addon share
/// them, so one that meant something else somewhere would not be a word.
pub const CLASSES: &[(&str, &str)] = &[
    ("WIDTH_NARROW", balaur_core::facts::NARROW),
    ("WIDTH_MEDIUM", balaur_core::facts::MEDIUM),
    ("WIDTH_WIDE", balaur_core::facts::WIDE),
    ("HEIGHT_SHORT", balaur_core::facts::SHORT),
    ("HEIGHT_TALL", balaur_core::facts::TALL),
    ("INPUT_TOUCH", balaur_core::tags::TOUCH),
    ("INPUT_POINTER", balaur_core::tags::POINTER),
];

/// A chord as a scene or a script writes it: modifiers and a key joined by
/// `+`, in any case, as in `cmd+shift+s` or `f5`.
///
/// One spelling for the `shortcut` property and the `ui::shortcut` binding,
/// and one place that knows `cmd` is Command on a Mac and Control elsewhere.
/// A word that names neither a modifier nor a key answers `None`, so a typo
/// is a shortcut that never fires rather than a key nobody asked for.
pub(crate) fn chord(text: &str) -> Option<(egui::Modifiers, egui::Key)> {
    let mut modifiers = egui::Modifiers::NONE;
    let mut key = None;
    for part in text.split('+') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        modifiers |= match part.to_ascii_lowercase().as_str() {
            w::CMD => egui::Modifiers::COMMAND,
            w::CTRL => egui::Modifiers::CTRL,
            w::ALT => egui::Modifiers::ALT,
            w::SHIFT => egui::Modifiers::SHIFT,
            // egui names its keys `Escape`, `F5`, `Backslash`; a scene writes
            // them the way it writes everything else, in lower case.
            lower => {
                let mut named = lower.to_string();
                named[..1].make_ascii_uppercase();
                key = egui::Key::from_name(part).or_else(|| egui::Key::from_name(&named));
                key?;
                continue;
            }
        };
    }
    Some((modifiers, key?))
}

/// A chord in the spelling the platform shows: `⌘⇧S` on a Mac, `Ctrl+Shift+S`
/// elsewhere. What a menu row draws against its far edge when it says a
/// shortcut and no `trailing` of its own.
pub(crate) fn chord_shown(ctx: &egui::Context, text: &str) -> Option<String> {
    let (modifiers, key) = chord(text)?;
    Some(ctx.format_shortcut(&egui::KeyboardShortcut::new(modifiers, key)))
}

/// Keyboard modifiers accepted by shortcut bindings.
pub const MODIFIERS: &[(&str, &str)] = &[
    ("MODIFIER_CMD", w::CMD),
    ("MODIFIER_CTRL", w::CTRL),
    ("MODIFIER_ALT", w::ALT),
    ("MODIFIER_SHIFT", w::SHIFT),
];

/// Declare `ui.*`. Takes the `Registry` rather than a module because it opens
/// the module itself: through the registry, so a scriptless app builds the
/// plugin instead of panicking.
pub(crate) fn install_ui_api(reg: &mut Registry<'_>) -> Result<()> {
    let mut m = reg.script_module("ui")?;
    let m: &mut dyn Bindings<Engine> = &mut *m;

    m.module_doc(
        "Immediate-mode UI redrawn from a script's `draw_ui` every frame: panels, layout containers and widgets. HUD elements in the scene tree are the `widget` component.",
    );

    for (name, value) in ANCHORS
        .iter()
        .chain(WIDGET_KINDS)
        .chain(ALIGNS)
        .chain(FONT_STYLES)
        .chain(FONTS)
        .chain(CLASSES)
        .chain(MODIFIERS)
    {
        m.constant(name, balaur_script::Value::Str((*value).to_string()));
    }
    crate::immediate::bindings::install_theme(m);
    crate::contrast::install(m);
    crate::immediate::bindings::install_panels(m);
    crate::immediate::bindings::install_containers(m);
    crate::immediate::bindings::install_text(m);
    crate::immediate::bindings::install_buttons(m);
    crate::immediate::bindings::install_controls(m);
    crate::immediate::bindings::install_text_input(m);
    crate::immediate::bindings::install_code(m);
    crate::immediate::bindings::install_modal(m);
    crate::immediate::bindings::install_window(m);
    crate::immediate::bindings::install_widget_layer(m);
    crate::immediate::bindings::install_scale(m);
    crate::immediate::bindings::install_classes(m);
    crate::pacing::install(m);
    crate::loading::install(m);
    crate::immediate::bindings::install_code_editor(m);
    crate::immediate::bindings::install_dropdown_select(m);
    crate::immediate::bindings::install_images(m);
    crate::immediate::bindings::install_queries(m);

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
    let seed = opts.string(k::VALUE);
    let mut buffer = {
        let mut state = state.borrow_mut();
        if let Some(value) = seed
            && state.text_seeds.get(id) != Some(&value)
        {
            state.text_seeds.insert(id.to_string(), value.clone());
            state.text_buffers.insert(id.to_string(), value);
        }
        state.text_buffers.get(id).cloned().unwrap_or_default()
    };
    let autofocus = opts.boolean(k::AUTOFOCUS, false) && {
        let mut state = state.borrow_mut();
        state.focused_once.insert(id.to_string())
    };
    let id_owned = id.to_string();
    let result = with_ui(|ui| {
        // The field's id is its own, so last pass's response is there to read:
        // the shell it wears is painted before egui lays the text out.
        let was = ui.ctx().read_response(egui::Id::new(id_owned.clone()));
        let opts = &opts.in_state(was.as_ref().is_some_and(egui::Response::hovered), false);
        let size = opts.px(k::FONT_SIZE, theme::size(t::FONT_SIZE));
        let family = theme::family(opts.str(k::FONT_FAMILY).unwrap_or(w::UI));
        // The hint carries the field's own font: a bare string is laid out in
        // egui's default body style, at neither this size nor this scale.
        let font = FontId::new(size, family);
        let font_for_margin = font.clone();
        let mut edit = egui::TextEdit::singleline(&mut buffer)
            .id(egui::Id::new(id_owned.clone()))
            .frame(egui::Frame::NONE)
            .hint_text(egui::RichText::new(placeholder).font(font.clone()))
            .font(font);
        if let Some(color) = opts.opt_color(k::TEXT_COLOR) {
            edit = edit.text_color(color);
        }
        // A `height` asks for the pill shell every other inspector control
        // wears; its padding comes out of the width the caller asked for.
        let h = opts.px(k::HEIGHT, 0.0);
        let pad = if h > 0.0 { 11.0 } else { 0.0 };
        let w = opts.px(k::WIDTH, 0.0);
        if w > 0.0 {
            edit = edit.desired_width((w - pad * 2.0).max(8.0));
        }
        let response = if h > 0.0 {
            // Centred by the margin, not by a centring layout: a layout that
            // centres also fills, and the field then took the whole panel.
            let line = ui.fonts_mut(|f| f.row_height(&font_for_margin));
            let vpad = ((h - line) / 2.0).max(0.0);
            let radius = opts.px(k::CORNER_RADIUS, 0.0);
            let corner = if radius > 0.0 {
                pill_radius(radius * 2.0)
            } else {
                pill_radius(theme::size(t::RADIUS_SMALL) * 2.0)
            };
            egui::Frame::new()
                .fill(opts.color(k::FILL, Color32::TRANSPARENT))
                .stroke(opts.stroke_or(Color32::TRANSPARENT))
                .corner_radius(corner)
                .inner_margin(Margin::symmetric(pad as i8, vpad as i8))
                .show(ui, |ui| ui.add(edit))
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

/// The wash a control takes under the pointer when its theme dresses no state
/// of its own: painted over whatever is already there, so a fill the caller
/// chose still reads as the fill it chose. Light on a dark theme, dark on a
/// light one.
pub(crate) fn wash(ui: &egui::Ui, held: bool) -> Color32 {
    let alpha = if held { 30 } else { 18 };
    if ui.visuals().dark_mode {
        Color32::from_white_alpha(alpha)
    } else {
        Color32::from_black_alpha(alpha)
    }
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
    let h = opts.px(k::HEIGHT, theme::size(t::CONTROL_HEIGHT));
    let w = {
        let w = opts.px(k::MIN_WIDTH, 0.0);
        if w > 0.0 {
            w
        } else {
            ui.available_width().max(40.0)
        }
    };
    let (rect, response) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    // The box is measured before the state is known and painted after: a row
    // that grew under the pointer would push the rows below it down.
    let opts = &opts.in_state(response.hovered(), response.is_pointer_button_down_on());
    let fill = opts.color(k::FILL, Color32::TRANSPARENT);
    let lit = response.hovered() && !opts.dressed();
    // Tiles by default, like every other button; `corner_radius = "full"`
    // opts back in.
    let asked = opts.px(k::CORNER_RADIUS, 0.0);
    let corner = if asked > 0.0 {
        pill_radius(asked * 2.0)
    } else if opts.is_pill() {
        pill_radius(h)
    } else {
        pill_radius(theme::size(t::RADIUS_SMALL) * 2.0)
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, corner, fill);
    }
    if lit {
        ui.painter()
            .rect_filled(rect, corner, wash(ui, response.is_pointer_button_down_on()));
    }
    if let Some(stroke) = opts.opt_stroke() {
        ui.painter().rect(
            rect,
            corner,
            Color32::TRANSPARENT,
            stroke,
            StrokeKind::Inside,
        );
    }
    let fam = opts.str(k::FONT_FAMILY).unwrap_or(w::UI);
    let size = opts.px(k::FONT_SIZE, theme::size(t::FONT_SIZE));
    let color = opts.color(k::TEXT_COLOR, Color32::WHITE);
    let mut x = rect.min.x + 10.0;
    if let Some(icon) = opts.string(k::ICON) {
        let icon_color = opts.opt_color(k::ICON_COLOR).unwrap_or(color);
        let galley = ui.painter().layout_no_wrap(
            icon,
            FontId::new(opts.px(k::ICON_SIZE, 12.0), theme::family(fam)),
            icon_color,
        );
        let y = rect.center().y - galley.size().y / 2.0;
        ui.painter().galley(pos2(x, y), galley, icon_color);
        x += 7.0 + opts.px(k::ICON_SIZE, 12.0);
    }
    let mut font = FontId::new(size, theme::family(fam));
    if opts.bold() {
        font = FontId::new(
            size,
            theme::family(if fam == w::UI { w::HEADING } else { fam }),
        );
    }
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, color);
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter().galley(pos2(x, y), galley, color);
    if let Some(trailing) = opts.string(k::TRAILING) {
        let t_color = opts.opt_color(k::TRAILING_COLOR).unwrap_or(color);
        let galley = ui.painter().layout_no_wrap(
            trailing,
            FontId::new(opts.px(k::TRAILING_SIZE, 11.0), theme::family(fam)),
            t_color,
        );
        let ty = rect.center().y - galley.size().y / 2.0;
        ui.painter().galley(
            pos2(rect.max.x - 11.0 - galley.size().x, ty),
            galley,
            t_color,
        );
    }
    if let Some(text) = opts.str(k::TOOLTIP) {
        crate::widget::theme::tip(&response, text);
    }
    crate::immediate::layout::attach_menus(eng, &response, opts);
    note_pill(rect);
    Ok(response.clicked())
}

#[cfg(test)]
mod tests {
    use super::KNOWN_KEYS;

    #[test]
    fn known_keys_stay_sorted_for_the_binary_search() {
        let mut sorted = KNOWN_KEYS.to_vec();
        sorted.sort_unstable();
        assert_eq!(KNOWN_KEYS, sorted.as_slice());
    }

    /// A key a widget reads and the list leaves out is warned about on every
    /// call that passes it, and a warning fails a test run.
    #[test]
    fn every_key_a_widget_reads_is_known() {
        let source = [
            include_str!("mod.rs"),
            include_str!("bindings.rs"),
            include_str!("code.rs"),
            include_str!("layout.rs"),
        ]
        .concat();
        let vocabulary = include_str!("../vocabulary.rs");
        let keys = &vocabulary[vocabulary.find("mod keys {").unwrap()..];
        let value_of = |name: &str| {
            let at = keys.find(&format!("const {name}: &str = \"")).unwrap();
            let rest = &keys[at..];
            let open = rest.find('"').unwrap() + 1;
            let close = open + rest[open..].find('"').unwrap();
            rest[open..close].to_string()
        };
        let mut missing = Vec::new();
        for (at, _) in source.match_indices("opts.") {
            let call = &source[at..];
            let Some(open) = call.find('(').filter(|&i| i < 24) else {
                continue;
            };
            let Some(name) = call[open + 1..].trim_start().strip_prefix("k::") else {
                continue;
            };
            let name: String = name
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                .collect();
            let value = value_of(&name);
            if !KNOWN_KEYS.contains(&value.as_str()) && !missing.contains(&value) {
                missing.push(value);
            }
        }
        assert!(
            missing.is_empty(),
            "read by a widget, missing from KNOWN_KEYS: {missing:?}"
        );
    }
}
