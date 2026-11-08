//! The widget kinds as presets: what the node picker offers to add.

use anyhow::Result;
use balaur_plugin::Registry;

use crate::vocabulary::{keys, words};

/// The widget kinds as recipes, so the picker offers "Column" rather than
/// "a `widget`, then set `kind`".
///
/// Presets, not node types: balaur has no classes, and one for UI alone would
/// be a second model of what a node is (`balaur_core::presets`).
pub(crate) fn register_widget_presets(reg: &mut Registry<'_>) -> Result<()> {
    use balaur_core::presets::preset;
    let recipes = [
        (words::LABEL, "A line of text"),
        (words::TEXT_FIELD, "A line the player types into"),
        (words::BUTTON, "Text that reports its clicks"),
        (words::PANEL, "A framed box that lays out what is inside it"),
        (
            words::ROW,
            "Children side by side, sharing the leftover by `grow`",
        ),
        (
            words::COLUMN,
            "Children stacked, sharing the leftover by `grow`",
        ),
        (
            words::SCROLL,
            "A box that holds its size and clips what runs past it",
        ),
        (
            words::STACK,
            "Children over one another, each placed in the box by its `anchor`",
        ),
        (
            words::TABS,
            "One child showing, the rest named on a strip above it",
        ),
        (
            words::IMAGE,
            "A picture from the project, sized by itself or by what it states",
        ),
        (words::CHECKBOX, "A box that ticks"),
        (words::DROPDOWN, "One of its `options`, picked from a list"),
        (words::SLIDER, "A number dragged between `min` and `max`"),
        (
            words::PROGRESS_BAR,
            "A bar filled to `value` between `min` and `max`",
        ),
        (
            words::GRID,
            "Children in the columns `grid_columns` names, two equal ones where it names none",
        ),
        (
            words::FLOW,
            "Children left to right, wrapping when the row is full",
        ),
        (words::FOLD, "A header that shows or hides what is under it"),
        (
            words::TOAST,
            "A message that stacks at its anchor and leaves when its `duration` is up",
        ),
        (
            words::DIALOG,
            "A panel over everything, with the screen behind it dimmed and deaf",
        ),
        (words::SEPARATOR, "A line between siblings"),
        (
            words::WINDOW,
            "A panel with a title bar, moved by dragging it and shut by its cross",
        ),
    ];
    for (name, description) in recipes {
        // A window is dragged by its title bar; no other root moves unasked.
        let params = if name == words::WINDOW {
            format!("{} = \"{name}\"\n{} = true", keys::KIND, keys::MOVABLE)
        } else {
            format!("{} = \"{name}\"", keys::KIND)
        };
        // The kind's own rows come with it, so the inspector shows them.
        let mut parts = vec![("widget", Some(params.as_str()))];
        parts.extend(
            crate::widget::beside::brought_by(name)
                .into_iter()
                .map(|beside| (beside, None)),
        );
        reg.register_preset(
            name,
            preset(description, &[balaur_core::components::tag::UI], &parts)?,
        );
    }
    Ok(())
}
