//! The `animation` script module, declared against the neutral seam.
//!
//! One declaration list, no language named, so every function here reaches
//! Luau and Rune alike — and a third language the day its backend lands. The
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
    install_transport_api(m);
    install_playhead_api(m);
    install_tween_api(m);
}

/// Starting, queueing and holding a clip.
fn install_transport_api(m: &mut dyn Bindings<Engine>) {
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
    // One verb for both things that can be running on a node: given a node
    // it ends that node's clip, given a tween handle it ends that tween.
    // `kill` would be a third destruction verb beside `node.queue_free` and
    // `physics.clear` (`docs/NAMING.md` N1).
    //
    // Ending a clip is not pausing it: the clip stops being current, so
    // `current` answers nil and `resume` has nothing to go back to.
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
    // The node is posed where the playhead lands, so a seek shows even on a
    // clip that is paused or has ended — which is what an editor timeline
    // scrubs. Nothing between the old playhead and the new one counts as
    // passed: a seek does not fire the method keys it skipped.
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
