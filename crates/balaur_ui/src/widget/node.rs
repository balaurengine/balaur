//! The `widget` component and the resources the layer reads: what a scene
//! file says about a widget, where the layer draws, and which widget has focus.

use std::collections::HashMap;

use balaur_core::hecs::Entity;
use egui::Color32;
// A widget's words are short and cloned once a node a frame; inline they are
// a copy, and as `String` they were an allocation each.
use smol_str::SmolStr;

use crate::vocabulary::words as w;

/// A component colour (`[r, g, b, a]` in 0..=1) as egui's 8-bit one.
pub(crate) fn rgba_color(rgba: [f32; 4]) -> Color32 {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(
        channel(rgba[0]),
        channel(rgba[1]),
        channel(rgba[2]),
        channel(rgba[3]),
    )
}

#[derive(Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per scene property, and a scene sets them independently"
)]
pub struct Widget {
    pub kind: SmolStr,
    pub text: SmolStr,
    /// Hidden widgets draw nothing and take no clicks, but keep their state.
    pub visible: bool,
    pub anchor: SmolStr,
    pub x: f32,
    pub y: f32,
    /// Panel size in design pixels; 0 sizes to content. A minimum on buttons.
    pub width: f32,
    pub height: f32,
    /// Height of the widget's text, in design pixels.
    pub font_size: f32,
    /// The text's colour as `[r, g, b, a]` in 0..=1, the same representation
    /// the `color` component uses.
    pub text_color: [f32; 4],
    /// Method on this node's script, called when the widget is clicked.
    /// Empty means nothing is connected. A name rather than a function value:
    /// scene files cannot hold closures, and a name works on any backend.
    pub on_click: SmolStr,
    pub clicked: bool,
    /// Space inside a container's edge, in design pixels.
    pub padding: f32,
    /// Space between a container's children.
    pub gap: f32,
    /// Cross-axis placement of a container's children.
    pub align: SmolStr,
    /// Whether focus may land here, for a widget that could take it.
    pub focusable: bool,
    /// Method on this node's script, called when focus arrives.
    pub on_focus: SmolStr,
    /// A `widget_theme` reference, or empty to take the one above.
    pub theme: SmolStr,
    /// A localization key drawn instead of `text` when it is set.
    pub text_key: SmolStr,
    /// Share of a container's leftover space along its axis; 0 takes only
    /// what `width`/`height` or the content asks for.
    pub grow: f32,
    /// The author's floor, whatever the content measures.
    pub min_width: f32,
    pub min_height: f32,
    /// What fills a `draw` widget's rect: a method on this node's script or
    /// the nearest scripted ancestor's, or `file.rn:function` for a free
    /// function that needs no instance.
    pub draw: SmolStr,
    /// How wide a grab the seams between this container's children get, in
    /// design pixels; 0 leaves them fixed.
    pub handle: f32,
    /// Which child a `tab` shows, by node name; empty shows the first.
    pub active: SmolStr,
    /// The drawing surface a *root* widget belongs to; empty is the default
    /// one. Ignored on a child, which is placed by its parent.
    pub layer: SmolStr,
    /// Whether text breaks to the width it was given rather than running past
    /// it on one line.
    pub wrap: bool,
    /// A menu row that leaves the menu open when clicked, as a toggle does.
    pub keep_open: bool,
    /// Text against a button's far edge: a shortcut, or a menu's caret.
    pub trailing: SmolStr,
    /// A menu held open by the scene rather than by a click.
    pub showing: bool,

