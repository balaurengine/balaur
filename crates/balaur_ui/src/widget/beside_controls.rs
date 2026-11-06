//! The components beside `widget` about what a control holds and how it
//! dresses itself. No property shows on a condition (PLAN-property-types §5).

use balaur_core::components::derived::Component;
use smol_str::SmolStr;

use crate::vocabulary::words as w;
use crate::widget::beside::{Beside, egui, puts};
use crate::widget::properties::Item;

/// The rows a `button` or `menu` reads: what its face holds beside its caption.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_button")]
pub(crate) struct WidgetButton {
    /// The space between its picture, icon and caption, in design pixels; below zero takes the theme's own, else half the font size
    #[property(default = -1.0)]
    pub(crate) face_gap: f32,
}

impl Beside for WidgetButton {
    fn reads(kind: &str) -> bool {
        matches!(kind, w::BUTTON | w::MENU)
    }

    puts! { held, widget;
        face_gap => widget.face_gap = held.face_gap;
    }
}

/// The rows a `checkbox`, `switch` or `button` reads: whether it is on, and what a click does to that.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_check")]
pub(crate) struct WidgetCheck {
    /// Whether a `checkbox` or `switch` is on, every click flipping it and calling `on_change` with the new state; a checked `button` or `menu` is held down, wearing its pressed look
    pub(crate) checked: bool,
    /// A `button` a click holds down and the next releases, flipping `checked` as a `checkbox` does, before `on_click` runs: Godot's toggle mode
    pub(crate) toggle: bool,
    /// A name this `checkbox`, `switch` or `toggle` button shares with the ones it is exclusive with: ticking one unticks the rest, and one already ticked stays ticked. Empty leaves it flipping on its own
    pub(crate) radio_group: SmolStr,
    /// Draw a `checkbox` as neither ticked nor clear, for a group whose members disagree; a click still flips `checked`
    pub(crate) indeterminate: bool,
}

impl Beside for WidgetCheck {
    fn reads(kind: &str) -> bool {
        matches!(kind, w::CHECKBOX | w::SWITCH | w::BUTTON | w::MENU)
    }

    puts! { held, widget;
        checked => widget.checked = held.checked;
        toggle => widget.toggle = held.toggle;
        radio_group => widget.radio_group = held.radio_group.clone();
        indeterminate => egui(widget).indeterminate = held.indeterminate;
    }
}

/// The rows a `list`, `tree` or `table` reads: what it holds and what is picked.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_rows")]
pub(crate) struct WidgetRows {
    /// The rows a `list`, `tree` or `table` holds. `text` is the key of the one picked, and every row event reports a row by its key
    pub(crate) items: Vec<Item>,
    /// The keys of the rows a `list`, `tree` or `table` has picked, one of them where it holds one. `text` is the key of the last row clicked, which is where a shift range measures from; `on_change` hears the whole list where the widget holds many, and the key where it holds one
    pub(crate) selection: Vec<SmolStr>,
    /// Let a `list`, `tree` or `table` hold more than one row: the platform's command key toggles a row and shift takes the run from the last one clicked
    pub(crate) multi_select: bool,
    /// The pitch of a `list`, `tree` or `table` row, in design pixels; 0 takes the font's own line height
    #[property(min = 0.0)]
    pub(crate) row_height: f32,
    /// Script method called with the row double-clicked in a `list`, `tree` or `table`, on this node or the nearest ancestor whose script declares it
    pub(crate) on_activate: SmolStr,
}

impl Beside for WidgetRows {
    fn reads(kind: &str) -> bool {
        matches!(kind, w::LIST | w::TREE | w::TABLE)
    }

    puts! { held, widget;
        items => widget.items.clone_from(&held.items);
        selection => widget.selection.clone_from(&held.selection);
        multi_select => widget.multi = held.multi_select;
        row_height => widget.row_height = held.row_height;
        on_activate => widget.on_activate = held.on_activate.clone();
    }
}
