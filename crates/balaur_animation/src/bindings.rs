//! The `animation` script module, declared against the neutral seam.
//!
//! The module is `animation` rather than `anim` because the script API spells
//! words out (`docs/NAMING.md` D4), and the readers are named for what they
//! return with `is_` kept for the one that answers a boolean (N7).

use anyhow::{Result, anyhow};
use balaur_core::{Engine, entity_of, node_api};
use balaur_script::{Module, NodeId, Value};

use crate::ease::Easing;
use crate::keys as k;
use crate::player;
use crate::tween::{self, TweenId};

/// Declare `animation` into a binding group.
pub fn install_animation_api(m: &mut Module<Engine>) {
    m.module_doc(
        "Clip playback on a node's `animation` component: play, pause, seek and query. `tween` builds a short clip from a table of steps and returns a handle.",
    );
    install_transport_api(m);
    install_playhead_api(m);
    install_tween_api(m);
    install_machine_api(m);
    m.describe(&[(
        "ease_names",
        &[],
        "() -> [string]",
        "Every curve an `ease` takes, by name: the bare `linear`, then eleven transitions in four modes each.",
    )]);
    m.function("ease_names", |_: &Engine, (): ()| {
        Ok(Value::List(
            crate::ease::names()
                .into_iter()
                .map(|name| Value::Str(name.to_string()))
                .collect(),
        ))
    });
    // The words a clip, a machine and a modifier spell, so a script names
    // `animation::EASE_IN_OUT_SINE` rather than a string it can misspell.
    for (name, value) in crate::CONSTANTS.iter().flat_map(|table| table.iter()) {
        m.constant(name, Value::Str((*value).to_string()));
    }
    for (name, value) in crate::ease_constants() {
        m.constant(&name, Value::Str(value.to_string()));
    }
}

/// Driving a node's `state_machine`.
fn install_machine_api(m: &mut Module<Engine>) {
    const MACHINE: &[&str] = &[crate::machine::COMPONENT];
    m.describe(&[
        ("travel", MACHINE, "", "Head for the named state through the cheapest chain of transitions, each costing its priority and fading as it says; a state no transition reaches is cut to directly."),
        ("jump", MACHINE, "", "Cut the state machine to the named state on the next step, with no fade."),
        ("set_condition", MACHINE, "", "Turn on or off a condition that `auto` transitions wait on."),
        ("current_state", MACHINE, "(node: node) -> string?", "The state the machine is in, or nil before it has entered one."),
    ]);
    m.function("travel", |eng: &Engine, (node, to): (NodeId, String)| {
        crate::machine::travel(eng, entity_of(node)?, &to)
    });
    m.function("jump", |eng: &Engine, (node, to): (NodeId, String)| {
        crate::machine::jump(eng, entity_of(node)?, &to)
    });
    m.function(
        "set_condition",
        |eng: &Engine, (node, name, on): (NodeId, String, bool)| {
            crate::machine::set_condition(eng, entity_of(node)?, &name, on)
        },
    );
    m.function("current_state", |eng: &Engine, node: NodeId| {
        Ok(crate::machine::current_state(eng, entity_of(node)?).map_or(Value::Nil, Value::text))
    });
}

