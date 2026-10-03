//! Rumble: the script verbs, the tick that plays them, and the motors gilrs
//! drives.
//!
//! Output, not input: a recording never carries a rumble, and a replay re-runs
//! the script that asked for one. What the recording does carry is whether a
//! pad can rumble at all, because a script may branch on that and a replay has
//! to take the same branch: see [`crate::gamepad::Pad::can_rumble`]. The shape
//! of a rumble over time is the engine's (`rumble.rs`); a backend only holds
//! two motor levels for as long as the rumble has left.

#[cfg(not(target_family = "wasm"))]
use gilrs::ff::{BaseEffect, BaseEffectType, Effect, EffectBuilder, Repeat, Replay, Ticks};
#[cfg(not(target_family = "wasm"))]
use gilrs::{GamepadId, Gilrs};

use balaur_script::{Bindings, BindingsExt, Value};

use balaur_core::Engine;

use crate::gamepad::{GamepadState, Pad};
use crate::rumble::{self, Falloff, Placed, Spec, Step};
use crate::vocabulary::{keys as k, words as w};

/// Where a placed rumble is at full strength and where it is gone, the same
/// numbers a sound uses, so a rumble and its sound fade together.
const DEFAULT_MIN_DISTANCE: f32 = 1.0;
const DEFAULT_MAX_DISTANCE: f32 = 50.0;

/// What drives a pad's two motors. A backend holds the levels it is given for
/// at most `seconds`, so a game that stops ticking cannot leave a pad buzzing.
#[cfg(not(target_family = "wasm"))]
pub(crate) trait Motors: Send {
    /// Hold the strong and weak motors at 0..1 for `seconds`; zero stops them.
    fn hold(&mut self, strong: f32, weak: f32, seconds: f32);
}

/// gilrs silences an effect whose gain is under this: its force-feedback
/// thread rounds the attenuation to nothing. So a motor is several effects,
/// each this factor weaker than the last, and a level plays on the strongest
/// one whose gain clears the floor.
#[cfg(not(target_family = "wasm"))]
const GAIN_FLOOR: f32 = 0.05;

/// Three steps reach a level of 8 in 65535, the motor's own resolution.
#[cfg(not(target_family = "wasm"))]
const RUNGS: i32 = 3;

/// The longest gilrs is asked to hold anything, in milliseconds.
#[cfg(not(target_family = "wasm"))]
const MAX_HOLD_MS: u32 = 60_000;

/// The rung a level plays on and its gain there, or `None` for a level below
/// every rung.
#[cfg(not(target_family = "wasm"))]
fn rung_for(level: f32) -> Option<(usize, f32)> {
    (0..RUNGS).find_map(|rung| {
        let gain = level / GAIN_FLOOR.powi(rung);
        (gain >= GAIN_FLOOR).then_some((rung.unsigned_abs() as usize, gain.min(1.0)))
    })
}

/// A pad's two motors through gilrs.
#[cfg(not(target_family = "wasm"))]
pub(crate) struct GilrsMotors {
    strong: Ladder,
    weak: Ladder,
}

/// One motor's effects, strongest first, and the one playing.
#[cfg(not(target_family = "wasm"))]
struct Ladder {
    rungs: Vec<Effect>,
    playing: Option<usize>,
}

#[cfg(not(target_family = "wasm"))]
impl GilrsMotors {
    /// Both motors of `pad`, idle until held. On the thread that owns gilrs.
    pub(crate) fn build(gilrs: &mut Gilrs, pad: GamepadId) -> Option<Self> {
        let strong = Ladder::build(gilrs, pad, |magnitude| BaseEffectType::Strong { magnitude })?;
        let weak = Ladder::build(gilrs, pad, |magnitude| BaseEffectType::Weak { magnitude })?;
        Some(Self { strong, weak })
    }
}

#[cfg(not(target_family = "wasm"))]
impl Motors for GilrsMotors {
    fn hold(&mut self, strong: f32, weak: f32, seconds: f32) {
        self.strong.hold(strong, seconds);
        self.weak.hold(weak, seconds);
    }
}

