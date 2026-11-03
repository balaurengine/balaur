//! Components beside `widget` holding the rows only some of its kinds read:
//! a `code` widget's gutter, an `inspector`'s target, a `window`'s chrome.
//! No property shows on a condition (PLAN-property-types §5), so a row that
//! applies to one kind is a component of its own on the same node.
//!
//! Each puts its rows on the [`Widget`] the layer reads, the node's own where
//! the widget's kind reads them and its defaults otherwise, so a widget that
//! never names one draws as its kind always did. A write or a removal of one
//! redoes those fields; a rebuild of the widget reads them all again.

use std::sync::{Arc, OnceLock};

use anyhow::Result;
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{Component, Fields, Written, to_table};
use balaur_core::hecs::{Entity, World};
use balaur_core::warnings::Warning;
use balaur_plugin::Registry;
use rustc_hash::FxHashMap;
use smol_str::SmolStr;

use crate::vocabulary::words as w;
use crate::widget::node::{EguiOptions, Inspecting, Widget};
use crate::widget::properties::{
    Alpha, Clamp, CloseOn, Cursor, DragScroll, Fit, Handle, NumberFormat, Numeral, Placement,
    Scrollbar, SidePlacement, WidgetProperties,
};

/// A component beside `widget`, read by the kinds `reads` names.
pub(crate) trait Beside: Component + Clone {
    /// Whether a widget of this kind reads the rows.
    fn reads(kind: &str) -> bool;

    /// Whether a preset of this kind brings the component. Every kind may
    /// carry one any kind reads, and a node names it where it asks for a row.
    fn brought_by(kind: &str) -> bool {
        Self::reads(kind)
    }

    /// The rows onto the widget the layer reads.
    fn put(&self, widget: &mut Widget);

    /// One row onto the widget, by its name, leaving the others; `false` for
    /// a name it does not hold.
    fn put_row(&self, widget: &mut Widget, row: &str) -> bool;

    /// Refuse what the types alone cannot judge.
    fn check(&self) -> Result<()> {
        Ok(())
    }
}

/// Each row's place in the [`Widget`], written once for both ways it is
/// filled: [`Beside::put`] whole, [`Beside::put_row`] the one a write changed.
macro_rules! puts {
    ($held:ident, $widget:ident; $($row:ident => $write:expr;)*) => {
        fn put(&self, $widget: &mut Widget) {
            let $held = self;
            $($write;)*
        }

        fn put_row(&self, $widget: &mut Widget, row: &str) -> bool {
            let $held = self;
            match row {
                $(stringify!($row) => $write,)*
                _ => return false,
            }
            true
        }
    };
}

/// The rows a `code` widget reads: what it highlights and its gutter.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_code")]
pub(crate) struct WidgetCode {
    /// The language a `code` widget highlights
    pub(crate) language: SmolStr,
    /// The lines a `code` widget dots in its gutter, counting from 1; whole numbers or the text of them. A click on the gutter reports its line through `on_gutter` and the script decides what the mark means
    pub(crate) breakpoints: Vec<Numeral>,
    /// The lines a `code` widget underlines as errors, counting from 1, each also marked on the inner edge of its gutter
    pub(crate) problems: Vec<Numeral>,
    /// The lines a `code` widget underlines as warnings, counting from 1; an error on the same line outranks it
    pub(crate) warnings: Vec<Numeral>,
    /// The line a `code` widget fills across its whole width, counting from 1, for the row a debugger is stopped on; 0 fills none
    #[property(min = 0)]
    pub(crate) current_line: i64,
    /// How wide a `code` widget's gutter is, in design pixels; 0 takes the built-in width, which holds four digits
    #[property(min = 0.0)]
    pub(crate) gutter_width: f32,
    /// Script method called with the line a click on a `code` widget's gutter landed on, on this node or the nearest ancestor whose script declares it
    pub(crate) on_gutter: SmolStr,
}

impl Beside for WidgetCode {
    fn reads(kind: &str) -> bool {
        kind == w::CODE
    }

    puts! { held, widget;
        language => widget.source = held.language.clone();
        breakpoints => widget.breakpoints = lines(&held.breakpoints);
        problems => widget.problems = lines(&held.problems);
        warnings => widget.warnings = lines(&held.warnings);
        current_line => widget.current_line =
            u32::try_from(held.current_line.max(0)).unwrap_or(u32::MAX);
        gutter_width => widget.gutter_width = held.gutter_width;
        on_gutter => widget.on_gutter = held.on_gutter.clone();
    }
}

/// The rows an `inspector` reads: the node and component it shows.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_inspector")]
pub(crate) struct WidgetInspector {
    /// The component an `inspector` shows a row of each property of, read off the registry's schema and the node every pass. The widget's `options` names the properties and their order; empty shows them all, sorted. `on_change` hears `{ property, path, value }` for an edit, `path` being the steps into a list, map or record, and `verb` in place of `value` where an asset is to be embedded or saved
    pub(crate) component: SmolStr,
    /// The node an `inspector` shows, by path: as `scene.get_node` takes it, or as `node.path()` spells it
    pub(crate) target: SmolStr,
    /// The folder an `inspector` shows a file's path from: a value under it loses that much of its front
    pub(crate) relative_to: SmolStr,
    /// How wide an `inspector`'s label column is, in design pixels; 0 takes a third of its width. Under `wrap` each label goes above its control instead
    #[property(min = 0.0)]
    pub(crate) label_width: f32,
}

