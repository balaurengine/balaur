//! The components beside `widget` about boxes: how a container lays out what
//! is in it, a grid's tracks, what a child says to its parent, and the rarer
//! words any box may ask for. No property shows on a condition
//! (PLAN-property-types §5).

use balaur_core::components::derived::{Component, Result};
use smol_str::SmolStr;

use crate::vocabulary::words as w;
use crate::widget::beside::{Beside, puts};
use crate::widget::grid;
use crate::widget::node::lays_out;
use crate::widget::properties::{
    Containment, ContentAlign, Direction, Flow, ItemAlign, Justify, Overflow, Sizing, Wrapping,
};

/// The rows a container reads: how it lays out what is in it.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_layout")]
pub(crate) struct WidgetLayout {
    /// Space between a container's children, across and down, in design pixels: a row puts the first between its children and the second between the lines it wraps onto, a column the second between its children, and a grid the first between its columns and the second between its rows. Below zero on an axis takes the theme's own, which is 8 where it says nothing, and a stated zero puts them edge to edge
    #[property(default = [-1.0, -1.0])]
    pub(crate) gap: [f32; 2],
    /// Where a container puts its children across its own direction, and a `grid` each child in its cell down: `stretch` makes each as wide (or tall) as the container, `baseline` lines up their first lines of text, the others keep each child's own size
    #[property(default = ItemAlign::new(w::STRETCH))]
    pub(crate) align_items: ItemAlign,
    /// How a container spreads its children along its own direction once they have their sizes, and a `grid` its columns: `stretch` grows a grid's `auto` columns into the room, and in a row or a column is `start`
    #[property(default = Justify::new(w::START))]
    pub(crate) justify: Justify,
    /// How a container that wraps spreads its lines across its direction, and a `grid` its rows, where they leave room: `stretch` shares the room among them
    #[property(default = ContentAlign::new(w::STRETCH))]
    pub(crate) align_content: ContentAlign,
    /// Whether a container's children run onto more lines when they do not fit: `auto` wraps a `flow` and keeps every other container on one line; `balance` evens the lines out over at least `min_lines`; a `_reverse` word stacks the lines the other way
    #[property(default = Wrapping::new(w::AUTO))]
    pub(crate) wrap_children: Wrapping,
    /// How many lines a wrapping container divides its children between at least: `balance` evens them over this many, and any wrap shares a known cross size among them
    #[property(default = 1, min = 1)]
    pub(crate) min_lines: i64,
    /// Which way a row and a grid's columns run: `right_to_left` starts them at the right edge
    #[property(default = Direction::new(w::direction::LEFT_TO_RIGHT))]
    pub(crate) direction: Direction,
    /// Lay a `row`, `column` or `flow`'s children out from its far end
    pub(crate) reverse: bool,
    /// How wide a grab the seams between this container's children get, in design pixels; 0 leaves them fixed. A drag writes the new size onto the neighbour that states one
    #[property(min = 0.0)]
    pub(crate) splitter_width: f32,
}

impl Beside for WidgetLayout {
    fn reads(kind: &str) -> bool {
        lays_out(kind)
    }

    puts! { held, widget;
        gap => widget.gap = held.gap;
        align_items => widget.align = held.align_items.0.clone();
        justify => widget.justify = held.justify.0.clone();
        align_content => widget.layout.align_content = held.align_content.0.clone();
        wrap_children => widget.layout.wrap_children = held.wrap_children.0.clone();
        min_lines => widget.layout.min_lines =
            held.min_lines.clamp(1, i64::from(u16::MAX)) as u16;
        direction => widget.layout.direction = held.direction.0.clone();
        reverse => widget.reverse = held.reverse;
        splitter_width => widget.handle = held.splitter_width;
    }
}

/// The rarer words about a box, which any kind may carry: what its size measures, what it keeps for what runs past it, and how it lays out on its own.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_box")]
pub(crate) struct WidgetBox {
    /// What `width` and `height` measure: `border`, the whole box, or `content`, the box inside `padding` and `border`
    #[property(default = Sizing::new(w::sizing::BORDER))]
    pub(crate) box_sizing: Sizing,
    /// What the layout keeps for content past this widget's box: `scroll` keeps `scrollbar_width` clear inside its edge, the others nothing. Layout only: a container never gives a child a floor of its own content here (`min_width` is that floor), a `scroll` kind is what scrolls, and each kind clips as it draws
    #[property(default = Overflow::new(w::overflow::VISIBLE))]
    pub(crate) overflow: Overflow,
    /// The room a box with `overflow = "scroll"` keeps clear inside its edge for a bar, in design pixels
    #[property(min = 0.0)]
    pub(crate) scrollbar_width: f32,
    /// Containment: `layout` or `paint` makes the box lay out on its own, and `layout` also gives it no baseline for a parent's `baseline` alignment
    #[property(flags)]
    pub(crate) contain: Vec<Containment>,
    /// Where `align_items`, `align_self`, `align_content` or `justify` would push a child past its container's start, put it at the start instead, so the start stays in view
    pub(crate) safe_align: bool,
}

impl Beside for WidgetBox {
    fn reads(_: &str) -> bool {
        true
    }

    fn brought_by(_: &str) -> bool {
        false
    }

    puts! { held, widget;
        box_sizing => widget.layout.box_sizing = held.box_sizing.0.clone();
        overflow => widget.layout.overflow = held.overflow.0.clone();
        scrollbar_width => widget.layout.scrollbar_width =
            crate::widget::node::Bits::of(held.scrollbar_width);
        contain => widget.layout.contain = containment(&held.contain);
        safe_align => widget.layout.safe_align = held.safe_align;
    }
}

