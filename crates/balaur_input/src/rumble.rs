//! Rumble as the engine plays it: a start delay, pulses, an envelope and a
//! distance falloff, worked out each tick in tick time.
//!
//! A backend only holds two motor levels for a length, so every backend
//! rumbles alike, and the finish a script hears is derived from tick time
//! and the recorded snapshot rather than from the hardware: a replay without
//! the pad hears it on the same tick.

/// The longest a rumble or its delay may run. A rumble asked to run longer is
/// a bug, and a pad left buzzing is the kind a player has to unplug to escape.
const MAX_SECONDS: f32 = 60.0;

/// Closer than this, two positions are one: a falloff never divides by it.
const MIN_DISTANCE: f32 = 1e-3;

/// How a positional rumble weakens between `min_distance` and `max_distance`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Falloff {
    /// `min / (min + rolloff * (d - min))`: audio's model, halving with each
    /// doubling of distance at a rolloff of 1.
    Inverse,
    /// Straight down to nothing at `max_distance`, at a rolloff of 1.
    Linear,
    /// `(d / min) ^ -rolloff`.
    Exponential,
}

/// Where a rumble comes from, for one that weakens with distance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placed {
    pub(crate) position: [f32; 3],
    pub(crate) min_distance: f32,
    pub(crate) max_distance: f32,
    pub(crate) falloff: Falloff,
    pub(crate) rolloff: f32,
}

impl Placed {
    fn gain(&self, listener: [f32; 3]) -> f32 {
        let d = distance(self.position, listener);
        let min = self.min_distance.max(MIN_DISTANCE);
        let max = self.max_distance.max(min);
        let rolloff = self.rolloff.max(0.0);
        if d <= min {
            return 1.0;
        }
        if d >= max {
            return 0.0;
        }
        let gain = match self.falloff {
            Falloff::Inverse => min / rolloff.mul_add(d - min, min),
            Falloff::Linear => 1.0 - rolloff * (d - min) / (max - min),
            Falloff::Exponential => libm::powf(d / min, -rolloff),
        };
        gain.clamp(0.0, 1.0)
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (x, y, z) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    libm::sqrtf(x.mul_add(x, y.mul_add(y, z * z)))
}

/// One rumble as a script asked for it. Every length is in seconds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Spec {
    pub(crate) strong: f32,
    pub(crate) weak: f32,
    pub(crate) duration: f32,
    pub(crate) delay: f32,
    /// On for `pulse`, off for `gap`, over and over; `None` is one long pulse.
    pub(crate) pulse: Option<f32>,
    pub(crate) gap: f32,
    pub(crate) attack: f32,
    pub(crate) attack_level: f32,
    pub(crate) fade: f32,
    pub(crate) fade_level: f32,
    pub(crate) placed: Option<Placed>,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            strong: 1.0,
            weak: 1.0,
            duration: 0.2,
            delay: 0.0,
            pulse: None,
            gap: 0.0,
            attack: 0.0,
            attack_level: 0.0,
            fade: 0.0,
            fade_level: 0.0,
            placed: None,
        }
    }
}

/// A length a script gave, with NaN and negatives read as none and anything
/// past [`MAX_SECONDS`] cut to it.
pub(crate) fn seconds(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, MAX_SECONDS)
    }
}

