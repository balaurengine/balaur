//! The `widget_theme` asset: the chrome a widget kind is drawn with.
//!
//! What a widget owns is its text and its geometry — what it says and where
//! it sits. What a *theme* owns is how its kind looks: the fill behind it, the
//! outline around it, how round the corners are, how much air is inside. Those
//! are the values that were hardcoded until now, and the ones a game wants to
//! change once rather than on every node.
//!
//! ```toml
//! type = "widget_theme"
//!
//! [colors]
//! ink = "#2a2f3a"
//! edge = "#d5814e"
//!
//! [button]
//! fill = "ink"
//! stroke = "edge"
//! radius = 12
//! padding = 8
//!
//! [button.hover]
//! fill = "edge"
//!
//! [roles.danger]
//! fill = "#a03030"
//! color = "#ffffff"
//! ```
//!
//! A kind the file does not mention keeps the built-in look, so a theme that
//! restyles buttons alone is three lines and says only what it changes. A
//! `role` is the same table under a name of its own: a widget naming one takes
//! it over its kind's, which is how one file dresses both the `ui::*` calls a
//! script makes and the nodes a scene holds.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use balaur_core::Engine;
use smol_str::SmolStr;

use crate::theme::family;
use crate::vocabulary::keys as k;
use crate::vocabulary::states as st;
use crate::vocabulary::tokens as t;
use crate::vocabulary::weights;
use crate::vocabulary::words as w;
use crate::widget::node::{Widget, rgba_color};
use egui::Color32;

/// Where the pointer is, as far as one widget is concerned.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Pointer {
    #[default]
    Away,
    Over,
    Held,
}

impl Pointer {
    /// Where the pointer is for a widget that answers with its own response
    /// rather than with the box the layout gave it.
    #[must_use]
    pub fn of(response: &egui::Response) -> Self {
        if response.is_pointer_button_down_on() {
            Self::Held
        } else if response.hovered() {
            Self::Over
        } else {
            Self::Away
        }
    }
}

/// What a widget is being, which the theme's state tables answer to.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct WidgetState {
    pub pointer: Pointer,
    pub disabled: bool,
    pub focused: bool,
    /// On: a switch or checkbox that is set, a toggle button held down, the
    /// tab or row that is picked.
    pub checked: bool,
}

impl WidgetState {
    /// Whether any state table could apply.
    #[must_use]
    pub fn any(self) -> bool {
        self.pointer != Pointer::Away || self.disabled || self.focused || self.checked
    }
}

/// How one widget kind is drawn.
///
/// Every field is optional so a style can sit over another and say only what
/// it changes; [`Style::over`] is that merge, and it is what stacks a node's
/// own values over its role over its kind.
#[derive(Clone, Default)]
pub struct Style {
    pub fill: Option<Color32>,
    pub stroke: Option<Color32>,
    /// Corner radius in design pixels; `None` keeps the kind's own rule,
    /// which for a button is "as round as its text is tall".
    pub radius: Option<f32>,
    pub padding: Option<f32>,
    pub stroke_width: Option<f32>,
    /// A project-relative picture drawn as the kind's background instead of
    /// `fill`, stretched by `slice`.
    pub image: Option<String>,
    /// Left, top, right and bottom borders of `image` kept unstretched, in
    /// the picture's own pixels.
    pub slice: [f32; 4],
    /// The ink a caption is drawn in, which a role spells `text_color`.
    pub text_color: Option<Color32>,
    /// The disc a control's picture sits on, so a dark mark reads on a dark
    /// sheet.
    pub plate: Option<Color32>,
    /// Where a control's content sits across it: `left` for a row, else centred.
    pub align: Option<String>,
    pub font_size: Option<f32>,
    /// Which of the theme's families, by the names `font_family` offers.
    pub font: Option<String>,
    /// Weight on the CSS scale; a role's `strong = true` is 700.
    pub weight: Option<f32>,
    /// The box a role asks for, in design pixels, for a role that carries its
    /// own control size the way the editor's `tab` and `chip` do. A node that
    /// states one of its own wins; this is not a floor.
    pub height: Option<f32>,
    pub width: Option<f32>,
    /// The space inside the left and right edges, and inside the top and
    /// bottom ones, in design pixels: either side of a control's caption, and
    /// round a container's children. Each wins over `padding` on its axis.
    pub padding_x: Option<f32>,
    pub padding_y: Option<f32>,
    /// The gap between a container's children, in design pixels: Godot's
    /// theme separations, which a scene overrides with its own `gap`.
    pub gap: Option<f32>,
    /// The ink a control's picture is drawn in — a button's icon — where the
    /// theme tints it rather than showing the artwork's own colours.
    pub icon_color: Option<Color32>,
    /// As round as it is tall, which is what `corner_radius = "full"` says.
    pub round: Option<bool>,
    /// What replaces this style while the pointer is over the widget, and
    /// while it is held down. Each is a whole style over this one.
    pub hover: Option<Rc<Style>>,
    /// The `[kind.<class>]` tables this entry carries, by class word. Folded
    /// over the entry when the screen answers to one, so a theme states a
    /// touch height beside the height it states for a cursor.
    pub classes: Option<Rc<Vec<(SmolStr, Style)>>>,
    pub active: Option<Rc<Style>>,
    /// What replaces this style while the widget is disabled, and while
    /// keyboard focus is on it and the pointer is not.
    pub disabled: Option<Rc<Style>>,
    pub focus: Option<Rc<Style>>,
    /// What the widget wears while it is on, under whichever pointer table
    /// applies; its own `hover` and `active` win over the entry's.
    pub checked: Option<Rc<Style>>,
    /// A `fold`'s arrow picture, and the frame around its open children.
    pub arrow: Option<String>,
    pub body: Option<Rc<Style>>,
}