/// Starting, queueing and holding a clip.
fn install_transport_api(m: &mut Module<Engine>) {
    m.describe(&[
        ("play", &[crate::COMPONENT], "(node: node, name: string, opts: PlayOptions?)", "Start the clip of that name on this node; the trailing options table takes `speed_scale` (a multiplier), `from_start`, `blend_time` (seconds to blend out of the clip before), `ease` (the blend's curve, an `EASE_*` constant), and `retarget` (a `bone_map` reference, so this rig can play another rig's clips)."),
        ("queue", &[crate::COMPONENT], "", "Play the clip of that name once the current one ends; a looping clip never ends, so a queue behind one never drains."),
        ("stop", &[crate::COMPONENT], "", "End the clip on this node, leaving the pose where it is; `resume` cannot revive it."),
        ("pause", &[crate::COMPONENT], "", "Hold the playhead where it is, keeping the clip current so `resume` has something to go back to."),
        ("resume", &[crate::COMPONENT], "", "Carry on from where `pause` left off; a stopped, finished or never-started node is left alone."),
        ("add_clip", &[crate::COMPONENT], "(node: node, name: string, clip: Clip)", "Give this node a clip of its own under that name, shaped like a scene file's."),
    ]);
    m.records(&[
        (
            "PlayOptions",
            "{ speed_scale: float?, from_start: bool?, blend_time: float?, ease: string?, retarget: string? }",
            "How `play` starts a clip: its speed, whether from the start, the seconds and curve it blends over, and a `bone_map` to retarget another rig's clip.",
        ),
        (
            "Clip",
            "{ length: float?, loop_mode: string?, tracks: [Track]? }",
            "A clip as a scene file spells one: how long it lasts, the last key's time unless given, how it loops, and its tracks.",
        ),
        (
            "Track",
            "{ target: string?, property: string?, interpolation: string?, keys: [Key] }",
            "What one clip track drives: the node path it acts on, the property, or none for a track of method calls, and its keys.",
        ),
        (
            "Key",
            "{ time: float, value: any, ease: string?, call: string?, args: [any]? }",
            "One key: its time, and the `value` its track's property takes there, or the method it calls and what with.",
        ),
    ]);
    m.function(
        "play",
        |eng: &Engine, (node, name, opts): (NodeId, String, Option<Value>)| {
            let entity = entity_of(node)?;
            if let Some(scale) = option(opts.as_ref(), k::SPEED_SCALE)
                .as_ref()
                .and_then(number)
            {
                player::set_speed_scale(eng, entity, scale);
            }
            // Before the clip starts: a map that will not load should stop
            // the call rather than let one frame play unretargeted.
            if let Some(Value::Str(reference)) = option(opts.as_ref(), k::RETARGET) {
                player::set_retarget(eng, entity, &reference)?;
            }
            let from_start = !matches!(
                option(opts.as_ref(), k::FROM_START),
                Some(Value::Bool(false))
            );
            let blend_time = option(opts.as_ref(), k::BLEND_TIME)
                .as_ref()
                .and_then(number)
                .unwrap_or(0.0);
            let ease = match option(opts.as_ref(), k::EASE) {
                Some(Value::Str(name)) => Easing::parse(&name)?,
                _ => Easing::LINEAR,
            };
            player::play_blended(eng, entity, &name, blend_time, ease, from_start)
        },
    );
    // Plays once the current clip ends. A looping clip never ends, so a queue
    // behind one never drains.
    m.function("queue", |eng: &Engine, (node, name): (NodeId, String)| {
        player::queue(eng, entity_of(node)?, &name);
        Ok(())
    });
    // Stopping is not pausing: `current_clip` goes nil and `resume` cannot revive it.
    m.function("stop", |eng: &Engine, node: NodeId| {
        player::stop(eng, entity_of(node)?);
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
    // A clip of this node's own, from a definition table: the same shape a
    // scene file writes inline, and cached by its content like one.
    m.function(
        "add_clip",
        |eng: &Engine, (node, name, body): (NodeId, String, Value)| {
            player::add_clip(eng, entity_of(node)?, &name, node_api::to_toml(&body)?)
        },
    );
}

/// Where the playhead is, and what it just did.
fn install_playhead_api(m: &mut Module<Engine>) {
    m.describe(&[
        ("seek", &[crate::COMPONENT], "", "Move the playhead to a number of seconds and pose the node there, even on a paused or ended clip."),
        ("current_clip", &[crate::COMPONENT], "", "The clip playing or paused on this node, and nil once it has ended, been stopped, or never started."),
        ("time", &[crate::COMPONENT], "", "Seconds of playback since the current clip started, before wrapping; a stopped clip keeps where it stopped."),
        ("is_playing", &[crate::COMPONENT], "", "Whether a clip is advancing on this node; a paused, stopped, finished or absent one answers false."),
    ]);
    // Poses the node even on a paused or ended clip, and does not fire the
    // method keys it skips over.
    m.function("seek", |eng: &Engine, (node, time): (NodeId, f32)| {
        player::seek(eng, entity_of(node)?, time);
        Ok(())
    });
    // The clip playing or paused on this node, or nil once it has ended.
    m.function("current_clip", |eng: &Engine, node: NodeId| {
        Ok(player::current_clip(eng, entity_of(node)?))
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
/// described and how it is addressed afterwards: the playing of it is the
/// same code path a clip goes through.
fn install_tween_api(m: &mut Module<Engine>) {
    // No component: a tween is generated from the node's current values and
    // kept beside the players, so the node needs no `animation` of its own.
    m.describe(&[
        ("tween", &[], "(node: node, spec: Tween) -> int", "Generate a clip on the node from a table of steps and run it, returning the handle `stop_tween` and `is_tween_running` take. The table also takes `delay` in seconds, `then = <handle>` to wait for another tween, `loops` and `speed_scale`; a step's `call` names a method; the node's `on_tween_finished(handle)` is called when it runs out."),
        ("stop_tween", &[], "(handle: int)", "End the tween a handle names, leaving the pose where it is."),
        ("is_tween_running", &[], "(handle: int) -> bool", "Whether a handle still names a running tween; one that finished, was stopped, or lost its node answers false. Takes a tween handle, where `is_playing` takes a node and asks about its clip."),
        ("tween_float", &[], "(from: float, to: float, seconds: float, ease: string?) -> int", "A tween over a number that drives no node: read it each frame with `tween_float_of` and write it wherever you like. Returns a handle `stop_tween` takes."),
        ("tween_float_of", &[], "(handle: int) -> float?", "Where a number tween has got to; nil once it is over."),
    ]);
    m.records(&[
        (
            "Tween",
            "{ steps: [Step], loops: int?, speed_scale: float?, delay: float?, then: int? }",
            "A tween: its steps in order, how often it loops, its speed, the seconds it waits, and the tween it waits for.",
        ),
        (
            "Step",
            "{ property: string?, to: any, by: any, from: any, duration: float?, ease: string?, parallel: bool?, target: string?, call: string?, interval: float? }",
            "One step of a tween: a `property` going `to` a value or `by` an offset over `duration` seconds on an `ease`, a `call` of a method, or an `interval` waited; `parallel` runs it beside the step before, `target` names the node it acts on.",
        ),
    ]);
    m.function("tween", |eng: &Engine, (node, spec): (NodeId, Value)| {
        tween::start(
            eng,
            entity_of(node)?,
            &balaur_core::node_api::to_toml(&spec)?,
        )
    });
    // The 90% case without a table: one property, one destination, one
    // duration, and the curve to get there on.
    m.function(
        "tween_float",
        |eng: &Engine, (from, to, duration, ease): (f32, f32, f32, Option<String>)| {
            tween::start_float(eng, from, to, duration, ease.as_deref())
        },
    );
    m.function("tween_float_of", |eng: &Engine, handle: i64| {
        Ok(tween::float_of(eng, tween_id(handle)?))
    });
    m.function("stop_tween", |eng: &Engine, handle: i64| {
        tween::stop(eng, tween_id(handle)?);
        Ok(())
    });
    // Whether a tween handle still names something going. A handle whose
    // tween has finished, been stopped, or died with its node names nothing,
    // and answers false.
    m.function("is_tween_running", |eng: &Engine, handle: i64| {
        let state = eng.resource::<crate::AnimationState>();
        let going = state
            .borrow()
            .tweens
            .get(&tween_id(handle)?)
            .is_some_and(|t| t.running);
        Ok(going)
    });
}

fn tween_id(handle: i64) -> Result<TweenId> {
    TweenId::try_from(handle).map_err(|_| anyhow!("{handle} is no tween handle"))
}
