//! State machines over a player's clips: Godot's `AnimationTree` with an
//! `AnimationNodeStateMachine` root.
//!
//! A `state_machine` asset names states (each a clip of the player's library)
//! and the transitions between them; the `state_machine` component runs one
//! against a node's `animation` player. A transition fires on its own when it
//! is `auto` and its condition holds, or when a script travels through it.
//! Everything is decided on the fixed step, after the players have advanced,
//! so a machine switches clips on the same tick on every machine.

use anyhow::{Result, anyhow, bail};
use balaur_core::Engine;
use balaur_core::collections::DetHashMap;
use balaur_core::hecs::{Entity, World};
use std::collections::VecDeque;
use std::rc::Rc;

use crate::clip::{Clip, Wrap};
use crate::player::{AnimationState, FIXED_DT, Fade, Playback};

/// The asset type a machine's states and transitions are parsed through.
pub const MACHINE_ASSET_TYPE: &str = "state_machine";

/// The component key that runs a machine on a node.
pub(crate) const COMPONENT: &str = "state_machine";

pub(crate) mod keys {
    pub(crate) const ACTIVE: &str = "active";
    pub(crate) const ADVANCE: &str = "advance";
    pub(crate) const CONDITION: &str = "condition";
    pub(crate) const FADE: &str = "fade";
    pub(crate) const FROM: &str = "from";
    pub(crate) const MACHINE: &str = "machine";
    pub(crate) const PLAYER: &str = "player";
    pub(crate) const START: &str = "start";
    pub(crate) const STATES: &str = "states";
    pub(crate) const SWITCH: &str = "switch";
    pub(crate) const TO: &str = "to";
    pub(crate) const TRANSITIONS: &str = "transitions";
}

/// `advance` values: never on its own, only by travel, or on its own too.
const ADVANCE_DISABLED: &str = "disabled";
const ADVANCE_ENABLED: &str = "enabled";
const ADVANCE_AUTO: &str = "auto";

/// `switch` values: cut now, cut now keeping the playhead, or wait for the
/// clip to reach its end.
const SWITCH_IMMEDIATE: &str = "immediate";
const SWITCH_SYNC: &str = "sync";
const SWITCH_AT_END: &str = "at_end";

pub(crate) const MACHINE_ASSET_DOC: &str = r#"A state machine switches a player between clips. `start` is the state
entered first; `[states]` maps each state to the clip it plays from the
player's library (an empty clip is the state's own name). Each transition
names `from` and `to`, a `fade` in seconds, an `advance` (`disabled` never
fires, `enabled` fires only on `animation.travel`, `auto` also fires on its
own), a `switch` (`immediate` once any fade already running has finished,
`sync` the same keeping the playhead, `at_end` fading so the fade ends with
the clip) and an optional `condition` that `animation.set_condition` turns
on.

```toml
type = "state_machine"
start = "idle"

[states]
idle = "idle"
walk = "walk_cycle"

[[transitions]]
from = "idle"
to = "walk"
fade = 0.2
advance = "auto"
condition = "moving"
```"#;

/// When a transition may fire without being travelled through.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Advance {
    Disabled,
    Enabled,
    Auto,
}

/// How a transition cuts over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Switch {
    Immediate,
    Sync,
    AtEnd,
}

#[derive(Clone, Debug)]
pub struct Transition {
    pub from: String,
    pub to: String,
    pub fade: f32,
    pub advance: Advance,
    pub switch: Switch,
    /// The condition that has to be on for `auto` to fire; empty needs none.
    pub condition: String,
}

/// A parsed `state_machine` asset. Shared by every node running it.
#[derive(Debug, Default)]
pub struct Machine {
    pub start: String,
    /// State name to clip name, in authored order.
    pub states: DetHashMap<String, String>,
    pub transitions: Vec<Transition>,
}

impl Machine {
    fn clip_of<'a>(&'a self, state: &'a str) -> &'a str {
        self.states.get(state).map_or(state, |clip| {
            if clip.is_empty() {
                state
            } else {
                clip.as_str()
            }
        })
    }

    fn between(&self, from: &str, to: &str) -> Option<&Transition> {
        self.transitions
            .iter()
            .find(|t| t.from == from && t.to == to && t.advance != Advance::Disabled)
    }

