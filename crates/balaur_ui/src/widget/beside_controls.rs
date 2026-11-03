//! The components beside `widget` about what a control holds and how it
//! dresses itself. No property shows on a condition (PLAN-property-types §5).

use balaur_core::components::derived::Component;

use crate::vocabulary::words as w;
use crate::widget::beside::{Beside, puts};

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
