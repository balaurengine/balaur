//! The `widget_theme` asset: the chrome a widget kind is drawn with.
//!
//! What a widget owns is its text and its geometry — what it says and where
//! it sits. What a *theme* owns is how its kind looks: the fill behind it, the
//! outline around it, how round the corners are, how much air is inside. Those
//! are the values a game wants to change once rather than on every node.
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

pub(crate) use crate::widget::gradient::{Gradient, GradientShape};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use smol_str::SmolStr;

pub(crate) use super::theme_files::theme_of;

use crate::vocabulary::keys as k;
use crate::vocabulary::states as st;
use crate::vocabulary::tokens as t;
use crate::vocabulary::weights;
use crate::vocabulary::words as w;
use crate::widget::class_words::Words;
use crate::widget::node::{Widget, rgba_color};
use crate::widget::paint::Brush;
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
    /// The ring a focused control draws 2 px outside its edge, as CSS's
    /// `outline`, which leaves `stroke`, the edge, as it is. Unset rings in
    /// the edge's colour.
    pub outline: Option<Color32>,
    /// What a `dialog` or a sheet dims the screen behind it with, as the
    /// web's `::backdrop`; a dialog's own `backdrop_color` wins.
    pub backdrop: Option<Color32>,
    /// That ring's width and its gap from the edge, in design pixels, as
    /// CSS's `outline-width` and `outline-offset`; 2 and 2 unset.
    pub outline_width: Option<f32>,
    pub outline_offset: Option<f32>,
    /// Seconds a `spinner` takes to turn once; 0.75 unset, daisyUI's.
    pub duration: Option<f32>,
    /// The pitch of a list's, a tree's or a table's rows, in design pixels;
    /// a widget's own `row_height` wins, and the font's line decides it where
    /// neither says.
    pub row_height: Option<f32>,
    /// Corner radius in design pixels; `None` keeps the kind's own rule,
    /// which for a button is "as round as its text is tall".
    pub radius: Option<f32>,
    pub padding: Option<f32>,
    /// `padding` written per side, `[left, top, right, bottom]`, as a widget
    /// writes its own: a screen's gutter is wider than its top.
    pub padding_sides: Option<[f32; 4]>,
    pub stroke_width: Option<f32>,
    /// The outline in dashes rather than whole.
    pub dashed: Option<bool>,
    /// A project-relative picture drawn as the kind's background instead of
    /// `fill`, stretched by `slice`.
    pub image: Option<SmolStr>,
    /// Left, top, right and bottom borders of `image` kept unstretched, in
    /// the picture's own pixels.
    pub slice: [f32; 4],
    /// The ink a caption is drawn in, which a role spells `text_color`.
    pub text_color: Option<Color32>,
    /// The disc a control's picture sits on, so a dark mark reads on a dark
    /// sheet.
    pub plate: Option<Color32>,
    /// Where a control's content sits across it: `left` for a row, else centred.
    pub align: Option<SmolStr>,
    /// A control's picture, icon and caption side by side (`row`, unset) or
    /// stacked with the caption under them (`column`), as a tab bar's tabs.
    pub direction: Option<SmolStr>,
    /// A control's icon's size in design pixels, apart from its caption's: a
    /// tab's 24 over its 10 words. A widget's own `icon_size` wins.
    pub icon_size: Option<f32>,
    /// The pointer over the widget while it is live, a `cursor` word: a
    /// hand on every control, the way the web shows one. A node's own
    /// `cursor` wins, a disabled widget keeps the arrow, unset keeps it.
    pub cursor: Option<SmolStr>,
    pub font_size: Option<f32>,
    /// The font, a token of the theme's `[fonts]` already read as the font
    /// it names. Unset on a resolved style is the theme's default font.
    pub font: Option<SmolStr>,
    /// Weight on the CSS scale; a role's `strong = true` is 700.
    pub weight: Option<f32>,
    /// Extra space between glyphs, in design pixels, as CSS's
    /// `letter-spacing`. A widget's `text_style` that states one wins.
    pub letter_spacing: Option<f32>,
    /// How the text's letters are cased, as CSS's `text-transform`; what
    /// a field types is shown as typed.
    pub text_transform: Option<balaur_text::Case>,
    /// The box a role asks for, in design pixels, for a role that carries its
    /// own control size the way the editor's `tab` and `chip` do. A node that
    /// states one of its own wins; this is not a floor.
    pub height: Option<f32>,
    pub width: Option<f32>,
    /// The least and greatest box a role takes, in design pixels, as a
    /// node's own `min_width`, `max_width`, `min_height` and `max_height`;
    /// a node that states one wins on that side.
    pub min_width: Option<f32>,
    pub max_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_height: Option<f32>,
    /// The space inside the left and right edges, and inside the top and
    /// bottom ones, in design pixels: either side of a control's caption, and
    /// round a container's children. Each wins over `padding` on its axis,
    /// and a widget's own `padding` wins over both on each side it states.
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
    /// The `[kind.<word>]` tables this entry carries, by class word. Folded
    /// over the entry, in the words' override order, while a word is in
    /// force where the widget sits: the screen's, so a theme states a touch
    /// height beside the height it states for a cursor, or one a container
    /// puts in force.
    pub classes: Option<Rc<Vec<(SmolStr, Style)>>>,
    /// The class words a container takes in order until what it holds fits
    /// on one line, as a widget's `widget_layout.fit` says them.
    pub fit: Option<Rc<[SmolStr]>>,
    /// A control drawn without its caption, the caption then its tooltip and
    /// still its name to a screen reader; or without its icon.
    pub show_text: Option<bool>,
    pub show_icon: Option<bool>,
    /// A caption past its room cut with an ellipsis, as `widget_text`'s
    /// `truncate`; one in a row gives way for it rather than overflow.
    pub truncate: Option<bool>,
    pub active: Option<Rc<Style>>,
    /// What sits over this style while the widget is disabled, and while
    /// keyboard focus is on it, under whichever pointer table applies.
    pub disabled: Option<Rc<Style>>,
    pub focus: Option<Rc<Style>>,
    /// What the widget wears while it is on, under whichever pointer table
    /// applies; its own `hover` and `active` win over the entry's.
    pub checked: Option<Rc<Style>>,
    /// A `fold`'s arrow picture, and the frame around its open children.
    pub arrow: Option<SmolStr>,
    pub body: Option<Rc<Style>>,
    /// The band a widget's `border` keeps clear on each side, left, top,
    /// right and bottom, which its outline is painted in; never a theme's.
    pub border: Option<[f32; 4]>,
    /// A fill that runs through its colours, drawn in place of `fill`.
    pub gradient: Option<Rc<Gradient>>,
    /// The shadows round the box, CSS's `box-shadow` list: the first drawn
    /// on top, an `inset` one inside the box, over its fill and under its
    /// edge. Empty takes away the shadow a style under it states.
    pub shadows: Option<Rc<[Shadow]>>,
    /// Seconds a change of state eases over, as CSS's `transition`: the
    /// pointer's two, focus, `checked` and `disabled`, each from where it is.
    /// Colours, the shadow, `offset` and `scale` run between the tables,
    /// everything else steps at halfway. Unset steps at once. Read on a kind
    /// or a role, never inside one of its state tables.
    pub transition: Option<f32>,
    /// How far the widget and what it holds are drawn from where the layout
    /// put them, `[x, y]` in design pixels, as CSS's `translate`: a card
    /// lifted under the pointer. What sits beside it never moves, and the
    /// move is rounded to whole pixels of the screen, so text stays sharp.
    pub offset: Option<[f32; 2]>,
    /// How many times its width and its height the widget and what it holds
    /// are drawn, `[x, y]` about its middle, as CSS's `scale: x y`: a count's
    /// pop. The layout and what sits beside it never move.
    pub scale: Option<[f32; 2]>,
}

