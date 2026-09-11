//! A finger dragging a `scroll`: the deadzone before it scrolls, and the
//! fling that carries on after it lifts.

use balaur_core::Engine;
use balaur_core::hecs::Entity;

/// Below this a lift is a tap that happened to move, not a flick, and the
/// list should stop where the finger left it. Points per second.
const FLING_MIN: f32 = 120.0;
/// How fast a flick runs out, as a proportion kept per second. Time-based, so
/// the same flick throws the same distance at any frame rate.
const FLING_DECAY: f32 = 0.0025;

/// The offset a finger past a scroll's deadzone asks for, or `None` while
/// nothing is dragging that far. The press is remembered with the offset
/// the scroll had then, so the content follows the finger from there.
///
/// A lift hands over to a fling: the list keeps the speed the finger had and
/// slows to a stop. Godot's `ScrollContainer` does this, and a phone feels
/// broken without it.
pub(crate) fn deadzone_drag(
    ui: &egui::Ui,
    eng: &Engine,
    entity: Entity,
    dead: f32,
) -> Option<egui::Vec2> {
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
    let (origin, latest) = (origin?, latest?);
    if let std::collections::hash_map::Entry::Vacant(slot) = state.scroll_drags.entry(key) {
        let inside =
            crate::widget_arrange::drawn_at(entity).is_some_and(|rect| rect.contains(origin));
        if !inside {
            return None;
        }
        let id = ui.make_persistent_id(("balaur-scroll", entity));
        let offset =
            egui::scroll_area::State::load(ui.ctx(), id).map_or(egui::Vec2::ZERO, |s| s.offset);
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
        let instant = (latest - drag.last) / dt;
        drag.velocity = (drag.velocity + instant) / 2.0;
    }
    drag.last = latest;
    let (start, base) = (drag.from, drag.base);
    let travelled = latest - start;
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