impl Beside for WidgetInspector {
    fn reads(kind: &str) -> bool {
        kind == w::INSPECTOR
    }

    puts! { held, widget;
        component => inspecting(widget).component = held.component.clone();
        target => inspecting(widget).target = held.target.clone();
        relative_to => inspecting(widget).relative_to = held.relative_to.clone();
        label_width => inspecting(widget).label_width = held.label_width;
    }
}

/// The rows a `table` reads: its columns and how they sort.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_table")]
pub(crate) struct WidgetTable {
    /// A `table`'s column names, in order, and with them how many columns it has: a name ending in `>` draws its column against the right edge, which is what a column of numbers wants. None takes the first row as the names
    pub(crate) titles: Vec<SmolStr>,
    /// Each column's share of the width, in the order `titles` names them: `["2", "1", "1"]` gives the first half and the other two a quarter each. Numbers and the text of them both; empty divides the width evenly, and a drag on a seam in the header writes the shares back
    pub(crate) widths: Vec<Numeral>,
    /// The column the rows are ordered by, by the name in `titles`; empty leaves them in the order they were given. A cell that starts with a number sorts as one, so `12 KB` follows `3 KB`. The widget's `reverse` takes them the other way round
    pub(crate) sort: SmolStr,
    /// Let a click on the header sort by that column, and the next click on the same one turn it round; the column sorted by carries a caret
    pub(crate) sortable: bool,
}

impl Beside for WidgetTable {
    fn reads(kind: &str) -> bool {
        kind == w::TABLE
    }

    puts! { held, widget;
        titles => widget.titles.clone_from(&held.titles);
        widths => widget.widths = held.widths.iter().filter_map(Numeral::share).collect();
        sort => widget.sort = held.sort.clone();
        sortable => widget.sortable = held.sortable;
    }
}

/// The rows a `window` or a `dialog` reads: its chrome, and what shuts it.
#[allow(
    clippy::struct_excessive_bools,
    reason = "a property a field: each switch the component offers is one"
)]
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_window")]
pub(crate) struct WidgetWindow {
    /// Give a `window`'s title bar an arrow that folds the window to its bar and back
    pub(crate) collapsible: bool,
    /// Whether a `collapsible` window starts unfolded
    #[property(default = true)]
    pub(crate) default_open: bool,
    /// Draw a `window`'s cross
    #[property(default = true)]
    pub(crate) closable: bool,
    /// Whether a `window`'s close button shuts it; off, the button only emits `close_request` and the script decides
    #[property(default = true)]
    pub(crate) hide_on_close: bool,
    /// Give a `window` a grip in its bottom-right corner that a drag resizes it by, writing `width` and `height` within `min_width`, `min_height`, `max_width` and `max_height`
    pub(crate) resizable: bool,
    /// Let Escape or a click on the dim shut a `dialog`; off, only the scene does
    #[property(default = true)]
    pub(crate) dismissable: bool,
    /// What a `dialog` dims the screen behind it with
    #[property(color, default = [0.0, 0.0, 0.0, 0.549])]
    pub(crate) backdrop_color: [f32; 4],
}

impl Beside for WidgetWindow {
    fn reads(kind: &str) -> bool {
        matches!(kind, w::WINDOW | w::DIALOG)
    }

    puts! { held, widget;
        collapsible => egui(widget).collapsible = held.collapsible;
        default_open => egui(widget).default_open = held.default_open;
        closable => egui(widget).closable = held.closable;
        hide_on_close => widget.hide_on_close = held.hide_on_close;
        resizable => egui(widget).resizable = held.resizable;
        dismissable => egui(widget).dismissable = held.dismissable;
        backdrop_color => egui(widget).backdrop_color = held.backdrop_color;
    }
}

/// The row a `toast` reads: how long it stays.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_toast")]
pub(crate) struct WidgetToast {
    /// How long a `toast` stays, in seconds, counting the half second it fades over; zero leaves it up until the game takes it away
    #[property(default = 3.0, min = 0.0)]
    pub(crate) duration: f32,
}

impl Beside for WidgetToast {
    fn reads(kind: &str) -> bool {
        kind == w::TOAST
    }

    puts! { held, widget;
        duration => widget.duration = held.duration;
    }
}

/// The rows a `separator` reads: the room it takes and how far its line runs.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_separator")]
pub(crate) struct WidgetSeparator {
    /// The room a `separator` takes across its line, in design pixels, the line drawn down its middle; below zero is 6
    #[property(default = -1.0)]
    pub(crate) spacing: f32,
    /// How far a `separator`'s line runs past each end of the room it was given, in design pixels; below zero stops it short
    pub(crate) overhang: f32,
}

impl Beside for WidgetSeparator {
    fn reads(kind: &str) -> bool {
        kind == w::SEPARATOR
    }

    puts! { held, widget;
        spacing => egui(widget).spacing = held.spacing;
        overhang => egui(widget).overhang = held.overhang;
    }
}