    /// Where text sits in the width the widget was given.
    pub text_align: SmolStr,
    /// A project-relative image for an `image` widget.
    pub source: SmolStr,
    /// Whether the text carries inline marks: `[b]`, `[i]`, `[color=#hex]`,
    /// `[center]`, `[wave]`, `[img=path width=N]`.
    pub markup: bool,
    /// Weight on the CSS scale, 100 to 900; 400 is regular, 700 bold.
    pub font_weight: f32,
    /// `normal` or `italic`.
    pub font_style: SmolStr,
    /// What a `field` shows while empty.
    pub placeholder: SmolStr,
    /// The most characters a `field` takes; 0 is no limit.
    pub max_length: f32,
    /// Draw a `field`'s text as dots.
    pub secret: bool,
    /// Keep a `field` to digits, a sign and a point.
    pub numeric: bool,
    /// Method on this node's script, called with the text after every edit.
    pub on_change: SmolStr,
    /// Method on this node's script, called with the text on Enter or when
    /// focus leaves the field.
    pub on_submit: SmolStr,
    /// What a `color` swatch holds, as `[r, g, b, a]` in 0..=1. Separate from
    /// `text_color`, which is the ink a widget draws its caption in.
    pub color: [f32; 4],
    /// The pitch of a `list` or `tree` row, in design pixels; 0 takes the
    /// font's own line height. Separate from `height`, which is the widget's.
    pub row_height: f32,
    /// Which of the theme's families the widget draws in: `ui`, `mono`,
    /// `heading` or `icon`.
    pub font: SmolStr,
    /// Whether a `check` is ticked.
    pub checked: bool,
    /// The name a `check` shares with the checks it is exclusive with: ticking
    /// one unticks the rest, and a ticked one clicked again stays ticked.
    /// Empty leaves the check on its own, flipping with every click.
    pub group: SmolStr,
    /// Where a `slider` or `progress` stands, between `min` and `max`.
    pub value: f32,
    pub min: f32,
    pub max: f32,
    /// The grid a `slider` snaps to; 0 is continuous.
    pub step: f32,
    /// What a `dropdown` offers; `text` is the one chosen.
    pub options: Vec<SmolStr>,
    /// How many children a `grid` puts on each row.
    pub columns: u32,
    /// Whether a `fold` shows its children.
    pub open: bool,
    /// Left, top, right and bottom margins a `fill` root keeps from its
    /// surface, in design pixels.
    pub inset: [f32; 4],
    /// A root that measures its bottom from the top of the on-screen
    /// keyboard, so a form stays above it.
    pub avoid_keyboard: bool,
    /// The nine-patch borders of an `image`, in the picture's own pixels.
    pub slice: [f32; 4],
    /// How far a finger drags a `scroll` before it scrolls, in design pixels.
    pub deadzone: f32,
    /// A `[roles.<name>]` entry of the theme, taken over the kind's own style.
    pub role: SmolStr,
    /// Text shown after the pointer rests on the widget.
    pub tooltip: SmolStr,
    /// A glyph from the theme's icon family, drawn before `text`.
    pub icon: SmolStr,
    /// Greyed out, and deaf to clicks.
    pub disabled: bool,
    /// A fill and an outline this one widget states, as `#rrggbb` or a name
    /// from the theme's `[colors]`; empty takes the theme's own.
    pub fill: SmolStr,
    pub stroke: SmolStr,
    /// Corner radius in design pixels; below zero takes the theme's own.
    pub radius: f32,
    /// How a container spreads its children along its own direction.
    pub justify: SmolStr,
    /// The air either side of a caption; below zero takes the theme's.
    pub padding_x: f32,
}

/// Whether this kind lays its widget children out rather than ignoring them.
///
/// A `panel` counts: it already draws a frame, and a frame with things in it
/// is what a menu is made of. One with no children behaves exactly as before.
pub(crate) fn lays_out(kind: &str) -> bool {
    matches!(
        kind,
        w::ROW
            | w::COLUMN
            | w::PANEL
            | w::SCROLL
            | w::TAB
            | w::GRID
            | w::FLOW
            | w::FOLD
            | w::DIALOG
            | w::WINDOW
            | w::MENU
    )
}

/// Where and whether the widget layer draws. Games leave the default (full
/// window); editors point it at their viewport and enable it during play.
pub struct WidgetLayerConfig {
    pub enabled: bool,
    /// Whether arrows, Tab, Enter and Space move and activate the focus.
    ///
    /// Off by default: a game that moves with the arrows and jumps with Space
    /// would otherwise click its own HUD button. `standard_app` turns it on
    /// for a project that declares the `ui_*` actions, and a script asks for
    /// it with `ui.set_keyboard_focus`.
    pub keyboard: bool,
    /// Design-px rect (x, y, w, h); None = whole screen.
    pub rect: Option<[f32; 4]>,
    /// Where a root that names a `layer` draws instead. A name nothing here
    /// configures takes the default surface, so a host that confines the
    /// default confines every layer it was never told about.
    pub layers: HashMap<String, Surface>,
}

/// One drawing surface: whether roots on it draw, and where.
#[derive(Clone, Copy)]
pub struct Surface {
    pub enabled: bool,
    pub rect: Option<[f32; 4]>,
}

impl Default for Surface {
    fn default() -> Self {
        Self {
            enabled: true,
            rect: None,
        }
    }
}

impl Default for WidgetLayerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            keyboard: false,
            rect: None,
            layers: HashMap::new(),
        }
    }
}

/// Which widget the keyboard and the pad are pointing at.
///
/// One per screen, because that is what focus means: the thing an `accept`
/// would activate. Held as a resource rather than on the widget so that
/// moving it is one write, and so a script can ask without walking the tree.
#[derive(Default)]
pub struct UiFocus {
    /// The focused widget, or `None` before anything has taken focus.
    pub focused: Option<Entity>,
    /// Set by `focus_next` and friends and consumed by the next draw, so a
    /// script can move focus outside the pass that will act on it.
    pub pending: Option<Move>,
}

/// What a script or the keyboard asked focus to do.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Next,
    Previous,
    /// Activate what is focused, as a click would.
    Accept,
}