#[cfg(not(target_family = "wasm"))]
impl Ladder {
    fn build(gilrs: &mut Gilrs, pad: GamepadId, kind: fn(u16) -> BaseEffectType) -> Option<Self> {
        let rungs = (0..RUNGS)
            .map(|rung| {
                let magnitude = (GAIN_FLOOR.powi(rung) * f32::from(u16::MAX)).round() as u16;
                EffectBuilder::new()
                    .add_effect(BaseEffect {
                        kind: kind(magnitude),
                        scheduling: Replay {
                            after: Ticks::from_ms(0),
                            play_for: Ticks::from_ms(MAX_HOLD_MS),
                            with_delay: Ticks::from_ms(0),
                        },
                        ..BaseEffect::default()
                    })
                    .gamepads(&[pad])
                    .finish(gilrs)
                    .map_err(|err| tracing::debug!(?pad, "rumble: {err}"))
                    .ok()
            })
            .collect::<Option<Vec<Effect>>>()?;
        Some(Self {
            rungs,
            playing: None,
        })
    }

    fn hold(&mut self, level: f32, seconds: f32) {
        let rung = rung_for(level).filter(|_| seconds > 0.0);
        if let Some(old) = self.playing.take()
            && rung.is_none_or(|(next, _)| next != old)
        {
            let _ = self.rungs[old].stop();
        }
        let Some((next, gain)) = rung else {
            return;
        };
        let ms = ((seconds * 1000.0).ceil() as u32).clamp(1, MAX_HOLD_MS);
        let effect = &self.rungs[next];
        let played = effect
            .set_gain(gain)
            .and_then(|()| effect.set_repeat(Repeat::For(Ticks::from_ms(ms))))
            .and_then(|()| effect.play());
        match played {
            Ok(()) => self.playing = Some(next),
            Err(err) => tracing::debug!("rumble: {err}"),
        }
    }
}

/// Advance every rumble by a tick and hand the motors their new levels.
/// Skipped on a rollback's second run of a tick, which would count its time
/// twice.
pub(crate) fn rumble_system(eng: &Engine, dt: f32) {
    if balaur_core::rollback::is_resimulating(eng) {
        return;
    }
    let state = eng.resource::<GamepadState>();
    let mut state = state.borrow_mut();
    // From the pads as the tick sees them, polled or restored, so a replay
    // drops a gone pad's rumble on the tick the recording did.
    let connected: Vec<i64> = state.pads().iter().map(|pad| pad.id).collect();
    state.rumble.forget_all_but(&connected);
    let step = state.rumble.step(dt);
    deliver(eng, &state, &step);
}

/// Send a step's levels to the motors, tell scripts what finished, and wake
/// a sleeping loop when the levels next change.
fn deliver(eng: &Engine, state: &GamepadState, step: &Step) {
    for (pad, strong, weak, seconds) in &step.holds {
        state.hold_motors(*pad, *strong, *weak, *seconds);
    }
    for pad in &step.finished {
        balaur_core::facts::notice(
            eng,
            balaur_core::hooks::ON_GAMEPAD_RUMBLE_FINISHED,
            Value::Int(*pad),
        );
    }
    match step.next {
        Some(next) if next <= 0.0 => balaur_core::wake::wake(),
        Some(next) if next.is_finite() => wake_in(next),
        _ => {}
    }
}

/// Wake a sleeping loop `seconds` from now.
#[allow(
    clippy::disallowed_methods,
    reason = "a deadline to wake the loop at, never a simulation input"
)]
fn wake_in(seconds: f32) {
    balaur_core::wake::at(
        balaur_core::time::Instant::now() + std::time::Duration::from_secs_f32(seconds),
    );
}