/// The rows a `menu` reads: where its popup opens and what shuts it.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_menu")]
pub(crate) struct WidgetMenu {
    /// Where a `menu` opens: under its button, above it, to its right or to its left, each from the side's start (`below` starts at the button's left edge, `right` at its top) or at `_center` or `_end`; at the pointer; or centred on the screen
    #[property(default = Placement::new(w::BELOW))]
    pub(crate) placement: Placement,
    /// The placements a `menu`'s popup tries, in order, where its own does not fit the screen; empty tries egui's, and naming the placement alone keeps it where it is
    pub(crate) placement_fallbacks: Vec<SidePlacement>,
    /// The space between a `menu`'s button and its popup, in design pixels; below zero takes egui's
    #[property(default = -1.0)]
    pub(crate) popup_gap: f32,
    /// How wide a `menu`'s popup starts, in design pixels; 0 sizes it to its rows
    #[property(min = 0.0)]
    pub(crate) popup_width: f32,
    /// What shuts a `menu`'s popup besides a row: a click outside it, any click at all, or nothing but Escape
    #[property(default = CloseOn::new(w::CLICK_OUTSIDE))]
    pub(crate) close_on: CloseOn,
    /// Holds a menu's rows up from the scene, as a click would; for an offscreen run or a tutorial, since nothing can click there
    pub(crate) showing: bool,
}

impl Beside for WidgetMenu {
    fn reads(kind: &str) -> bool {
        kind == w::MENU
    }

    puts! { held, widget;
        placement => widget.placement = held.placement.0.clone();
        placement_fallbacks => egui(widget).placement_fallbacks =
            held.placement_fallbacks.iter().map(|side| side.0.clone()).collect();
        popup_gap => egui(widget).popup_gap = held.popup_gap;
        popup_width => egui(widget).popup_width = held.popup_width;
        close_on => egui(widget).close_on = held.close_on.0.clone();
        showing => widget.showing = held.showing;
    }
}

/// The rows a `color_picker` reads: the colour it holds and what it offers.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_color_picker")]
pub(crate) struct WidgetColorPicker {
    /// What a `color_picker` holds; `on_change` hears the new one
    #[property(color, default = [1.0, 1.0, 1.0, 1.0])]
    pub(crate) picked_color: [f32; 4],
    /// What a `color_picker` offers for alpha: `none` keeps the colour opaque, `blend` offers transparency, `additive` that and additive blending
    #[property(default = Alpha::new(w::BLEND))]
    pub(crate) alpha: Alpha,
    /// Draw a `color_picker`'s picker in place, rather than behind a swatch that opens it
    pub(crate) inline: bool,
}

impl Beside for WidgetColorPicker {
    fn reads(kind: &str) -> bool {
        kind == w::COLOR_PICKER
    }

    puts! { held, widget;
        picked_color => widget.color = held.picked_color;
        alpha => egui(widget).alpha = held.alpha.0.clone();
        inline => egui(widget).inline = held.inline;
    }
}

/// The rows a `slider`, `number_field` or `progress_bar` reads: where it stands in its range, how it moves and how it is drawn.
#[allow(
    clippy::struct_excessive_bools,
    reason = "a property a field: each switch the component offers is one"
)]
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_range")]
pub(crate) struct WidgetRange {
    /// Where a `slider`, `number_field` or `progress_bar` stands, between `min` and `max`; a slider and a drag value write it and call `on_change` with it
    pub(crate) value: f32,
    /// The low end of a `slider` or `progress_bar`; a `number_field` runs free while this pair is the default 0 and 1
    pub(crate) min: f32,
    /// The high end of a `slider` or `progress_bar`; a `number_field` runs free while this pair is the default 0 and 1
    #[property(default = 1.0)]
    pub(crate) max: f32,
    /// The grid a `slider` snaps to, and how far a `number_field`'s `arrows` move it; 0 is continuous, and 1 for the arrows
    #[property(min = 0.0)]
    pub(crate) step: f32,
    /// How many decimals a `slider`'s or `number_field`'s number shows; below zero lets egui decide
    #[property(default = -1)]
    pub(crate) decimals: i64,
    /// Whether a `slider` holds its number inside `min` and `max`: `always`, one the scene set outside them included; `edits`, only what the reader enters; `never`, only the track
    #[property(default = Clamp::new(w::ALWAYS))]
    pub(crate) clamp: Clamp,
    /// Pull a `number_field`'s number back inside `min` and `max` where the scene set it outside them; off, it is shown as set and only an edit is bounded
    pub(crate) clamp_existing: bool,
    /// Space a `slider`'s track logarithmically, for a range across orders of magnitude
    pub(crate) logarithmic: bool,
    /// The smallest number above zero a `logarithmic` slider reaches, for a range that starts at or crosses zero; 0 takes egui's millionth
    #[property(min = 0.0)]
    pub(crate) smallest_positive: f32,
    /// The largest number a `logarithmic` slider reaches short of an unbounded end; 0 is no limit
    #[property(min = 0.0)]
    pub(crate) largest_finite: f32,
    /// Round a dragged `slider` to the simplest number near the pointer
    #[property(default = true)]
    pub(crate) smart_aim: bool,
    /// Draw a `slider`'s number beside its track, for the reader to drag or type into
    pub(crate) show_value: bool,
    /// Draw a `progress_bar`'s fill as a percentage on it, in place of its `text`
    pub(crate) show_percentage: bool,
    /// The base a `slider` or `number_field` writes its number in; the others than `decimal` write it whole
    #[property(default = NumberFormat::new(w::DECIMAL))]
    pub(crate) number_format: NumberFormat,
    /// How far a `number_field`, or a `slider`'s shown number, moves per design pixel dragged; 0 takes egui's, which is 1 on a number field and scales with the range on a slider
    #[property(min = 0.0)]
    pub(crate) drag_speed: f32,
    /// Draw a step up and a step down beside a `number_field`, each moving it by `step` within `min` and `max`
    pub(crate) arrows: bool,
    /// Write a `slider`'s or `number_field`'s number as the reader types it; off, only once the typing is left
    #[property(default = true)]
    pub(crate) update_while_editing: bool,
    /// A `slider`'s handle: a `circle`, or a `rect` `handle_aspect` times as wide as the track is tall
    #[property(default = Handle::new(w::CIRCLE))]
    pub(crate) handle: Handle,
    /// How wide a `rect` handle is against its height
    #[property(default = 0.5, min = 0.0)]
    pub(crate) handle_aspect: f32,
    /// Fill a `slider`'s track from its start up to the handle
    pub(crate) trailing_fill: bool,
    /// Shimmer a `progress_bar` that is short of full, with a spinner at its end unless it states a `corner_radius`. It repaints every frame while it is on screen and short of full, and never otherwise
    pub(crate) animate: bool,
}

