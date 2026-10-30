//! Keyboard focus across the widget layer: which widgets are stops, where
//! Tab sends it in order and an arrow across the screen, and what a pass
//! reports when it moves.

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
        if i.key_pressed(Key::Tab) {
            return Some(if i.modifiers.shift {
                Move::Previous
            } else {
                Move::Next
            });
        }
        if i.key_pressed(Key::Enter) || i.key_pressed(Key::Space) {
            return Some(Move::Accept);
        }
        None
    })
}

/// An arrow's way across the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Side {
    Up,
    Down,
    Left,
    Right,
}

/// The arrow pressed this frame, if any.
pub(super) fn keyboard_side(ctx: &egui::Context) -> Option<Side> {
    use egui::Key;
    ctx.input(|i| {
        [
            (Key::ArrowUp, Side::Up),
            (Key::ArrowDown, Side::Down),
            (Key::ArrowLeft, Side::Left),
            (Key::ArrowRight, Side::Right),
        ]
        .into_iter()
        .find(|(key, _)| i.key_pressed(*key))
        .map(|(_, side)| side)
    })
}

/// The stop nearest `from` on `side`: one whose centre lies that way, scored
/// by the gap ahead plus twice the gap across, as a browser's spatial
/// navigation scores it, so the one in line wins over a nearer one aside.
/// A tie goes to the nearer centre, then to the earlier stop.
pub(super) fn toward(
    from: egui::Rect,
    side: Side,
    stops: &[(Entity, egui::Rect)],
) -> Option<Entity> {
    let gap = |a: egui::Rangef, b: egui::Rangef| (a.min - b.max).max(b.min - a.max).max(0.0);
    let score = |to: egui::Rect| {
        let (ahead, across, beyond) = match side {
            Side::Up => (
                from.min.y - to.max.y,
                gap(from.x_range(), to.x_range()),
                to.center().y < from.center().y,
            ),
            Side::Down => (
                to.min.y - from.max.y,
                gap(from.x_range(), to.x_range()),
                to.center().y > from.center().y,
            ),
            Side::Left => (
                from.min.x - to.max.x,
                gap(from.y_range(), to.y_range()),
                to.center().x < from.center().x,
            ),
            Side::Right => (
                to.min.x - from.max.x,
                gap(from.y_range(), to.y_range()),
                to.center().x > from.center().x,
            ),
        };
        beyond.then(|| {
            (
                ahead.max(0.0) + 2.0 * across,
                from.center().distance(to.center()),
            )
        })
    };
    stops
        .iter()
        .filter(|(_, rect)| *rect != from)
        .filter_map(|(entity, rect)| score(*rect).map(|s| (s, *entity)))
        .min_by(|(a, _), (b, _)| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)))
        .map(|(_, entity)| entity)
}

/// Past the last stop on `side`, the first on the far side: a menu is a ring.
/// `from` is moved beyond the opposite edge of every stop, in its own lane,
/// and looked from again.
pub(super) fn around(
    from: egui::Rect,
    side: Side,
    stops: &[(Entity, egui::Rect)],
) -> Option<Entity> {
    let all = stops
        .iter()
        .fold(egui::Rect::NOTHING, |all, (_, rect)| all.union(*rect));
    let shift = match side {
        Side::Down => egui::vec2(0.0, all.min.y - from.max.y - 1.0),
        Side::Up => egui::vec2(0.0, all.max.y - from.min.y + 1.0),
        Side::Right => egui::vec2(all.min.x - from.max.x - 1.0, 0.0),
        Side::Left => egui::vec2(all.max.x - from.min.x + 1.0, 0.0),
    };
    toward(from.translate(shift), side, stops)
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
    let typing = ctx.egui_wants_keyboard_input();
    shown
        .iter()
        .filter_map(|&index| {
            let widget = &placed[index].widget;
            if widget.disabled {
                return None;
            }
            let (modifiers, key) = crate::chord::chord(&widget.shortcut)?;
            if typing && !fires_while_typing(widget, modifiers, key) {
                return None;
            }
            ctx.input_mut(|input| input.consume_key(modifiers, key))
                .then_some(placed[index].entity)
        })
        .collect()
}

/// Whether a chord still clicks while a field is being typed into: only one
/// the widget allows, and only one that is not itself typing. A plain or
/// shifted key is a character for the field, so `h` cannot hide a node while
/// someone writes a name; a function key and a command chord are not.
fn fires_while_typing(widget: &Widget, modifiers: egui::Modifiers, key: egui::Key) -> bool {
    let commanded = modifiers.command || modifiers.ctrl || modifiers.alt || modifiers.mac_cmd;
    widget.takes_typing || (widget.while_typing && (commanded || is_function_key(key)))
}