/// One of `shadows = [{ y = 2, blur = 8, spread = 0, color = "#00000040" }]`,
/// in design pixels. A negative spread draws it smaller than the box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color32,
    /// Inside the box rather than under it.
    pub inset: bool,
}

impl Style {
    /// Whether a control stacks its parts, the caption under the icon.
    pub(crate) fn stacked(&self) -> bool {
        self.direction.as_deref() == Some(crate::vocabulary::words::COLUMN)
    }

    /// The outline width to draw, once a style has said whether it wants one.
    pub fn stroke_px(&self) -> f32 {
        self.stroke_width.unwrap_or(1.0)
    }

    /// Whether a box drawn in this style shows anything: a fill, an
    /// outline, a gradient or a shadow.
    #[must_use]
    pub fn paints(&self) -> bool {
        self.fill.is_some()
            || self.stroke.is_some()
            || self.gradient.is_some()
            || self.shadows.as_deref().is_some_and(|list| !list.is_empty())
    }

    /// The paint half of a style, for a `hover` or `active` table. A state
    /// that resized would move whatever sits beside the widget, and the
    /// layout is solved before anything knows where the pointer is.
    #[must_use]
    pub fn paint_only(&self) -> Self {
        Self {
            padding: None,
            padding_sides: None,
            padding_x: None,
            padding_y: None,
            height: None,
            width: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            font_size: None,
            font: None,
            weight: None,
            letter_spacing: None,
            text_transform: None,
            fit: None,
            show_text: None,
            show_icon: None,
            truncate: None,
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
            outline: self.outline.or(base.outline),
            outline_width: self.outline_width.or(base.outline_width),
            outline_offset: self.outline_offset.or(base.outline_offset),
            duration: self.duration.or(base.duration),
            row_height: self.row_height.or(base.row_height),
            radius: self.radius.or(base.radius),
            padding: self.padding.or(base.padding),
            padding_sides: self.padding_sides.or(base.padding_sides),
            stroke_width: self.stroke_width.or(base.stroke_width),
            dashed: self.dashed.or(base.dashed),
            image: self.image.clone().or_else(|| base.image.clone()),
            slice,
            text_color: self.text_color.or(base.text_color),
            plate: self.plate.or(base.plate),
            backdrop: self.backdrop.or(base.backdrop),
            align: self.align.clone().or_else(|| base.align.clone()),
            direction: self.direction.clone().or_else(|| base.direction.clone()),
            icon_size: self.icon_size.or(base.icon_size),
            cursor: self.cursor.clone().or_else(|| base.cursor.clone()),
            font_size: self.font_size.or(base.font_size),
            font: self.font.clone().or_else(|| base.font.clone()),
            weight: self.weight.or(base.weight),
            height: self.height.or(base.height),
            width: self.width.or(base.width),
            min_width: self.min_width.or(base.min_width),
            max_width: self.max_width.or(base.max_width),
            min_height: self.min_height.or(base.min_height),
            max_height: self.max_height.or(base.max_height),
            padding_x: self.padding_x.or(base.padding_x),
            padding_y: self.padding_y.or(base.padding_y),
            gap: self.gap.or(base.gap),
            icon_color: self.icon_color.or(base.icon_color),
            round: self.round.or(base.round),
            hover: self.hover.clone().or_else(|| base.hover.clone()),
            classes: self.classes.clone().or_else(|| base.classes.clone()),
            fit: self.fit.clone().or_else(|| base.fit.clone()),
            show_text: self.show_text.or(base.show_text),
            show_icon: self.show_icon.or(base.show_icon),
            truncate: self.truncate.or(base.truncate),
            active: self.active.clone().or_else(|| base.active.clone()),
            disabled: self.disabled.clone().or_else(|| base.disabled.clone()),
            focus: self.focus.clone().or_else(|| base.focus.clone()),
            checked: self.checked.clone().or_else(|| base.checked.clone()),
            arrow: self.arrow.clone().or_else(|| base.arrow.clone()),
            body: self.body.clone().or_else(|| base.body.clone()),
            border: self.border.or(base.border),
            gradient: self.gradient.clone().or_else(|| base.gradient.clone()),
            shadows: self.shadows.clone().or_else(|| base.shadows.clone()),
            letter_spacing: self.letter_spacing.or(base.letter_spacing),
            text_transform: self.text_transform.or(base.text_transform),
            transition: self.transition.or(base.transition),
            offset: self.offset.or(base.offset),
            scale: self.scale.or(base.scale),
        }
    }

