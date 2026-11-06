//! A screen reader between kiss3d's platform adapter and the widget tree:
//! what a reader asks goes to `balaur_ui` before the frame's pass, and what the
//! pass changed goes back after it. Until a reader asks, a frame reads an empty
//! queue.

use balaur_core::Engine;
use kiss3d::window::{AccessEvent, Window};

/// Hand what a reader asked since the last frame to the widget tree. Answers
/// whether it asked anything.
pub(crate) fn hear_reader(eng: &Engine, window: &Window) -> bool {
    let events = window.take_access_events();
    for event in &events {
        hear(eng, event);
    }
    !events.is_empty()
}

fn hear(eng: &Engine, event: &AccessEvent) {
    match event {
        AccessEvent::Activated => balaur_ui::enable_access(eng),
        AccessEvent::Action(request) => balaur_ui::act(eng, request),
        AccessEvent::Deactivated => balaur_ui::disable_access(eng),
    }
}

/// Hand the reader what the pass just run changed.
pub(crate) fn answer_reader(eng: &Engine, window: &Window) {
    if let Some(update) = balaur_ui::take_access_update(eng) {
        window.update_access(update);
    }
}

#[cfg(test)]
mod tests {
    use super::hear;
    use balaur_core::Engine;
    use kiss3d::window::AccessEvent;

    #[test]
    fn a_reader_arriving_turns_the_tree_on_and_leaving_turns_it_off() {
        let eng = Engine::new();
        eng.insert_resource(balaur_ui::AccessTree::default());
        assert!(!balaur_ui::access_enabled(&eng), "off until a reader asks");
        hear(&eng, &AccessEvent::Activated);
        assert!(balaur_ui::access_enabled(&eng));
        hear(&eng, &AccessEvent::Deactivated);
        assert!(!balaur_ui::access_enabled(&eng));
    }
}
