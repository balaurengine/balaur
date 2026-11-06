//! The `widget` component as a struct: a property a field, in the schema's
//! order (PLAN-property-types §5).
//!
//! [`WidgetProperties`] holds what a scene or a script asked of a widget and
//! nothing else. What the widget layer reads every frame is the
//! [`Widget`](crate::widget::node::Widget) built from it, which
//! [`crate::widget::schema`] keeps beside it.
//!
//! A word property takes its options from the crate's vocabulary, so a type
//! here is the list it offers rather than a Rust enum: the layer compares
//! words, and a vocabulary list is the one place each is spelled.

use balaur_core::components::derived::{
    Component, Property, Record, Result, describe_enum, not_an_option, toml,
};
use balaur_text::vocabulary::words as tw;
use smol_str::SmolStr;

use crate::vocabulary::words as w;

/// A mark at the right of a `list` or `tree` row.
#[derive(Clone, Debug, PartialEq, Record)]
pub(crate) struct Mark {
    /// What `on_mark` reports when it is clicked
    pub(crate) name: SmolStr,
    /// What it draws, from the icon face
    pub(crate) glyph: SmolStr,
}

/// One row of a `list`, `tree` or `table`.
#[derive(Clone, Debug, PartialEq, Record)]
pub(crate) struct Item {
    /// What the row says, and on a card its caption
    pub(crate) label: SmolStr,
    /// A glyph from the icon face before the label. On a card in a `list` that names a `source`, `x,y,w,h` in that picture's pixels
    pub(crate) icon: SmolStr,
    /// What the row trails at the right, and on a card its tooltip
    pub(crate) note: SmolStr,
    /// The row's own `#rrggbb`; empty takes the widget's ink
    pub(crate) tint: SmolStr,
    /// What `text`, `selection` and every row event know the row by, never drawn. Empty takes the label, or a `table` row's first cell, so two rows that say the same need one to stay two rows
    pub(crate) key: SmolStr,
    /// How many levels into a `tree` the row sits
    pub(crate) depth: u32,
    /// Drawn at the right, the last rightmost, and reported by `on_mark`
    pub(crate) marks: Vec<Mark>,
    /// A `table` row's text, one cell a column
    pub(crate) cells: Vec<SmolStr>,
}

impl Item {
    /// What the row is known by.
    pub(crate) fn key(&self) -> &str {
        if !self.key.is_empty() {
            return &self.key;
        }
        if self.label.is_empty() {
            return self.cells.first().map_or("", SmolStr::as_str);
        }
        &self.label
    }

    /// The row's own colour, where it names one.
    pub(crate) fn ink(&self) -> Option<egui::Color32> {
        crate::theme::parse_hex(&self.tint)
    }
}

/// The refusal of a value that is not text, where a word was expected.
fn not_text(value: &toml::Value) -> anyhow::Error {
    anyhow::anyhow!("is {}, where text was expected", value.type_str())
}

