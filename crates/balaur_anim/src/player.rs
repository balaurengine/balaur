//! Per-node playback: what a node is playing, where it is, and what happens
//! next.
//!
//! Everything here is a read or a write of one [`Playback`] in
//! [`AnimationState`]. The clip itself is shared and immutable — two nodes
//! playing one asset have one `Rc<Clip>` between them — so the playhead, the
//! speed, the queue and the clips a script defined at run time all live here
//! rather than on the asset.

use anyhow::{anyhow, bail, Context, Result};
use balaur_core::assets;
use balaur_core::collections::DetHashMap;
use balaur_core::hecs::Entity;
use balaur_core::Engine;

use crate::clip::Clip;
use crate::tween::{Tween, TweenId};

/// The simulation tick animation advances on, matching physics.
pub(crate) const FIXED_DT: f32 = 1.0 / 60.0;
/// How far behind a frame is allowed to fall before time is dropped rather
/// than caught up on. Without it a stalled frame spends its recovery in a
/// spiral of catch-up steps.
pub(crate) const MAX_STEPS: u32 = 4;

/// The asset type name every clip is parsed through, and the one this crate
/// registers.
pub const CLIP_ASSET_TYPE: &str = "animation_clip";

/// What one node is playing, and where it is.
///
/// Separate from the clip because the clip is shared: two nodes playing one
/// asset have one `Rc<Clip>` between them and a `Playback` each.
pub struct Playback {
    /// The asset reference the `library` property named.
    pub library: String,
    /// The clip to start when the scene loads, as authored. Empty means none.
    pub autoplay: String,
    /// The entry currently playing, or empty when the library reference is
    /// itself the clip.
    pub clip_name: String,
    /// The clip being sampled, held so reloading the asset cannot pull it out
    /// from under a frame in progress.
    pub clip: Option<std::rc::Rc<Clip>>,
    /// Seconds of playback, before wrapping. Advanced by the fixed step.
    pub time: f32,
    pub speed: f32,
    /// Whether the playhead is advancing.
    pub playing: bool,
    /// A clip is current but held. `stop` clears both; `pause` moves from one
    /// to the other, which is what lets `resume` know there is something to
    /// go back to.
    pub paused: bool,
    /// Node path a track's `target` resolves against; empty is this node.
    pub root: String,
    /// What to play when the current clip ends, in the order it was asked
    /// for. A looping clip never ends, so it never drains — the same as
    /// Godot's queue.
    pub queue: Vec<String>,
    /// Clips this node was given at run time by `animation.define`, as asset
    /// references the cache already holds. Insertion-ordered, because a name
    /// lookup that iterates must not depend on a hasher's seed.
    pub defined: DetHashMap<String, String>,
    /// The clip that ended during the last step, readable for exactly one
    /// frame. Cleared at the top of every advance, which is after the script
    /// tick that could read it.
    pub finished: String,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            library: String::new(),
            autoplay: String::new(),
            clip_name: String::new(),
            clip: None,
            time: 0.0,
            speed: 1.0,
            playing: false,
            paused: false,
            root: String::new(),
            queue: Vec::new(),
            defined: DetHashMap::default(),
            finished: String::new(),
        }
    }
}

impl Playback {
    /// Whether a clip is current — playing or held by `pause`.
    ///
    /// Not the same question as [`crate::is_playing`], which asks whether the
    /// playhead is moving.
    #[must_use]
    pub const fn active(&self) -> bool {
        self.playing || self.paused
    }

    /// The reference a clip name resolves to for this node.
    ///
    /// A name a script defined wins over the library, so `define` genuinely
    /// replaces a clip rather than sitting beside one that shadows it. An
    /// empty name is the library reference itself, which is what a
    /// single-clip file, an `[[assets]]` block and an inline definition each
    /// are.
    #[must_use]
    pub fn reference(&self, clip_name: &str) -> String {
        if let Some(defined) = self.defined.get(clip_name) {
            return defined.clone();
        }
        if clip_name.is_empty() {
            self.library.clone()
        } else {
            format!("{}#{clip_name}", self.library)
        }
    }
}