impl Style {
    /// The outline width to draw, once a style has said whether it wants one.
    pub fn stroke_px(&self) -> f32 {
        self.stroke_width.unwrap_or(1.0)
    }

    /// The paint half of a style, for a `hover` or `active` table. A state
    /// that resized would move whatever sits beside the widget, and the
    /// layout is solved before anything knows where the pointer is.
    #[must_use]
    pub fn paint_only(&self) -> Self {
        Self {
            padding: None,
            padding_x: None,
            padding_y: None,
            height: None,
            width: None,
            font_size: None,
            font: None,
            weight: None,
            ..self.clone()
        }
    }

    /// `self` over `base`: what `self` states wins and what it leaves out
    /// falls through, so a role says only how it differs from its kind.
    #[must_use]
    pub fn over(&self, base: &Self) -> Self {
        #[allow(clippy::float_cmp, reason = "a slice left unset, not one measured")]
        let slice = if self.slice == [0.0; 4] {
            base.slice
        } else {
            self.slice
        };
        Self {
            fill: self.fill.or(base.fill),
            stroke: self.stroke.or(base.stroke),
            radius: self.radius.or(base.radius),
            padding: self.padding.or(base.padding),
            stroke_width: self.stroke_width.or(base.stroke_width),
            image: self.image.clone().or_else(|| base.image.clone()),
            slice,
            text_color: self.text_color.or(base.text_color),
            plate: self.plate.or(base.plate),
            align: self.align.clone().or_else(|| base.align.clone()),
            font_size: self.font_size.or(base.font_size),
            font: self.font.clone().or_else(|| base.font.clone()),
            weight: self.weight.or(base.weight),
            height: self.height.or(base.height),
            width: self.width.or(base.width),
            padding_x: self.padding_x.or(base.padding_x),
            padding_y: self.padding_y.or(base.padding_y),
            gap: self.gap.or(base.gap),
            icon_color: self.icon_color.or(base.icon_color),
            round: self.round.or(base.round),
            hover: self.hover.clone().or_else(|| base.hover.clone()),
            classes: self.classes.clone().or_else(|| base.classes.clone()),
            active: self.active.clone().or_else(|| base.active.clone()),
            disabled: self.disabled.clone().or_else(|| base.disabled.clone()),
            focus: self.focus.clone().or_else(|| base.focus.clone()),
            checked: self.checked.clone().or_else(|| base.checked.clone()),
            arrow: self.arrow.clone().or_else(|| base.arrow.clone()),
            body: self.body.clone().or_else(|| base.body.clone()),
        }
    }

