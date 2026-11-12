//! The components beside `widget` about what a control holds and how it
//! dresses itself. No property shows on a condition (PLAN-property-types §5).

use std::sync::Arc;

use balaur_core::components::derived::Component;
use balaur_text::vocabulary::words as tw;
use smol_str::SmolStr;

use crate::vocabulary::words as w;
use crate::widget::beside::{Beside, egui, puts};
use crate::widget::node::Widget;
use crate::widget::properties::{Item, LineBreak, Sense, TextAlign, TruncateAt};
use crate::widget::text_look::{TextLook, color_of};

/// The rows a `button` or `menu` reads: what its face holds beside its caption, and what the pointer may do to it.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_button")]
pub(crate) struct WidgetButton {
    /// A glyph from the theme's icon family, drawn before `text`
    pub(crate) icon: SmolStr,
    /// What that glyph is tinted with, as `#rrggbb` or a name from the theme's `[colors]`; empty takes the role's own
    pub(crate) icon_color: SmolStr,
    /// That glyph's size in design pixels; below zero takes the caption's
    #[property(default = -1.0)]
    pub(crate) icon_size: f32,
    /// The project-relative picture drawn before the caption at the caption's height
    pub(crate) image: SmolStr,
    /// The space between its picture, icon and caption, in design pixels; below zero takes the theme's own, else half the font size
    #[property(default = -1.0)]
    pub(crate) face_gap: f32,
    /// Text drawn against its far edge, dimmer than its caption: a shortcut, or a menu's caret
    pub(crate) trailing: SmolStr,
    /// What the pointer may do to it: `click` reports a press let go as a click; under `click_and_drag` a press that moves past egui's drag distance is a drag and no click; `drag` and `hover` never click
    #[property(default = Sense::new(w::CLICK))]
    pub(crate) sense: Sense,
}

impl Beside for WidgetButton {
    fn reads(kind: &str) -> bool {
        matches!(kind, w::BUTTON | w::MENU)
    }

    puts! { held, widget;
        icon => widget.icon = held.icon.clone();
        icon_color => widget.icon_color = held.icon_color.clone();
        icon_size => widget.icon_size = held.icon_size;
        image => widget.source = held.image.clone();
        face_gap => widget.face_gap = held.face_gap;
        trailing => widget.trailing = held.trailing.clone();
        sense => egui(widget).sense = held.sense.0.clone();
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

/// The rows a kind drawing a caption reads: how its lines sit, break and are cut, and what the reader may do to them.
#[allow(
    clippy::struct_excessive_bools,
    reason = "a property a field: each switch the component offers is one"
)]
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_text")]
pub(crate) struct WidgetText {
    /// Where text sits in the width the widget was given
    #[property(default = TextAlign::new(w::START))]
    pub(crate) text_align: TextAlign,
    /// Read inline marks in the text: `[b]`, `[i]`, `[color=#hex]`, `[center]`, `[right]`, `[wave amp=N freq=N]` and `[img=path width=N]`; off, brackets are text
    pub(crate) markup: bool,
    /// Break text to the width the widget was given instead of running past it on one line; a `dropdown` breaks its picked text
    pub(crate) wrap: bool,
    /// Cut a caption too long for the width the widget was given and end it with an ellipsis; with neither this nor `wrap` a button clips it at its edge. A `dropdown` cuts its picked text
    pub(crate) truncate: bool,
    /// Which part of a cut caption the ellipsis stands in for
    #[property(default = TruncateAt::new(tw::END))]
    pub(crate) truncate_at: TruncateAt,
    /// Show a `label`'s whole text when the pointer rests on it and `truncate` cut it short
    #[property(default = true)]
    pub(crate) show_tooltip_when_elided: bool,
    /// The most lines a wrapped label keeps; zero keeps every line
    #[property(min = 0)]
    pub(crate) max_lines: i64,
    /// Where a wrapped line may break
    #[property(default = LineBreak::new(tw::WORD_OR_GLYPH))]
    pub(crate) line_break: LineBreak,
    /// Spaces between tab stops
    #[property(default = 8, min = 1, max = 64)]
    pub(crate) tab_width: i64,
    /// Let a drag over this label select its text, and the platform's copy key take it
    pub(crate) selectable: bool,
    /// Script method called with the target of a `[url=target]` span in `markup` text that was clicked, on this node or the nearest ancestor whose script declares it
    pub(crate) on_link: SmolStr,
    /// A wash behind the text of the kinds egui draws, a `checkbox`, `dropdown`, `menu`, `tabs`, `panel` and the like, where a `label` and a `button` shape their own; alpha 0 is none
    #[property(color, default = [0.0, 0.0, 0.0, 0.0])]
    pub(crate) text_background: [f32; 4],
}

