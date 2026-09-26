//! Keyboard focus across the widget layer: which widgets are stops, where
//! Tab and the arrows send it, and what a pass reports when it moves.

use balaur_core::Engine;
use balaur_core::hecs::Entity;

use crate::vocabulary::words as w;
use crate::widget::arena::Placed;
use crate::widget::layer::Edit;
use crate::widget::node::{Move, UiFocus, Widget};

/// Whether focus can land on this widget.
///
/// Derived rather than declared: focus exists to activate something, so a
/// widget with nothing to activate is never a stop on the way to one. The
/// `focusable` flag can only take a candidate out, never put one in.
fn takes_focus(widget: &Widget) -> bool {
    widget.visible
        && widget.focusable
        && (matches!(
            widget.kind.as_str(),
            // A line being typed into is where focus lands as much as a
            // button is: Tab reaches it, and a script may put the caret there.
            w::BUTTON | w::CHECKBOX | w::FOLD | w::TEXT_FIELD | w::TEXT_AREA
        ) || !widget.on_click.is_empty())
}

/// What the keyboard asked this frame, if the game has not asked already.
///
/// egui is where the widget layer's input comes from — clicks arrive that way
/// — so keys do too, and no dependency on the input plugin is needed for a
/// menu to work with a keyboard. A gamepad reaches focus through
/// `ui.focus_next()` and friends, wired to actions by whoever assembles the
/// plugins, which is the crate that knows about both.
pub(super) fn keyboard_move(ctx: &egui::Context) -> Option<Move> {
    use egui::Key;
    ctx.input(|i| {
        let shifted_tab = i.key_pressed(Key::Tab) && i.modifiers.shift;
        if i.key_pressed(Key::ArrowUp) || i.key_pressed(Key::ArrowLeft) || shifted_tab {
            return Some(Move::Previous);
        }
        if i.key_pressed(Key::ArrowDown)
            || i.key_pressed(Key::ArrowRight)
            || i.key_pressed(Key::Tab)
        {
            return Some(Move::Next);
        }
        if i.key_pressed(Key::Enter) || i.key_pressed(Key::Space) {
            return Some(Move::Accept);
        }
        None
    })
}

/// Every widget the scene is showing, in the order the draw reaches them.
///
/// Walked from the roots rather than read off the arena: a button under a
/// hidden panel or on a surface the host turned off is never drawn, and an
/// `accept` on it would fire an `on_click` nobody could have seen to ask for.
pub(super) fn reachable(
    placed: &[Placed],
    roots: &[usize],
    on: &dyn Fn(&str) -> bool,
) -> Vec<usize> {
    let mut stops = Vec::new();
    let mut stack: Vec<usize> = roots
        .iter()
        .rev()
        .copied()
        .filter(|&root| on(&placed[root].widget.layer))
        .collect();
    while let Some(index) = stack.pop() {
        let one = &placed[index];
        if !one.widget.visible {
            continue;
        }
        stops.push(index);
        // Reversed, so the stack pops them in declaration order.
        stack.extend(one.children.iter().rev().copied());
    }
    stops
}

/// Where focus may land, of those.
pub(super) fn focus_stops(placed: &[Placed], shown: &[usize]) -> Vec<Entity> {
    shown
        .iter()
        .filter(|&&index| takes_focus(&placed[index].widget))
        .map(|&index| placed[index].entity)
        .collect()
}

/// The widgets whose `shortcut` landed this frame.
///
/// Consumed, so the chord a menu row owns does not also reach a script
/// polling for it. A row of a shut menu answers: that is what a shortcut is
/// for, and the menu never has to be opened to reach the command.
pub(super) fn shortcuts(ctx: &egui::Context, placed: &[Placed], shown: &[usize]) -> Vec<Entity> {
    shown
        .iter()
        .filter_map(|&index| {
            let widget = &placed[index].widget;
            if widget.disabled {
                return None;
            }
            let (modifiers, key) = crate::immediate::chord(&widget.shortcut)?;
            ctx.input_mut(|input| input.consume_key(modifiers, key))
                .then_some(placed[index].entity)
        })
        .collect()
}

/// Move focus, or say which widget an `accept` activated.
///
/// Order is the order the widgets are drawn in, which is the order the scene
/// declares them — so focus walks a menu the way the tree reads.
pub(super) fn advance(eng: &Engine, stops: &[Entity], asked: Option<Move>) -> Option<Entity> {
    let focus = eng.try_resource::<UiFocus>()?;
    let mut focus = focus.borrow_mut();
    // A focused widget that was hidden, freed or made unfocusable is no
    // longer a place focus can be.
    if focus.focused.is_some_and(|e| !stops.contains(&e)) {
        focus.focused = None;
    }
    let asked = focus.pending.take().or(asked)?;
    if stops.is_empty() {
        return None;
    }
    let at = focus
        .focused
        .and_then(|e| stops.iter().position(|s| *s == e));
    match asked {
        Move::Accept => return focus.focused,
        // Wraps, because a menu is a ring: past the last entry is the first.
        Move::Next => {
            let next = at.map_or(0, |i| (i + 1) % stops.len());
            focus.focused = Some(stops[next]);
        }
        Move::Previous => {
            let previous = at.map_or(stops.len() - 1, |i| (i + stops.len() - 1) % stops.len());
            focus.focused = Some(stops[previous]);
        }
    }
    None
}

/// Where focus rests as the draw starts, and whether it was just put there.
/// `taking` is consumed here, so a field takes the caret on the pass after the
/// script asked and never steals it back from whatever the reader clicked.
pub(super) fn focus_before(eng: &Engine) -> (Option<Entity>, bool) {
    let Some(focus) = eng.try_resource::<UiFocus>() else {
        return (None, false);
    };
    let mut focus = focus.borrow_mut();
    (focus.focused, std::mem::take(&mut focus.taking))
}

/// Where focus went this pass: a blur for the widget it left, and the one it
/// arrived at, reported only on the change. Read after the draw, which a
/// clicked field may have moved it in.
pub(super) fn focus_moved(
    eng: &Engine,
    was_focused: Option<Entity>,
    taking: bool,
    edits: &mut Vec<(Entity, Edit)>,
) -> Option<Entity> {
    let focused = eng
        .try_resource::<UiFocus>()
        .and_then(|f| f.borrow().focused);
    if let Some(was) = was_focused.filter(|was| Some(*was) != focused)
        && !edits
            .iter()
            .any(|(e, edit)| *e == was && matches!(edit, Edit::Blurred))
    {
        edits.push((was, Edit::Blurred));
    }
    (taking || focused != was_focused)
        .then_some(focused)
        .flatten()
}