impl Beside for WidgetRange {
    fn reads(kind: &str) -> bool {
        matches!(kind, w::SLIDER | w::NUMBER_FIELD | w::PROGRESS_BAR)
    }

    puts! { held, widget;
        value => widget.value = held.value;
        min => widget.min = held.min;
        max => widget.max = held.max;
        step => widget.step = held.step;
        decimals => egui(widget).decimals = i32::try_from(held.decimals)
            .unwrap_or(if held.decimals < 0 { i32::MIN } else { i32::MAX });
        clamp => egui(widget).clamp = held.clamp.0.clone();
        clamp_existing => egui(widget).clamp_existing = held.clamp_existing;
        logarithmic => egui(widget).logarithmic = held.logarithmic;
        smallest_positive => egui(widget).smallest_positive = held.smallest_positive;
        largest_finite => egui(widget).largest_finite = held.largest_finite;
        smart_aim => egui(widget).smart_aim = held.smart_aim;
        show_value => egui(widget).show_value = held.show_value;
        show_percentage => egui(widget).show_percentage = held.show_percentage;
        number_format => egui(widget).number_format = held.number_format.0.clone();
        drag_speed => egui(widget).drag_speed = held.drag_speed;
        arrows => widget.arrows = held.arrows;
        update_while_editing => egui(widget).update_while_editing = held.update_while_editing;
        handle => egui(widget).handle = held.handle.0.clone();
        handle_aspect => egui(widget).handle_aspect = held.handle_aspect;
        trailing_fill => egui(widget).trailing_fill = held.trailing_fill;
        animate => egui(widget).animate = held.animate;
    }
}

/// The rows a `text_field`, `text_area` or `code` reads: what may be typed and how the editor behaves.
#[allow(
    clippy::struct_excessive_bools,
    reason = "a property a field: each switch the component offers is one"
)]
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_field")]
pub(crate) struct WidgetField {
    /// What a `text_field` or `text_area` shows while it is empty
    pub(crate) placeholder: SmolStr,
    /// The most characters a `text_field` takes; 0 is no limit
    #[property(min = 0.0)]
    pub(crate) max_length: f32,
    /// Keep a `text_field` to digits, a sign and a point
    pub(crate) numeric: bool,
    /// Draw a `text_field`'s text as dots, for a password
    pub(crate) secret: bool,
    /// Let the reader change a `text_field`, `text_area` or `code`'s text; off, it can be neither edited nor selected
    #[property(default = true)]
    pub(crate) editable: bool,
    /// Cut a `text_field`'s text at its edge; off, the field grows to show it whole
    #[property(default = true)]
    pub(crate) clip_text: bool,
    /// Put the caret at the end of a `text_field`, `text_area` or `code`'s text when it first takes focus; off, at the start
    #[property(default = true)]
    pub(crate) caret_at_end: bool,
    /// Let Tab type a tab into a `text_field`, `text_area` or `code` rather than move focus on
    pub(crate) tab_inserts: bool,
    /// The chord that submits a `text_field`, as `enter` or `cmd+enter`; empty is Enter. A `text_area` submits when focus leaves it whatever this says
    pub(crate) submit_key: SmolStr,
}

impl Beside for WidgetField {
    fn reads(kind: &str) -> bool {
        matches!(kind, w::TEXT_FIELD | w::TEXT_AREA | w::CODE)
    }

    /// A `submit_key` that is not a chord.
    fn check(&self) -> Result<()> {
        let chord = &self.submit_key;
        if !chord.is_empty() && crate::chord::chord(chord).is_none() {
            anyhow::bail!(
                "widget_field: `submit_key` `{chord}` is not a chord, as `enter` or `cmd+enter`"
            );
        }
        Ok(())
    }

    puts! { held, widget;
        placeholder => widget.placeholder = held.placeholder.clone();
        max_length => widget.max_length = held.max_length;
        numeric => widget.numeric = held.numeric;
        secret => widget.secret = held.secret;
        editable => egui(widget).editable = held.editable;
        clip_text => egui(widget).clip_text = held.clip_text;
        caret_at_end => egui(widget).caret_at_end = held.caret_at_end;
        tab_inserts => egui(widget).tab_inserts = held.tab_inserts;
        submit_key => egui(widget).submit_key = held.submit_key.clone();
    }
}

