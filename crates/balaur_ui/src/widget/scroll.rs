//! A finger dragging a `scroll`: the deadzone before it scrolls, and the
//! fling that carries on after it lifts.

use std::cell::Cell;

use balaur_core::Engine;
use balaur_core::hecs::Entity;

thread_local! {
    /// What this pass's drags read once: whether a finger drives the screen
    /// (or the mouse stands in for one), and the project's deadzone.
    static PASS: Cell<(bool, f32)> = const { Cell::new((false, 0.0)) };
}

/// Read once a pass, after its classes: whether a `drag_scroll = "touch"`
/// scroll drags, which is the screen's `touch` class or `[input]
/// emulate_touch_from_mouse` (the mouse standing in for a finger, as
/// Godot's does, so a desktop tries a phone's scrolling), and `[ui]
/// scroll_deadzone`, which under a finger is also how far a tap may wobble.
/// egui's own `OnTouch` waits for touch events egui is never sent, so
/// balaur decides it.
pub(crate) fn begin_pass(eng: &Engine, ctx: &egui::Context) {
    let number = |path: &str| {
        balaur_core::settings::get(eng, path).and_then(|value| {
            value
                .as_float()
                .or_else(|| value.as_integer().map(|whole| whole as f64))
        })
    };
    let emulated = balaur_core::settings::get(eng, "input/emulate_touch_from_mouse")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let finger = crate::widget::theme::pass_is_touch() || emulated;
    let dead = number("ui/scroll_deadzone").unwrap_or(0.0).max(0.0) as f32;
    PASS.with(|held| held.set((finger, dead)));
    // Under a finger a tap that wobbles less than the deadzone is still a
    // tap, as a phone's touch slop is; a mouse keeps egui's own six.
    let slop = if finger {
        dead.max(CLICK_SLOP)
    } else {
        CLICK_SLOP
    };
    ctx.options_mut(|options| {
        if (options.input_options.max_click_dist - slop).abs() > f32::EPSILON {
            options.input_options.max_click_dist = slop;
        }
    });
}

/// The widget a box whose `overflow` is `scroll` reads its scroll rows from:
/// itself once they were put on it, else a copy carrying their defaults. A
/// container is built without them (`Beside::defaults`), and one that never
/// was given them has no `axis`: only `widget_scroll` writes a container's.
pub(crate) fn rows_of(
    widget: &crate::widget::node::Widget,
) -> std::borrow::Cow<'_, crate::widget::node::Widget> {
    use crate::widget::beside::Beside;
    if !widget.axis.is_empty() {
        return std::borrow::Cow::Borrowed(widget);
    }
    let mut copy = widget.clone();
    crate::widget::beside::WidgetScroll::default().put(&mut copy);
    std::borrow::Cow::Owned(copy)
}

/// Whether dragging a scroll's content scrolls it, by its `drag_scroll`:
/// `always`, `never`, else under a finger.
pub(crate) fn drags(mode: &str) -> bool {
    use crate::vocabulary::words as w;
    match mode {
        w::ALWAYS => true,
        w::NEVER => false,
        _ => PASS.with(Cell::get).0,
    }
}

/// How far a drag travels before a scroll moves: its own `scroll_deadzone`,
/// else the project's.
pub(crate) fn deadzone(stated: f32) -> f32 {
    if stated > 0.0 {
        stated
    } else {
        PASS.with(Cell::get).1
    }
}

/// Below this a lift is a tap that happened to move, not a flick, and the
/// list should stop where the finger left it. Points per second.
const FLING_MIN: f32 = 120.0;
/// How fast a flick runs out, as a proportion kept per second. Time-based, so
/// the same flick throws the same distance at any frame rate.
const FLING_DECAY: f32 = 0.0025;
/// How far a press may move and still click, egui's own default, in points.
const CLICK_SLOP: f32 = 6.0;

/// The offset a finger past a scroll's deadzone asks for, or `None` while
/// nothing is dragging that far. The press is remembered with the offset
/// the scroll had then, so the content follows the finger from there.
///
/// A lift hands over to a fling: the list keeps the speed the finger had and
/// slows to a stop. Godot's `ScrollContainer` does this, and a phone feels
/// broken without it. `axes` are the ones the area scrolls along: a finger
/// moving across the other one moves nothing.
pub(crate) fn deadzone_drag(
    ui: &egui::Ui,
    eng: &Engine,
    entity: Entity,
    (dead, axes): (f32, [bool; 2]),
) -> Option<egui::Vec2> {
    let along = egui::vec2(f32::from(u8::from(axes[0])), f32::from(u8::from(axes[1])));
    let (down, origin, latest, dt) = ui.input(|i| {
        (
            i.pointer.primary_down(),
            i.pointer.press_origin(),
            i.pointer.latest_pos(),
            i.stable_dt,
        )
    });
    let state = eng.resource::<crate::UiState>();
    let mut state = state.borrow_mut();
    let key = entity.to_bits().get();
    if !down {
        // The finger left: carry its speed over, then let it run down.
        if let Some(drag) = state.scroll_drags.remove(&key)
            && drag.velocity.length() >= FLING_MIN
        {
            let offset = (drag.base - (drag.last - drag.from)).max(egui::Vec2::ZERO);
            state.scroll_flings.insert(key, (drag.velocity, offset));
        }
        return fling(&mut state, key, dt);
    }
    // A finger back on the list stops it where it is, which is what everyone
    // expects of a list still coasting.
    state.scroll_flings.remove(&key);
    // A drag a child took (a slider, the bar's handle) is never the list's;
    // one a box round the list took, as egui's modal takes every drag, is.
    let around = crate::widget::arrange::drawn_at(entity);
    let taken = ui.ctx().dragged_id().is_some_and(|id| {
        ui.ctx()
            .read_response(id)
            .is_none_or(|held| around.is_none_or(|list| !held.rect.contains_rect(list)))
    });
    if taken {
        state.scroll_drags.remove(&key);
        return None;
    }
    let (origin, latest) = (origin?, latest?);
    if let std::collections::hash_map::Entry::Vacant(slot) = state.scroll_drags.entry(key) {
        let inside =
            crate::widget::arrange::drawn_at(entity).is_some_and(|rect| rect.contains(origin));
        if !inside {
            return None;
        }
        let id = ui.make_persistent_id(("balaur-scroll", entity));
        let offset = egui::scroll_area::State::load(ui.ctx(), id)
            .map_or(egui::Vec2::ZERO, |s| s.clamped_offset());
        slot.insert(crate::ScrollDrag {
            from: origin,
            base: offset,
            last: origin,
            velocity: egui::Vec2::ZERO,
        });
    }
    let drag = state.scroll_drags.get_mut(&key)?;
    if dt > 0.0 {
        // Half the new reading, half the old: enough smoothing that one bad
        // frame cannot throw the list across the screen.
        let instant = (latest - drag.last) * along / dt;
        drag.velocity = (drag.velocity + instant) / 2.0;
    }
    drag.last = latest;
    let (start, base) = (drag.from, drag.base);
    let travelled = (latest - start) * along;
    if travelled.length() < dead {
        return None;
    }
    Some((base - travelled).max(egui::Vec2::ZERO))
}

