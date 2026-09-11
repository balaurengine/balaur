//! The `ui` module: panels, containers, and the design system's widget
//! shapes (pill, circle button, field, toggle, slider, code line, modal).
//! Every color arrives per call from the script's token table, so themes are
//! entirely script-defined. The code editor is [`code`].

pub(crate) mod bindings;
pub(crate) mod code;
pub(crate) mod layout;

use anyhow::Result;
use balaur_core::Engine;
use balaur_plugin::Registry;
use balaur_script::{Bindings, CallbackId, Value};
use egui::{Color32, CornerRadius, FontId, Margin, Sense, Stroke, StrokeKind, pos2, vec2};
use std::cell::RefCell;
use std::collections::BTreeSet;

use crate::UiState;
use crate::bridge::{scale, with_ui};
use crate::theme::{self, parse_hex};
use crate::vocabulary::{keys as k, words as w};

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
);

/// What a role's `hover` or `active` table may set. Paint only: a control
/// that resized under the pointer would move whatever sits beside it.
const STATE_KEYS: &[&str] = &[
    k::COLOR,
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
    k::ALIGN,
    k::AUTOFOCUS,
    k::BG,
    k::BREAKPOINT_COLOR,
    k::BREAKPOINTS,
    k::CLOSABLE,
    k::COLLAPSIBLE,
    k::COLOR,
    k::CURRENT_FILL,
    k::CURRENT_LINE,
    k::D,
    k::DASHED,
    k::DECIMALS,
    k::DISABLED,
    k::FILL,
    k::FONT,
    k::GUTTER_COLOR,
    k::GUTTER_WIDTH,
    k::H,
    k::HEIGHT,
    k::HIGHLIGHT,
    k::HOVER_FILL,
    k::ICON,
    k::ICON_COLOR,
    k::ICON_SIZE,
    k::K_COM,
    k::K_FN,
    k::K_KEY,
    k::K_NUM,
    k::K_PUNC,
    k::K_STR,
    k::K_TYPE,
    k::KEEP_OPEN,
    k::KNOB,
    k::LANGUAGE,
    k::LINE_HEIGHT,
    k::MAX,
    k::MAX_HEIGHT,
    k::MENU,
    k::MENU_CLICK,
    k::MIN,
    k::MIN_WIDTH,
    k::OFF_FILL,
    k::OFF_KNOB,
    k::OFFSET,
    k::ON_FILL,
    k::ON_KNOB,
    k::PADDING,
    k::PADDING_X,
    k::PADDING_Y,
    k::PREFIX,
    k::PREFIX_COLOR,
    k::PROBLEM_COLOR,
    k::PROBLEMS,
    k::RADIUS,
    k::RAIL,
    k::RESIZABLE,
    k::ROLE,
    k::ROUND,
    k::ROW_HEIGHT,
    k::SCRIM,
    k::SEPARATOR,
    k::SIZE,
    k::SPEED,
    k::STICK_TO_BOTTOM,
    k::STROKE,
    k::STRONG,
    k::SUFFIX,
    k::TIGHT,
    k::TITLE,
    k::TOOLTIP,
    k::TOP,
    k::TRAILING,
    k::TRAILING_COLOR,
    k::TRAILING_SIZE,
    k::TRANSPARENT,
    k::TRUNCATE,
    k::VALUE,
    k::W,
    k::WARNING_COLOR,
    k::WARNINGS,
    k::WIDTH,
    k::WRAP,
    k::X,
    k::Y,
];

/// Report an option key no widget reads, once per name.
///
/// A typo used to do nothing at all: the value fell back to its default and
/// the call looked as though it had been honoured.
///
/// Runs on every options table of every widget of every frame, so the hit
/// path is a binary search and nothing else: `KNOWN_KEYS` is sorted, checked
/// by the test below, and the miss path is the only one that allocates.
fn warn_unknown(entries: &[(String, Value)]) {
    thread_local! {
        static WARNED: RefCell<BTreeSet<String>> = const { RefCell::new(BTreeSet::new()) };
    }
    for (key, _) in entries {
        if KNOWN_KEYS.binary_search(&key.as_str()).is_ok() {
            continue;
        }
        WARNED.with(|warned| {
            if warned.borrow_mut().insert(key.clone()) {
                tracing::warn!("ui: no widget reads the option `{key}`");
            }
        });
    }
}

impl Opts {
    /// The caller's options over the named role's. A call site then says only
    /// what it changes, and the look lives in the theme asset.
    pub(crate) fn with_roles(opts: Option<Value>) -> Self {
        let Some(Value::Map(given)) = opts.as_ref() else {
            return Self(opts, None, "");
        };
        warn_unknown(given);
        let role = match given.iter().find(|(k, _)| k == k::ROLE).map(|(_, v)| v) {
            Some(Value::Str(name)) => crate::bridge::role(name),
            _ => None,
        };
        Self(opts, role, "")
    }