/// Whether `contain` names layout containment, then paint containment.
fn containment(named: &[Containment]) -> [bool; 2] {
    [w::contain::LAYOUT, w::contain::PAINT].map(|word| named.iter().any(|held| held.0 == word))
}

/// The rows a `grid` reads: its tracks and its named areas.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_grid")]
pub(crate) struct WidgetGrid {
    /// A `grid`'s columns, left to right and space-separated: design pixels (`120`), a share of what is left (`1fr`), a percentage (`25%`), `auto`, `min_content`, `max_content`, `fit_content(120)`, `minmax(80, 1fr)`, and `repeat(3, 1fr)`, whose count may be `auto_fill` or `auto_fit`. Empty is two equal columns
    pub(crate) grid_columns: SmolStr,
    /// A `grid`'s rows, top to bottom, in the words `grid_columns` takes; empty makes each row as tall as what is in it
    pub(crate) grid_rows: SmolStr,
    /// A `grid`'s named areas, one string a row of space-separated names with `.` for a cell in none: `["head head", "side main"]`. A child names one in `row` and `column`
    pub(crate) areas: Vec<SmolStr>,
    /// The order a `grid` fills cells with the children that name none: along each row, or down each column; a `_dense` order goes back to fill a hole a bigger child left
    #[property(default = Flow::new(w::flow::ROW))]
    pub(crate) auto_flow: Flow,
    /// The size of each column a `grid` adds past `grid_columns`, for a child placed beyond them or flowing down under `auto_flow = "column"`, in the words `grid_columns` takes without `repeat`; several take turns. Empty is `auto`
    pub(crate) auto_columns: SmolStr,
    /// The size of each row a `grid` adds past `grid_rows`, in the words `auto_columns` takes; empty is `auto`
    pub(crate) auto_rows: SmolStr,
}

impl Beside for WidgetGrid {
    fn reads(kind: &str) -> bool {
        kind == w::GRID
    }

    /// A track or an area that does not read.
    fn check(&self) -> Result<()> {
        grid::check_tracks(
            Self::NAME,
            crate::vocabulary::keys::GRID_COLUMNS,
            &self.grid_columns,
        )?;
        grid::check_tracks(
            Self::NAME,
            crate::vocabulary::keys::GRID_ROWS,
            &self.grid_rows,
        )?;
        grid::check_auto(
            Self::NAME,
            crate::vocabulary::keys::AUTO_COLUMNS,
            &self.auto_columns,
        )?;
        grid::check_auto(
            Self::NAME,
            crate::vocabulary::keys::AUTO_ROWS,
            &self.auto_rows,
        )?;
        grid::check_areas(Self::NAME, crate::vocabulary::keys::AREAS, &self.areas)
    }

    puts! { held, widget;
        grid_columns => widget.layout.grid_columns = held.grid_columns.clone();
        grid_rows => widget.layout.grid_rows = held.grid_rows.clone();
        areas => widget.layout.areas.clone_from(&held.areas);
        auto_flow => widget.layout.auto_flow = held.auto_flow.0.clone();
        auto_columns => widget.layout.auto_columns = held.auto_columns.clone();
        auto_rows => widget.layout.auto_rows = held.auto_rows.clone();
    }
}

/// What a child says to the parent it sits in, whatever its own kind: a `grid`'s cell, a `fold`'s header, a `menu`'s row.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_child")]
pub(crate) struct WidgetChild {
    /// On a `grid`'s child: the rows it covers, as a line counted from 1 (`2`, or from the end below zero), a first and last line (`1 / 3`), a span (`span 2`, `2 / span 2`), or the name of one of the grid's `areas`. Empty puts it in the next free cell
    pub(crate) row: SmolStr,
    /// On a `grid`'s child: the columns it covers, in the words `row` takes
    pub(crate) column: SmolStr,
    /// On a `fold`'s child: drawn in the fold's header after its arrow and caption, as Godot's title bar control is
    pub(crate) title_bar: bool,
    /// On a `menu`'s row: leave the menu open when clicked, as a toggle does; any other row closes it
    pub(crate) keep_open: bool,
}

impl Beside for WidgetChild {
    fn reads(_: &str) -> bool {
        true
    }

    fn brought_by(_: &str) -> bool {
        false
    }

    /// A line or a span that does not read.
    fn check(&self) -> Result<()> {
        grid::check_placement(Self::NAME, crate::vocabulary::keys::ROW, &self.row)?;
        grid::check_placement(Self::NAME, crate::vocabulary::keys::COLUMN, &self.column)
    }

    puts! { held, widget;
        row => widget.layout.row = held.row.clone();
        column => widget.layout.column = held.column.clone();
        title_bar => widget.title_bar = held.title_bar;
        keep_open => widget.keep_open = held.keep_open;
    }
}

/// The row a `fold` reads: whether it shows what is under its header.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_fold")]
pub(crate) struct WidgetFold {
    /// Whether a `fold` shows its children; its header flips it and calls `on_change` with the new state
    #[property(default = true)]
    pub(crate) open: bool,
}

impl Beside for WidgetFold {
    fn reads(kind: &str) -> bool {
        kind == w::FOLD
    }

    puts! { held, widget;
        open => widget.open = held.open;
    }
}

/// The row a `tabs` reads: which child it shows.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_tabs")]
pub(crate) struct WidgetTabs {
    /// Which child a `tabs` shows, by node name; empty shows the first. A click on the strip writes it and calls `on_change` with the page's name
    pub(crate) current_page: SmolStr,
}

impl Beside for WidgetTabs {
    fn reads(kind: &str) -> bool {
        kind == w::TABS
    }

    puts! { held, widget;
        current_page => widget.active = held.current_page.clone();
    }
}