/// One frame of a list still coasting, or `None` once it has stopped.
fn fling(state: &mut crate::UiState, key: u64, dt: f32) -> Option<egui::Vec2> {
    let (velocity, offset) = state.scroll_flings.get_mut(&key)?;
    *offset = (*offset - *velocity * dt).max(egui::Vec2::ZERO);
    *velocity *= libm::powf(FLING_DECAY, dt);
    let out = *offset;
    if velocity.length() < FLING_MIN / 4.0 {
        state.scroll_flings.remove(&key);
    }
    Some(out)
}

/// A scroll area dressed with what the widget states: when its bars show,
/// whether it sticks to its end, how the wheel and a drag move it, and the
/// offset a scene or a script sent it to.
///
/// The offset is sent only when the widget's differs from the last one this
/// pass saw or reported: the reader's own scrolling comes back as that same
/// number a tick later, and must not be sent again over a newer one.
pub(crate) fn dressed(
    ui: &egui::Ui,
    mut area: egui::ScrollArea,
    widget: &crate::widget::node::Widget,
    entity: Entity,
) -> egui::ScrollArea {
    use crate::vocabulary::words as w;
    use egui::scroll_area::{DragScroll, ScrollBarVisibility, ScrollSource};
    let o = &widget.egui;
    area = area
        .scroll_bar_visibility(match o.scrollbar.as_str() {
            w::ALWAYS => ScrollBarVisibility::AlwaysVisible,
            w::NEVER => ScrollBarVisibility::AlwaysHidden,
            _ => ScrollBarVisibility::VisibleWhenNeeded,
        })
        .animated(o.animated)
        .wheel_scroll_multiplier(egui::vec2(o.wheel_speed[0], o.wheel_speed[1]))
        .scroll_source(ScrollSource {
            scroll_bar: true,
            drag: if drags(&o.drag_scroll) {
                DragScroll::Always
            } else {
                DragScroll::Never
            },
            mouse_wheel: o.wheel_scroll,
        });
    if o.stick_to_end {
        area = area.stick_to_bottom(true).stick_to_right(true);
    }
    if o.min_scrolled_width > 0.0 {
        area = area.min_scrolled_width(o.min_scrolled_width);
    }
    if o.min_scrolled_height > 0.0 {
        area = area.min_scrolled_height(o.min_scrolled_height);
    }
    if let Some(icon) = crate::widget::layer::pointer_icon(&o.drag_cursor) {
        area = area.on_drag_cursor(icon);
    }
    let said = said_id(entity);
    let seen = ui.data(|d| d.get_temp::<[f32; 2]>(said));
    let asked = o.scroll_offset;
    ui.data_mut(|d| d.insert_temp(said, asked));
    if seen == Some(asked) {
        return area;
    }
    if asked[0] >= 0.0 {
        area = area.horizontal_scroll_offset(asked[0]);
    }
    if asked[1] >= 0.0 {
        area = area.vertical_scroll_offset(asked[1]);
    }
    area
}

/// Where the widget's scroll area is now, reported as a `Scrolled` edit when
/// it moved since the last pass; the number the widget will hold once the
/// edit lands is noted, so [`dressed`] does not send it back.
pub(crate) fn report(
    ui: &egui::Ui,
    edits: &mut Vec<(Entity, crate::widget::layer::Edit)>,
    entity: Entity,
    offset: egui::Vec2,
) {
    let last = egui::Id::unique(("balaur-scrolled", entity));
    let moved = ui
        .data(|d| d.get_temp::<egui::Vec2>(last))
        .is_some_and(|was| was != offset);
    ui.data_mut(|d| d.insert_temp(last, offset));
    if moved {
        edits.push((
            entity,
            crate::widget::layer::Edit::Scrolled([offset.x, offset.y]),
        ));
        ui.data_mut(|d| d.insert_temp(said_id(entity), [offset.x, offset.y]));
    }
}

fn said_id(entity: Entity) -> egui::Id {
    egui::Id::unique(("balaur-scroll-said", entity))
}