    /// Options as given, with no role behind them.
    pub(crate) fn plain(opts: Option<Value>) -> Self {
        Self(opts, None, "")
    }

    /// The same options with the role's `hover` or `active` table painting
    /// over them. Held falls back to hovered, the way a theme's own styles do.
    pub(crate) fn in_state(&self, hovered: bool, held: bool) -> Self {
        let state = if held {
            "active"
        } else if hovered {
            "hover"
        } else {
            ""
        };
        Self(self.0.clone(), self.1.clone(), state)
    }

    /// What the role's state table says about `key`, for the paint keys a
    /// state may set. `active` falls through to `hover` for what it leaves out.
    fn state(&self, key: &str) -> Option<&Value> {
        if self.2.is_empty() || !STATE_KEYS.contains(&key) {
            return None;
        }
        let role = self.1.as_ref()?;
        let table = |name: &str| match role.iter().find(|(k, _)| k == name).map(|(_, v)| v) {
            Some(Value::Map(entries)) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        };
        table(self.2).or_else(|| (self.2 == "active").then(|| table("hover")).flatten())
    }

    /// Whether the role dresses the state the control is in. One that does
    /// not takes [`wash`] instead, so every control answers the pointer.
    pub(crate) fn dressed(&self) -> bool {
        let Some(role) = self.1.as_ref() else {
            return false;
        };
        !self.2.is_empty()
            && role
                .iter()
                .any(|(k, v)| (k == self.2 || k == "hover") && matches!(v, Value::Map(_)))
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

/// Scale a literal design dimension.
pub(crate) fn sc(v: f32) -> f32 {
    v * scale()
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
    ("ANCHOR_FILL", "fill"),
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
    ("WIDGET_TAB", w::TAB),
    ("WIDGET_DRAW", "draw"),
    ("WIDGET_IMAGE", w::IMAGE),
    ("WIDGET_FIELD", w::FIELD),
    ("WIDGET_TEXT_AREA", w::TEXT_AREA),
    ("WIDGET_CHECK", w::CHECK),
    ("WIDGET_COLOR", w::COLOR),
    ("WIDGET_DROPDOWN", w::DROPDOWN),
    ("WIDGET_MENU", w::MENU),
    ("WIDGET_LIST", w::LIST),
    ("WIDGET_TREE", w::TREE),
    ("WIDGET_TABLE", w::TABLE),
    ("WIDGET_SLIDER", w::SLIDER),
    ("WIDGET_DRAG_VALUE", w::DRAG_VALUE),
    ("WIDGET_PROGRESS", w::PROGRESS),
    ("WIDGET_GRID", w::GRID),
    ("WIDGET_FLOW", w::FLOW),
    ("WIDGET_FOLD", w::FOLD),
    ("WIDGET_DIALOG", w::DIALOG),
    ("WIDGET_WINDOW", w::WINDOW),
    ("WIDGET_SEPARATOR", "separator"),
    ("WIDGET_CODE", w::CODE),
];

/// Where a container puts its children, and where text sits in its width.
pub const ALIGNS: &[(&str, &str)] = &[
    ("ALIGN_START", w::START),
    ("ALIGN_CENTER", w::CENTER),
    ("ALIGN_END", w::END),
];

/// The one `align` a `ui.pill` reads: against the left edge, or centred.
pub const PILL_ALIGNS: &[(&str, &str)] = &[("ALIGN_LEFT", w::LEFT)];

/// Slant, for `font_style`.
pub const FONT_STYLES: &[(&str, &str)] = &[
    ("FONT_STYLE_NORMAL", w::NORMAL),
    ("FONT_STYLE_ITALIC", w::ITALIC),
];

/// Font families the theme registers.
pub const FONTS: &[(&str, &str)] = &[("FONT_MONO", w::MONO), ("FONT_HEADING", w::HEADING)];

/// Keyboard modifiers accepted by shortcut bindings.
pub const MODIFIERS: &[(&str, &str)] = &[
    ("MOD_CMD", w::CMD),
    ("MOD_CTRL", w::CTRL),
    ("MOD_ALT", w::ALT),
    ("MOD_SHIFT", w::SHIFT),
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
        .chain(ALIGNS)
        .chain(PILL_ALIGNS)
        .chain(FONT_STYLES)
        .chain(FONTS)
        .chain(MODIFIERS)
    {
        m.constant(name, balaur_script::Value::Str((*value).to_string()));
    }
    crate::immediate::bindings::install_theme(m);
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
    crate::pacing::install(m);
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
        let size = opts.px(k::SIZE, 13.0);
        let family = theme::family(opts.str(k::FONT).unwrap_or(w::UI));
        // The hint carries the field's own font: a bare string is laid out in
        // egui's default body style, at neither this size nor this scale.
        let font = FontId::new(size, family);
        let font_for_margin = font.clone();
        let mut edit = egui::TextEdit::singleline(&mut buffer)
            .id(egui::Id::new(id_owned.clone()))
            .frame(egui::Frame::NONE)
            .hint_text(egui::RichText::new(placeholder).font(font.clone()))
            .font(font);
        if let Some(color) = opts.opt_color(k::COLOR) {
            edit = edit.text_color(color);
        }
        // A `height` asks for the pill shell every other inspector control
        // wears; its padding comes out of the width the caller asked for.
        let h = opts.px(k::HEIGHT, 0.0);
        let pad = if h > 0.0 { sc(11.0) } else { 0.0 };
        let w = opts.px(k::WIDTH, 0.0);
        if w > 0.0 {
            edit = edit.desired_width((w - pad * 2.0).max(sc(8.0)));
        }
        let response = if h > 0.0 {
            // Centred by the margin, not by a centring layout: a layout that
            // centres also fills, and the field then took the whole panel.
            let line = ui.fonts_mut(|f| f.row_height(&font_for_margin));
            let vpad = ((h - line) / 2.0).max(0.0);
            let radius = opts.px(k::RADIUS, 0.0);
            let corner = if radius > 0.0 {
                pill_radius(radius * 2.0)
            } else {
                pill_radius(sc(5.0) * 2.0)
            };
            egui::Frame::new()
                .fill(opts.color(k::FILL, Color32::TRANSPARENT))
                .stroke(Stroke::new(
                    1.0,
                    opts.color(k::STROKE, Color32::TRANSPARENT),
                ))
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
    let h = opts.px(k::HEIGHT, 27.0);
    let w = {
        let w = opts.px(k::MIN_WIDTH, 0.0);
        if w > 0.0 {
            w
        } else {
            ui.available_width().max(sc(40.0))
        }
    };
    let (rect, mut response) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    // The box is measured before the state is known and painted after: a row
    // that grew under the pointer would push the rows below it down.
    let opts = &opts.in_state(response.hovered(), response.is_pointer_button_down_on());
    let mut fill = opts.color(k::FILL, Color32::TRANSPARENT);
    if fill == Color32::TRANSPARENT
        && response.hovered()
        && let Some(hover) = opts.opt_color(k::HOVER_FILL)
    {
        fill = hover;
    }
    let lit = response.hovered() && !opts.dressed() && opts.opt_color(k::HOVER_FILL).is_none();
    // Tiles by default, like every other pill; `round` opts back in.
    let asked = opts.px(k::RADIUS, 0.0);
    let corner = if asked > 0.0 {
        pill_radius(asked * 2.0)
    } else if opts.boolean(k::ROUND, false) {
        pill_radius(h)
    } else {
        pill_radius(sc(5.0) * 2.0)
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, corner, fill);
    }
    if lit {
        ui.painter()
            .rect_filled(rect, corner, wash(ui, response.is_pointer_button_down_on()));
    }
    if let Some(stroke) = opts.opt_color(k::STROKE) {
        ui.painter().rect(
            rect,
            corner,
            Color32::TRANSPARENT,
            Stroke::new(1.0, stroke),
            StrokeKind::Inside,
        );
    }
    let fam = opts.str(k::FONT).unwrap_or(w::UI);
    let size = opts.px(k::SIZE, 12.0);
    let color = opts.color(k::COLOR, Color32::WHITE);
    let mut x = rect.min.x + sc(10.0);
    if let Some(icon) = opts.string(k::ICON) {
        let icon_color = opts.opt_color(k::ICON_COLOR).unwrap_or(color);
        let galley = ui.painter().layout_no_wrap(
            icon,
            FontId::new(opts.px(k::ICON_SIZE, 12.0), theme::family(fam)),
            icon_color,
        );
        let y = rect.center().y - galley.size().y / 2.0;
        ui.painter().galley(pos2(x, y), galley, icon_color);
        x += sc(7.0) + opts.px(k::ICON_SIZE, 12.0);
    }
    let mut font = FontId::new(size, theme::family(fam));
    if opts.boolean(k::STRONG, false) {
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
            pos2(rect.max.x - sc(11.0) - galley.size().x, ty),
            galley,
            t_color,
        );
    }
    if let Some(tip) = opts.string(k::TOOLTIP) {
        response = response.on_hover_text(tip);
    }
    crate::immediate::layout::attach_menus(eng, &response, opts);
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
}