/// The rows a `scroll`, `list`, `tree` or `table` reads: its bars, where it is scrolled to and what scrolls it.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_scroll")]
pub(crate) struct WidgetScroll {
    /// When a `scroll`, `list`, `tree` or `table` has its bars: while its content runs past it, always, or never. The bars float, so one shows once the pointer is over the box
    #[property(default = Scrollbar::new(w::AUTO))]
    pub(crate) scrollbar: Scrollbar,
    /// Where a `scroll`, `list`, `tree` or `table` is scrolled to, across and down, in design pixels: written as the reader scrolls, and a new one written by the scene or a script scrolls it there. Below zero on an axis leaves that axis where it is; `ui.scroll` takes it too
    #[property(default = [-1.0, -1.0])]
    pub(crate) scroll_offset: [f32; 2],
    /// Keep a `scroll`, `list`, `tree` or `table` at its end as content arrives, until the reader scrolls away from it; `ui.scroll` takes it too
    pub(crate) stick_to_end: bool,
    /// Ease a `scroll`, `list`, `tree` or `table` to where it is sent, such as a row scrolled into view, rather than jump
    #[property(default = true)]
    pub(crate) animated: bool,
    /// Let the mouse wheel scroll a `scroll`, `list`, `tree` or `table`
    #[property(default = true)]
    pub(crate) wheel_scroll: bool,
    /// How far a mouse wheel scrolls a `scroll`, `list`, `tree` or `table`, across and down, as a multiple of egui's own
    #[property(default = [1.0, 1.0])]
    pub(crate) wheel_speed: [f32; 2],
    /// When dragging a `scroll`, `list`, `tree` or `table`'s content scrolls it: on a touch screen, always, or never; a `scroll_deadzone` takes the drag over itself
    #[property(default = DragScroll::new(w::TOUCH))]
    pub(crate) drag_scroll: DragScroll,
    /// The pointer's shape while a drag scrolls a `scroll`, `list`, `tree` or `table`'s content; `arrow` leaves it as it is
    #[property(default = Cursor::new(w::cursor::ARROW))]
    pub(crate) drag_cursor: Cursor,
    /// How far a finger drags a `scroll` before it scrolls, in design pixels, so a tap on a child still lands; 0 scrolls at once
    #[property(min = 0.0)]
    pub(crate) scroll_deadzone: f32,
    /// The narrowest a `scroll`, `list`, `tree` or `table` draws while its content runs past it sideways, in design pixels; 0 takes egui's
    #[property(min = 0.0)]
    pub(crate) min_scrolled_width: f32,
    /// The shortest a `scroll`, `list`, `tree` or `table` draws while its content runs past it downwards, in design pixels; 0 takes egui's
    #[property(min = 0.0)]
    pub(crate) min_scrolled_height: f32,
}

impl Beside for WidgetScroll {
    fn reads(kind: &str) -> bool {
        matches!(kind, w::SCROLL | w::LIST | w::TREE | w::TABLE)
    }

    puts! { held, widget;
        scrollbar => egui(widget).scrollbar = held.scrollbar.0.clone();
        scroll_offset => egui(widget).scroll_offset = held.scroll_offset;
        stick_to_end => egui(widget).stick_to_end = held.stick_to_end;
        animated => egui(widget).animated = held.animated;
        wheel_scroll => egui(widget).wheel_scroll = held.wheel_scroll;
        wheel_speed => egui(widget).wheel_speed = held.wheel_speed;
        drag_scroll => egui(widget).drag_scroll = held.drag_scroll.0.clone();
        drag_cursor => egui(widget).drag_cursor = held.drag_cursor.0.clone();
        scroll_deadzone => widget.deadzone = held.scroll_deadzone;
        min_scrolled_width => egui(widget).min_scrolled_width = held.min_scrolled_width;
        min_scrolled_height => egui(widget).min_scrolled_height = held.min_scrolled_height;
    }
}

/// The rows an `image` reads: the part of the picture drawn and how it sits in its box.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_image")]
pub(crate) struct WidgetImage {
    /// What an `image` is multiplied by; white draws it as it is
    #[property(color, default = [1.0, 1.0, 1.0, 1.0])]
    pub(crate) tint: [f32; 4],
    /// How an `image` sits in the box it was given: `contain` and `cover` keep its shape, `fill` stretches, `none` leaves it its own size, centred. Empty lets the picture decide the box instead
    pub(crate) fit: Fit,
    /// The part of an `image` drawn, as left, top, width and height in the picture's own pixels, which is how an atlas shows one tile; a zero width or height draws the whole picture. `ui.image` takes it too
    #[property(default = [0.0, 0.0, 0.0, 0.0])]
    pub(crate) region: [f32; 4],
    /// How far an `image` is turned clockwise about `angle_origin`, in degrees; a turned image keeps square corners
    pub(crate) angle_degrees: f32,
    /// The point an `image` turns about, as fractions of its box from the top left: `[0.5, 0.5]` is its middle
    #[property(default = [0.5, 0.5])]
    pub(crate) angle_origin: [f32; 2],
    /// Text an `image` draws in place of a picture that will not load
    pub(crate) alt_text: SmolStr,
}

impl Beside for WidgetImage {
    fn reads(kind: &str) -> bool {
        kind == w::IMAGE
    }

