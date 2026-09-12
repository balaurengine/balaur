//! Pinch, pan, swipe and long press, derived from the fingers already in the
//! snapshot.
//!
//! Nothing here is fed and nothing here is recorded. A gesture is a function
//! of the touches a recording already holds and the fixed step it already
//! replays, so deriving it again on playback gives the same answer, and
//! recording it as well would be the second reading of one finger that
//! `docs/PLAN-input.md` rule 5 forbids.
//!
//! Every reading is neutral where there is no gesture: no finger, one finger
//! where two are wanted, or a desktop with no touch screen all read zero.

use balaur_core::Engine;

use crate::InputSnapshot;
use crate::settings::InputConfig;

/// What the fingers are doing this frame, past the raw positions.
///
/// Rebuilt each tick from the snapshot, so it holds no history a replay would
/// have to restore beyond the two spans below, which are themselves derived
/// from the recorded touches.
#[derive(Default)]
pub struct Gestures {
    pinch: Option<Pinch>,
    pan: (f32, f32),
    swipe: Option<Swipe>,
    long_press: Option<(f32, f32)>,
    /// Per finger: where it landed, where it is, and how long it has been
    /// down, in the fixed step's own seconds.
    spans: Vec<Span>,
}

struct Span {
    id: u64,
    from: (f32, f32),
    at: (f32, f32),
    held: f32,
    /// A hold reports once. Cleared with the finger.
    announced: bool,
}

/// Two fingers moving apart or together.
#[derive(Clone, Copy)]
pub struct Pinch {
    /// This frame's spread over last frame's: above 1 is apart.
    pub scale: f32,
    pub center: (f32, f32),
}

/// A finger that travelled far enough before it lifted.
#[derive(Clone, Copy)]
pub struct Swipe {
    /// Where it went, as a unit vector in screen pixels.
    pub direction: (f32, f32),
    /// Pixels per second over the whole gesture.
    pub speed: f32,
}

impl Gestures {
    pub const fn pinch(&self) -> Option<Pinch> {
        self.pinch
    }

    /// Two fingers moving together, as a delta in the same pixels as
    /// `mouse_position`. Zero with fewer than two fingers down.
    pub const fn pan(&self) -> (f32, f32) {
        self.pan
    }

    /// Reported on the frame the finger lifted, and never again.
    pub const fn swipe(&self) -> Option<Swipe> {
        self.swipe
    }

    /// Reported on the frame a hold passed its threshold, and never again for
    /// that finger.
    pub const fn long_press(&self) -> Option<(f32, f32)> {
        self.long_press
    }
}

/// Fold this frame's touches into the spans, then read the four gestures off
/// them. Runs in `Stage::First`, after a replay has restored the snapshot, so
/// it reads the recorded fingers rather than the live ones.
pub(crate) fn tick(eng: &Engine, dt: f32) {
    let Some(gestures) = eng.try_resource::<Gestures>() else {
        return;
    };
    let Some(snapshot) = eng.try_resource::<InputSnapshot>() else {
        return;
    };
    let (swipe_pixels, hold_seconds, hold_slop) =
        eng.try_resource::<InputConfig>()
            .map_or((0.0, f32::MAX, 0.0), |s| {
                let s = s.borrow();
                (s.swipe_pixels, s.long_press_seconds, s.long_press_slop)
            });
    let snapshot = snapshot.borrow();
    let mut g = gestures.borrow_mut();
    let previous = spread(&g.spans);

    g.pinch = None;
    g.swipe = None;
    g.long_press = None;
    g.pan = (0.0, 0.0);

    // A finger that lifted is a swipe or nothing, and either way its span
    // goes. The oldest lift wins, because a two-finger lift is a pan ending.
    for id in snapshot.touches_ended() {
        let Some(at) = g.spans.iter().position(|s| s.id == *id) else {
            continue;
        };
        let span = g.spans.remove(at);
        let (dx, dy) = (span.at.0 - span.from.0, span.at.1 - span.from.1);
        let distance = libm::hypotf(dx, dy);
        if distance >= swipe_pixels && span.held > 0.0 && g.swipe.is_none() {
            g.swipe = Some(Swipe {
                direction: (dx / distance, dy / distance),
                speed: distance / span.held,
            });
        }
    }

    let mut moved = (0.0, 0.0);
    let mut moving = 0u32;
    for (id, x, y) in snapshot.touches() {
        match g.spans.iter_mut().find(|s| s.id == *id) {
            Some(span) => {
                moved.0 += x - span.at.0;
                moved.1 += y - span.at.1;
                moving += 1;
                span.at = (*x, *y);
                span.held += dt;
                let wander = libm::hypotf(span.at.0 - span.from.0, span.at.1 - span.from.1);
                if wander > hold_slop {
                    // Moved too far to be a hold, and too far to become one
                    // later: a drag that pauses is still a drag.
                    span.announced = true;
                } else if !span.announced && span.held >= hold_seconds {
                    span.announced = true;
                    g.long_press = Some(span.at);
                }
            }
            None => g.spans.push(Span {
                id: *id,
                from: (*x, *y),
                at: (*x, *y),
                held: 0.0,
                announced: false,
            }),
        }
    }

    // Two fingers or more: the average movement is the pan, and the change in
    // how far apart they are is the pinch.
    if moving >= 2 {
        let count = moving as f32;
        g.pan = (moved.0 / count, moved.1 / count);
        if let (Some(now), Some(was)) = (spread(&g.spans), previous)
            && was.0 > f32::EPSILON
        {
            g.pinch = Some(Pinch {
                scale: now.0 / was.0,
                center: now.1,
            });
        }
    }
}

/// How far apart the two oldest fingers are, and the point between them.
/// `None` with fewer than two down, which is what makes a pinch neutral.
fn spread(spans: &[Span]) -> Option<(f32, (f32, f32))> {
    let (a, b) = (spans.first()?, spans.get(1)?);
    let distance = libm::hypotf(a.at.0 - b.at.0, a.at.1 - b.at.1);
    let center = (f32::midpoint(a.at.0, b.at.0), f32::midpoint(a.at.1, b.at.1));
    Some((distance, center))
}