    /// The states to pass through from `from` to reach `to`, `to` last; the
    /// fewest hops, and `None` when no chain of transitions gets there.
    fn path(&self, from: &str, to: &str) -> Option<Vec<String>> {
        let mut came: DetHashMap<&str, &str> = DetHashMap::default();
        let mut open = VecDeque::from([from]);
        while let Some(at) = open.pop_front() {
            if at == to && at != from {
                break;
            }
            for t in self
                .transitions
                .iter()
                .filter(|t| t.from == at && t.advance != Advance::Disabled)
            {
                if t.to != from && !came.contains_key(t.to.as_str()) {
                    came.insert(&t.to, at);
                    open.push_back(&t.to);
                }
            }
        }
        let mut path = vec![to.to_string()];
        let mut at = *came.get(to)?;
        while at != from {
            path.push(at.to_string());
            at = came.get(at)?;
        }
        path.reverse();
        Some(path)
    }
}

/// Parse a `state_machine` document.
///
/// # Errors
/// When a transition names a state the machine does not have, or a word
/// `advance` or `switch` does not know.
pub fn parse(value: &toml::Value) -> Result<Machine> {
    let text = |item: &toml::Value, key: &str| {
        item.get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let mut machine = Machine {
        start: text(value, keys::START),
        ..Machine::default()
    };
    if let Some(states) = value.get(keys::STATES) {
        let states = states.as_table().ok_or_else(|| {
            anyhow!(
                "`states` is {}, not a table of state to clip",
                states.type_str()
            )
        })?;
        for (name, clip) in states {
            let clip = clip.as_str().ok_or_else(|| {
                anyhow!("`states.{name}` is {}, not a clip name", clip.type_str())
            })?;
            machine.states.insert(name.clone(), clip.to_string());
        }
    }
    let known = |state: &str| machine.states.contains_key(state);
    if !machine.start.is_empty() && !known(&machine.start) {
        bail!(
            "`start` names '{}', which is not in `states`",
            machine.start
        );
    }
    let items = value
        .get(keys::TRANSITIONS)
        .and_then(toml::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut transitions = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let (from, to) = (text(item, keys::FROM), text(item, keys::TO));
        for state in [&from, &to] {
            if !known(state) {
                bail!("transition {i} names '{state}', which is not in `states`");
            }
        }
        let advance = match text(item, keys::ADVANCE).as_str() {
            ADVANCE_DISABLED => Advance::Disabled,
            "" | ADVANCE_ENABLED => Advance::Enabled,
            ADVANCE_AUTO => Advance::Auto,
            other => bail!("transition {i}: `advance` is '{other}', not disabled, enabled or auto"),
        };
        let switch = match text(item, keys::SWITCH).as_str() {
            "" | SWITCH_IMMEDIATE => Switch::Immediate,
            SWITCH_SYNC => Switch::Sync,
            SWITCH_AT_END => Switch::AtEnd,
            other => bail!("transition {i}: `switch` is '{other}', not immediate, sync or at_end"),
        };
        let fade = item
            .get(keys::FADE)
            .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
            .unwrap_or_default() as f32;
        transitions.push(Transition {
            from,
            to,
            fade,
            advance,
            switch,
            condition: text(item, keys::CONDITION),
        });
    }
    machine.transitions = transitions;
    Ok(machine)
}

/// One node running a machine.
#[derive(Default)]
pub struct MachineRun {
    /// The asset reference the component named.
    pub reference: String,
    pub machine: Option<Rc<Machine>>,
    /// Path to the node whose `animation` player this drives; empty is this
    /// node.
    pub player: String,
    pub active: bool,
    /// The state the machine is in; empty until it has entered `start`.
    pub current: String,
    /// States still to pass through on the way to where `travel` asked.
    pub travel: Vec<String>,
    /// A state to cut straight to on the next step, with no fade.
    pub jump: Option<String>,
    pub conditions: DetHashMap<String, bool>,
    /// Each state's clip, loaded ahead of the step that needs it.
    pub(crate) clips: DetHashMap<String, Rc<Clip>>,
    /// The asset generation `clips` were loaded at.
    pub(crate) resolved_at: Option<u64>,
}

/// Load what every machine needs before the fixed steps run: its asset, and
/// the clip each state plays from its player's library.
pub(crate) fn prepare(eng: &Engine) {
    let state = eng.resource::<AnimationState>();
    let generation = balaur_core::assets::generation(eng);
    let wanted: Vec<(Entity, String, Option<Entity>)> = {
        let state = state.borrow();
        let world = eng.world();
        state
            .machines
            .iter()
            .filter(|(_, run)| run.active && run.resolved_at != Some(generation))
            .map(|(&entity, run)| {
                (
                    entity,
                    run.reference.clone(),
                    player_of(&world, entity, &run.player),
                )
            })
            .collect()
    };
    for (entity, reference, player) in wanted {
        let machine = if reference.trim().is_empty() {
            None
        } else {
            match balaur_core::assets::load_typed::<Machine>(eng, &reference) {
                Ok(machine) => Some(machine),
                Err(why) => {
                    tracing::warn!("state machine '{reference}': {why:#}");
                    None
                }
            }
        };
        // A player not applied yet leaves the clips for a later frame.
        let references: Option<Vec<(String, String)>> = machine.as_ref().and_then(|machine| {
            let state = state.borrow();
            let playback = state.players.get(&player?)?;
            Some(
                machine
                    .states
                    .keys()
                    .map(|name| (name.clone(), playback.reference(machine.clip_of(name))))
                    .collect(),
            )
        });
        let clips: Vec<(String, Rc<Clip>)> = references
            .iter()
            .flatten()
            .filter_map(|(name, reference)| {
                match balaur_core::assets::load_typed::<Clip>(eng, reference) {
                    Ok(clip) => Some((name.clone(), clip)),
                    Err(why) => {
                        tracing::warn!("state '{name}' plays '{reference}': {why:#}");
                        None
                    }
                }
            })
            .collect();
        let mut state = state.borrow_mut();
        if let Some(run) = state.machines.get_mut(&entity) {
            run.machine = machine;
            run.clips = clips.into_iter().collect();
            if references.is_some() || reference.trim().is_empty() {
                run.resolved_at = Some(generation);
            }
        }
    }
}

fn player_of(world: &World, entity: Entity, path: &str) -> Option<Entity> {
    if path.is_empty() {
        Some(entity)
    } else {
        balaur_core::scene::find_node(world, entity, path)
    }
}

/// One fixed step of every machine, after the players it drives have moved.
/// `ended` holds the players whose clip ended on this step.
pub(crate) fn step(
    world: &World,
    machines: &mut DetHashMap<Entity, MachineRun>,
    players: &mut DetHashMap<Entity, Playback>,
    ended: &[Entity],
) {
    for (&entity, run) in machines.iter_mut() {
        if !run.active {
            continue;
        }
        let Some(machine) = run.machine.clone() else {
            continue;
        };
        let Some(player) = player_of(world, entity, &run.player) else {
            continue;
        };
        let Some(playback) = players.get_mut(&player) else {
            continue;
        };
        if let Some(to) = run.jump.take() {
            enter(run, &machine, playback, &to, 0.0, false);
            continue;
        }
        if run.current.is_empty() {
            if !machine.start.is_empty() {
                let start = machine.start.clone();
                enter(run, &machine, playback, &start, 0.0, false);
            }
            continue;
        }
        let ended = ended.contains(&player);
        if let Some(next) = run.travel.first().cloned() {
            match machine.between(&run.current, &next) {
                Some(t) if ready(t, playback, ended) => {
                    let (fade, sync) = (t.fade, t.switch == Switch::Sync);
                    run.travel.remove(0);
                    enter(run, &machine, playback, &next, fade, sync);
                }
                Some(_) => {}
                None => run.travel.clear(),
            }
            continue;
        }
        let fires = machine.transitions.iter().find(|t| {
            t.from == run.current
                && t.advance == Advance::Auto
                && (t.condition.is_empty()
                    || run.conditions.get(&t.condition).copied().unwrap_or(false))
                && ready(t, playback, ended)
        });
        if let Some(t) = fires {
            let (to, fade, sync) = (t.to.clone(), t.fade, t.switch == Switch::Sync);
            enter(run, &machine, playback, &to, fade, sync);
        }
    }
}

/// Whether `t` may cut over now. As Godot's: an `at_end` transition starts
/// its fade so it finishes with the clip, and any other waits for a fade
/// already running to finish.
fn ready(t: &Transition, playback: &Playback, ended: bool) -> bool {
    if t.switch != Switch::AtEnd {
        return playback.fade.is_none();
    }
    if ended || wrapped(playback) {
        return true;
    }
    let Some(clip) = playback
        .clip
        .as_ref()
        .filter(|_| t.fade > 0.0 && playback.playing)
    else {
        return false;
    };
    let into = if clip.wrap == Wrap::None || clip.length <= 0.0 {
        playback.time
    } else {
        playback.time - libm::floorf(playback.time / clip.length) * clip.length
    };
    clip.length - into <= t.fade
}

/// Whether a looping clip went round its end on the step just taken.
fn wrapped(playback: &Playback) -> bool {
    let Some(clip) = playback.clip.as_ref() else {
        return false;
    };
    if clip.wrap == Wrap::None || clip.length <= 0.0 || !playback.playing {
        return false;
    }
    let before = (playback.time - FIXED_DT * playback.speed).max(0.0);
    // Which pass over the clip each end of the step is on, as a whole number.
    let pass = |time: f32| libm::floorf(time / clip.length) as i64;
    pass(before) != pass(playback.time)
}

fn enter(
    run: &mut MachineRun,
    machine: &Machine,
    playback: &mut Playback,
    to: &str,
    fade: f32,
    sync: bool,
) {
    run.current = to.to_string();
    let Some(clip) = run.clips.get(to).cloned() else {
        return;
    };
    let leaving = playback
        .clip
        .clone()
        .filter(|_| fade > 0.0 && playback.active())
        .map(|leaving| Fade {
            clip_name: playback.clip_name.clone(),
            clip: leaving,
            time: playback.time,
            speed: playback.speed,
            elapsed: 0.0,
            duration: fade,
        });
    playback.clip_name = machine.clip_of(to).to_string();
    playback.clip = Some(clip);
    if !sync {
        playback.time = 0.0;
    }
    playback.playing = true;
    playback.paused = false;
    playback.fade = leaving;
}

fn with_run<T>(eng: &Engine, entity: Entity, f: impl FnOnce(&mut MachineRun) -> T) -> Result<T> {
    let state = eng.resource::<AnimationState>();
    let mut state = state.borrow_mut();
    state
        .machines
        .get_mut(&entity)
        .map(f)
        .ok_or_else(|| anyhow!("this node has no `state_machine` component"))
}

/// Head for `to` through the fewest transitions, each fading as it says; a
/// state no chain of transitions reaches is cut to directly.
///
/// # Errors
/// When the node runs no machine, or the machine has no such state.
pub fn travel(eng: &Engine, entity: Entity, to: &str) -> Result<()> {
    with_run(eng, entity, |run| {
        let machine = run
            .machine
            .clone()
            .ok_or_else(|| anyhow!("the state machine has not loaded"))?;
        if !machine.states.contains_key(to) {
            bail!("the state machine has no state '{to}'");
        }
        match machine.path(&run.current, to) {
            Some(path) if !run.current.is_empty() => run.travel = path,
            _ if run.current == to => run.travel.clear(),
            _ => {
                run.travel.clear();
                run.jump = Some(to.to_string());
            }
        }
        Ok(())
    })?
}

/// Cut to `to` on the next step, with no fade and no transition.
///
/// # Errors
/// When the node runs no machine.
pub fn jump(eng: &Engine, entity: Entity, to: &str) -> Result<()> {
    with_run(eng, entity, |run| {
        run.travel.clear();
        run.jump = Some(to.to_string());
    })
}

/// Turn a condition an `auto` transition waits on on or off.
///
/// # Errors
/// When the node runs no machine.
pub fn set_condition(eng: &Engine, entity: Entity, name: &str, on: bool) -> Result<()> {
    with_run(eng, entity, |run| {
        run.conditions.insert(name.to_string(), on);
    })
}

/// The state the machine is in, or `None` before it has entered one.
#[must_use]
pub fn state(eng: &Engine, entity: Entity) -> Option<String> {
    let state = eng.try_resource::<AnimationState>()?;
    let state = state.borrow();
    state
        .machines
        .get(&entity)
        .map(|run| run.current.clone())
        .filter(|current| !current.is_empty())
}

pub(crate) fn apply(eng: &Engine, entity: Entity, params: &toml::Value) {
    let text = |key: &str| {
        params
            .get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let state = eng.resource::<AnimationState>();
    let mut state = state.borrow_mut();
    let run = state.machines.entry(entity).or_default();
    let reference = text(keys::MACHINE);
    let player = text(keys::PLAYER);
    if run.reference != reference || run.player != player {
        *run = MachineRun::default();
        run.reference = reference;
        run.player = player;
    }
    run.active = params
        .get(keys::ACTIVE)
        .and_then(toml::Value::as_bool)
        .unwrap_or(true);
}

pub(crate) fn remove(eng: &Engine, entity: Entity) {
    if let Some(state) = eng.try_resource::<AnimationState>() {
        state.borrow_mut().machines.shift_remove(&entity);
    }
}

pub(crate) fn get(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.try_resource::<AnimationState>()?;
    let state = state.borrow();
    let run = state.machines.get(&entity)?;
    let mut out = toml::map::Map::new();
    out.insert(keys::MACHINE.into(), run.reference.clone().into());
    out.insert(keys::PLAYER.into(), run.player.clone().into());
    out.insert(keys::ACTIVE.into(), run.active.into());
    Some(toml::Value::Table(out))
}