    puts! { held, widget;
        tint => egui(widget).tint = held.tint;
        fit => widget.fit = held.fit.0.clone();
        region => egui(widget).region = held.region;
        angle_degrees => egui(widget).angle_degrees = held.angle_degrees;
        angle_origin => egui(widget).angle_origin = held.angle_origin;
        alt_text => egui(widget).alt_text = held.alt_text.clone();
    }
}

/// The rows a `list` or `tree` reads: its card faces, and what a drag or a mark on a row does.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_list")]
pub(crate) struct WidgetList {
    /// The picture a `list` cuts its card faces from
    pub(crate) sheet: SmolStr,
    /// Let a drag carry a card of a `list` with `grid_columns` out of it, drawn under the pointer; `on_drop` says where it was let go
    pub(crate) draggable: bool,
    /// Let a drag move a row of a `list` or a `tree`. The kind moves nothing itself: it draws where the row would land and calls `on_move`, and the rows are the script's to reorder
    pub(crate) reorderable: bool,
    /// Script method called with the card a drag let go outside the list, on this node or the nearest ancestor whose script declares it; the pointer is where it landed
    pub(crate) on_drop: SmolStr,
    /// Script method called when a dragged row is dropped, with the row moved, the row it landed on, and `before`, `after` or `into`, on this node or the nearest ancestor whose script declares it
    pub(crate) on_move: SmolStr,
    /// Script method called with `{ row, mark }` when a mark at the right of a `list` or `tree` row is clicked, on this node or the nearest ancestor whose script declares it. The click picks nothing
    pub(crate) on_mark: SmolStr,
}

impl Beside for WidgetList {
    fn reads(kind: &str) -> bool {
        matches!(kind, w::LIST | w::TREE)
    }

    puts! { held, widget;
        sheet => widget.source = held.sheet.clone();
        draggable => widget.draggable = held.draggable;
        reorderable => widget.reorderable = held.reorderable;
        on_drop => widget.on_drop = held.on_drop.clone();
        on_move => widget.on_move = held.on_move.clone();
        on_mark => widget.on_mark = held.on_mark.clone();
    }
}

/// The row a `dropdown` reads: how tall its list grows.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_dropdown")]
pub(crate) struct WidgetDropdown {
    /// The tallest a `dropdown`'s list grows before it scrolls, in design pixels; 0 takes egui's
    #[property(min = 0.0)]
    pub(crate) list_height: f32,
}

impl Beside for WidgetDropdown {
    fn reads(kind: &str) -> bool {
        kind == w::DROPDOWN
    }

    puts! { held, widget;
        list_height => egui(widget).list_height = held.list_height;
    }
}

/// Every component beside `widget`, in the order a scene names them.
macro_rules! each_beside {
    ($apply:ident!($($args:tt)*)) => {
        $apply!(
            $($args)*;
            WidgetCode,
            WidgetInspector,
            WidgetTable,
            WidgetWindow,
            WidgetToast,
            WidgetSeparator,
            WidgetMenu,
            WidgetColorPicker,
            WidgetRange,
            WidgetField,
            WidgetScroll,
            WidgetImage,
            WidgetList,
            WidgetDropdown
        )
    };
}

/// Every component beside `widget`, by name.
pub(crate) fn names() -> &'static [&'static str] {
    macro_rules! name_each {
        (; $($beside:ty),*) => {
            &[$(<$beside as Component>::NAME),*]
        };
    }
    each_beside!(name_each!())
}

/// The components beside `widget` a preset of `kind` gives the node, by name.
pub(crate) fn brought_by(kind: &str) -> Vec<&'static str> {
    macro_rules! bring_each {
        ($kind:expr; $($beside:ty),*) => {
            [$((<$beside>::brought_by($kind), <$beside as Component>::NAME)),*]
        };
    }
    each_beside!(bring_each!(kind))
        .into_iter()
        .filter_map(|(brought, name)| brought.then_some(name))
        .collect()
}

/// A component beside `widget` holding a row, and the kinds it holds it for.
pub(crate) struct Owner {
    pub(crate) name: &'static str,
    pub(crate) reads: fn(&str) -> bool,
}

/// Every component beside `widget` holding the row `key`: two where their
/// kinds are apart, as a `scroll`'s and a `slider`'s `axis`. A pooled control
/// asks for each row of its table, so the answer is looked up, not searched.
pub(crate) fn owners_of(key: &str) -> &'static [Owner] {
    static OWNERS: OnceLock<FxHashMap<&'static str, Vec<Owner>>> = OnceLock::new();
    OWNERS
        .get_or_init(|| {
            let mut owners: FxHashMap<&'static str, Vec<Owner>> = FxHashMap::default();
            macro_rules! own_each {
                ($owners:expr; $($beside:ty),*) => {
                    $(for row in <$beside as Fields>::NAMES {
                        $owners.entry(*row).or_default().push(Owner {
                            name: <$beside as Component>::NAME,
                            reads: <$beside>::reads,
                        });
                    })*
                };
            }
            each_beside!(own_each!(owners));
            owners
        })
        .get(key)
        .map_or(&[][..], Vec::as_slice)
}

