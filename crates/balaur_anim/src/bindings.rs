//! The `animation` script module, declared against the neutral seam.
//!
//! One declaration list, no language named, so every function here reaches
//! Rune and a second language the day its backend lands. The
//! module is `animation` rather than `anim` because the script API spells
//! words out (`docs/NAMING.md` D4), and the readers are named for what they
//! return with `is_` kept for the one that answers a boolean (N7).

use anyhow::{anyhow, Result};
use balaur_core::{entity_of, node_api, Engine};
use balaur_script::{Bindings, BindingsExt as _, NodeId, Value};

use crate::player;
use crate::tween::{self, TweenId};

/// Declare `animation` into a binding group.
pub fn install_animation_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Clip playback on a node's `animation` component — starting, holding, \
         seeking — and tweens, short clips generated from a table of steps and \
         addressed by the handle they hand back.",
    );
    install_transport_api(m);
    install_playhead_api(m);
    install_tween_api(m);
}

/// Starting, queueing and holding a clip.
fn install_transport_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("play", &["animation"], "Start the clip of that name on this node; the trailing options table takes `speed` (a multiplier) and `from_start`."),
        ("queue", &["animation"], "Play the clip of that name once the current one ends; a looping clip never ends, so a queue behind one never drains."),
        ("stop", &["animation"], "End the clip on a node, or the tween a handle names, leaving the pose where it is; `resume` cannot revive it."),
        ("pause", &["animation"], "Hold the playhead where it is, keeping the clip current so `resume` has something to go back to."),
        ("resume", &["animation"], "Carry on from where `pause` left off; a stopped, finished or never-started node is left alone."),
        ("define", &["animation"], "Give this node a clip of its own under that name, from a definition table shaped like a scene file's."),
    ]);
    // `opts` is `{ speed = 1.5, from_start = false }`, both optional. A flag
    // in a trailing options table rather than a `play_from_start` (N9).
    m.function(
        "play",
        |eng: &Engine, (node, name, opts): (NodeId, String, Option<Value>)| {
            let entity = entity_of(node)?;
            if let Some(speed) = option(opts.as_ref(), "speed").as_ref().and_then(number) {
                player::set_speed(eng, entity, speed);
            }
            let from_start = !matches!(
                option(opts.as_ref(), "from_start"),
                Some(Value::Bool(false))
            );
            player::play_from(eng, entity, &name, from_start)
        },
    );
    // Plays once the current clip ends. A looping clip never ends, so a queue
    // behind one never drains.
    m.function("queue", |eng: &Engine, (node, name): (NodeId, String)| {
        player::queue(eng, entity_of(node)?, &name);
        Ok(())
    });
    // Takes a node (ends its clip) or a tween handle (ends that tween).
    // Stopping is not pausing: `current` goes nil and `resume` cannot revive it.
    m.function("stop", |eng: &Engine, what: Value| {
        match handle_of(&what)? {
            Stoppable::Node(node) => player::stop(eng, entity_of(node)?),
            Stoppable::Tween(id) => tween::stop(eng, id),
        }
        Ok(())
    });
    m.function("pause", |eng: &Engine, node: NodeId| {
        player::pause(eng, entity_of(node)?);
        Ok(())
    });
    m.function("resume", |eng: &Engine, node: NodeId| {
        player::resume(eng, entity_of(node)?);
        Ok(())
    });
    // A clip of this node's own, from a definition table — the same shape a
    // scene file writes inline, and cached by its content like one.
    m.function(
        "define",
        |eng: &Engine, (node, name, body): (NodeId, String, Value)| {
            player::define(eng, entity_of(node)?, &name, node_api::to_toml(&body)?)
        },
    );
}

