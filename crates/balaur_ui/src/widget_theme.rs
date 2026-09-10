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

use smol_str::SmolStr;

use crate::vocabulary::keys as k;
use egui::Color32;

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
    /// The ink a caption is drawn in, which a role spells `color`.
    pub text_color: Option<Color32>,
    pub font_size: Option<f32>,
    /// Which of the theme's families, by the names `font` offers.
    pub font: Option<String>,
    /// Weight on the CSS scale; a role's `strong = true` is 700.
    pub weight: Option<f32>,
    /// A floor on the box, in design pixels, for a role that carries its own
    /// control height the way the editor's `tab` and `chip` do.
    pub height: Option<f32>,
    pub width: Option<f32>,
    /// The gap either side of a caption, in design pixels.
    pub padding_x: Option<f32>,
    /// As round as it is tall, whatever `radius` says.
    pub round: Option<bool>,
    /// What replaces this style while the pointer is over the widget, and
    /// while it is held down. Each is a whole style over this one.
    pub hover: Option<Rc<Style>>,
    pub active: Option<Rc<Style>>,
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
            font_size: self.font_size.or(base.font_size),
            font: self.font.clone().or_else(|| base.font.clone()),
            weight: self.weight.or(base.weight),
            height: self.height.or(base.height),
            width: self.width.or(base.width),
            padding_x: self.padding_x.or(base.padding_x),
            round: self.round.or(base.round),
            hover: self.hover.clone().or_else(|| base.hover.clone()),
            active: self.active.clone().or_else(|| base.active.clone()),
        }
    }

    /// The style for how the widget is being touched right now. A theme that
    /// named neither state gets the base back, which is what it drew before.
    #[must_use]
    pub fn in_state(&self, hovered: bool, held: bool) -> Self {
        let patch = if held {
            self.active.as_ref().or(self.hover.as_ref())
        } else if hovered {
            self.hover.as_ref()
        } else {
            None
        };
        patch.map_or_else(|| self.clone(), |patch| patch.over(self))
    }
}

/// A parsed theme: a style per widget kind, the same per named role, and the
/// colour tokens both spell their fills with.
#[derive(Default)]
pub struct WidgetTheme {
    kinds: BTreeMap<String, Style>,
    roles: BTreeMap<String, Style>,
    colors: BTreeMap<String, Color32>,
    /// `resolved` remembered: a kind and a role name the same style for as
    /// long as the theme lives, and working it out walked two maps and cloned
    /// a style for every widget on the screen, every frame.
    settled: RefCell<rustc_hash::FxHashMap<(SmolStr, SmolStr), Rc<Style>>>,
}

impl WidgetTheme {
    /// The style for a kind, or the empty one — which means "as before".
    #[must_use]
    pub fn style(&self, kind: &str) -> Style {
        self.kinds.get(kind).cloned().unwrap_or_default()
    }

    /// A kind's style with the named role over it. An unknown role is no
    /// role: a theme that has not been given one yet still draws.
    #[must_use]
    pub fn resolved(&self, kind: &str, role: &str) -> Rc<Style> {
        let key = (SmolStr::new(kind), SmolStr::new(role));
        if let Some(held) = self.settled.borrow().get(&key) {
            return Rc::clone(held);
        }
        let base = self.style(kind);
        let made = Rc::new(match self.roles.get(role) {
            Some(style) => style.over(&base),
            None => base,
        });
        self.settled.borrow_mut().insert(key, Rc::clone(&made));
        made
    }