/// A level a script gave, 0 to 1, with NaN read as nothing.
pub(crate) fn level(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

impl Spec {
    fn end(&self) -> f32 {
        self.delay + self.duration
    }

    /// The window a moment falls in: its time into the window and the
    /// window's length, or `None` for a delay or a gap.
    fn window(&self, t: f32) -> Option<(f32, f32)> {
        if t < self.delay {
            return None;
        }
        let u = t - self.delay;
        let Some(pulse) = self.pulse else {
            return Some((u, self.duration));
        };
        let period = pulse + self.gap;
        if period <= 0.0 {
            return None;
        }
        let phase = u.rem_euclid(period);
        (phase < pulse).then_some((phase, pulse))
    }

    /// The envelope shapes each pulse, rising from `attack_level` and falling
    /// to `fade_level`.
    fn envelope(&self, u: f32, window: f32) -> f32 {
        if self.attack > 0.0 && u < self.attack {
            return (1.0 - self.attack_level).mul_add(u / self.attack, self.attack_level);
        }
        let left = window - u;
        if self.fade > 0.0 && left < self.fade {
            return (1.0 - self.fade_level).mul_add(left / self.fade, self.fade_level);
        }
        1.0
    }

    /// The strong and weak motor levels `t` seconds after the rumble began,
    /// or `None` once it has finished.
    pub(crate) fn levels_at(&self, t: f32, listener: Option<[f32; 3]>) -> Option<(f32, f32)> {
        if t >= self.end() {
            return None;
        }
        let Some((u, window)) = self.window(t) else {
            return Some((0.0, 0.0));
        };
        let distance = match (self.placed, listener) {
            (Some(placed), Some(listener)) => placed.gain(listener),
            _ => 1.0,
        };
        let k = self.envelope(u, window) * distance;
        Some((level(self.strong * k), level(self.weak * k)))
    }

    /// Seconds from `t` until the levels next change: zero while they change
    /// smoothly, through an envelope or with a listener that may move.
    pub(crate) fn next_change(&self, t: f32) -> f32 {
        let to_end = self.end() - t;
        if t < self.delay {
            return (self.delay - t).min(to_end);
        }
        if self.placed.is_some() {
            return 0.0;
        }
        let u = t - self.delay;
        let (phase, window, period) = match self.pulse {
            Some(pulse) if pulse + self.gap > 0.0 => {
                let period = pulse + self.gap;
                (u.rem_euclid(period), pulse, period)
            }
            _ => (u, self.duration, f32::INFINITY),
        };
        if phase < window && (phase < self.attack || window - phase < self.fade) {
            return 0.0;
        }
        let steps = [self.attack, window - self.fade, window, period];
        let next = steps
            .into_iter()
            .filter(|step| *step > phase)
            .fold(f32::INFINITY, f32::min);
        (next - phase).min(to_end).max(0.0)
    }
}

/// One playing rumble.
struct Playing {
    pad: i64,
    spec: Spec,
    elapsed: f32,
    /// The levels last sent, at the motors' 16-bit resolution.
    sent: Option<(u16, u16)>,
}

/// What a tick's rumble step asks of the motors and tells the scripts.
#[derive(Default, Debug, PartialEq)]
pub(crate) struct Step {
    /// Pad, strong, weak, and seconds left: hold these until told otherwise.
    pub(crate) holds: Vec<(i64, f32, f32, f32)>,
    /// Pads whose rumble ran its course this step.
    pub(crate) finished: Vec<i64>,
    /// Seconds until a level next changes, when one is playing.
    pub(crate) next: Option<f32>,
}

/// Every pad's rumble, one each: a second rumble on a pad replaces the first.
#[derive(Default)]
pub(crate) struct Rumbles {
    playing: Vec<Playing>,
    listeners: Vec<(i64, [f32; 3])>,
}

impl Rumbles {
    pub(crate) fn start(&mut self, pad: i64, spec: Spec) {
        self.playing.retain(|playing| playing.pad != pad);
        self.playing.push(Playing {
            pad,
            spec,
            elapsed: 0.0,
            sent: None,
        });
    }

    /// End a pad's rumble early, without the finish a natural end announces.
    /// True when one was playing.
    pub(crate) fn stop(&mut self, pad: i64) -> bool {
        let before = self.playing.len();
        self.playing.retain(|playing| playing.pad != pad);
        self.playing.len() != before
    }

    /// Where the player a pad belongs to is, for a rumble placed in the world.
    pub(crate) fn set_listener(&mut self, pad: i64, position: [f32; 3]) {
        self.listeners.retain(|(id, _)| *id != pad);
        self.listeners.push((pad, position));
    }

    /// Drop everything about pads that are gone, quietly.
    pub(crate) fn forget_all_but(&mut self, connected: &[i64]) {
        self.playing
            .retain(|playing| connected.contains(&playing.pad));
        self.listeners.retain(|(pad, _)| connected.contains(pad));
    }

    /// Advance every rumble by `dt` and work out what the motors hold now.
    pub(crate) fn step(&mut self, dt: f32) -> Step {
        let mut out = Step::default();
        let listeners = &self.listeners;
        self.playing.retain_mut(|playing| {
            playing.elapsed += dt;
            let listener = listeners
                .iter()
                .find(|(pad, _)| *pad == playing.pad)
                .map(|(_, at)| *at);
            let Some((strong, weak)) = playing.spec.levels_at(playing.elapsed, listener) else {
                out.holds.push((playing.pad, 0.0, 0.0, 0.0));
                out.finished.push(playing.pad);
                return false;
            };
            let sent = (motor_units(strong), motor_units(weak));
            if playing.sent != Some(sent) {
                playing.sent = Some(sent);
                let left = playing.spec.end() - playing.elapsed;
                out.holds.push((playing.pad, strong, weak, left));
            }
            let next = playing.spec.next_change(playing.elapsed);
            out.next = Some(out.next.map_or(next, |soonest: f32| soonest.min(next)));
            true
        });
        out
    }
}

/// A level at the 16-bit resolution a motor is driven at.
fn motor_units(level: f32) -> u16 {
    (level * f32::from(u16::MAX)).round() as u16
}

#[cfg(test)]
mod tests {
    use super::{Falloff, Placed, Rumbles, Spec};

    fn close(got: f32, want: f32) -> bool {
        (got - want).abs() < 1e-4
    }

    fn strong_at(spec: &Spec, t: f32) -> Option<f32> {
        spec.levels_at(t, None).map(|(strong, _)| strong)
    }

    #[test]
    fn a_rumble_waits_out_its_delay_then_runs_its_duration() {
        let spec = Spec {
            delay: 0.5,
            duration: 1.0,
            ..Spec::default()
        };
        assert_eq!(strong_at(&spec, 0.25), Some(0.0));
        assert_eq!(strong_at(&spec, 1.0), Some(1.0));
        assert_eq!(
            strong_at(&spec, 1.5),
            None,
            "finished at delay plus duration"
        );
    }

    #[test]
    fn pulses_alternate_on_and_off_until_the_duration_ends() {
        let spec = Spec {
            duration: 1.0,
            pulse: Some(0.1),
            gap: 0.15,
            ..Spec::default()
        };
        assert_eq!(strong_at(&spec, 0.05), Some(1.0));
        assert_eq!(strong_at(&spec, 0.2), Some(0.0));
        assert_eq!(strong_at(&spec, 0.3), Some(1.0), "the second pulse");
    }

    #[test]
    fn the_envelope_rises_from_attack_level_and_falls_to_fade_level() {
        let spec = Spec {
            duration: 1.0,
            attack: 0.2,
            attack_level: 0.5,
            fade: 0.4,
            fade_level: 0.0,
            ..Spec::default()
        };
        assert!(close(strong_at(&spec, 0.0).unwrap(), 0.5));
        assert!(close(strong_at(&spec, 0.1).unwrap(), 0.75));
        assert!(close(strong_at(&spec, 0.5).unwrap(), 1.0));
        assert!(close(strong_at(&spec, 0.8).unwrap(), 0.5));
    }

    #[test]
    fn a_placed_rumble_weakens_with_the_listener_s_distance() {
        let placed = |falloff| Placed {
            position: [0.0; 3],
            min_distance: 1.0,
            max_distance: 10.0,
            falloff,
            rolloff: 1.0,
        };
        let at = |falloff, d: f32| placed(falloff).gain([d, 0.0, 0.0]);
        assert!(close(at(Falloff::Inverse, 0.5), 1.0));
        assert!(close(at(Falloff::Inverse, 2.0), 0.5));
        assert!(close(at(Falloff::Linear, 5.5), 0.5));
        assert!(close(at(Falloff::Exponential, 4.0), 0.25));
        assert!(close(at(Falloff::Inverse, 10.0), 0.0), "silent at max");
        let spec = Spec {
            placed: Some(placed(Falloff::Inverse)),
            ..Spec::default()
        };
        assert_eq!(
            spec.levels_at(0.0, None),
            Some((1.0, 1.0)),
            "no listener, no falloff"
        );
    }

    #[test]
    fn a_rumble_finishes_once_and_sends_only_what_changed() {
        let mut rumbles = Rumbles::default();
        rumbles.start(
            0,
            Spec {
                duration: 0.1,
                ..Spec::default()
            },
        );
        let first = rumbles.step(0.0);
        assert_eq!(first.holds.len(), 1, "the start reaches the motors at once");
        assert!(rumbles.step(0.05).holds.is_empty(), "nothing changed");
        let end = rumbles.step(0.06);
        assert_eq!(end.finished, [0]);
        assert_eq!(end.holds, [(0, 0.0, 0.0, 0.0)]);
        assert!(rumbles.step(0.1).finished.is_empty(), "only once");
    }

    #[test]
    fn a_stopped_rumble_ends_without_a_finish() {
        let mut rumbles = Rumbles::default();
        rumbles.start(0, Spec::default());
        assert!(rumbles.stop(0));
        assert!(rumbles.step(1.0).finished.is_empty());
    }

    #[test]
    fn the_next_change_is_the_next_edge_or_now_inside_a_ramp() {
        let flat = Spec {
            duration: 1.0,
            pulse: Some(0.2),
            gap: 0.3,
            ..Spec::default()
        };
        assert!(close(flat.next_change(0.05), 0.15), "the pulse's end");
        assert!(close(flat.next_change(0.3), 0.2), "the gap's end");
        let ramp = Spec {
            attack: 0.5,
            duration: 1.0,
            ..Spec::default()
        };
        assert!(close(ramp.next_change(0.1), 0.0));
    }
}