    /// The style with the tables for the widget's state over it: `checked`
    /// under everything, then disabled, held, hovered, and focused with the
    /// pointer elsewhere.
    #[must_use]
    pub fn in_states(&self, state: WidgetState) -> Self {
        if state.checked && self.checked.is_none() && !state.disabled {
            // A theme with no `checked` table dresses a checked widget as held.
            let held = WidgetState {
                pointer: Pointer::Held,
                checked: false,
                ..state
            };
            return self.in_states(held);
        }
        let on = match (&self.checked, state.checked) {
            (Some(checked), true) => checked.over(self),
            _ => self.clone(),
        };
        let patch = if state.disabled {
            on.disabled.as_ref()
        } else if state.pointer == Pointer::Held {
            on.active.as_ref().or(on.hover.as_ref())
        } else if state.pointer == Pointer::Over {
            on.hover.as_ref()
        } else if state.focused {
            on.focus.as_ref()
        } else {
            None
        };
        patch.map_or_else(|| on.clone(), |patch| patch.over(&on))
    }
}

/// A parsed theme: a style per widget kind, the same per named role, and the
/// colour tokens both spell their fills with.
#[derive(Default)]
pub struct WidgetTheme {
    kinds: BTreeMap<String, Style>,
    roles: BTreeMap<String, Style>,
    colors: BTreeMap<String, Color32>,
    sizes: BTreeMap<String, f32>,
    /// `resolved` remembered: a kind and a role name the same style for as
    /// long as the theme lives, and working it out walked two maps and cloned
    /// a style for every widget on the screen, every frame.
    settled: RefCell<rustc_hash::FxHashMap<(SmolStr, SmolStr), Rc<Style>>>,
    /// The class generation `settled` was filled under. A new one empties it
    /// rather than keying beside it, so a rotation does not leave the last
    /// screen's styles in the map for the life of the theme.
    settled_at: std::cell::Cell<u32>,
}

impl WidgetTheme {
    /// The style for a kind, or the empty one — which means "as before".
    #[must_use]
    pub fn style(&self, kind: &str) -> Style {
        if let Some(style) = self.kinds.get(kind) {
            return style.clone();
        }
        // A stack is a panel that lays its children over one another, so
        // either takes the other's entry when the theme names only one.
        let sibling = match kind {
            // A stack is a panel that lays its children over one another,
            // and a toast one that leaves on its own.
            w::STACK | w::TOAST => w::PANEL,
            w::PANEL => w::STACK,
            _ => return Style::default(),
        };
        self.kinds.get(sibling).cloned().unwrap_or_default()
    }

    /// A kind's style with the named role over it. An unknown role is no
    /// role: a theme that has not been given one yet still draws.
    #[must_use]
    pub fn resolved(&self, kind: &str, role: &str) -> Rc<Style> {
        let (active, generation) = pass_classes();
        // The screen's classes are part of the answer, so a rotation empties
        // what the last screen settled on rather than reading it back.
        if self.settled_at.replace(generation) != generation {
            self.settled.borrow_mut().clear();
        }
        let key = (SmolStr::new(kind), SmolStr::new(role));
        if let Some(held) = self.settled.borrow().get(&key) {
            return Rc::clone(held);
        }
        let base = self.style(kind);
        let settled = match self.roles.get(role) {
            Some(style) => style.over(&base),
            None => base,
        };
        let made = Rc::new(in_classes(&settled, &active));
        self.settled.borrow_mut().insert(key, Rc::clone(&made));
        made
    }

    /// A size by name: the theme's, or the one every theme derives when it
    /// states none.
    #[must_use]
    pub fn size(&self, name: &str) -> f32 {
        self.sizes
            .get(name)
            .copied()
            .unwrap_or_else(|| crate::palette::default_size(name))
    }

    /// A colour by the name `[colors]` filed it under, for a widget that
    /// names one rather than spelling a hex.
    #[must_use]
    pub fn token(&self, name: &str) -> Option<Color32> {
        if name == w::NONE {
            return Some(Color32::TRANSPARENT);
        }
        parse_color(name).or_else(|| self.colors.get(name).copied())
    }
}

