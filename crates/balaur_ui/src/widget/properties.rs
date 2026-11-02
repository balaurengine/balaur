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
    Component, Property, Result, describe_enum, not_an_option, toml,
};
use balaur_text::vocabulary::words as tw;
use smol_str::SmolStr;

use crate::vocabulary::words as w;

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
    /// The drawing surface this root belongs to; empty is the default one, and a name nothing has configured takes the default surface
    #[property(group = "placement")]
    pub(crate) layer: SmolStr,
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
    /// Space between a container's children, across and down, in design pixels: a row puts the first between its children and the second between the lines it wraps onto, a column the second between its children, and a grid the first between its columns and the second between its rows. On a `button` the first is the space between its picture, icon and caption. Below zero on an axis takes the theme's own, which is 8 where it says nothing and half the font size on a button, and a stated zero puts them edge to edge
    #[property(default = [-1.0, -1.0], group = "layout")]
    pub(crate) gap: [f32; 2],
    /// Where a container puts its children across its own direction, and a `grid` each child in its cell down: `stretch` makes each as wide (or tall) as the container, `baseline` lines up their first lines of text, the others keep each child's own size
    #[property(default = ItemAlign::new(w::STRETCH), group = "layout")]
    pub(crate) align_items: ItemAlign,
    /// How a container spreads its children along its own direction once they have their sizes, and a `grid` its columns: `stretch` grows a grid's `auto` columns into the room, and in a row or a column is `start`
    #[property(default = Justify::new(w::START), group = "layout")]
    pub(crate) justify: Justify,
    /// How a container that wraps spreads its lines across its direction, and a `grid` its rows, where they leave room: `stretch` shares the room among them
    #[property(default = ContentAlign::new(w::STRETCH), group = "layout")]
    pub(crate) align_content: ContentAlign,
    /// Whether a container's children run onto more lines when they do not fit: `auto` wraps a `flow` and keeps every other container on one line; `balance` evens the lines out over at least `min_lines`; a `_reverse` word stacks the lines the other way
    #[property(default = Wrapping::new(w::AUTO), group = "layout")]
    pub(crate) wrap_children: Wrapping,
    /// How many lines a wrapping container divides its children between at least: `balance` evens them over this many, and any wrap shares a known cross size among them
    #[property(default = 1, min = 1, group = "layout")]
    pub(crate) min_lines: i64,
    /// Which way a row and a grid's columns run: `right_to_left` starts them at the right edge
    #[property(default = Direction::new(w::direction::LEFT_TO_RIGHT), group = "layout")]
    pub(crate) direction: Direction,
    /// What the layout keeps for content past this widget's box: `scroll` keeps `scrollbar_width` clear inside its edge, the others nothing. Layout only: a container never gives a child a floor of its own content here (`min_width` is that floor), a `scroll` kind is what scrolls, and each kind clips as it draws
    #[property(default = Overflow::new(w::overflow::VISIBLE), group = "layout")]
    pub(crate) overflow: Overflow,
    /// The width of this widget's outline on each side, left, top, right and bottom, in design pixels: the layout keeps it clear inside the edge as it keeps `padding`, and the `stroke` is painted in exactly that band. All zero leaves the stroke at the theme's `stroke_width`, over the padding
    #[property(default = [0.0, 0.0, 0.0, 0.0], group = "layout")]
    pub(crate) border: [f32; 4],
    /// What `width` and `height` measure: `border`, the whole box, or `content`, the box inside `padding` and `border`
    #[property(default = Sizing::new(w::sizing::BORDER), group = "layout")]
    pub(crate) box_sizing: Sizing,
    /// Where `align_items`, `align_self`, `align_content` or `justify` would push a child past its container's start, put it at the start instead, so the start stays in view
    #[property(group = "layout")]
    pub(crate) safe_align: bool,
    /// Containment: `layout` or `paint` makes the box lay out on its own, and `layout` also gives it no baseline for a parent's `baseline` alignment
    #[property(flags, group = "layout")]
    pub(crate) contain: Vec<Containment>,
    /// The room a box with `overflow = "scroll"` keeps clear inside its edge for a bar, in design pixels
    #[property(min = 0.0, group = "layout")]
    pub(crate) scrollbar_width: f32,
    /// Which way a scroll moves, the other way its contents filling the box it was given. A `slider` runs up its box under `vertical`, and a `separator` draws the line it names, `both` leaving it across its parent's direction
    #[property(default = Axis::new(w::BOTH), group = "layout")]
    pub(crate) axis: Axis,
    /// A `grid`'s columns, left to right and space-separated: design pixels (`120`), a share of what is left (`1fr`), a percentage (`25%`), `auto`, `min_content`, `max_content`, `fit_content(120)`, `minmax(80, 1fr)`, and `repeat(3, 1fr)`, whose count may be `auto_fill` or `auto_fit`. Empty is two equal columns. A `list` flows its cards into as many columns as this names, and draws a line a row where it is empty; a `table`'s columns are its `titles`
    #[property(group = "layout")]
    pub(crate) grid_columns: SmolStr,
    /// A `grid`'s rows, top to bottom, in the words `grid_columns` takes; empty makes each row as tall as what is in it
    #[property(group = "layout")]
    pub(crate) grid_rows: SmolStr,
    /// A `grid`'s named areas, one string a row of space-separated names with `.` for a cell in none: `["head head", "side main"]`. A child names one in `row` and `column`
    #[property(group = "layout")]
    pub(crate) areas: Vec<SmolStr>,
    /// The order a `grid` fills cells with the children that name none: along each row, or down each column; a `_dense` order goes back to fill a hole a bigger child left
    #[property(default = Flow::new(w::flow::ROW), group = "layout")]
    pub(crate) auto_flow: Flow,
    /// The size of each column a `grid` adds past `grid_columns`, for a child placed beyond them or flowing down under `auto_flow = "column"`, in the words `grid_columns` takes without `repeat`; several take turns. Empty is `auto`
    #[property(group = "layout")]
    pub(crate) auto_columns: SmolStr,
    /// The size of each row a `grid` adds past `grid_rows`, in the words `auto_columns` takes; empty is `auto`
    #[property(group = "layout")]
    pub(crate) auto_rows: SmolStr,
    /// On a `grid`'s child: the rows it covers, as a line counted from 1 (`2`, or from the end below zero), a first and last line (`1 / 3`), a span (`span 2`, `2 / span 2`), or the name of one of the grid's `areas`. Empty puts it in the next free cell
    #[property(group = "layout")]
    pub(crate) row: SmolStr,
    /// On a `grid`'s child: the columns it covers, in the words `row` takes
    #[property(group = "layout")]
    pub(crate) column: SmolStr,
    /// Draw the strip that names a `table`'s columns, and a `window`'s title bar. Off, a table's columns are still `titles`', and a table that names none keeps its first row as a row
    #[property(default = true, group = "layout")]
    pub(crate) header: bool,
    /// On a `fold`'s child: drawn in the fold's header after its arrow and caption, as Godot's title bar control is
    #[property(group = "layout")]
    pub(crate) title_bar: bool,
    /// The pitch of a `list` or `tree` row, in design pixels; 0 takes the font's own line height. An `inspector`'s file rows are this tall and its chips 4 less, 24 where it is 0
    #[property(min = 0.0, group = "layout")]
    pub(crate) row_height: f32,
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
    /// A glyph from the theme's icon family, drawn before `text`
    #[property(group = "paint")]
    pub(crate) icon: SmolStr,
    /// What that glyph is tinted with, as `#rrggbb` or a name from the theme's `[colors]`; empty takes the role's own
    #[property(group = "paint")]
    pub(crate) icon_color: SmolStr,
    /// That glyph's size in design pixels; below zero takes the caption's
    #[property(default = -1.0, group = "paint")]
    pub(crate) icon_size: f32,
    /// The project-relative image an `image` widget draws, or the picture a `button` draws before its caption at the caption's height
    #[property(group = "paint")]
    pub(crate) image: SmolStr,
    /// Left, top, right and bottom borders of an `image` kept unstretched, in the picture's own pixels; all zero stretches the whole picture
    #[property(default = [0.0, 0.0, 0.0, 0.0], group = "paint")]
    pub(crate) slice: [f32; 4],
    /// A wash behind the text of the kinds egui draws; alpha 0 is none
    #[property(color, default = [0.0, 0.0, 0.0, 0.0], group = "paint")]
    pub(crate) text_background: [f32; 4],
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
    /// Read inline marks in the text: `[b]`, `[i]`, `[color=#hex]`, `[center]`, `[right]`, `[wave amp=N freq=N]` and `[img=path width=N]`; off, brackets are text
    #[property(group = "text")]
    pub(crate) markup: bool,
    /// Where text sits in the width the widget was given, what a `text_field` or `text_area` edits included, and where every cell of a `table` sits in its column; a column whose name ends in `>` pins its own to the right
    #[property(default = TextAlign::new(w::START), group = "text")]
    pub(crate) text_align: TextAlign,
    /// Break text to the width the widget was given instead of running past it on one line; a `dropdown` breaks its picked text, and an `inspector` puts each label above its control
    #[property(group = "text")]
    pub(crate) wrap: bool,
    /// Cut a caption too long for the width the widget was given and end it with an ellipsis; with neither this nor `wrap` a button clips it at its edge. A `dropdown` cuts its picked text
    #[property(group = "text")]
    pub(crate) truncate: bool,
    /// Which part of a cut caption the ellipsis stands in for
    #[property(default = TruncateAt::new(tw::END), group = "text")]
    pub(crate) truncate_at: TruncateAt,
    /// Show a `label`'s whole text when the pointer rests on it and `truncate` cut it short
    #[property(default = true, group = "text")]
    pub(crate) show_tooltip_when_elided: bool,
    /// The most lines a wrapped label keeps; zero keeps every line
    #[property(min = 0, group = "text")]
    pub(crate) max_lines: i64,
    /// Where a wrapped line may break
    #[property(default = LineBreak::new(tw::WORD_OR_GLYPH), group = "text")]
    pub(crate) line_break: LineBreak,
    /// Spaces between tab stops
    #[property(default = 8, min = 1, max = 64, group = "text")]
    pub(crate) tab_width: i64,
    /// Text drawn before a `number_field`'s or a shown `slider`'s number with a space between, and before a `text_field`'s or `text_area`'s text
    #[property(group = "text")]
    pub(crate) prefix: SmolStr,
    /// Units drawn after a `number_field`'s or a shown `slider`'s number with a space between, and after a `text_field`'s or `text_area`'s text
    #[property(group = "text")]
    pub(crate) suffix: SmolStr,
    /// Text a button draws against its far edge, dimmer than its caption: a shortcut, or a menu's caret
    #[property(group = "text")]
    pub(crate) trailing: SmolStr,
    /// Text shown after the pointer rests on the widget; still shown while `enabled` is off, which is where it says why
    #[property(group = "text")]
    pub(crate) tooltip: SmolStr,
    /// Let a drag over this label select its text, and the platform's copy key take it
    #[property(group = "text")]
    pub(crate) selectable: bool,
    /// Script method called when the widget is clicked, on this node or the nearest ancestor whose script declares it. An `image` that names one senses clicks too, which is how a picture becomes a button
    #[property(group = "events")]
    pub(crate) on_click: SmolStr,
    /// Script method called with a `text_field`'s text after every edit, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_change: SmolStr,
    /// Script method called with a `text_field`'s text on Enter, or when focus leaves it, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_submit: SmolStr,
    /// Script method called when focus arrives, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_focus: SmolStr,
    /// Script method called with the target of a `[url=target]` span in `markup` text that was clicked, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_link: SmolStr,
    /// Script method called on a double click, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_double_click: SmolStr,
    /// Script method called when focus leaves, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_blur: SmolStr,
    /// Script method called with the value once a drag or a typed number that changed it is over, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_commit: SmolStr,
    /// Script method called with `{ x, y, index }` when a text widget's caret moves, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_caret: SmolStr,
    /// Script method called with the row double-clicked in a `list`, `tree` or `table`, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_activate: SmolStr,
    /// Script method called with `{ row, open }` when a `tree` row folds, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_fold: SmolStr,
    /// Script method called when a popup, dialog or window comes up, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_opened: SmolStr,
    /// Script method called when a popup, dialog or window goes away, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_closed: SmolStr,
    /// Script method called with the offset, `[x, y]`, when a `scroll` moves, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_scrolled: SmolStr,
    /// Script method called when a `window`'s close button is pressed, before it shuts or, with `hide_on_close` off, instead of shutting, on this node or the nearest ancestor whose script declares it
    #[property(group = "events")]
    pub(crate) on_close_request: SmolStr,
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
    /// What the pointer may do to a `button`: `click` reports a press let go as a click; under `click_and_drag` a press that moves past egui's drag distance is a drag and no click; `drag` and `hover` never click
    #[property(default = Sense::new(w::CLICK), group = "events")]
    pub(crate) sense: Sense,
    /// The pointer's shape while it is over the widget: `hand` over anything that opens on a click, `none` to hide it; `arrow` is the platform's own
    #[property(default = Cursor::new(w::cursor::ARROW), group = "events")]
    pub(crate) cursor: Cursor,
    /// A chord that clicks this widget wherever it is, as `cmd+shift+s` or `f5`; a menu row fires while its menu is shut, and draws the chord against its far edge unless it says its own `trailing`
    #[property(group = "events")]
    pub(crate) shortcut: SmolStr,
    /// Whether `shortcut` fires while a text field holds the keyboard. A chord without cmd, ctrl or alt never does unless it is a function key, since that is typing; off, no chord does, for one the field has a use for itself, as `cmd+z`
    #[property(default = true, group = "events")]
    pub(crate) while_typing: bool,
    /// Whether `shortcut` takes its key ahead of a field being typed into, a plain key included, so the field never sees it: a completion list's arrows, Enter and Escape while it is up
    #[property(group = "events")]
    pub(crate) takes_typing: bool,
    /// Name of a `menu` node whose rows open at the pointer on a right click or a long press; give that menu `visible = false` to show no button of its own
    #[property(group = "events")]
    pub(crate) context: SmolStr,
    /// Whether a `fold` shows its children; its header flips it and calls `on_change` with the new state
    #[property(default = true, group = "events")]
    pub(crate) open: bool,
    /// Which child a `tabs` shows, by node name; empty shows the first. A click on the strip writes it and calls `on_change` with the page's name
    #[property(group = "events")]
    pub(crate) current_page: SmolStr,
    /// A menu row that leaves its menu open when clicked, as a toggle does; any other row closes it. A `dropdown` with it stays open past a pick until a click outside it
    #[property(group = "events")]
    pub(crate) keep_open: bool,
    /// Whether a `checkbox` is ticked, every click flipping it and calling `on_change` with the new state; a checked `button` is held down, wearing its pressed look
    #[property(group = "value")]
    pub(crate) checked: bool,
    /// The items a `dropdown`, `menu`, `list`, `tree` or `table` holds; `text` is the one picked, except on a `menu` where it is the button caption. A `tree` row starts with one tab per level, a `list` or `tree` row splits on U+001F into icon, label, a trailing note, an `#rrggbb` for that row, a key that is never drawn, which two rows with the same label need to stay two rows, and marks, each `name=glyph` from the icon face and joined on U+001E, drawn at the right and reported by `on_mark`; and a `table` row splits on the same into one cell a column. `on_change` hears every pick. An `inspector` shows the properties it names, in that order
    #[property(group = "value")]
    pub(crate) options: Vec<SmolStr>,
    /// The rows a `list`, `tree` or `table` has picked, one of them where it holds one. `text` is the last row clicked, which is where a shift range measures from; `on_change` hears the whole list where the widget holds many, and the row where it holds one
    #[property(group = "value")]
    pub(crate) selection: Vec<SmolStr>,
    /// Let a `list`, `tree` or `table` hold more than one row: the platform's command key toggles a row and shift takes the run from the last one clicked
    #[property(group = "value")]
    pub(crate) multi_select: bool,
    /// A name this `checkbox` or `toggle` button shares with the ones it is exclusive with: ticking one unticks the rest, and one already ticked stays ticked. Empty leaves it flipping on its own
    #[property(group = "value")]
    pub(crate) radio_group: SmolStr,
    /// A `button` a click holds down and the next releases, flipping `checked` as a `checkbox` does, before `on_click` runs: Godot's toggle mode
    #[property(group = "value")]
    pub(crate) toggle: bool,
    /// Draw a `checkbox` as neither ticked nor clear, for a group whose members disagree; a click still flips `checked`
    #[property(group = "value")]
    pub(crate) indeterminate: bool,
    /// How wide a grab the seams between this container's children get, in design pixels; 0 leaves them fixed. A drag writes the new size onto the neighbour that states one. On a `table` it is the grab between two columns, which is six pixels where it says nothing
    #[property(min = 0.0, group = "value")]
    pub(crate) splitter_width: f32,
    /// Take a `table`'s rows the other way round: the `sort` descending, or the order they were given bottom to top where none is named. A `row`, `column` or `flow` lays its children out from its far end
    #[property(group = "value")]
    pub(crate) reverse: bool,
}