/// Where the playhead is, and what it just did.
fn install_playhead_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("seek", &["animation"], "Move the playhead to a number of seconds and pose the node there, even on a paused or ended clip."),
        ("current", &["animation"], "The clip playing or paused on this node, and nil once it has ended, been stopped, or never started."),
        ("time", &["animation"], "Seconds of playback since the current clip started, before wrapping; a stopped clip keeps where it stopped."),
        ("is_playing", &["animation"], "Whether a clip is advancing on this node; a paused, stopped, finished or absent one answers false."),
        ("just_finished", &["animation"], "The clip that ended on this node during the last step, and nil on every other frame."),
    ]);
    // Poses the node even on a paused or ended clip, and does not fire the
    // method keys it skips over.
    m.function("seek", |eng: &Engine, (node, time): (NodeId, f32)| {
        player::seek(eng, entity_of(node)?, time);
        Ok(())
    });
    // The clip playing or paused on this node, or nil once it has ended.
    m.function("current", |eng: &Engine, node: NodeId| {
        Ok(player::current(eng, entity_of(node)?))
    });
    // Seconds of playback since the current clip started, before wrapping.
    m.function("time", |eng: &Engine, node: NodeId| {
        Ok(player::time(eng, entity_of(node)?))
    });
    m.function("is_playing", |eng: &Engine, node: NodeId| {
        let state = eng.resource::<crate::AnimationState>();
        let advancing = state
            .borrow()
            .players
            .get(&entity_of(node)?)
            .is_some_and(|p| p.playing);
        Ok(advancing)
    });
    // The clip that ended on this node last frame, and nil every other frame.
    // The polling half of `on_animation_finished`, for a script that would
    // rather ask than define a method.
    m.function("just_finished", |eng: &Engine, node: NodeId| {
        Ok(player::just_finished(eng, entity_of(node)?))
    });
}

/// A number from an options entry, whichever way the language spelled it.
fn number(value: &Value) -> Option<f32> {
    match value {
        Value::Num(n) => Some(*n as f32),
        Value::Int(i) => Some(*i as f32),
        _ => None,
    }
}

/// One entry of a trailing options table, or `None` when it was left out.
fn option(opts: Option<&Value>, key: &str) -> Option<Value> {
    match opts {
        Some(Value::Map(pairs)) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()),
        _ => None,
    }
}

/// Building and steering a tween.
///
/// A tween is a generated clip, so the only new surface here is how one is
/// described and how it is addressed afterwards — the playing of it is the
/// same code path a clip goes through.
fn install_tween_api(m: &mut dyn Bindings<Engine>) {
    // No component: a tween is generated from the node's current values and
    // kept beside the players, so the node needs no `animation` of its own.
    m.describe(&[
        ("tween", &[], "Generate a clip on the node from a table of steps and run it, returning the handle `stop` and `is_running` take."),
        ("tween_to", &[], "Move one property of the node to a value over a number of seconds on an optional easing curve, returning a handle."),
        ("is_running", &[], "Whether a handle still names a running tween; one that finished, was stopped, or lost its node answers false."),
    ]);
    // `{ loops = 1, speed = 1.0, steps = { ... } }`. Steps run one after
    // another; `parallel = true` joins a step to the one before it. Returns
    // the handle `stop` and `is_running` take.
    m.function("tween", |eng: &Engine, (node, spec): (NodeId, Value)| {
        tween::start(eng, entity_of(node)?, &node_api::to_toml(&spec)?)
    });
    // The 90% case without a table: one property, one destination, one
    // duration, and the curve to get there on.
    m.function(
        "tween_to",
        |eng: &Engine,
         (node, property, to, duration, ease): (NodeId, String, Value, f32, Option<String>)| {
            tween::start_to(
                eng,
                entity_of(node)?,
                &property,
                &node_api::to_toml(&to)?,
                duration,
                ease.as_deref(),
            )
        },
    );
    // Whether a tween handle still names something going. A handle whose
    // tween has finished, been stopped, or died with its node names nothing,
    // and answers false.
    m.function("is_running", |eng: &Engine, what: Value| {
        let state = eng.resource::<crate::AnimationState>();
        let going = state
            .borrow()
            .tweens
            .get(&tween_id(&what)?)
            .is_some_and(|t| t.running);
        Ok(going)
    });
}

/// What `stop` was handed.
enum Stoppable {
    Node(NodeId),
    Tween(TweenId),
}

/// A node or a tween handle, told apart by which one the script passed.
fn handle_of(what: &Value) -> Result<Stoppable> {
    match what {
        Value::Node(bits) => Ok(Stoppable::Node(NodeId(*bits))),
        _ => tween_id(what).map(Stoppable::Tween),
    }
}

/// A tween handle, whichever numeric shape the language handed it back in.
fn tween_id(what: &Value) -> Result<TweenId> {
    match what {
        Value::Int(id) if *id >= 0 => Ok(*id as TweenId),
        Value::Num(id) if *id >= 0.0 => Ok(*id as TweenId),
        other => Err(anyhow!(
            "expected a node or a tween handle, got {}",
            other.type_name()
        )),
    }
}