    /// Whether no state table is stated, so every state draws like rest.
    #[must_use]
    pub fn states_nothing(&self) -> bool {
        self.hover.is_none()
            && self.active.is_none()
            && self.disabled.is_none()
            && self.focus.is_none()
            && self.checked.is_none()
    }
}

/// A parsed theme: a style per widget kind, the same per named role, and the
/// colour, size and font tokens both spell theirs with.
#[derive(Default)]
pub struct WidgetTheme {
    kinds: BTreeMap<String, Style>,
    roles: BTreeMap<String, Style>,
    colors: BTreeMap<String, Color32>,
    sizes: BTreeMap<String, f32>,
    /// `[fonts]`: a font by the name a kind or a role writes for it.
    fonts: BTreeMap<String, String>,
    /// `[icons] mirrored`: the glyphs that point a way, a chevron or an
    /// arrow, which a mirrored UI draws turned round.
    mirrored_icons: Vec<SmolStr>,
    /// `resolved` remembered: a kind and a role name the same style for as
    /// long as the theme lives, and working it out walked two maps and cloned
    /// a style for every widget on the screen, every frame.
    settled: RefCell<rustc_hash::FxHashMap<(SmolStr, SmolStr, Words), Rc<Style>>>,
    /// The class generation `settled` was filled under. A new one empties it
    /// rather than keying beside it, so a rotation does not leave the last
    /// screen's styles in the map for the life of the theme.
    settled_at: std::cell::Cell<u32>,
}

