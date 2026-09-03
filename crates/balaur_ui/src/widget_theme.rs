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
//! [button]
//! fill = "#2a2f3a"
//! stroke = "#d5814e"
//! radius = 12
//! padding = 8
//!
//! [panel]
//! fill = "#00000060"
//! radius = 8
//! ```
//!
//! A kind the file does not mention keeps the built-in look, so a theme that
//! restyles buttons alone is three lines and says only what it changes.

use std::collections::BTreeMap;

use egui::Color32;

/// How one widget kind is drawn.
#[derive(Clone, Copy)]
pub struct Style {
    pub fill: Option<Color32>,
    pub stroke: Option<Color32>,
    /// Corner radius in design pixels; `None` keeps the kind's own rule,
    /// which for a button is "as round as its text is tall".
    pub radius: Option<f32>,
    pub padding: Option<f32>,
    pub stroke_width: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            fill: None,
            stroke: None,
            radius: None,
            padding: None,
            stroke_width: 1.0,
        }
    }
}

/// A parsed theme: a style per widget kind, and nothing for kinds it is
/// silent about.
#[derive(Default)]
pub struct WidgetTheme {
    kinds: BTreeMap<String, Style>,
}

impl WidgetTheme {
    /// The style for a kind, or the empty one — which means "as before".
    #[must_use]
    pub fn style(&self, kind: &str) -> Style {
        self.kinds.get(kind).copied().unwrap_or_default()
    }
}

/// `#rrggbb` or `#rrggbbaa`, the spelling the editor's theme already uses.
///
/// A colour that does not parse is reported and dropped rather than being
/// silently black: a theme is authored by hand, and a typo should say so.
fn color(value: &toml::Value, what: &str) -> Option<Color32> {
    let text = value.as_str()?;
    let hex = text.strip_prefix('#').unwrap_or(text);
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    let parsed = match hex.len() {
        6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(
            byte(0)?,
            byte(2)?,
            byte(4)?,
            byte(6)?,
        )),
        _ => None,
    };
    if parsed.is_none() {
        tracing::warn!("widget theme: '{text}' is not a colour for {what}");
    }
    parsed
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
    for (kind, body) in table {
        // `type` names the asset, and is not a widget kind.
        if kind == "type" {
            continue;
        }
        let Some(body) = body.as_table() else {
            continue;
        };
        let number = |key: &str| {
            body.get(key)
                .and_then(balaur_core::components::as_f64)
                .map(|v| v as f32)
        };
        theme.kinds.insert(
            kind.clone(),
            Style {
                fill: body.get("fill").and_then(|v| color(v, "fill")),
                stroke: body.get("stroke").and_then(|v| color(v, "stroke")),
                radius: number("radius"),
                padding: number("padding"),
                stroke_width: number("stroke_width").unwrap_or(1.0),
            },
        );
    }
    theme
}

/// The doc string `balaur api` and the editor's asset picker show.
pub(crate) const ASSET_DOC: &str =
    "How each widget kind is drawn: `fill`, `stroke`, `stroke_width`, \
     `radius` and `padding` under a table named for the kind (`[button]`, `[panel]`, `[row]`, \
     ...). A kind the file leaves out keeps the built-in look. A widget takes the theme of the \
     nearest ancestor that names one, so a screen is themed by its root.";

pub(crate) const ASSET_TYPE: &str = "widget_theme";