/// Every node's playback and every running tween, plus the fixed-step
/// accumulator: state this plugin owns outright, in the shape `PhysicsState`
/// established.
#[derive(Default)]
pub struct AnimationState {
    /// Insertion-ordered with an unseeded hasher: the order nodes are
    /// advanced in must be the same on every run and platform (see
    /// `balaur_core::collections`).
    pub players: DetHashMap<Entity, Playback>,
    /// Running tweens, keyed by the handle the caller was given. Ordered for
    /// the same reason `players` is: two tweens driving one property must
    /// land in the same order every run.
    pub tweens: DetHashMap<TweenId, Tween>,
    /// The last handle given out. Counts up and never reuses, so a handle
    /// held past the end of its tween names nothing rather than something
    /// else.
    pub(crate) next_tween: TweenId,
    pub(crate) accumulator: f32,
    /// The asset generation these players' clips were resolved at. When the
    /// cache moves past it — a file saved in dev mode, an editor writing a
    /// clip — every live playback re-resolves and keeps its playhead.
    pub(crate) asset_generation: u64,
}

/// Run `f` over one node's playback, or answer `None` when it has none.
fn with_playback<T>(eng: &Engine, entity: Entity, f: impl FnOnce(&mut Playback) -> T) -> Option<T> {
    let state = eng.try_resource::<AnimationState>()?;
    let mut state = state.borrow_mut();
    state.players.get_mut(&entity).map(f)
}

/// Read one node's playback without holding the borrow open.
fn read<T>(eng: &Engine, entity: Entity, f: impl FnOnce(&Playback) -> T) -> Option<T> {
    let state = eng.try_resource::<AnimationState>()?;
    let state = state.borrow();
    state.players.get(&entity).map(f)
}

/// Start `clip_name` on `entity`, from the beginning.
///
/// # Errors
/// If the node has no `animation` component, or the clip cannot be loaded —
/// the message carries the reference that was asked for.
pub fn play(eng: &Engine, entity: Entity, clip_name: &str) -> Result<()> {
    play_from(eng, entity, clip_name, true)
}

/// [`play`], with the option of picking the current clip back up where it was.
///
/// `from_start = false` is Godot's behaviour when `play` names the clip that
/// is already current: the playhead stays, which is what makes calling `play`
/// every frame from a state machine not a stutter.
///
/// # Errors
/// As [`play`].
pub fn play_from(eng: &Engine, entity: Entity, clip_name: &str, from_start: bool) -> Result<()> {
    let state = eng.resource::<AnimationState>();
    let (reference, addressable, same) = {
        let state = state.borrow();
        let playback = state
            .players
            .get(&entity)
            .ok_or_else(|| anyhow!("this node has no `animation` component to play a clip on"))?;
        (
            playback.reference(clip_name),
            playback.defined.contains_key(clip_name) || !playback.library.trim().is_empty(),
            playback.active() && playback.clip_name == clip_name,
        )
    };
    if !addressable {
        bail!(
            "the `animation` component names no `library` and no clip called '{clip_name}' was \
             defined on this node, so there is nothing to play"
        );
    }
    let clip = assets::load_typed::<Clip>(eng, &reference)
        .with_context(|| format!("playing animation '{reference}'"))?;
    if let Some(playback) = state.borrow_mut().players.get_mut(&entity) {
        playback.clip_name = clip_name.to_string();
        playback.clip = Some(clip);
        if from_start || !same {
            playback.time = 0.0;
        }
        playback.playing = true;
        playback.paused = false;
    }
    Ok(())
}

/// Play `clip_name` once the current clip ends, after anything already queued.
///
/// A looping clip never ends, so a queue behind one never drains — the same
/// as Godot's, and the same reason.
pub fn queue(eng: &Engine, entity: Entity, clip_name: &str) {
    with_playback(eng, entity, |playback| {
        playback.queue.push(clip_name.to_string());
    });
}