/// `input.gamepad_rumble` and friends. Declared on every platform: a build
/// with no motors plays the rumble's timing all the same, so the finish a
/// script waits for arrives everywhere.
pub(crate) fn install_haptics_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("gamepad_can_rumble", &[], "", "Whether the pad has motors to rumble; false for a pad that is not connected."),
        ("gamepad_rumble", &[], "(id: int, opts: map?)", "Rumble the pad. Options: `strong` and `weak`, the two motors at 0..1; `duration` in seconds; `delay` before it starts; `pulse` and `gap` to beat on and off; `attack`, `attack_level`, `fade` and `fade_level` to shape each pulse; `position` with `min_distance`, `max_distance`, `falloff` (`inverse`, `linear` or `exponential`) and `rolloff` to weaken it with distance from `gamepad_set_listener`. Returns whether the pad can rumble; a second rumble replaces the first."),
        ("gamepad_stop_rumble", &[], "", "Silence the pad now, without the `on_gamepad_rumble_finished` a rumble's natural end sends."),
        ("gamepad_set_listener", &[], "(id: int, position: vec3)", "Where the pad's player is, for a rumble given a `position`; until set, a placed rumble plays at full strength."),
        ("vibrate", &[], "(seconds: float)", "Buzz the device for that many seconds: a phone's motor, or a page's `navigator.vibrate`. Nothing on a desktop, and never recorded, like rumble."),
    ]);
    for falloff in w::FALLOFFS {
        m.constant(
            &format!("GAMEPAD_FALLOFF_{}", falloff.to_ascii_uppercase()),
            Value::Str((*falloff).to_string()),
        );
    }
    m.function("vibrate", |_: &Engine, seconds: f64| {
        vibrate(millis(seconds));
        Ok(())
    });
    m.function("gamepad_can_rumble", |eng: &Engine, id: i64| {
        let state = eng.resource::<GamepadState>();
        let v = state.borrow().pad(id).is_some_and(Pad::can_rumble);
        Ok(v)
    });
    m.function(
        "gamepad_rumble",
        |eng: &Engine, (id, opts): (i64, Option<Value>)| {
            let state = eng.resource::<GamepadState>();
            let can = state.borrow().pad(id).is_some_and(Pad::can_rumble);
            if !can || balaur_core::rollback::is_resimulating(eng) {
                return Ok(can);
            }
            let spec = spec_of(opts.as_ref())?;
            let mut state = state.borrow_mut();
            state.rumble.start(id, spec);
            let step = state.rumble.step(0.0);
            deliver(eng, &state, &step);
            Ok(true)
        },
    );
    m.function("gamepad_stop_rumble", |eng: &Engine, id: i64| {
        let state = eng.resource::<GamepadState>();
        let mut state = state.borrow_mut();
        if state.rumble.stop(id) {
            state.hold_motors(id, 0.0, 0.0, 0.0);
        }
        Ok(())
    });
    m.function(
        "gamepad_set_listener",
        |eng: &Engine, (id, position): (i64, Value)| {
            let Some(position) = vec3(&position) else {
                anyhow::bail!("gamepad_set_listener takes a vec3 or a vec2 position");
            };
            eng.resource::<GamepadState>()
                .borrow_mut()
                .rumble
                .set_listener(id, position);
            Ok(())
        },
    );
}

