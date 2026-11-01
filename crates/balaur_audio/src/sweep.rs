//! What each frame tidies: sounds a hold keeps quiet, voices of freed nodes,
//! sinks that played out.

use balaur_core::Engine;
use balaur_core::hecs::Entity;

use crate::{Access, AudioState, Sound, announce_finished, publish_playback_times, spatial};

/// Drop finished sinks, and stop the sounds of nodes that were freed.
pub(crate) fn sweep_sounds_system(eng: &Engine, _: f32) {
    // Every frame and not on the fixed step: a debugger stop has none.
    hold_sounds(eng);
    let state = eng.resource::<AudioState>();
    let mut state = state.borrow_mut();
    // A device that would not open gets another try each time the window comes
    // back: that is when a headset was plugged in.
    if state.access == Access::Tried && state.device.is_none() {
        let focused = balaur_core::facts::device(eng).focused;
        if focused && !state.was_focused {
            state.access = Access::Untried;
            state.begin_open();
        }
        state.was_focused = focused;
    }
    let world = eng.world();
    spatial::sweep_listeners(&mut state, &world);
    let AudioState {
        nodes,
        playing,
        routing,
        spatial,
        releasing,
        ..
    } = &mut *state;
    // A voice lives here, not on the entity, so this is where a freed node's
    // playback stops.
    nodes.retain(|&entity, voice| {
        if world.contains(entity) {
            return true;
        }
        if let Some(handle) = voice.handle {
            spatial.shift_remove(&handle);
            routing.shift_remove(&handle);
            if let Some(sink) = playing.shift_remove(&handle) {
                sink.stop();
            }
        }
        false
    });
    // A sink that has played out is dropped. A sound whose length is known
    // ends on the fixed step's count instead, which a machine with no device
    // reaches on the same tick; only one of unknown length ends here.
    let mut ended = Vec::new();
    playing.retain(|handle, sink| {
        if !sink.finished() {
            return true;
        }
        if routing
            .get(handle)
            .is_some_and(|routed| !routed.timeline.counted())
        {
            spatial.shift_remove(handle);
            routing.shift_remove(handle);
            ended.push(*handle);
        }
        false
    });
    releasing.retain(|_, release| release.sink.as_ref().is_none_or(|sink| !sink.finished()));
    drop(world);
    drop(state);
    if !ended.is_empty() {
        announce_finished(eng, &ended);
        publish_playback_times(eng);
    }
}

/// A node's sound holds with its node: a game pause, a `process` mode, a parked
/// scene, a debugger stop. Each voice goes back to what it asks for when the
/// hold ends. A cue has no node, so a pause menu's click still plays.
fn hold_sounds(eng: &Engine) {
    let Some(state) = eng.try_resource::<AudioState>() else {
        return;
    };
    let mut state = state.borrow_mut();
    let world = eng.world();
    let paused = balaur_core::process::pause(eng);
    let held = eng.frozen_root();
    let voices: Vec<(Entity, u64)> = state
        .nodes
        .iter()
        .filter_map(|(entity, voice)| voice.handle.map(|handle| (*entity, handle)))
        .collect();
    for (entity, handle) in voices {
        let asked = world.get::<&Sound>(entity).is_ok_and(|sound| sound.paused);
        let holds = !balaur_core::process::ticks(&world, entity, paused)
            || held.is_some_and(|root| balaur_core::scene::is_within(&world, entity, root));
        if state.is_paused(handle) != (asked || holds) {
            state.set_paused(handle, asked || holds);
        }
    }
}