/// One word from a vocabulary list, as an `enum` property offering that list.
macro_rules! words {
    ($($(#[$doc:meta])* $name:ident = $options:expr;)*) => {$(
        $(#[$doc])*
        #[derive(Clone, Debug, Default, PartialEq, Eq)]
        pub(crate) struct $name(pub(crate) SmolStr);

        impl $name {
            /// The word, which a default names from the vocabulary.
            pub(crate) fn new(word: &str) -> Self {
                Self(SmolStr::new(word))
            }
        }

        impl Property for $name {
            fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
                describe_enum(spec, $options);
            }

            fn to_value(&self) -> toml::Value {
                toml::Value::String(self.0.to_string())
            }

            fn from_value(value: &toml::Value) -> Result<Self> {
                let word = value.as_str().ok_or_else(|| not_text(value))?;
                if !$options.contains(&word) {
                    return Err(not_an_option(value, $options));
                }
                Ok(Self::new(word))
            }
        }
    )*};
}

words! {
    /// A widget kind.
    Kind = w::WIDGET_KINDS;
    /// Where a widget's offset is measured from.
    Anchor = w::ANCHORS;
    /// Where one child sits across its container's direction.
    SelfAlign = w::SELF_ALIGNS;
    /// Where a menu opens.
    Placement = w::PLACEMENTS;
    /// A placement against a side of the button, which a popup falls back to.
    SidePlacement = w::SIDE_PLACEMENTS;
    /// One edge of the surface.
    Edge = w::EDGES;
    /// Where a container puts its children across its direction.
    ItemAlign = w::ITEM_ALIGNS;
    /// How a container spreads its children along its direction.
    Justify = w::JUSTIFYS;
    /// How a wrapping container spreads its lines.
    ContentAlign = w::CONTENT_ALIGNS;
    /// Whether a container's children run onto more lines.
    Wrapping = w::wrapping::ALL;
    /// Which way a row runs.
    Direction = w::direction::ALL;
    /// What the layout keeps for content past the box.
    Overflow = w::overflow::ALL;
    /// What `width` and `height` measure.
    Sizing = w::sizing::ALL;
    /// One kind of containment.
    Containment = w::contain::ALL;
    /// Which way a scroll moves.
    Axis = w::AXES;
    /// The order a grid fills its cells in.
    Flow = w::flow::ALL;
    /// How an image sits in its box.
    Fit = w::FITS;
    /// A slider's handle.
    Handle = w::HANDLES;
    /// Upright, italic or slanted.
    FontStyle = w::FONT_STYLES;
    /// Where text sits in its width.
    TextAlign = w::ALIGNS;
    /// Which part of a cut caption the ellipsis stands in for.
    TruncateAt = tw::TRUNCATE_ATS;
    /// Where a wrapped line may break.
    LineBreak = tw::LINE_BREAKS;
    /// What the pointer may do to a button.
    Sense = w::SENSES;
    /// The pointer's shape.
    Cursor = w::cursor::ALL;
    /// What shuts a menu's popup.
    CloseOn = w::CLOSE_ONS;
    /// Whether a slider holds its number inside its range.
    Clamp = w::CLAMPS;
    /// The base a number is written in.
    NumberFormat = w::NUMBER_FORMATS;
    /// What a colour picker offers for alpha.
    Alpha = w::ALPHAS;
    /// When a scrolling box has its bars.
    Scrollbar = w::SCROLLBARS;
    /// When a drag scrolls a box.
    DragScroll = w::DRAG_SCROLLS;
}

/// Four sides, left, top, right and bottom: a `vec4` that one number also
/// fills, every side alike, as `padding = 8.0` is written.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Sides(pub(crate) [f32; 4]);

impl Property for Sides {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        <[f32; 4]>::describe(spec);
    }

    fn to_value(&self) -> toml::Value {
        self.0.to_value()
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        match balaur_core::components::as_f64(value) {
            Some(every) => Ok(Self([every as f32; 4])),
            None => <[f32; 4]>::from_value(value).map(Self),
        }
    }
}

/// A number held as its text, in a list of `string`s: a scene writes
/// `["12", "40"]` and a script handed numbers writes those, which read as the
/// same text.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Numeral(pub(crate) SmolStr);

impl Numeral {
    /// A number's text, as a drag on a table's seam writes a share back.
    pub(crate) fn of(number: f32) -> Self {
        Self(smol_str::format_smolstr!("{number}"))
    }

    /// The line it names, counting from 1, where it is a whole number.
    pub(crate) fn line(&self) -> Option<u32> {
        self.0.trim().parse().ok()
    }

    /// The share of a width it names, where it is a number above zero.
    pub(crate) fn share(&self) -> Option<f32> {
        self.0
            .trim()
            .parse()
            .ok()
            .filter(|share: &f32| *share > 0.0)
    }
}

impl Property for Numeral {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        SmolStr::describe(spec);
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::String(self.0.to_string())
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        match value {
            toml::Value::String(text) => Ok(Self(SmolStr::new(text))),
            toml::Value::Integer(number) => Ok(Self(smol_str::format_smolstr!("{number}"))),
            toml::Value::Float(number) => Ok(Self(smol_str::format_smolstr!("{number}"))),
            other => Err(anyhow::anyhow!(
                "is {}, where a number or its text was expected",
                other.type_str()
            )),
        }
    }
}