/// One style with every class table the screen answers to folded over it,
/// broad to narrow. A style with no class table is returned as it stands.
fn in_classes(style: &Style, active: &[SmolStr]) -> Style {
    let Some(classes) = style.classes.as_ref() else {
        return style.clone();
    };
    let mut settled = style.clone();
    for (word, over) in classes.iter() {
        if active.iter().any(|held| held == word) {
            settled = over.over(&settled);
        }
    }
    settled
}

/// `#rrggbb` or `#rrggbbaa`, the spelling the editor's theme already uses.
fn parse_color(text: &str) -> Option<Color32> {
    let hex = text.strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(
            byte(0)?,
            byte(2)?,
            byte(4)?,
            byte(6)?,
        )),
        _ => None,
    }
}

/// A colour that does not parse is reported and dropped rather than being
/// silently black: a theme is authored by hand, and a typo should say so.
fn color(value: &toml::Value, what: &str, colors: &BTreeMap<String, Color32>) -> Option<Color32> {
    let text = value.as_str()?;
    let parsed = parse_color(text).or_else(|| colors.get(text).copied());
    if parsed.is_none() {
        tracing::warn!("widget theme: '{text}' is not a colour or a token for {what}");
    }
    parsed
}

/// The tables that name something other than a widget kind.
fn is_reserved(key: &str) -> bool {
    [k::TYPE, k::DARK, k::COLORS, k::ROLES, k::SIZES, k::BASE].contains(&key)
}

// The class words in force, and a number that changes when they do, so the
// resolved-style cache answers for this pass's screen, not the last one's.
thread_local! {
    static PASS_CLASSES: RefCell<(Rc<[SmolStr]>, u32)> = RefCell::new((Rc::from([]), 0));
}

/// Tell the theme which classes this pass answers to. Called once a pass,
/// before anything is drawn.
pub(crate) fn set_pass_classes(active: &[&str]) {
    PASS_CLASSES.with(|held| {
        let mut held = held.borrow_mut();
        if held.0.len() == active.len() && held.0.iter().zip(active).all(|(a, b)| a == b) {
            return;
        }
        held.0 = active.iter().map(|word| SmolStr::new(*word)).collect();
        held.1 = held.1.wrapping_add(1);
        // Shared with every `resolved` call this pass, which was cloning a
        // vector per widget before.
    });
}

/// The classes in force and the number that stands for them.
fn pass_classes() -> (Rc<[SmolStr]>, u32) {
    PASS_CLASSES.with(|held| {
        let held = held.borrow();
        (Rc::clone(&held.0), held.1)
    })
}

/// Whether a finger is what reaches this screen, as the pass settled it.
pub(crate) fn pass_is_touch() -> bool {
    PASS_CLASSES.with(|held| {
        held.borrow()
            .0
            .iter()
            .any(|word| word == balaur_core::tags::TOUCH)
    })
}

/// A tooltip a cursor gets by resting and a finger gets by holding.
///
/// Touch has no hover: a finger that lands is a click, and egui hides a
/// tooltip that a click preceded, so `on_hover_text` alone means a phone
/// never sees one. Held open until the finger lifts, since a long press is
/// one frame and a tooltip nobody can read is not one.
pub(crate) fn tip(response: &egui::Response, text: &str) {
    if text.is_empty() {
        return;
    }
    if !pass_is_touch() {
        response.clone().on_hover_text(text.to_owned());
        return;
    }
    // Timed here rather than taken from `Response::long_touched`, which egui
    // only sets on a widget that senses a click: a tooltip's own rect senses
    // hover, and giving it a click would take the press off the control.
    let (down, now) = response.ctx.input(|i| (i.pointer.any_down(), i.time));
    let length = response
        .ctx
        .options(|options| options.input_options.max_click_duration);
    let over = response.contains_pointer();
    HELD_TIP.with(|held| {
        let mut held = held.borrow_mut();
        if !down || !over {
            if held.is_some_and(|(id, _)| id == response.id) {
                *held = None;
            }
            return;
        }
        let (_, since) = *held.get_or_insert((response.id, now));
        if held.is_some_and(|(id, _)| id != response.id) {
            *held = Some((response.id, now));
            return;
        }
        if now - since >= length {
            response.show_tooltip_text(text.to_owned());
        }
    });
}

thread_local! {
    /// The widget a finger is resting on, and when it landed. One at a time,
    /// because one finger drives the pointer.
    static HELD_TIP: std::cell::RefCell<Option<(egui::Id, f64)>> =
        const { std::cell::RefCell::new(None) };
}