impl Beside for WidgetText {
    fn reads(kind: &str) -> bool {
        matches!(
            kind,
            w::LABEL
                | w::BUTTON
                | w::MENU
                | w::DROPDOWN
                | w::CHECKBOX
                | w::TABS
                | w::PANEL
                | w::DIALOG
                | w::PROGRESS_BAR
        )
    }

    fn brought_by(kind: &str) -> bool {
        matches!(kind, w::LABEL | w::BUTTON | w::MENU | w::DROPDOWN)
    }

    puts! { held, widget;
        text_align => widget.text_align = held.text_align.0.clone();
        markup => widget.markup = held.markup;
        wrap => widget.wrap = held.wrap;
        truncate => widget.truncate = held.truncate;
        truncate_at => look(widget).options.truncate_at =
            balaur_text::TruncateAt::of(&held.truncate_at.0);
        show_tooltip_when_elided => egui(widget).show_tooltip_when_elided =
            held.show_tooltip_when_elided;
        max_lines => look(widget).options.max_lines =
            u32::try_from(held.max_lines.max(0)).unwrap_or(u32::MAX);
        line_break => look(widget).options.line_break =
            balaur_text::LineBreak::of(&held.line_break.0);
        tab_width => look(widget).options.tab_width = tab_stops(held.tab_width);
        selectable => widget.selectable = held.selectable;
        on_link => widget.on_link = held.on_link.clone();
        text_background => look(widget).background = color_of(held.text_background);
    }
}

/// The text look of a widget, to write one: a copy where the arena still
/// shares it.
pub(crate) fn look(widget: &mut Widget) -> &mut TextLook {
    Arc::make_mut(&mut widget.text_look)
}

/// The spaces between tab stops a row names, held to what the shaper counts.
pub(crate) fn tab_stops(spaces: i64) -> u16 {
    spaces.clamp(1, 64) as u16
}

/// The rows of a chord that clicks a widget, which any kind may carry.
#[derive(Clone, PartialEq, Component)]
#[component(name = "widget_shortcut")]
pub(crate) struct WidgetShortcut {
    /// A chord that clicks this widget wherever it is, as `cmd+shift+s` or `f5`; a menu row fires while its menu is shut, and draws the chord against its far edge unless it says its own `trailing`
    pub(crate) shortcut: SmolStr,
    /// Whether `shortcut` fires while a text field holds the keyboard. A chord without cmd, ctrl or alt never does unless it is a function key, since that is typing; off, no chord does, for one the field has a use for itself, as `cmd+z`
    #[property(default = true)]
    pub(crate) while_typing: bool,
    /// Whether `shortcut` takes its key ahead of a field being typed into, a plain key included, so the field never sees it: a completion list's arrows, Enter and Escape while it is up
    pub(crate) takes_typing: bool,
}

impl Beside for WidgetShortcut {
    fn reads(_: &str) -> bool {
        true
    }

    fn brought_by(_: &str) -> bool {
        false
    }

    puts! { held, widget;
        shortcut => widget.shortcut = held.shortcut.clone();
        while_typing => widget.while_typing = held.while_typing;
        takes_typing => widget.takes_typing = held.takes_typing;
    }
}