impl WidgetTheme {
    /// Whether the glyph `name` points a way, so a mirrored UI turns it
    /// round: `[icons] mirrored` lists it.
    #[must_use]
    pub fn mirrors_icon(&self, name: &str) -> bool {
        !name.is_empty() && self.mirrored_icons.iter().any(|held| held == name)
    }

    /// The style for a kind, or the empty one — which means "as before".
    #[must_use]
    pub fn style(&self, kind: &str) -> Style {
        if let Some(style) = self.kinds.get(kind) {
            return style.clone();
        }
        // A stack is a panel that lays its children over one another, so
        // either takes the other's entry when the theme names only one.
        let sibling = match kind {
            // A stack is a panel that lays its children over one another.
            w::STACK => w::PANEL,
            w::PANEL => w::STACK,
            _ => return Style::default(),
        };
        self.kinds.get(sibling).cloned().unwrap_or_default()
    }

    /// A kind's style with the named role over it, under the window's
    /// classes. An unknown role is no role: a theme that has not been given
    /// one yet still draws.
    #[must_use]
    pub fn resolved(&self, kind: &str, role: &str) -> Rc<Style> {
        self.resolved_in(kind, role, pass_words())
    }

    /// The same under the class words in force where a widget sits.
    pub(crate) fn resolved_in(&self, kind: &str, role: &str, words: Words) -> Rc<Style> {
        let generation = pass_classes_at();
        // The screen's classes are part of the answer, so a rotation empties
        // what the last screen settled on rather than keeping it beside.
        if self.settled_at.replace(generation) != generation {
            self.settled.borrow_mut().clear();
        }
        let active = words.list();
        let key = (SmolStr::new(kind), SmolStr::new(role), words);
        if let Some(held) = self.settled.borrow().get(&key) {
            return Rc::clone(held);
        }
        // Each in its own classes before the next goes over it: the kind's
        // touch height is the kind's, and a role's own height still wins.
        let base = in_classes(&self.style(kind), &active);
        let mut settled = match self.roles.get(role) {
            Some(style) => in_classes(style, &active).over(&base),
            None => base,
        };
        if settled.font.is_none() {
            settled.font = self.fonts.get(t::FONT_DEFAULT).map(SmolStr::new);
        }
        let made = Rc::new(settled);
        self.settled.borrow_mut().insert(key, Rc::clone(&made));
        made
    }

