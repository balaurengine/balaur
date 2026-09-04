//! Rumble, through gilrs's force feedback.
//!
//! Output, not input: a recording never carries a rumble, and a replay re-runs
//! the script that asked for one. What the recording does carry is whether a
//! pad can rumble at all, because a script may branch on that and a replay has
//! to take the same branch — see [`crate::gamepad::Pad::can_rumble`].
//!
//! gilrs stops an effect as soon as its handle drops, so a pad's live effect
//! is held here until it is replaced or stopped.

#[cfg(not(target_family = "wasm"))]
use gilrs::ff::{BaseEffect, BaseEffectType, Effect, EffectBuilder, Repeat, Replay, Ticks};
#[cfg(not(target_family = "wasm"))]
use gilrs::{GamepadId, Gilrs};

use balaur_script::{Bindings, BindingsExt, Value};

use balaur_core::Engine;

use crate::gamepad::{GamepadState, Pad};

/// gilrs schedules force feedback in 50 ms ticks, so anything shorter still
/// costs one tick; a zero-length effect divides by zero inside its server.
#[cfg(not(target_family = "wasm"))]
const MIN_RUMBLE_MS: u32 = 1;

/// A rumble asked to run longer than this is a bug, and a pad left buzzing is
/// the kind of bug a player has to unplug something to escape. `stop_rumble`
/// is how a game ends one early.
#[cfg(not(target_family = "wasm"))]
const MAX_RUMBLE_MS: u32 = 60_000;

/// One live effect per pad: a second rumble on a pad replaces the first rather
/// than mixing with it, which is what a game asking for the next hit wants.
#[cfg(not(target_family = "wasm"))]
#[derive(Default)]
pub(crate) struct Rumble {
    playing: Vec<(i64, Effect)>,
}

#[cfg(not(target_family = "wasm"))]
impl Rumble {
    /// Start `strong`/`weak` (0..1, the two motors of the xinput model) on the
    /// pad for `seconds`. False when the pad is gone or has no motors.
    pub(crate) fn play(
        &mut self,
        gilrs: &mut Gilrs,
        id: i64,
        strong: f32,
        weak: f32,
        seconds: f32,
    ) -> bool {
        let Some(pad) = gamepad_id(gilrs, id) else {
            return false;
        };
        // A NaN duration casts to 0 and so becomes the shortest rumble, not
        // an endless one.
        let ms = (seconds * 1000.0) as u32;
        let ticks = Ticks::from_ms(ms.clamp(MIN_RUMBLE_MS, MAX_RUMBLE_MS));
        let scheduling = Replay {
            after: Ticks::from_ms(0),
            play_for: ticks,
            with_delay: Ticks::from_ms(0),
        };
        let effect = EffectBuilder::new()
            .add_effect(BaseEffect {
                kind: BaseEffectType::Strong {
                    magnitude: magnitude(strong),
                },
                scheduling,
                ..BaseEffect::default()
            })
            .add_effect(BaseEffect {
                kind: BaseEffectType::Weak {
                    magnitude: magnitude(weak),
                },
                scheduling,
                ..BaseEffect::default()
            })
            .gamepads(&[pad])
            .repeat(Repeat::For(ticks))
            .finish(gilrs);
        let effect = match effect {
            Ok(effect) => effect,
            Err(err) => {
                tracing::debug!(id, "rumble: {err}");
                return false;
            }
        };
        if let Err(err) = effect.play() {
            tracing::debug!(id, "rumble: {err}");
            return false;
        }
        self.stop(id);
        self.playing.push((id, effect));
        true
    }

    /// Silence the pad. Dropping the handle would stop it too; stopping first
    /// is what makes the pad go quiet now rather than at the server's next tick.
    pub(crate) fn stop(&mut self, id: i64) {
        self.playing.retain(|(playing, effect)| {
            if *playing == id {
                let _ = effect.stop();
            }
            *playing != id
        });
    }

    /// Drop the effects of pads that are no longer connected.
    pub(crate) fn retain_pads(&mut self, connected: &[i64]) {
        self.playing.retain(|(id, _)| connected.contains(id));
    }
}

/// 0..1 onto the range a motor is driven over.
#[cfg(not(target_family = "wasm"))]
fn magnitude(value: f32) -> u16 {
    // NaN clamps to the low end rather than wrapping to a full-strength jolt.
    let value = if value.is_nan() { 0.0 } else { value };
    (value.clamp(0.0, 1.0) * f32::from(u16::MAX)) as u16
}

/// Our `i64` pad id back into the one gilrs hands out, as [`crate::gamepad`]
/// derived it.
#[cfg(not(target_family = "wasm"))]
fn gamepad_id(gilrs: &Gilrs, id: i64) -> Option<GamepadId> {
    gilrs
        .gamepads()
        .map(|(pad, _)| pad)
        .find(|pad| i64::try_from(usize::from(*pad)).unwrap_or(i64::MAX) == id)
}

/// `input.gamepad_rumble` and friends. Declared on every platform: a build
/// with no force feedback answers false and does nothing, the same neutral
/// contract the rest of input keeps.
pub(crate) fn install_haptics_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("gamepad_can_rumble", &[], "", "Whether the pad has motors to rumble; false for a pad that is not connected, and on a build with no force feedback."),
        ("gamepad_rumble", &[], "", "Rumble the pad, `{ strong, weak, duration }` — the two motors at 0..1 for that many seconds. Returns whether it started; a second rumble replaces the first."),
        ("gamepad_stop_rumble", &[], "", "Silence the pad now, rather than waiting out the rumble's duration."),
    ]);
    m.function("gamepad_can_rumble", |eng: &Engine, id: i64| {
        let state = eng.resource::<GamepadState>();
        let v = state.borrow().pad(id).is_some_and(Pad::can_rumble);
        Ok(v)
    });
    // Strength and duration live in an options table, so a curve or a motor
    // the next pad exposes can join them without a second function (N9).
    m.function(
        "gamepad_rumble",
        |eng: &Engine, (id, opts): (i64, Option<Value>)| {
            let opts = opts.as_ref();
            let strong = number(opts, "strong").unwrap_or(1.0);
            let weak = number(opts, "weak").unwrap_or(1.0);
            let duration = number(opts, "duration").unwrap_or(0.2);
            let state = eng.resource::<GamepadState>();
            let v = state.borrow_mut().rumble(id, strong, weak, duration);
            Ok(v)
        },
    );
    m.function("gamepad_stop_rumble", |eng: &Engine, id: i64| {
        eng.resource::<GamepadState>().borrow_mut().stop_rumble(id);
        Ok(())
    });
}

/// A number from the options table, whichever way the language spelled it.
fn number(opts: Option<&Value>, key: &str) -> Option<f32> {
    let Some(Value::Map(entries)) = opts else {
        return None;
    };
    match entries.iter().find(|(k, _)| k == key).map(|(_, v)| v) {
        Some(Value::Num(n)) => Some(*n as f32),
        Some(Value::Int(i)) => Some(*i as f32),
        _ => None,
    }
}