fn is_function_key(key: egui::Key) -> bool {
    key.name()
        .strip_prefix('F')
        .is_some_and(|n| n.parse::<u8>().is_ok())
}

/// Move focus, or say which widget an `accept` activated.
///
/// Order is the order the widgets are drawn in, which is the order the scene
/// declares them — so focus walks a menu the way the tree reads.
pub(super) fn advance(
    eng: &Engine,
    stops: &[Entity],
    asked: Option<Move>,
    side: Option<Side>,
) -> Option<Entity> {
    let focus = eng.try_resource::<UiFocus>()?;
    let mut focus = focus.borrow_mut();
    // A focused widget that was hidden, freed or made unfocusable is no
    // longer a place focus can be.
    if focus.focused.is_some_and(|e| !stops.contains(&e)) {
        focus.focused = None;
    }
    let pending = focus.pending.take().or(asked);
    if stops.is_empty() {
        return None;
    }
    let Some(asked) = pending else {
        // An arrow goes across the screen from the stop it is on, by where
        // each stop was drawn last, round to the far side past the last one;
        // from nowhere it starts at an end.
        let side = side?;
        let next = match focus.focused.and_then(crate::widget::arrange::drawn_at) {
            Some(from) => {
                let drawn: Vec<(Entity, egui::Rect)> = stops
                    .iter()
                    .filter_map(|&e| crate::widget::arrange::drawn_at(e).map(|r| (e, r)))
                    .collect();
                toward(from, side, &drawn).or_else(|| around(from, side, &drawn))
            }
            None if matches!(side, Side::Up | Side::Left) => stops.last().copied(),
            None => stops.first().copied(),
        };
        if next.is_some() {
            focus.focused = next;
        }
        return None;
    };
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

#[cfg(test)]
mod tests {
    use super::{Side, around, toward};
    use balaur_core::hecs::Entity;

    /// A three by three grid of 40 by 20 boxes, 10 apart; its entities are
    /// 1 to 9, row by row.
    fn grid() -> Vec<(Entity, egui::Rect)> {
        (0..9_u32)
            .map(|i| {
                let (col, row) = (i % 3, i / 3);
                let min = egui::pos2(col as f32 * 50.0, row as f32 * 30.0);
                let entity = Entity::from_bits(u64::from(i + 1) | (1 << 32)).unwrap();
                (
                    entity,
                    egui::Rect::from_min_size(min, egui::vec2(40.0, 20.0)),
                )
            })
            .collect()
    }

    #[test]
    fn an_arrow_goes_to_the_stop_in_line_on_its_side() {
        let stops = grid();
        let centre = stops[4].1;
        assert_eq!(toward(centre, Side::Up, &stops), Some(stops[1].0));
        assert_eq!(toward(centre, Side::Down, &stops), Some(stops[7].0));
        assert_eq!(toward(centre, Side::Left, &stops), Some(stops[3].0));
        assert_eq!(toward(centre, Side::Right, &stops), Some(stops[5].0));
    }

    #[test]
    fn past_the_edge_an_arrow_goes_nowhere() {
        let stops = grid();
        assert_eq!(toward(stops[0].1, Side::Up, &stops), None);
        assert_eq!(toward(stops[0].1, Side::Left, &stops), None);
        assert_eq!(toward(stops[8].1, Side::Right, &stops), None);
    }

    #[test]
    fn past_the_last_stop_an_arrow_comes_round_in_its_lane() {
        let stops = grid();
        assert_eq!(around(stops[7].1, Side::Down, &stops), Some(stops[1].0));
        assert_eq!(around(stops[1].1, Side::Up, &stops), Some(stops[7].0));
        assert_eq!(around(stops[5].1, Side::Right, &stops), Some(stops[3].0));
        assert_eq!(around(stops[3].1, Side::Left, &stops), Some(stops[5].0));
    }

    /// A wide button over two narrow ones goes down to the nearer one in
    /// line, and a narrow one off to the side still goes up to it.
    #[test]
    fn a_stop_in_line_wins_over_a_nearer_one_aside() {
        let e = |n: u64| Entity::from_bits(n | (1 << 32)).unwrap();
        let wide = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 20.0));
        let left = egui::Rect::from_min_size(egui::pos2(0.0, 40.0), egui::vec2(40.0, 20.0));
        let aside = egui::Rect::from_min_size(egui::pos2(200.0, 25.0), egui::vec2(40.0, 20.0));
        let stops = [(e(1), wide), (e(2), left), (e(3), aside)];
        assert_eq!(toward(wide, Side::Down, &stops), Some(e(2)));
        assert_eq!(toward(left, Side::Up, &stops), Some(e(1)));
    }
}