/// A rumble from its options table; a key left out keeps its default.
fn spec_of(opts: Option<&Value>) -> anyhow::Result<Spec> {
    let n = |key| number(opts, key);
    let base = Spec::default();
    let placed = match field(opts, k::POSITION) {
        None => None,
        Some(value) => {
            let Some(position) = vec3(value) else {
                anyhow::bail!("a rumble's position is a vec3 or a vec2");
            };
            let falloff = match field(opts, k::FALLOFF) {
                None => Falloff::Inverse,
                Some(Value::Str(word)) if word == w::FALLOFF_INVERSE => Falloff::Inverse,
                Some(Value::Str(word)) if word == w::FALLOFF_LINEAR => Falloff::Linear,
                Some(Value::Str(word)) if word == w::FALLOFF_EXPONENTIAL => Falloff::Exponential,
                Some(other) => {
                    anyhow::bail!("{other:?} is not a falloff: {}", w::FALLOFFS.join(", "))
                }
            };
            Some(Placed {
                position,
                min_distance: n(k::MIN_DISTANCE).unwrap_or(DEFAULT_MIN_DISTANCE),
                max_distance: n(k::MAX_DISTANCE).unwrap_or(DEFAULT_MAX_DISTANCE),
                falloff,
                rolloff: n(k::ROLLOFF).unwrap_or(1.0),
            })
        }
    };
    Ok(Spec {
        strong: n(k::STRONG).map_or(base.strong, rumble::level),
        weak: n(k::WEAK).map_or(base.weak, rumble::level),
        duration: n(k::DURATION).map_or(base.duration, rumble::seconds),
        delay: n(k::DELAY).map_or(base.delay, rumble::seconds),
        pulse: n(k::PULSE).map(rumble::seconds),
        gap: n(k::GAP).map_or(base.gap, rumble::seconds),
        attack: n(k::ATTACK).map_or(base.attack, rumble::seconds),
        attack_level: n(k::ATTACK_LEVEL).map_or(base.attack_level, rumble::level),
        fade: n(k::FADE).map_or(base.fade, rumble::seconds),
        fade_level: n(k::FADE_LEVEL).map_or(base.fade_level, rumble::level),
        placed,
    })
}

/// One entry of an options table.
pub(crate) fn field<'a>(opts: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    let Some(Value::Map(entries)) = opts else {
        return None;
    };
    entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// A number from the options table, whichever way the language spelled it.
pub(crate) fn number(opts: Option<&Value>, key: &str) -> Option<f32> {
    match field(opts, key) {
        Some(Value::Num(n)) => Some(*n as f32),
        Some(Value::Int(i)) => Some(*i as f32),
        _ => None,
    }
}

/// A position a script gave: a vec3, or a vec2 on the plane z = 0.
pub(crate) fn vec3(value: &Value) -> Option<[f32; 3]> {
    match value {
        Value::Vec3(v) => Some(*v),
        Value::Vec2([x, y]) => Some([*x, *y, 0.0]),
        _ => None,
    }
}

/// Whole milliseconds, which is what the motors take; a negative or NaN
/// duration is none.
fn millis(seconds: f64) -> u32 {
    let ms = (seconds * 1000.0).round();
    if ms.is_nan() || ms <= 0.0 {
        0
    } else if ms >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        ms as u32
    }
}

/// The device's own motor. A page has `navigator.vibrate`; a desktop has
/// nothing, and a phone's native hook is the export's to wire.
#[cfg(target_family = "wasm")]
fn vibrate(milliseconds: u32) {
    if let Some(window) = web_sys::window() {
        let _ = window.navigator().vibrate_with_duration(milliseconds);
    }
}

#[cfg(not(target_family = "wasm"))]
fn vibrate(milliseconds: u32) {
    tracing::debug!(milliseconds, "vibrate: no motor on this platform");
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_vibration_is_given_in_seconds() {
        assert_eq!(super::millis(0.25), 250);
        assert_eq!(super::millis(-1.0), 0);
        assert_eq!(super::millis(f64::NAN), 0);
    }

    /// The level that reaches the motor is the rung's strength times its
    /// gain: what a script asked for, at any strength gilrs would silence.
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn every_level_plays_on_a_rung_whose_gain_clears_gilrs_s_floor() {
        use super::{GAIN_FLOOR, rung_for};
        for asked in [1.0_f32, 0.5, 0.05, 0.049, 0.01, 0.002, 0.0002] {
            let (rung, gain) = rung_for(asked).expect("a rung for every audible level");
            assert!((GAIN_FLOOR..=1.0).contains(&gain), "{asked}: gain {gain}");
            let played = GAIN_FLOOR.powi(i32::try_from(rung).unwrap()) * gain;
            assert!((played - asked).abs() < 1e-6, "{asked} played as {played}");
        }
        assert_eq!(rung_for(0.0), None);
        assert_eq!(rung_for(1e-6), None, "below the motor's resolution");
    }
}