/// The colours and sizes a theme's tables name, resolved from its `[colors]`
/// and `[sizes]`.
pub(crate) struct Tokens {
    pub(crate) colors: BTreeMap<String, Color32>,
    pub(crate) sizes: BTreeMap<String, f32>,
}

/// One `[kind]` or `[roles.name]` table as a style, with its state tables
/// read as styles over it.
fn style_of(body: &toml::Table, tokens: &Tokens, what: &str) -> Style {
    // A number is written as one, or as the name of a size the theme states.
    let number = |key: &str| {
        let value = body.get(key)?;
        balaur_core::components::as_f64(value)
            .map(|v| v as f32)
            .or_else(|| tokens.sizes.get(value.as_str()?).copied())
    };
    let nested = |key: &str| {
        body.get(key)
            .and_then(toml::Value::as_table)
            .map(|table| Rc::new(style_of(table, tokens, what).paint_only()))
    };
    let colour = |key: &str| body.get(key).and_then(|v| color(v, what, &tokens.colors));
    let pill = body.get(k::CORNER_RADIUS).and_then(toml::Value::as_str) == Some(w::FULL);
    Style {
        fill: colour(k::FILL),
        stroke: colour(k::STROKE),
        radius: if pill { None } else { number(k::CORNER_RADIUS) },
        padding: number(k::PADDING),
        stroke_width: number(k::STROKE_WIDTH),
        image: body
            .get(k::IMAGE)
            .and_then(toml::Value::as_str)
            .filter(|path| !path.is_empty())
            .map(str::to_string),
        slice: four_of(body.get(k::SLICE)),
        text_color: colour(k::TEXT_COLOR),
        plate: colour(k::ICON_FILL),
        align: body
            .get(k::TEXT_ALIGN)
            .and_then(toml::Value::as_str)
            .map(str::to_string),
        font_size: number(k::FONT_SIZE),
        font: body
            .get(k::FONT_FAMILY)
            .and_then(toml::Value::as_str)
            .map(str::to_string),
        weight: number(k::FONT_WEIGHT),
        height: number(k::HEIGHT),
        width: number(k::WIDTH),
        padding_x: number(k::PADDING_X),
        padding_y: number(k::PADDING_Y),
        gap: number(k::GAP),
        icon_color: colour(k::ICON_COLOR),
        round: pill.then_some(true),
        hover: nested(st::HOVER),
        active: nested(st::ACTIVE),
        disabled: nested(st::DISABLED),
        focus: nested(st::FOCUS),
        checked: body
            .get(k::CHECKED)
            .and_then(toml::Value::as_table)
            .map(|table| Rc::new(checked_style(table, tokens, what))),
        classes: class_styles(body, tokens, what),
        arrow: body
            .get(k::ARROW)
            .and_then(toml::Value::as_str)
            .filter(|path| !path.is_empty())
            .map(str::to_string),
        body: body
            .get(k::BODY)
            .and_then(toml::Value::as_table)
            .map(|table| Rc::new(style_of(table, tokens, what))),
    }
}

/// A `checked` table: paint over the entry, with its own pointer tables.
fn checked_style(table: &toml::Table, tokens: &Tokens, what: &str) -> Style {
    let full = style_of(table, tokens, what);
    Style {
        hover: full.hover.clone(),
        active: full.active.clone(),
        ..full.paint_only()
    }
}

/// The `[kind.<class>]` tables, in the order they override: the input class,
/// then the height, then the width.
fn class_styles(
    body: &toml::Table,
    tokens: &Tokens,
    what: &str,
) -> Option<Rc<Vec<(SmolStr, Style)>>> {
    let found: Vec<(SmolStr, Style)> = crate::widget::schema::CLASS_KEYS
        .into_iter()
        .filter_map(|word| {
            let table = body.get(word)?.as_table()?;
            Some((SmolStr::new(word), style_of(table, tokens, what)))
        })
        .collect();
    (!found.is_empty()).then(|| Rc::new(found))
}