/// The component holding the row `key` for a widget of `kind`: the one of its
/// owners that kind reads, else the first, which then says it is idle there.
pub(crate) fn owner_for<'a>(owners: &'a [Owner], kind: &str) -> Option<&'a Owner> {
    owners
        .iter()
        .find(|owner| (owner.reads)(kind))
        .or_else(|| owners.first())
}

/// Refuse a row of a screen class's table that no component a widget of
/// `kind` reads could take, or a value the row would refuse.
pub(crate) fn check_class_row(kind: &str, key: &str, value: &toml::Value) -> Result<()> {
    macro_rules! try_each {
        ($key:expr, $value:expr, $kind:expr; $($beside:ty),*) => {
            $(if <$beside>::reads($kind)
                && let Some(index) = <$beside as Fields>::NAMES.iter().position(|row| *row == $key)
            {
                return <$beside>::default().write(index, $value);
            })*
        };
    }
    each_beside!(try_each!(key, value, kind));
    match owners_of(key).first() {
        Some(owner) => anyhow::bail!("a `{kind}` widget reads nothing of `{}`", owner.name),
        None => anyhow::bail!("is not a widget property"),
    }
}

/// Write the rows of the screen class tables in force that a component beside
/// `widget` holds, over what the node holds, and put them on `widget`.
pub(crate) fn put_class_rows(
    world: &World,
    entity: Entity,
    kind: &str,
    rows: &[(&str, &toml::Value)],
    widget: &mut Widget,
) {
    macro_rules! class_each {
        ($world:expr, $entity:expr, $kind:expr, $rows:expr, $widget:expr; $($beside:ty),*) => {
            $(class_rows_of::<$beside>($world, $entity, $kind, $rows, $widget);)*
        };
    }
    each_beside!(class_each!(world, entity, kind, rows, widget));
}

fn class_rows_of<C: Beside>(
    world: &World,
    entity: Entity,
    kind: &str,
    rows: &[(&str, &toml::Value)],
    widget: &mut Widget,
) {
    if !C::reads(kind) {
        return;
    }
    let mut classed: Option<C> = None;
    for (key, value) in rows {
        let Some(index) = C::NAMES.iter().position(|row| row == key) else {
            continue;
        };
        let held = classed.get_or_insert_with(|| {
            world
                .get::<&C>(entity)
                .map(|held| (*held).clone())
                .unwrap_or_default()
        });
        // A value the row refuses leaves what the node holds.
        let _ = held.write(index, value);
    }
    if let Some(classed) = classed {
        classed.put(widget);
    }
}

/// Put every beside component the widget's kind reads on `widget`: the
/// node's own, or its defaults where the node holds none.
pub(crate) fn put_all(
    world: &World,
    entity: Entity,
    asked: &WidgetProperties,
    widget: &mut Widget,
) {
    macro_rules! put_each {
        ($world:expr, $entity:expr, $asked:expr, $widget:expr; $($beside:ty),*) => {
            $(put_held::<$beside>($world, $entity, $asked, $widget);)*
        };
    }
    each_beside!(put_each!(world, entity, asked, widget));
}

/// `C` on `widget` where its kind reads it.
fn put_held<C: Beside>(
    world: &World,
    entity: Entity,
    asked: &WidgetProperties,
    widget: &mut Widget,
) {
    if !C::reads(&asked.kind.0) {
        return;
    }
    match world.get::<&C>(entity) {
        Ok(held) => held.put(widget),
        Err(_) => C::default().put(widget),
    }
}

/// Write `rows` of `C` on the node with `change`, the component at its
/// defaults where the node holds none yet, and the widget's fields after
/// them: an edit the reader made, kept so a save keeps it. Nothing where the
/// widget's kind does not read `C`: a script's `ui.edit` may hand any widget
/// a value.
pub(crate) fn edit<C: Beside>(
    eng: &Engine,
    entity: Entity,
    rows: &[&str],
    change: impl FnOnce(&mut C),
) {
    let added = {
        let world = eng.world();
        let Ok(mut widget) = world.get::<&mut Widget>(entity) else {
            return;
        };
        if !C::reads(&widget.kind) {
            return;
        }
        if let Ok(mut held) = world.get::<&mut C>(entity) {
            change(&mut held);
            for row in rows {
                let placed = held.put_row(&mut widget, row);
                debug_assert!(placed, "`{row}` is a row of `{}`", C::NAME);
            }
            None
        } else {
            let mut held = C::default();
            change(&mut held);
            Some(to_table(&held))
        }
    };
    match added {
        // Through the registry, so the node says it carries it and a save
        // keeps it; the write puts it on the widget.
        Some(table) => {
            let _ = balaur_core::components::add(eng, entity, C::NAME, Some(&table));
        }
        None => crate::widget::arena::widget_changed(entity),
    }
}

/// Register every component beside `widget`.
pub(crate) fn register_all(reg: &mut Registry<'_>) {
    macro_rules! register_each {
        ($reg:expr; $($beside:ty),*) => {
            $(register::<$beside>($reg);)*
        };
    }
    each_beside!(register_each!(reg));
}

/// Register `C` beside `widget`, which it expects on the node: a write puts
/// its rows on the widget, and a removal puts its defaults back.
fn register<C: Beside>(reg: &mut Registry<'_>) {
    reg.register_component(
        C::NAME,
        ComponentDef {
            tags: &[balaur_core::components::tag::UI],
            expects: &[<WidgetProperties as Component>::NAME],
            warnings: Some(Box::new(idle::<C>)),
            remove: Box::new(|eng, entity| {
                let _ = eng.remove_one::<C>(entity);
                refresh::<C>(eng, entity, None);
                Ok(())
            }),
            ..ComponentDef::written_by::<C>(Some(written::<C>))
        },
    );
}