/// Stop playback, leaving the pose where it is. A no-op on a node that is not
/// playing.
///
/// The clip stops being current, so `current` answers nothing afterwards and
/// `resume` has nothing to go back to — that is what separates this from
/// [`pause`].
pub fn stop(eng: &Engine, entity: Entity) {
    with_playback(eng, entity, |playback| {
        playback.playing = false;
        playback.paused = false;
        playback.queue.clear();
    });
}

/// Hold the playhead where it is, keeping the clip current.
pub fn pause(eng: &Engine, entity: Entity) {
    with_playback(eng, entity, |playback| {
        if playback.playing {
            playback.playing = false;
            playback.paused = true;
        }
    });
}

/// Carry on from where [`pause`] left off. A no-op on anything else.
pub fn resume(eng: &Engine, entity: Entity) {
    with_playback(eng, entity, |playback| {
        if playback.paused {
            playback.paused = false;
            playback.playing = true;
        }
    });
}

/// Scale playback: 2.0 is twice as fast, a negative speed runs the clip
/// backwards. Also what the component's `speed` property writes.
pub fn set_speed(eng: &Engine, entity: Entity, speed: f32) {
    with_playback(eng, entity, |playback| {
        playback.speed = speed;
    });
}

/// Move the playhead to `time` seconds of playback, and pose the node there.
///
/// Posing immediately is what makes a seek visible on a clip that is not
/// advancing — a paused one being scrubbed in an editor timeline, or one that
/// has already ended. A playing clip is posed again by its next fixed step,
/// from the same playhead, so the two agree.
///
/// Nothing between the old playhead and the new one is treated as passed: a
/// seek must not fire the method keys it skipped over, and posing travels no
/// span.
pub fn seek(eng: &Engine, entity: Entity, time: f32) {
    with_playback(eng, entity, |playback| {
        playback.time = time;
    });
    crate::system::pose_now(eng, entity);
}

/// The clip playing or held on `entity`, or `None` once it has ended or been
/// stopped.
#[must_use]
pub fn current(eng: &Engine, entity: Entity) -> Option<String> {
    read(eng, entity, |playback| {
        playback.active().then(|| playback.clip_name.clone())
    })
    .flatten()
}

/// Seconds of playback since the current clip started, before wrapping.
#[must_use]
pub fn time(eng: &Engine, entity: Entity) -> f32 {
    read(eng, entity, |playback| playback.time).unwrap_or_default()
}

/// Whether `entity` has a clip advancing. A paused node answers false.
#[must_use]
pub fn is_playing(eng: &Engine, entity: Entity) -> bool {
    read(eng, entity, |playback| playback.playing).unwrap_or(false)
}

/// The clip that ended on `entity` during the last step, for that one frame.
///
/// The other half of `on_animation_finished`: a script that would rather poll
/// than define a method reads this, and it answers for exactly as long as the
/// method call would have been in flight.
#[must_use]
pub fn just_finished(eng: &Engine, entity: Entity) -> Option<String> {
    read(eng, entity, |playback| playback.finished.clone()).filter(|name| !name.is_empty())
}

/// Give `entity` a clip of its own under `clip_name`, from a definition
/// table.
///
/// The definition goes into the asset cache keyed by its content, so two
/// nodes defining the same clip share one parsed object and defining the same
/// clip twice costs one entry rather than two.
///
/// # Errors
/// If the table is not a clip the `animation_clip` parser accepts.
pub fn define(eng: &Engine, entity: Entity, clip_name: &str, body: toml::Value) -> Result<()> {
    let reference = assets::define_inline(eng, CLIP_ASSET_TYPE, body)
        .with_context(|| format!("defining animation '{clip_name}'"))?
        .to_string();
    // Parse it now rather than at the first `play`, so a malformed definition
    // is an error where it was written.
    assets::load_typed::<Clip>(eng, &reference)
        .with_context(|| format!("defining animation '{clip_name}'"))?;
    with_playback(eng, entity, |playback| {
        playback
            .defined
            .insert(clip_name.to_string(), reference.clone());
    })
    .ok_or_else(|| anyhow!("this node has no `animation` component to define a clip on"))
}