/// Parse a theme document. Registered with `App::register_asset_type`, so
/// this is what a `type = "widget_theme"` file becomes.
///
/// Never fails: a malformed entry is reported and dropped, because a theme is
/// how something looks and a game that would not start over a bad colour is
/// worse than one that starts plain.
pub(crate) fn parse(value: &toml::Value) -> WidgetTheme {
    let mut theme = WidgetTheme::default();
    // The seven source colours and four sizes become every token first, so a
    // table naming `bg_panel` or `radius_large` finds it.
    let value = crate::palette::complete(value);
    let Some(table) = value.as_table() else {
        return theme;
    };
    if let Some(colors) = table.get(k::COLORS).and_then(toml::Value::as_table) {
        for (name, value) in colors {
            if let Some(parsed) = value.as_str().and_then(parse_color) {
                theme.colors.insert(name.clone(), parsed);
            }
        }
    }
    let sizes: BTreeMap<String, f32> = table
        .get(k::SIZES)
        .and_then(toml::Value::as_table)
        .map(|sizes| {
            sizes
                .iter()
                .filter_map(|(name, v)| {
                    Some((name.clone(), balaur_core::components::as_f64(v)? as f32))
                })
                .collect()
        })
        .unwrap_or_default();
    theme.sizes.clone_from(&sizes);
    let tokens = Tokens {
        colors: theme.colors.clone(),
        sizes,
    };
    for (kind, body) in table {
        if is_reserved(kind) {
            continue;
        }
        let Some(body) = body.as_table() else {
            continue;
        };
        theme
            .kinds
            .insert(kind.clone(), style_of(body, &tokens, kind));
    }
    if let Some(roles) = table.get(k::ROLES).and_then(toml::Value::as_table) {
        for (name, body) in roles {
            let Some(body) = body.as_table() else {
                continue;
            };
            theme
                .roles
                .insert(name.clone(), style_of(body, &tokens, name));
        }
    }
    theme
}

/// Four numbers, or zeros for anything else.
pub(crate) fn four_of(value: Option<&toml::Value>) -> [f32; 4] {
    let mut out = [0.0; 4];
    if let Some(items) = value.and_then(toml::Value::as_array) {
        for (slot, item) in out.iter_mut().zip(items) {
            *slot = balaur_core::components::as_f64(item).unwrap_or(0.0) as f32;
        }
    }
    out
}

/// The doc string `balaur api` and the editor's asset picker show.
pub(crate) const ASSET_DOC: &str = r##"How each widget kind is drawn, one table per kind. `[colors]` and `[sizes]` hold the tokens every table may name, and `[roles.<name>]` is a look a widget picks with `role`. Seven source colours and four sizes derive every other token; a token the file states wins.

```toml
type = "widget_theme"            # a widget takes the theme of the nearest ancestor naming one

[colors]                         # the sources; bg_panel, text_muted, primary_fill, ... derive from them
background = "#151f2a"           # dark or light follows from it; `dark = true` overrides
foreground = "#e6e9ee"
primary = "#4287cc"              # also secondary, success, warning, danger
contrast = 0.05                  # how far apart the surfaces step
row_selected = "#2f6fb0"         # a picked row of a `list`, `tree` or `table`
row_selected_text = "#ffffff"    # and the ink on it
row_hover = "#ffffff12"          # what a row takes under the pointer; `row_active` while held
row_stripe = "#ffffff08"         # a table's every other row; "#00000000" hides it
table_header = "#ffffff08"       # a table's header plate
table_rule = "#00000000"         # the lines down its columns, hidden here
tree_guide = "#8a8a8a8c"         # the lines down a tree's indent

[sizes]                          # font_size, radius, control_height, stroke_width; the rest derive
font_size = 16                   # font_size_small, font_size_large and font_size_title follow
radius = 6                       # radius_small and radius_large follow

[button]                         # one table per kind: [panel], [row], ...; a kind left out keeps the built-in look
fill = "primary_fill"
stroke = "border_default"
stroke_width = 1.0
corner_radius = "radius_large"   # a number, a size's name, or "full" for a pill
padding = 8.0
gap = 4.0
font_size = "font_size_large"
text_color = "text_on_primary"
icon_color = "text_on_primary"
font_family = "ui"               # ui, heading, mono or icon
font_weight = 700
text_align = "center"            # start, center or end

[button.hover]                   # under the pointer; [button.active] while held, [button.focus] with
                                 # keyboard focus, [button.disabled] while off, [button.checked] while on
fill = "primary_fill_hover"

[panel]
image = "art/panel.png"          # a nine-patch, sliced in its own pixels
slice = [8, 8, 8, 8]             # left, top, right, bottom

[table]                          # a row view is dressed like any other kind
fill = "bg_control"
stroke = "border_default"
corner_radius = 6.0
padding_x = 10.0                 # the air either side of a cell's text

[fold]                           # the header; [fold.checked] while open
arrow = "art/folded.png"         # the header's arrow picture; the ▸ and ▾ glyphs where none

[fold.body]                      # the frame around what an open fold shows
fill = "bg_panel"
padding = 8.0

[roles.danger]                   # what a widget with role = "danger" takes
fill = "danger_fill"
text_color = "text_on_danger"
```"##;