    /// The font `[fonts]` names `name`; empty where it names none, which is
    /// the project's default.
    #[must_use]
    pub fn font(&self, name: &str) -> &str {
        self.fonts.get(name).map_or("", String::as_str)
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

/// One style with the table of every word in force folded over it, in the
/// words' override order. A word's table may hold tables of other words in
/// force, folded over it the same way: `[roles.title.small.wide]` is the
/// title a size down on a wide screen. A style with no class table is
/// returned as it stands.
fn in_classes(style: &Style, active: &[&'static str]) -> Style {
    let Some(classes) = style.classes.as_ref() else {
        return style.clone();
    };
    let mut settled = style.clone();
    for word in active {
        if let Some((_, over)) = classes.iter().find(|(held, _)| held == word) {
            settled = in_classes(over, active).over(&settled);
        }
    }
    Style {
        classes: style.classes.clone(),
        ..settled
    }
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

/// A colour, or none for a value that is not one, which `check` says.
fn color(value: &toml::Value, colors: &BTreeMap<String, Color32>) -> Option<Color32> {
    let text = value.as_str()?;
    if text == w::NONE {
        return Some(Color32::TRANSPARENT);
    }
    parse_color(text).or_else(|| colors.get(text).copied())
}

/// The tables that name something other than a widget kind.
fn is_reserved(key: &str) -> bool {
    [
        k::TYPE,
        k::DARK,
        k::COLORS,
        k::ROLES,
        k::SIZES,
        k::FONTS,
        k::ICONS,
        k::HUES,
        k::MIXES,
        k::BASE,
    ]
    .contains(&key)
}

// The window's class words, as a set and as words, and a number that changes
// when they do, so the resolved-style cache answers for this pass's screen.
thread_local! {
    static PASS_CLASSES: RefCell<(Words, u32)> = const { RefCell::new((Words::EMPTY, 0)) };
}

/// Tell the theme which classes this pass answers to. Called once a pass,
/// before anything is drawn.
pub(crate) fn set_pass_classes(active: &[&'static str]) {
    let words = Words::of(active.iter().copied());
    PASS_CLASSES.with(|held| {
        let mut held = held.borrow_mut();
        if held.0 != words {
            held.0 = words;
            held.1 = held.1.wrapping_add(1);
        }
    });
}

/// The number that stands for the classes in force.
pub(crate) fn pass_classes_at() -> u32 {
    PASS_CLASSES.with(|held| held.borrow().1)
}

/// The window's class words.
pub(crate) fn pass_words() -> Words {
    PASS_CLASSES.with(|held| held.borrow().0)
}

/// Whether a finger is what reaches this screen, as the pass settled it.
pub(crate) fn pass_is_touch() -> bool {
    pass_words().list().contains(&balaur_core::tags::TOUCH)
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
        crate::widget::popups::hover_text(response, text);
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
            crate::widget::popups::held_text(response, text);
        }
    });
}

thread_local! {
    /// The widget a finger is resting on, and when it landed. One at a time,
    /// because one finger drives the pointer.
    static HELD_TIP: std::cell::RefCell<Option<(egui::Id, f64)>> =
        const { std::cell::RefCell::new(None) };
}

/// The colours, sizes and fonts a theme's tables name, resolved from its
/// `[colors]`, `[sizes]` and `[fonts]`.
pub(crate) struct Tokens {
    pub(crate) colors: BTreeMap<String, Color32>,
    pub(crate) sizes: BTreeMap<String, f32>,
    pub(crate) fonts: BTreeMap<String, String>,
}

impl Tokens {
    /// A number, written as one or as the name of a size the theme states.
    fn number(&self, value: &toml::Value) -> Option<f32> {
        balaur_core::components::as_f64(value)
            .map(|v| v as f32)
            .or_else(|| self.sizes.get(value.as_str()?).copied())
    }
}

/// One `[kind]` or `[roles.name]` table as a style, with its state tables
/// read as styles over it.
fn style_of(body: &toml::Table, tokens: &Tokens) -> Style {
    let number = |key: &str| tokens.number(body.get(key)?);
    let nested = |key: &str| {
        body.get(key).and_then(toml::Value::as_table).map(|table| {
            let style = style_of(table, tokens).paint_only();
            Rc::new(Style {
                transition: None,
                ..style
            })
        })
    };
    let colour = |key: &str| body.get(key).and_then(|v| color(v, &tokens.colors));
    let pill = body.get(k::CORNER_RADIUS).and_then(toml::Value::as_str) == Some(w::FULL);
    Style {
        fill: colour(k::FILL),
        stroke: colour(k::STROKE),
        outline: colour(k::OUTLINE),
        backdrop: colour(k::BACKDROP),
        outline_width: number(k::OUTLINE_WIDTH),
        outline_offset: number(k::OUTLINE_OFFSET),
        duration: number(k::DURATION).filter(|seconds| *seconds > 0.0),
        row_height: number(k::ROW_HEIGHT).filter(|pitch| *pitch > 0.0),
        radius: if pill { None } else { number(k::CORNER_RADIUS) },
        padding: number(k::PADDING),
        padding_sides: body
            .get(k::PADDING)
            .filter(|value| value.is_array())
            .map(|value| four_of(Some(value))),
        stroke_width: number(k::STROKE_WIDTH),
        dashed: body.get(k::DASHED).and_then(toml::Value::as_bool),
        image: path_of(body, k::IMAGE),
        slice: four_of(body.get(k::SLICE)),
        text_color: colour(k::TEXT_COLOR),
        plate: colour(k::ICON_FILL),
        align: word(body, k::TEXT_ALIGN),
        direction: word(body, k::FLEX_DIRECTION),
        icon_size: number(k::ICON_SIZE).filter(|size| *size > 0.0),
        cursor: word(body, k::CURSOR),
        font_size: number(k::FONT_SIZE),
        // A font is written as one, or as the name of one `[fonts]` states.
        font: body
            .get(k::FONT)
            .and_then(toml::Value::as_str)
            .map(|font| tokens.fonts.get(font).map_or(font, String::as_str).into()),
        weight: number(k::FONT_WEIGHT),
        height: number(k::HEIGHT),
        width: number(k::WIDTH),
        min_width: number(k::MIN_WIDTH).filter(|side| *side > 0.0),
        max_width: number(k::MAX_WIDTH).filter(|side| *side > 0.0),
        min_height: number(k::MIN_HEIGHT).filter(|side| *side > 0.0),
        max_height: number(k::MAX_HEIGHT).filter(|side| *side > 0.0),
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
            .map(|table| Rc::new(checked_style(table, tokens))),
        classes: class_styles(body, tokens),
        fit: words_of(body, k::FIT),
        show_text: body.get(k::SHOW_TEXT).and_then(toml::Value::as_bool),
        show_icon: body.get(k::SHOW_ICON).and_then(toml::Value::as_bool),
        truncate: body.get(k::TRUNCATE).and_then(toml::Value::as_bool),
        arrow: path_of(body, k::ARROW),
        body: body
            .get(k::BODY)
            .and_then(toml::Value::as_table)
            .map(|table| Rc::new(style_of(table, tokens))),
        // The sides a role's edge is drawn on, as the node's own `border`
        // states them: a bar's hairline on its top alone.
        border: body
            .get(k::BORDER)
            .filter(|sides| sides.is_array())
            .map(|sides| four_of(Some(sides))),
        gradient: gradient_of(body, tokens),
        shadows: shadows_of(body, tokens),
        letter_spacing: number(k::LETTER_SPACING),
        text_transform: case_of(body),
        transition: number(k::TRANSITION).filter(|seconds| *seconds > 0.0),
        offset: body.get(k::OFFSET).and_then(pair_of),
        scale: body.get(k::SCALE).and_then(pair_of),
    }
}

/// A project path a table names, none for an empty one.
fn path_of(body: &toml::Table, key: &str) -> Option<SmolStr> {
    body.get(key)
        .and_then(toml::Value::as_str)
        .filter(|path| !path.is_empty())
        .map(SmolStr::new)
}

/// A list of words a table names, as `fit` does.
fn words_of(body: &toml::Table, key: &str) -> Option<Rc<[SmolStr]>> {
    let words = body.get(key)?.as_array()?;
    Some(
        words
            .iter()
            .filter_map(toml::Value::as_str)
            .map(SmolStr::new)
            .collect(),
    )
}

/// A key whose value is one word, as `text_align` and `cursor` are; none
/// for an empty one.
fn word(body: &toml::Table, key: &str) -> Option<SmolStr> {
    body.get(key)
        .and_then(toml::Value::as_str)
        .filter(|word| !word.is_empty())
        .map(SmolStr::new)
}

fn case_of(body: &toml::Table) -> Option<balaur_text::Case> {
    balaur_text::Case::of(body.get(k::TEXT_TRANSFORM)?.as_str()?)
}

/// A `checked` table: paint over the entry, with its own pointer tables.
fn checked_style(table: &toml::Table, tokens: &Tokens) -> Style {
    let full = style_of(table, tokens);
    Style {
        hover: full.hover.clone(),
        active: full.active.clone(),
        transition: None,
        weight: full.weight,
        ..full.paint_only()
    }
}

/// The `[kind.<word>]` tables: every table an entry holds that is not one
/// of its own (a state, `checked`, `body`, `gradient`) is a class word's. A
/// theme is read before the project's own words are, so they are not checked
/// here; the order they override in is the words', read as they are folded.
fn class_styles(body: &toml::Table, tokens: &Tokens) -> Option<Rc<Vec<(SmolStr, Style)>>> {
    let found: Vec<(SmolStr, Style)> = body
        .iter()
        .filter(|(key, _)| !check::is_own(key))
        .filter_map(|(word, table)| {
            let table = table.as_table()?;
            Some((SmolStr::new(word), style_of(table, tokens)))
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
    let (theme, dropped) = read(value);
    let file = balaur_core::assets::parsing().map_or_else(String::new, |file| format!(" '{file}'"));
    for line in dropped {
        tracing::warn!("widget theme{file}: {line}");
    }
    theme
}

/// What a theme document says that its reader drops, one line a value: a
/// wrong shape, a word outside its list, a key no table reads.
#[must_use]
pub fn dropped_theme_values(value: &toml::Value) -> Vec<String> {
    read(value).1
}

fn read(written: &toml::Value) -> (WidgetTheme, Vec<String>) {
    let mut theme = WidgetTheme::default();
    // The seven source colours and four sizes become every token first, so a
    // table naming `bg_panel` or `radius_large` finds it.
    let value = crate::palette::complete(written);
    let Some(table) = value.as_table() else {
        return (theme, Vec::new());
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
    theme.fonts = table
        .get(k::FONTS)
        .and_then(toml::Value::as_table)
        .map(|fonts| {
            fonts
                .iter()
                .filter_map(|(name, v)| Some((name.clone(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    theme.mirrored_icons = table
        .get(k::ICONS)
        .and_then(toml::Value::as_table)
        .and_then(|icons| icons.get(k::MIRRORED))
        .and_then(toml::Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(|name| name.as_str().map(SmolStr::new))
                .collect()
        })
        .unwrap_or_default();
    let tokens = Tokens {
        colors: theme.colors.clone(),
        sizes,
        fonts: theme.fonts.clone(),
    };
    let dropped = check::dropped(table, written.as_table().unwrap_or(table), &tokens);
    for (kind, body) in table {
        if is_reserved(kind) {
            continue;
        }
        let Some(body) = body.as_table() else {
            continue;
        };
        theme.kinds.insert(kind.clone(), style_of(body, &tokens));
    }
    if let Some(roles) = table.get(k::ROLES).and_then(toml::Value::as_table) {
        for (name, body) in roles {
            let Some(body) = body.as_table() else {
                continue;
            };
            theme.roles.insert(name.clone(), style_of(body, &tokens));
        }
    }
    (theme, dropped)
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

/// `[x, y]` of a list of two, none for a number or a list of another length.
fn pair_of(value: &toml::Value) -> Option<[f32; 2]> {
    let [x, y, ..] = four_of(Some(value));
    (value.as_array()?.len() == 2).then_some([x, y])
}

pub(crate) const ASSET_TYPE: &str = "widget_theme";

/// The style a widget is drawn with: its kind's, the `role` it names over
/// that, and the `fill`, `stroke` and `corner_radius` it states over both.
///
/// The measure pass calls this too, so a row is sized at the face it draws at.
pub(crate) fn styled(theme: &WidgetTheme, widget: &Widget, words: Words) -> Rc<Style> {
    let settled = theme.resolved_in(&widget.kind, &widget.role, words);
    // A checked widget is measured at its checked weight (`theme_check::LAYOUT`).
    let weighed = (settled.checked.as_ref())
        .filter(|_| widget.checked)
        .and_then(|checked| checked.weight);
    // The theme's own answer, shared, unless this widget overrides part of
    // it — which most do not, and a screen of widgets is mostly one of a few
    // styles repeated.
    let border = widget.layout.border.map(crate::widget::node::Bits::get);
    let bordered = border.iter().any(|side| *side > 0.0);
    // The node's `offset` and `scale` are not folded in: they compose with
    // the state the role is in when it is drawn (`Brush::transform_since`).
    if widget.fill.is_empty()
        && widget.stroke.is_empty()
        && !widget.dashed
        && widget.icon_color.is_empty()
        && widget.radius < 0.0
        && !bordered
        && weighed.is_none()
    {
        return settled;
    }
    let mut style = (*settled).clone();
    style.weight = weighed.or(style.weight);
    if !widget.fill.is_empty() {
        style.fill = theme.token(&widget.fill);
    }
    if !widget.stroke.is_empty() {
        style.stroke = theme.token(&widget.stroke);
    }
    if widget.dashed {
        style.dashed = Some(true);
    }
    if !widget.icon_color.is_empty() {
        style.icon_color = theme.token(&widget.icon_color);
    }
    if widget.radius >= 0.0 {
        style.radius = Some(widget.radius);
    }
    if bordered {
        style.border = Some(border);
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

/// The font a widget draws in: the one it names, else its role's or its
/// kind's, else the theme's default; empty is the project's default.
pub(crate) fn font_of<'a>(style: &'a Style, widget: &'a Widget) -> &'a str {
    if widget.font.is_empty() {
        style.font.as_deref().unwrap_or("")
    } else {
        widget.font.as_str()
    }
}

/// The ink and the face a widget draws its caption in.
///
/// A property left at its default is the widget saying nothing, so the theme
/// answers: a transparent `text_color`, a `font_size` of 0, no `font` and a
/// weight of 400 each take what the role or the kind carries. `project` is
/// whose faces it draws in.
pub(crate) fn face(
    theme: &WidgetTheme,
    style: &Style,
    (widget, project): (&Widget, balaur_text::FontProject),
) -> (Color32, egui::FontId) {
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
    let family = crate::theme::weighted(
        project,
        font_of(style, widget),
        weight_of(style, widget),
        widget.font_style == w::ITALIC,
    );
    (ink, egui::FontId::new(size, family))
}

/// Whether egui slants a widget's text itself: it asks for italic, and the
/// project ships no italic face for its font and weight.
pub(crate) fn slanted(
    style: &Style,
    (widget, project): (&Widget, balaur_text::FontProject),
) -> bool {
    crate::theme::slanted(
        project,
        font_of(style, widget),
        weight_of(style, widget),
        widget.font_style == w::ITALIC,
    )
}

/// The family a theme's icons draw in: its `[fonts]` `icon`, else its
/// default.
pub(crate) fn icon_family(
    theme: &WidgetTheme,
    project: balaur_text::FontProject,
) -> egui::FontFamily {
    crate::theme::weighted(project, icon_font(theme), 400.0, false)
}

/// The font a theme's icons are drawn in and named by: its `icon` font, or
/// its default.
pub(crate) fn icon_font(theme: &WidgetTheme) -> &str {
    let icon = theme.font(t::FONT_ICON);
    if icon.is_empty() {
        theme.font(t::FONT_DEFAULT)
    } else {
        icon
    }
}

/// A caption as egui draws it, in `font` and slanted where [`slanted`] says.
pub(crate) fn rich(
    text: &str,
    font: &egui::FontId,
    color: Color32,
    slant: bool,
    look: &crate::widget::text_look::TextLook,
) -> egui::RichText {
    crate::theme::asked_of_egui(text);
    let text = egui::RichText::new(text).font(font.clone()).color(color);
    let text = look.dress(text, font.size);
    if slant { text.italics() } else { text }
}

/// One unwrapped line laid out for the painter, slanted where [`slanted`]
/// says; painted later in the ink the caller picks.
pub(crate) fn galley(
    painter: &Brush,
    text: &str,
    font: &egui::FontId,
    slant: bool,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        font.clone(),
        Color32::PLACEHOLDER,
    );
    for section in &mut job.sections {
        section.format.italics = slant;
    }
    painter.job(job)
}

/// The weight a widget draws at, the theme answering for one left regular.
pub(crate) fn weight_of(style: &Style, widget: &Widget) -> f32 {
    if (widget.font_weight - weights::REGULAR).abs() > f32::EPSILON {
        return widget.font_weight;
    }
    style.weight.unwrap_or(weights::REGULAR)
}

#[path = "theme_doc.rs"]
mod doc;

#[path = "theme_dress.rs"]
mod dress_egui;
pub(crate) use doc::ASSET_DOC;
pub(crate) use dress_egui::dress;

#[path = "theme_check.rs"]
mod check;

#[path = "theme_layers.rs"]
mod layers;
use layers::{gradient_of, shadows_of};

#[cfg(test)]
#[path = "theme_tests.rs"]
mod tests;