    /// A colour by the name `[colors]` filed it under, for a widget that
    /// names one rather than spelling a hex.
    #[must_use]
    pub fn token(&self, name: &str) -> Option<Color32> {
        if name == "none" {
            return Some(Color32::TRANSPARENT);
        }
        parse_color(name).or_else(|| self.colors.get(name).copied())
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
    matches!(key, "type" | "dark" | "colors" | "roles")
}

/// One `[kind]` or `[roles.name]` table as a style, with its own `hover` and
/// `active` sub-tables read as styles over it.
fn style_of(body: &toml::Table, colors: &BTreeMap<String, Color32>, what: &str) -> Style {
    let number = |key: &str| {
        body.get(key)
            .and_then(balaur_core::components::as_f64)
            .map(|v| v as f32)
    };
    let flag = |key: &str| body.get(key).and_then(toml::Value::as_bool);
    let nested = |key: &str| {
        body.get(key)
            .and_then(toml::Value::as_table)
            .map(|table| Rc::new(style_of(table, colors, what).paint_only()))
    };
    // A role spells its type the way a call site did: `size`, `color` and
    // `strong` rather than the component's longer property names.
    let weight =
        number(k::FONT_WEIGHT).or_else(|| flag(k::STRONG).map(|on| if on { 700.0 } else { 400.0 }));
    // `d` is a control that is as wide as it is tall, which is how the
    // editor's tool and transport buttons are written down.
    let square = number(k::D);
    Style {
        fill: body.get(k::FILL).and_then(|v| color(v, what, colors)),
        stroke: body.get(k::STROKE).and_then(|v| color(v, what, colors)),
        radius: number(k::RADIUS),
        padding: number(k::PADDING),
        stroke_width: number("stroke_width"),
        image: body
            .get(k::IMAGE)
            .and_then(toml::Value::as_str)
            .filter(|path| !path.is_empty())
            .map(str::to_string),
        slice: four_of(body.get(k::SLICE)),
        text_color: body.get(k::COLOR).and_then(|v| color(v, what, colors)),
        font_size: number(k::SIZE).or_else(|| number(k::FONT_SIZE)),
        font: body
            .get(k::FONT)
            .and_then(toml::Value::as_str)
            .map(str::to_string),
        weight,
        height: number(k::HEIGHT).or(square),
        width: number(k::WIDTH).or(square),
        padding_x: number(k::PADDING_X),
        round: flag(k::ROUND),
        // `hover_fill` is the one-line spelling the editor's roles already
        // use; a whole `[x.hover]` table wins over it.
        hover: nested("hover").or_else(|| {
            let fill = body
                .get(k::HOVER_FILL)
                .and_then(|v| color(v, what, colors))?;
            Some(Rc::new(Style {
                fill: Some(fill),
                ..Style::default()
            }))
        }),
        active: nested("active"),
    }
}

/// Parse a theme document. Registered with `App::register_asset_type`, so
/// this is what a `type = "widget_theme"` file becomes.
///
/// Never fails: a malformed entry is reported and dropped, because a theme is
/// how something looks and a game that would not start over a bad colour is
/// worse than one that starts plain.
pub(crate) fn parse(value: &toml::Value) -> WidgetTheme {
    let mut theme = WidgetTheme::default();
    let Some(table) = value.as_table() else {
        return theme;
    };
    if let Some(colors) = table.get("colors").and_then(toml::Value::as_table) {
        for (name, value) in colors {
            if let Some(parsed) = value.as_str().and_then(parse_color) {
                theme.colors.insert(name.clone(), parsed);
            }
        }
    }
    for (kind, body) in table {
        if is_reserved(kind) {
            continue;
        }
        let Some(body) = body.as_table() else {
            continue;
        };
        theme
            .kinds
            .insert(kind.clone(), style_of(body, &theme.colors, kind));
    }
    if let Some(roles) = table.get("roles").and_then(toml::Value::as_table) {
        for (name, body) in roles {
            let Some(body) = body.as_table() else {
                continue;
            };
            theme
                .roles
                .insert(name.clone(), style_of(body, &theme.colors, name));
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
pub(crate) const ASSET_DOC: &str = "How each widget kind is drawn: `fill`, `stroke`, `stroke_width`, \
     `radius`, `padding`, `size`, `color`, `font` and `strong` under a table named for the kind \
     (`[button]`, `[panel]`, `[row]`, ...), or an `image` with a nine-patch \
     `slice = [left, top, right, bottom]` in its own pixels. `[colors]` names the fills the rest \
     of the file spells, `[roles.<name>]` is the same table a widget takes with `role`, and a \
     `[<kind>.hover]` or `[<kind>.active]` sub-table says how it looks under the pointer. A kind \
     the file leaves out keeps the built-in look. A widget takes the theme of the nearest \
     ancestor that names one, so a screen is themed by its root.";

pub(crate) const ASSET_TYPE: &str = "widget_theme";