pub(crate) const ASSET_TYPE: &str = "widget_theme";

/// The style a widget is drawn with: its kind's, the `role` it names over
/// that, and the `fill`, `stroke` and `corner_radius` it states over both.
///
/// The measure pass calls this too, so a row is sized at the face it draws at.
/// Dress the egui widgets a kind is drawn from in that kind's own style.
///
/// A `number_field`, a `slider` and a `text_field` are egui's widgets, so they
/// wear egui's palette unless the theme is told to them here: a screen whose
/// theme names a `[number_field]` would otherwise show egui's grey beside the
/// buttons it dressed itself. A theme that names nothing changes nothing.
pub(crate) fn dress(ui: &mut egui::Ui, style: &Style, ink: Color32) {
    let radius = style
        .radius
        .map(|r| egui::CornerRadius::same(r.clamp(0.0, 255.0) as u8));
    let edge = |style: &Style| {
        style
            .stroke
            .map(|color| egui::Stroke::new(style.stroke_px(), color))
    };
    let visuals = ui.visuals_mut();
    for (state, look) in [
        (&mut visuals.widgets.inactive, Some(style)),
        (
            &mut visuals.widgets.hovered,
            style.hover.as_deref().or(Some(style)),
        ),
        (
            &mut visuals.widgets.active,
            style
                .active
                .as_deref()
                .or(style.hover.as_deref())
                .or(Some(style)),
        ),
    ] {
        let Some(look) = look else {
            continue;
        };
        if let Some(fill) = look.fill {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
        }
        if let Some(stroke) = edge(look) {
            state.bg_stroke = stroke;
        }
        if let Some(radius) = radius {
            state.corner_radius = radius;
        }
        if let Some(color) = look.text_color {
            state.fg_stroke.color = color;
        }
    }
    // What a `text_field` draws its line on, which is not a widget state.
    if let Some(fill) = style.fill {
        visuals.extreme_bg_color = fill;
    }
    visuals.override_text_color = Some(ink);
}

pub(crate) fn styled(theme: &WidgetTheme, widget: &Widget) -> Rc<Style> {
    let settled = theme.resolved(&widget.kind, &widget.role);
    // The theme's own answer, shared, unless this widget overrides part of
    // it — which most do not, and a screen of widgets is mostly one of a few
    // styles repeated.
    if widget.fill.is_empty()
        && widget.stroke.is_empty()
        && widget.icon_color.is_empty()
        && widget.radius < 0.0
        && widget.padding_x < 0.0
    {
        return settled;
    }
    let mut style = (*settled).clone();
    if !widget.fill.is_empty() {
        style.fill = theme.token(&widget.fill);
    }
    if !widget.stroke.is_empty() {
        style.stroke = theme.token(&widget.stroke);
    }
    if !widget.icon_color.is_empty() {
        style.icon_color = theme.token(&widget.icon_color);
    }
    if widget.radius >= 0.0 {
        style.radius = Some(widget.radius);
    }
    if widget.padding_x >= 0.0 {
        style.padding_x = Some(widget.padding_x);
    }
    Rc::new(style)
}

/// The near-white a caption takes when neither the widget nor its theme says.
pub(crate) const DEFAULT_INK: Color32 = Color32::from_rgb(238, 241, 244);