/// A HUD element drawn every frame: `kind` picks `label`, `button`, `panel` and more, `anchor` places it in design pixels. A button calls `on_click`.
#[derive(Clone, Component)]
#[component(name = "widget")]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per scene property, and a scene sets them independently"
)]
pub(crate) struct WidgetProperties {
    /// The HUD element the widget layer draws
    #[property(default = Kind::new(w::LABEL))]
    pub(crate) kind: Kind,
    /// Label or button caption
    pub(crate) text: SmolStr,
    /// Off, the widget is greyed out and swallows its clicks
    #[property(default = true)]
    pub(crate) enabled: bool,
    /// Corner, edge or middle the offset is measured from: of the surface for a root, of the parent's box inside a `stack`; `fill` takes the whole of it less `inset`
    #[property(default = Anchor::new(w::TOP_LEFT), group = "placement")]
    pub(crate) anchor: Anchor,
    /// Horizontal offset from the anchor, in design pixels
    #[property(default = 16.0, group = "placement")]
    pub(crate) x: f32,
    /// Vertical offset from the anchor, in design pixels
    #[property(default = 16.0, group = "placement")]
    pub(crate) y: f32,
    /// Panel width in design pixels; 0 sizes to content
    #[property(min = 0.0, group = "placement")]
    pub(crate) width: f32,
    /// Panel height in design pixels; 0 sizes to content
    #[property(min = 0.0, group = "placement")]
    pub(crate) height: f32,
    /// Width as a percentage of the container's box inside its padding; 0 leaves it to `width`, which a stated percentage wins over
    #[property(min = 0.0, group = "placement")]
    pub(crate) width_percent: f32,
    /// Height as a percentage of the container's box inside its padding; 0 leaves it to `height`, which a stated percentage wins over
    #[property(min = 0.0, group = "placement")]
    pub(crate) height_percent: f32,
    /// Smallest width a container may give this widget, and a `resizable` window be dragged to, in design pixels
    #[property(min = 0.0, group = "placement")]
    pub(crate) min_width: f32,
    /// Smallest height a container may give this widget, and a `resizable` window be dragged to, in design pixels
    #[property(min = 0.0, group = "placement")]
    pub(crate) min_height: f32,
    /// Largest width a container may give this widget, and a `resizable` window be dragged to, in design pixels; 0 is no limit
    #[property(min = 0.0, group = "placement")]
    pub(crate) max_width: f32,
    /// Largest height a container may give this widget, and a `resizable` window be dragged to, in design pixels; 0 is no limit
    #[property(min = 0.0, group = "placement")]
    pub(crate) max_height: f32,
    /// Width over height, kept where the layout decides only one of them; 0 keeps none
    #[property(min = 0.0, group = "placement")]
    pub(crate) aspect_ratio: f32,
    /// Space outside this widget's edge that its container keeps clear, left, top, right and bottom, in design pixels; below zero lets a neighbour over it
    #[property(default = [0.0, 0.0, 0.0, 0.0], group = "placement")]
    pub(crate) margin: [f32; 4],
    /// Share of the leftover space a container hands out along its own direction; 0 takes only what this widget asks for
    #[property(min = 0.0, group = "placement")]
    pub(crate) grow: f32,
    /// Share of the shortfall this child gives up when its container is too small along its direction; below zero is 1 for one that grows and 0 otherwise
    #[property(default = -1.0, group = "placement")]
    pub(crate) shrink: f32,
    /// The size this child starts from along its container's direction, before `grow` and `shrink`, in design pixels; below zero is 0 for one that grows and its own size otherwise
    #[property(default = -1.0, group = "placement")]
    pub(crate) basis: f32,
    /// Where this child sits across its container's direction; `auto` takes the container's `align_items`
    #[property(default = SelfAlign::new(w::AUTO), group = "placement")]
    pub(crate) align_self: SelfAlign,
    /// Take this child out of its container's run and place it by `inset` against the container's box inside its border, drawn in its turn among its siblings; a root ignores it
    #[property(group = "placement")]
    pub(crate) absolute: bool,
    /// Left, top, right and bottom margins a root with `anchor = "fill"` keeps from its surface, and how far an `absolute` child sits in from each edge of its container inside its border, where a side below zero is left free; in design pixels
    #[property(default = [0.0, 0.0, 0.0, 0.0], group = "placement")]
    pub(crate) inset: [f32; 4],
    /// Whether this root draws over the whole window, whatever `ui.set_widget_layer` says: what an editor's own chrome does. Ignored on a child.
    #[property(group = "placement")]
    pub(crate) whole_window: bool,
    /// The edges this root keeps clear of what a notch, a status bar or a home bar covers. A `fill` root's own colour still reaches the edge, and one with no children is a backdrop and reaches it whole
    #[property(flags, default = vec![Edge::new(w::LEFT), Edge::new(w::TOP), Edge::new(w::RIGHT), Edge::new(w::BOTTOM)], group = "placement")]
    pub(crate) safe_area: Vec<Edge>,
    /// Let a drag move this root and write its `x` and `y`: a `window` by its title bar, any other root by its whole box where no child takes the press
    #[property(group = "placement")]
    pub(crate) movable: bool,
    /// Keep a `movable` root inside its surface while it is dragged
    #[property(group = "placement")]
    pub(crate) constrain: bool,
    /// Not drawn while the room is narrower than this many design pixels. The room is the nearest container that states a size or grows, and the screen for a root: a minimum in numbers, where the class words are not fine enough. Zero is no line
    #[property(min = 0.0, group = "placement")]
    pub(crate) hide_narrower: f32,
    /// Not drawn while the room is this wide or wider, in design pixels: a control only a small space wants. Zero is no line
    #[property(min = 0.0, group = "placement")]
    pub(crate) hide_wider: f32,
    /// Not drawn while the room is shorter than this many design pixels. Zero is no line
    #[property(min = 0.0, group = "placement")]
    pub(crate) hide_shorter: f32,
    /// Not drawn while the room is this tall or taller, in design pixels. Zero is no line
    #[property(min = 0.0, group = "placement")]
    pub(crate) hide_taller: f32,
    /// Space inside the widget's edge, in design pixels: one number for every side, or left, top, right and bottom. A container keeps it round its children, a `button` and a `label` round their text, and every other kind in its box. A side below zero takes the theme's (`padding_x` across and `padding_y` down, else `padding`), else 8 round a `panel`, 12 either side of a `button`'s or `menu`'s caption and 0 elsewhere; a stated zero is no space at all. On a `text_field`, `text_area` or `code` it is the margin round the text, where below zero takes egui's
    #[property(default = Sides([-1.0; 4]), group = "layout")]
    pub(crate) padding: Sides,
    /// The width of this widget's outline on each side, left, top, right and bottom, in design pixels: the layout keeps it clear inside the edge as it keeps `padding`, and the `stroke` is painted in exactly that band. All zero leaves the stroke at the theme's `stroke_width`, over the padding
    #[property(default = [0.0, 0.0, 0.0, 0.0], group = "layout")]
    pub(crate) border: [f32; 4],
    /// How this widget and everything under it is drawn; inherited from the nearest ancestor that names one
    #[property(asset = "widget_theme", group = "paint")]
    pub(crate) theme: SmolStr,
    /// A `[roles.<name>]` entry of the widget's theme, taken over its kind's own style; the one place a look is named rather than spelled
    #[property(group = "paint")]
    pub(crate) role: SmolStr,
    /// What is painted behind this widget, an `image`'s picture included, as `#rrggbb` or a name from the theme's `[colors]`; empty takes the theme's own
    #[property(group = "paint")]
    pub(crate) fill: SmolStr,
    /// The outline around this widget, as `#rrggbb` or a name from the theme's `[colors]`; empty takes the theme's own
    #[property(group = "paint")]
    pub(crate) stroke: SmolStr,
    /// Text color; fully transparent takes the theme's colour for this widget's role or kind, and failing that a near-white
    #[property(color, default = [0.0, 0.0, 0.0, 0.0], group = "paint")]
    pub(crate) text_color: [f32; 4],
    /// Corner radius in design pixels, a `progress_bar`'s and an `image`'s included; below zero takes the theme's own, which for a button is as round as its text is tall
    #[property(default = -1.0, group = "paint")]
    pub(crate) corner_radius: f32,
    /// Draw the outline in dashes, as a guide over a picture is drawn, rather than whole; square at the corners
    #[property(group = "paint")]
    pub(crate) dashed: bool,
    /// Fade a root in as it appears, over egui's fade time
    #[property(group = "paint")]
    pub(crate) fade_in: bool,
    /// The face it draws in, and how: a face file such as `fonts/Inter.ttf`, or a `font` asset. Empty takes the theme's default font
    #[property(asset = "font", group = "font")]
    pub(crate) font: SmolStr,
    /// Text size in design pixels; 0 takes the size the role or the kind carries
    #[property(min = 0.0, group = "font")]
    pub(crate) font_size: f32,
    /// Weight on the CSS scale, resolved against the faces the project ships: 400 regular, 700 bold
    #[property(default = 400, min = 100, max = 900, group = "font")]
    pub(crate) font_weight: i64,
    /// Upright, the family's italic face, or the upright face slanted; a family with no italic is slanted either way
    #[property(default = FontStyle::new(w::NORMAL), group = "font")]
    pub(crate) font_style: FontStyle,
    /// How it is dressed past its font: line height and spacing, the lines drawn with the glyphs, an outline and a shadow. Empty draws it plain
    #[property(asset = "text_style", group = "font")]
    pub(crate) text_style: SmolStr,
    /// A localization key drawn in place of `text`, re-read every frame so a locale switch shows at once
    #[property(group = "text")]
    pub(crate) text_key: SmolStr,
    /// Text shown after the pointer rests on the widget; still shown while `enabled` is off, which is where it says why
    #[property(group = "text")]
    pub(crate) tooltip: SmolStr,
    /// Script method called when the widget is clicked, on this node or the nearest ancestor whose script declares it. An `image` that names one senses clicks too, which is how a picture becomes a button
    #[property(group = "events")]
    pub(crate) on_click: SmolStr,
    /// Script method called with a `text_field`'s text after every edit, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_change: SmolStr,
    /// Script method called when focus arrives, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_focus: SmolStr,
    /// Script method called on a double click, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_double_click: SmolStr,
    /// Script method called when focus leaves, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_blur: SmolStr,
    /// Off, the pointer passes through to the scene: the widget is drawn, never hovered or clicked, and `ui.wants_pointer()` stays false over it. A full-screen container over the world wants this
    #[property(default = true, group = "events")]
    pub(crate) interactive: bool,
    /// Let focus land here. A widget nothing can activate is never focused whatever this says; set it false to skip one that could be
    #[property(default = true, group = "events")]
    pub(crate) focusable: bool,
    /// The widget focus goes to when an arrow, a stick or a d-pad points up from this one, by node name; empty leaves it to where the widgets were drawn
    #[property(group = "events")]
    pub(crate) neighbour_up: SmolStr,
    /// The same pointing down
    #[property(group = "events")]
    pub(crate) neighbour_down: SmolStr,
    /// The same pointing left
    #[property(group = "events")]
    pub(crate) neighbour_left: SmolStr,
    /// The same pointing right
    #[property(group = "events")]
    pub(crate) neighbour_right: SmolStr,
    /// The pointer's shape while it is over the widget: `hand` over anything that opens on a click, `none` to hide it; `arrow` is the platform's own
    #[property(default = Cursor::new(w::cursor::ARROW), group = "events")]
    pub(crate) cursor: Cursor,
    /// Name of a `menu` node whose rows open at the pointer on a right click or a long press; give that menu `visible = false` to show no button of its own
    #[property(group = "events")]
    pub(crate) context: SmolStr,
}