fn written<C: Beside>(eng: &Engine, entity: Entity, written: Written) -> Result<()> {
    let held = eng.world().get::<&C>(entity).map(|held| held.check());
    if let Ok(Err(why)) = held {
        return Err(why);
    }
    match written {
        Written::Property(index) => refresh::<C>(eng, entity, C::NAMES.get(index).copied()),
        Written::Whole => refresh::<C>(eng, entity, None),
    }
    Ok(())
}

/// Put `row` of `C` on the node's widget again, or every row where it names
/// none, the defaults where the component is gone, and tell the arena and an
/// inspector it changed.
fn refresh<C: Beside>(eng: &Engine, entity: Entity, row: Option<&str>) {
    let inspector = {
        let world = eng.world();
        let Ok(mut widget) = world.get::<&mut Widget>(entity) else {
            return;
        };
        if C::reads(&widget.kind) {
            match (world.get::<&C>(entity), row) {
                (Ok(held), Some(row)) => {
                    held.put_row(&mut widget, row);
                }
                (Ok(held), None) => held.put(&mut widget),
                (Err(_), _) => C::default().put(&mut widget),
            }
        }
        widget.kind == w::INSPECTOR
    };
    crate::widget::schema::told(eng, entity, inspector);
}

/// What is off about `C` on a widget whose kind reads none of its rows.
fn idle<C: Beside>(eng: &Engine, entity: Entity) -> Vec<Warning> {
    let world = eng.world();
    let Ok(asked) = world.get::<&WidgetProperties>(entity) else {
        return Vec::new();
    };
    if C::reads(&asked.kind.0) {
        return Vec::new();
    }
    vec![Warning::whole(format!(
        "a `{}` widget reads nothing of `{}`",
        asked.kind.0,
        C::NAME
    ))]
}

/// The egui options of a widget, to write one: a copy where the arena still
/// shares them.
pub(crate) fn egui(widget: &mut Widget) -> &mut EguiOptions {
    Arc::make_mut(&mut widget.egui)
}

/// What an `inspector` shows, to write one: a copy where the arena still
/// shares it.
fn inspecting(widget: &mut Widget) -> &mut Inspecting {
    Arc::make_mut(&mut widget.inspect)
}

/// The lines a code widget's gutter property names, counting from 1.
fn lines(texts: &[Numeral]) -> Vec<u32> {
    texts.iter().filter_map(Numeral::line).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::schema::{changed_value, widget_of};

    /// Each row written alone leaves the widget as putting the component
    /// whole does: a row the table forgot would be left at its field's zero.
    fn each_row_fills_what_a_whole_put_fills<C: Beside>(kind: &str) {
        let asked = WidgetProperties {
            kind: crate::widget::properties::Kind::new(kind),
            ..WidgetProperties::default()
        };
        for (index, row) in C::NAMES.iter().enumerate() {
            let mut held = C::default();
            let mut alone = widget_of(&asked);
            C::default().put(&mut alone);
            let changed = changed_value::<C>(&held, index);
            held.write(index, &changed).expect("the value fits the row");
            assert!(
                held.put_row(&mut alone, row),
                "`{row}` of `{}` has no place",
                C::NAME
            );
            let mut whole = widget_of(&asked);
            held.put(&mut whole);
            assert_eq!(
                format!("{alone:?}"),
                format!("{whole:?}"),
                "`{row}` of `{}` written alone is not what a whole put leaves",
                C::NAME
            );
        }
    }

    #[test]
    fn a_row_written_alone_fills_what_a_whole_put_fills() {
        macro_rules! check_each {
            (; $($beside:ty),*) => {
                $(each_row_fills_what_a_whole_put_fills::<$beside>(
                    w::WIDGET_KINDS
                        .iter()
                        .find(|kind| <$beside>::reads(kind))
                        .expect("some kind reads every component"),
                );)*
            };
        }
        each_beside!(check_each!());
    }

    /// A pooled control's table says nothing of which component holds a row,
    /// so no kind may read two that hold the same one.
    #[test]
    fn no_kind_reads_two_components_holding_one_row() {
        macro_rules! rows_of_each {
            (; $($beside:ty),*) => {
                [$(<$beside as Fields>::NAMES),*]
            };
        }
        for rows in each_beside!(rows_of_each!()) {
            for row in rows {
                for kind in w::WIDGET_KINDS {
                    let readers: Vec<&str> = owners_of(row)
                        .iter()
                        .filter(|owner| (owner.reads)(kind))
                        .map(|owner| owner.name)
                        .collect();
                    assert!(
                        readers.len() <= 1,
                        "a `{kind}` reads `{row}` from each of {readers:?}"
                    );
                }
            }
        }
    }

    /// No component holds a row of `widget` itself, or a pooled control's
    /// table could not say which it meant.
    #[test]
    fn no_row_beside_the_widget_is_a_row_of_the_widget() {
        for row in WidgetProperties::NAMES {
            assert!(
                owners_of(row).is_empty(),
                "`{row}` is on `widget` and on `{}`",
                owners_of(row)[0].name
            );
        }
    }
}