/// Where a caption sits across the width it was given: the node's own
/// `text_align`, else its role's. `start` is the schema's default and
/// means the node asked for nothing.
pub(crate) fn text_align_of<'a>(style: &'a Style, widget: &'a Widget) -> &'a str {
    if widget.text_align.is_empty() || widget.text_align == w::START {
        style.align.as_deref().unwrap_or(w::START)
    } else {
        widget.text_align.as_str()
    }
}

/// The theme family a widget draws in: the one it names, else its role's,
/// else `ui`. The shaper needs the name as well as the face.
pub(crate) fn family_of<'a>(style: &'a Style, widget: &'a Widget) -> &'a str {
    // Unset is empty before the schema's default lands and `ui` after it, and
    // both mean the same: whatever the role or the kind asked for.
    if widget.font.is_empty() || widget.font == w::UI {
        style.font.as_deref().unwrap_or(w::UI)
    } else {
        widget.font.as_str()
    }
}

/// The ink and the face a widget draws its caption in.
///
/// A property left at its default is the widget saying nothing, so the theme
/// answers: a transparent `text_color`, a `font_size` of 0, the `ui` family
/// and a weight of 400 each take what the role or the kind carries.
pub(crate) fn face(theme: &WidgetTheme, style: &Style, widget: &Widget) -> (Color32, egui::FontId) {
    // The theme's own text colour last, not a constant: a widget with no role
    // drew in near-white, which is invisible on a light theme.
    let ink = if widget.text_color[3] > 0.0 {
        rgba_color(widget.text_color)
    } else {
        style
            .text_color
            .or_else(|| theme.token(t::TEXT_DEFAULT))
            .unwrap_or(DEFAULT_INK)
    };
    let size = if widget.font_size > 0.0 {
        widget.font_size
    } else {
        style.font_size.unwrap_or_else(|| theme.size(t::FONT_SIZE))
    };
    (
        ink,
        egui::FontId::new(size, family(family_of(style, widget))),
    )
}

/// The weight a widget draws at, the theme answering for one left regular.
pub(crate) fn weight_of(style: &Style, widget: &Widget) -> f32 {
    if (widget.font_weight - weights::REGULAR).abs() > f32::EPSILON {
        return widget.font_weight;
    }
    style.weight.unwrap_or(weights::REGULAR)
}

/// The theme in force for a widget: its own, or the nearest ancestor's.
///
/// Resolved once per frame per root rather than per widget, because a screen
/// has one look and walking up the tree for every button to find it out would
/// be work with a known answer.
pub(crate) fn theme_of(
    eng: &Engine,
    reference: &str,
    inherited: &Rc<WidgetTheme>,
) -> Rc<WidgetTheme> {
    if reference.is_empty() {
        return inherited.clone();
    }
    match balaur_core::assets::load_typed::<WidgetTheme>(eng, reference) {
        Ok(theme) => theme,
        Err(err) => {
            // A missing theme is a typo in a scene file: said once per reference.
            if balaur_core::logbuf::first_time("widget theme", reference) {
                tracing::warn!("widget theme '{reference}': {err:#}");
            }
            inherited.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Pointer, WidgetState, parse, parse_color};

    #[test]
    fn a_disabled_or_focused_widget_takes_the_state_table_over_its_kind() {
        let text = "type = \"widget_theme\"\n\n[button]\nfill = \"#ffffff\"\n\n[button.disabled]\nfill = \"#101010\"\n\n[button.focus]\nfill = \"#2020ff\"\n";
        let theme = parse(&toml::from_str::<toml::Value>(text).unwrap());
        let button = theme.resolved("button", "");
        assert_eq!(button.fill, parse_color("#ffffff"));
        assert_eq!(
            button
                .in_states(WidgetState {
                    disabled: true,
                    ..WidgetState::default()
                })
                .fill,
            parse_color("#101010"),
            "disabled wins"
        );
        assert_eq!(
            button
                .in_states(WidgetState {
                    focused: true,
                    ..WidgetState::default()
                })
                .fill,
            parse_color("#2020ff"),
            "focus with the pointer elsewhere"
        );
        assert_eq!(
            button
                .in_states(WidgetState {
                    pointer: Pointer::Over,
                    focused: true,
                    ..WidgetState::default()
                })
                .fill,
            parse_color("#ffffff"),
            "the pointer over it and no hover table: the kind's own look"
        );
    }
}
