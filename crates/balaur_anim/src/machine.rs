//! State machines over a player's clips: Godot's `AnimationTree` with an
//! `AnimationNodeStateMachine` root.
//!
//! A `state_machine` asset names states (each a clip of the player's library)
//! and the transitions between them; the `state_machine` component runs one
//! against a node's `animation` player. A transition fires on its own when it
//! is `auto` and its condition holds, or when a script travels through it.
//! Everything is decided on the fixed step, after the players have advanced,
//! so a machine switches clips on the same tick on every machine.

use anyhow::{Context, Result, anyhow, bail};
use balaur_core::Engine;
use balaur_core::collections::DetHashMap;
use balaur_core::hecs::{Entity, World};
use std::rc::Rc;

use crate::clip::{Clip, LoopMode};
use crate::ease::{Easing, Points};
use crate::keys as k;
use crate::player::{AnimationState, Playback, fixed_dt};
use crate::words as w;

/// The asset type a machine's states and transitions are parsed through.
pub const MACHINE_ASSET_TYPE: &str = "state_machine";

/// The component key that runs a machine on a node.
pub const COMPONENT: &str = "state_machine";

/// Emitted from the machine's node, with the state, as it enters one.
pub const STATE_STARTED_EVENT: &str = "state_started";
/// Emitted from the machine's node, with the state, as it leaves one.
pub const STATE_FINISHED_EVENT: &str = "state_finished";
/// The script methods the two events call on the machine's node.
const STATE_STARTED_METHOD: &str = "on_state_started";
const STATE_FINISHED_METHOD: &str = "on_state_finished";

/// Godot's default priority: every hop costs the same, so travel takes the
/// fewest.
const DEFAULT_PRIORITY: u32 = 1;

/// Joins a nested machine's name to its states: `locomotion/walk`.
pub const GROUP_SEPARATOR: &str = "/";

pub(crate) const MACHINE_ASSET_DOC: &str = r#"Switches an animation player between clips. `start` is the first state, `[states]` maps states to clips or to nested machines, each `[[transitions]]` entry names `from`, `to`, `blend_time`, `ease` or `blend_curve`, `advance_mode`, `switch_mode`, `condition`, `check`, `priority`, `reset` and `break_loop_at_end`. A transition to `end` stops the machine until a travel or a jump.

```toml
type = "state_machine"
start = "idle"

[states]                         # state = clip in the player's library; "" is the state's own name
idle = "idle"

[states.move]                    # a nested machine: its states are move/walk and move/run
start = "walk"
states = { walk = "", run = "run_cycle" }

[[transitions]]
from = "idle"
to = "move"                      # entering a nested machine enters its start
blend_time = 0.2                 # seconds
ease = "in_out_sine"             # the curve the blend follows; linear by default
advance_mode = "auto"            # disabled, enabled (fires on animation.travel) or auto
switch_mode = "immediate"        # immediate, sync (keeps the playhead) or at_end
condition = "moving"             # turned on by animation.set_condition
check = "can_move"               # a script method that has to answer true, asked each frame
priority = 1                     # lower wins among auto transitions and on travel
reset = true                     # false resumes where the state was last left
break_loop_at_end = false        # true holds a looping clip's end while it blends out

[[transitions]]
from = "move"                    # leaves from any state inside the nested machine
to = "end"
blend_curve = [[0.0, 0.0], [0.3, 0.8], [1.0, 1.0]]   # [u, weight] points, in place of ease
```"#;

/// When a transition may fire without being travelled through.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdvanceMode {
    Disabled,
    Enabled,
    Auto,
}

/// How a transition cuts over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchMode {
    Immediate,
    Sync,
    AtEnd,
}

#[derive(Clone, Debug)]
pub struct Transition {
    pub from: String,
    pub to: String,
    pub blend_time: f32,
    pub advance_mode: AdvanceMode,
    pub switch_mode: SwitchMode,
    /// The condition that has to be on for `auto` to fire; empty needs none.
    pub condition: String,
    /// The curve the fade's weight follows.
    pub ease: Easing,
    /// Lower is preferred: among `auto` transitions ready at once, and as
    /// the cost of a hop when travelling.
    pub priority: u32,
    /// Whether the state entered starts from its beginning, or resumes where
    /// the machine last left it.
    pub reset: bool,
    /// Whether a looping clip being left holds its end rather than wrapping
    /// while it fades out.
    pub break_loop_at_end: bool,
    /// A method on a script that has to answer true for `auto` to fire;
    /// empty asks nothing. Godot's `advance_expression`.
    pub check: String,
    /// A drawn curve for the fade, in place of `ease`.
    pub curve: Option<Points>,
}

/// A parsed `state_machine` asset. Shared by every node running it.
///
/// A nested machine is flattened into this one: its states are named
/// `group/state`, entering the group enters its `start`, and a transition
/// from the group leaves from any state inside it.
#[derive(Debug, Default)]
pub struct Machine {
    pub start: String,
    /// State name to clip name, in authored order.
    pub states: DetHashMap<String, String>,
    /// Each nested machine's name, and the state entering it lands on.
    pub groups: DetHashMap<String, String>,
    pub transitions: Vec<Transition>,
}

impl Machine {
    /// The state `name` lands on: itself, a group's entry state, or `end`.
    #[must_use]
    pub fn entry_of(&self, name: &str) -> Option<String> {
        if name == w::END || self.states.contains_key(name) {
            return Some(name.to_string());
        }
        self.groups.get(name).cloned()
    }

    /// Every state inside the group `name`, nested groups' included.
    fn members(&self, name: &str) -> Vec<String> {
        let prefix = format!("{name}{GROUP_SEPARATOR}");
        self.states
            .keys()
            .filter(|state| state.starts_with(&prefix))
            .cloned()
            .collect()
    }

    fn clip_of<'a>(&'a self, state: &'a str) -> &'a str {
        self.states.get(state).map_or(state, |clip| {
            if clip.is_empty() {
                state
            } else {
                clip.as_str()
            }
        })
    }

    /// The transition a travel takes from `from` to `to`: the lowest
    /// priority, as the path was costed.
    fn between(&self, from: &str, to: &str) -> Option<&Transition> {
        self.transitions
            .iter()
            .filter(|t| t.from == from && t.to == to && t.advance_mode != AdvanceMode::Disabled)
            .min_by_key(|t| t.priority)
    }

    /// The states to pass through from `from` to reach `to`, `to` last: the
    /// cheapest chain, each hop costing its transition's priority, the
    /// earlier-authored on a tie. `None` when no chain gets there.
    fn path(&self, from: &str, to: &str) -> Option<Vec<String>> {
        let mut best: DetHashMap<&str, (u64, &str)> = DetHashMap::default();
        let mut open: Vec<(u64, &str)> = vec![(0, from)];
        let mut done: Vec<&str> = Vec::new();
        while let Some(next) = (0..open.len()).min_by_key(|&i| open[i].0) {
            let (cost, at) = open.remove(next);
            if done.contains(&at) {
                continue;
            }
            done.push(at);
            if at == to && at != from {
                break;
            }
            for t in self
                .transitions
                .iter()
                .filter(|t| t.from == at && t.advance_mode != AdvanceMode::Disabled && t.to != from)
            {
                let reach = cost + u64::from(t.priority);
                if best
                    .get(t.to.as_str())
                    .is_none_or(|&(known, _)| reach < known)
                {
                    best.insert(&t.to, (reach, at));
                    open.push((reach, &t.to));
                }
            }
        }
        let mut path = vec![to.to_string()];
        let mut at = best.get(to)?.1;
        while at != from {
            path.push(at.to_string());
            at = best.get(at)?.1;
        }
        path.reverse();
        Some(path)
    }
}

/// Parse a `state_machine` document, flattening any machine nested in it.
///
/// # Errors
/// When a transition names a state the machine does not have, or a word
/// `advance_mode` or `switch_mode` does not know.
pub fn parse(value: &toml::Value) -> Result<Machine> {
    let mut machine = Machine::default();
    machine.start = flatten(&mut machine, value, "")?;
    Ok(machine)
}

/// Add one level of `value` under `prefix`, and answer the state entering it
/// lands on; empty when the root names no `start`.
fn flatten(machine: &mut Machine, value: &toml::Value, prefix: &str) -> Result<String> {
    if let Some(states) = value.get(k::STATES) {
        let states = states.as_table().ok_or_else(|| {
            anyhow!(
                "`states` is {}, not a table of state to clip",
                states.type_str()
            )
        })?;
        for (name, state) in states {
            if name == w::END || name.contains(GROUP_SEPARATOR) {
                bail!(
                    "`{name}` cannot name a state: `{}` is the machine's end, and `{GROUP_SEPARATOR}` joins a group to its states",
                    w::END
                );
            }
            let full = format!("{prefix}{name}");
            match state {
                toml::Value::String(clip) if clip.is_empty() => {
                    machine.states.insert(full, name.clone());
                }
                toml::Value::String(clip) => {
                    machine.states.insert(full, clip.clone());
                }
                toml::Value::Table(_) => {
                    let inner = format!("{full}{GROUP_SEPARATOR}");
                    let entry = flatten(machine, state, &inner)
                        .with_context(|| format!("state `{full}`"))?;
                    if entry.is_empty() {
                        bail!("state `{full}` is a machine with no `start`");
                    }
                    machine.groups.insert(full, entry);
                }
                other => bail!(
                    "`states.{name}` is {}, not a clip name or a machine",
                    other.type_str()
                ),
            }
        }
    }
    let start = text(value, k::START);
    let entry = if start.is_empty() {
        String::new()
    } else {
        machine
            .entry_of(&format!("{prefix}{start}"))
            .filter(|entry| entry != w::END)
            .ok_or_else(|| anyhow!("`start` names '{start}', which is not in `states`"))?
    };
    let items = value
        .get(k::TRANSITIONS)
        .and_then(toml::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for (i, item) in items.iter().enumerate() {
        let parsed = parse_transition(machine, prefix, i, item)?;
        machine.transitions.extend(parsed);
    }
    Ok(entry)
}

fn text(item: &toml::Value, key: &str) -> String {
    item.get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Transition `i` of the level under `prefix`: one transition, or one from
/// each state inside a group it leaves.
fn parse_transition(
    machine: &Machine,
    prefix: &str,
    i: usize,
    item: &toml::Value,
) -> Result<Vec<Transition>> {
    let (from, to) = (text(item, k::FROM), text(item, k::TO));
    let unknown = |state: &str| anyhow!("transition {i} names '{state}', which is not in `states`");
    if from == w::END {
        bail!("transition {i} leaves `{}`, which nothing leaves", w::END);
    }
    if to == w::END && !prefix.is_empty() {
        bail!(
            "transition {i}: a nested machine cannot reach `{}`; leave it by a transition from its group",
            w::END
        );
    }
    let to = machine
        .entry_of(&format!("{prefix}{to}"))
        .or_else(|| (to == w::END).then(|| to.clone()))
        .ok_or_else(|| unknown(&to))?;
    let whole = format!("{prefix}{from}");
    let froms = if machine.states.contains_key(&whole) {
        vec![whole]
    } else if machine.groups.contains_key(&whole) {
        machine.members(&whole)
    } else {
        return Err(unknown(&from));
    };
    let advance_mode = match text(item, k::ADVANCE_MODE).as_str() {
        w::DISABLED => AdvanceMode::Disabled,
        "" | w::ENABLED => AdvanceMode::Enabled,
        w::AUTO => AdvanceMode::Auto,
        other => bail!(
            "transition {i}: `advance_mode` is '{other}', not {}, {} or {}",
            w::DISABLED,
            w::ENABLED,
            w::AUTO
        ),
    };
    let switch_mode = match text(item, k::SWITCH_MODE).as_str() {
        "" | w::IMMEDIATE => SwitchMode::Immediate,
        w::SYNC => SwitchMode::Sync,
        w::AT_END => SwitchMode::AtEnd,
        other => bail!(
            "transition {i}: `switch_mode` is '{other}', not {}, {} or {}",
            w::IMMEDIATE,
            w::SYNC,
            w::AT_END
        ),
    };
    let blend_time = item
        .get(k::BLEND_TIME)
        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
        .unwrap_or_default() as f32;
    let ease = match text(item, k::EASE).as_str() {
        "" => Easing::LINEAR,
        name => Easing::parse(name).with_context(|| format!("transition {i}"))?,
    };
    let priority = match item.get(k::PRIORITY) {
        None => DEFAULT_PRIORITY,
        Some(v) => v
            .as_integer()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| {
                anyhow!("transition {i}: `priority` is {v}, not a whole number of 0 or more")
            })?,
    };
    let curve = item
        .get(k::BLEND_CURVE)
        .map(Points::parse)
        .transpose()
        .with_context(|| format!("transition {i}: `{}`", k::BLEND_CURVE))?;
    let flag = |key: &str, default: bool| {
        item.get(key)
            .and_then(toml::Value::as_bool)
            .unwrap_or(default)
    };
    let template = Transition {
        from: String::new(),
        to,
        blend_time,
        advance_mode,
        switch_mode,
        condition: text(item, k::CONDITION),
        ease,
        priority,
        reset: flag(k::RESET, true),
        break_loop_at_end: flag(k::BREAK_LOOP_AT_END, false),
        check: text(item, k::CHECK),
        curve,
    };
    Ok(froms
        .into_iter()
        .map(|from| Transition {
            from,
            ..template.clone()
        })
        .collect())
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
    pub enabled: bool,
    /// The state the machine is in; empty until it has entered `start`, and
    /// again once it has reached `end`.
    pub current: String,
    /// Whether it reached `end`: it enters nothing more until a travel or a
    /// jump.
    pub ended: bool,
    /// Path to the node whose script answers the transitions' `check`
    /// methods; empty is this node.
    pub check_node: String,
    /// What each `check` method answered this frame.
    pub(crate) checks: DetHashMap<String, bool>,
    /// States still to pass through on the way to where `travel` asked.
    pub travel: Vec<String>,
    /// A state to cut straight to on the next step, with no fade.
    pub jump: Option<String>,
    /// A travel asked for before the machine loaded, routed once it has.
    pub pending: Option<String>,
    pub conditions: DetHashMap<String, bool>,
    /// Where each state's clip was when the machine last left it, for a
    /// transition that does not `reset`.
    pub times: DetHashMap<String, f32>,
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
            .filter(|(_, run)| run.enabled && run.resolved_at != Some(generation))
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

/// A machine that changed state on a step, for the events that say so.
pub(crate) struct Moved {
    pub(crate) entity: Entity,
    /// Empty when the machine was entering its first state.
    pub(crate) left: String,
    /// Empty when the machine reached `end`.
    pub(crate) entered: String,
}

/// How a state is entered: over a transition, or cut to.
#[derive(Clone)]
struct Entry {
    fade: f32,
    ease: Easing,
    curve: Option<Points>,
    sync: bool,
    reset: bool,
    break_loop: bool,
}

impl Entry {
    const CUT: Self = Self {
        fade: 0.0,
        ease: Easing::LINEAR,
        curve: None,
        sync: false,
        reset: true,
        break_loop: false,
    };

    fn over(t: &Transition) -> Self {
        Self {
            fade: t.blend_time,
            ease: t.ease,
            curve: t.curve.clone(),
            sync: t.switch_mode == SwitchMode::Sync,
            reset: t.reset,
            break_loop: t.break_loop_at_end,
        }
    }
}

/// One fixed step of every machine, after the players it drives have moved.
/// `ended` holds the players whose clip ended on this step; every change of
/// state lands in `moved`.
pub(crate) fn step(
    world: &World,
    machines: &mut DetHashMap<Entity, MachineRun>,
    players: &mut DetHashMap<Entity, Playback>,
    ended: &[Entity],
    paused: balaur_core::process::Pause,
    moved: &mut Vec<Moved>,
) {
    for (&entity, run) in machines.iter_mut() {
        if !run.enabled || !balaur_core::process::ticks(world, entity, paused) {
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
        let left = run.current.clone();
        let to = decide(run, &machine, playback, ended.contains(&player));
        if let Some((to, entry)) = to {
            if to == w::END {
                finish(run, playback);
            } else {
                enter(run, &machine, playback, &to, entry);
            }
            moved.push(Moved {
                entity,
                left,
                entered: run.current.clone(),
            });
        }
    }
}

/// The state `run` moves to on this step, and how, if it moves at all.
fn decide(
    run: &mut MachineRun,
    machine: &Machine,
    playback: &Playback,
    ended: bool,
) -> Option<(String, Entry)> {
    if let Some(to) = run.pending.take() {
        if let Some(to) = machine.entry_of(&to) {
            route(run, machine, &to);
        } else {
            tracing::warn!("travel: the state machine has no state '{to}'");
        }
    }
    if let Some(to) = run.jump.take() {
        if let Some(to) = machine.entry_of(&to) {
            return Some((to, Entry::CUT));
        }
        tracing::warn!("jump: the state machine has no state '{to}'");
    }
    if run.ended {
        return None;
    }
    if run.current.is_empty() {
        return (!machine.start.is_empty()).then(|| (machine.start.clone(), Entry::CUT));
    }
    if let Some(next) = run.travel.first().cloned() {
        match machine.between(&run.current, &next) {
            Some(t) if ready(t, playback, ended) => {
                run.travel.remove(0);
                return Some((next, Entry::over(t)));
            }
            Some(_) => {}
            None => run.travel.clear(),
        }
        return None;
    }
    // Godot's order: the lowest priority is chosen first and then waits on
    // its own switch mode; among equals, one ready now goes first.
    machine
        .transitions
        .iter()
        .filter(|t| {
            t.from == run.current
                && t.advance_mode == AdvanceMode::Auto
                && (t.condition.is_empty()
                    || run.conditions.get(&t.condition).copied().unwrap_or(false))
                && (t.check.is_empty() || run.checks.get(&t.check).copied().unwrap_or(false))
        })
        .min_by_key(|t| (t.priority, !ready(t, playback, ended)))
        .filter(|t| ready(t, playback, ended))
        .map(|t| (t.to.clone(), Entry::over(t)))
}

/// Tell each machine's node what it left and what it entered: an event a
/// binding row answers, and a method on its script.
pub(crate) fn announce(eng: &Engine, moved: &[Moved]) {
    for m in moved {
        if !m.left.is_empty() {
            emit(
                eng,
                m.entity,
                STATE_FINISHED_EVENT,
                STATE_FINISHED_METHOD,
                &m.left,
            );
        }
        if !m.entered.is_empty() {
            emit(
                eng,
                m.entity,
                STATE_STARTED_EVENT,
                STATE_STARTED_METHOD,
                &m.entered,
            );
        }
    }
}

fn emit(eng: &Engine, entity: Entity, event: &str, method: &str, state: &str) {
    let value = balaur_script::Value::Str(state.to_string());
    balaur_core::events::emit_from(eng, entity, event, value.clone());
    if let Some(host) = eng.script_host() {
        host.call_on(balaur_core::node_id_of(entity), method, &[value]);
    }
}

/// Whether `t` may cut over now. As Godot's: an `at_end` transition starts
/// its fade so it finishes with the clip, and any other waits for a fade
/// already running to finish.
fn ready(t: &Transition, playback: &Playback, ended: bool) -> bool {
    if t.switch_mode != SwitchMode::AtEnd {
        return playback.fades.is_empty();
    }
    if ended || wrapped(playback) {
        return true;
    }
    let Some(clip) = playback
        .clip
        .as_ref()
        .filter(|_| t.blend_time > 0.0 && playback.playing)
    else {
        return false;
    };
    let into = if clip.loop_mode == LoopMode::None || clip.length <= 0.0 {
        playback.time
    } else {
        playback.time - libm::floorf(playback.time / clip.length) * clip.length
    };
    clip.length - into <= t.blend_time
}

/// Whether a looping clip went round its end on the step just taken.
fn wrapped(playback: &Playback) -> bool {
    let Some(clip) = playback.clip.as_ref() else {
        return false;
    };
    if clip.loop_mode == LoopMode::None || clip.length <= 0.0 || !playback.playing {
        return false;
    }
    let before = (playback.time - fixed_dt() * playback.speed_scale).max(0.0);
    // Which pass over the clip each end of the step is on, as a whole number.
    let pass = |time: f32| libm::floorf(time / clip.length) as i64;
    pass(before) != pass(playback.time)
}

fn enter(run: &mut MachineRun, machine: &Machine, playback: &mut Playback, to: &str, entry: Entry) {
    if !run.current.is_empty() && playback.active() {
        run.times.insert(run.current.clone(), playback.time);
    }
    run.current = to.to_string();
    run.ended = false;
    let Some(clip) = run.clips.get(to).cloned() else {
        return;
    };
    let leaving = crate::player::leaving(
        playback,
        entry.fade,
        entry.ease,
        entry.curve,
        entry.break_loop,
    );
    playback.clip_name = machine.clip_of(to).to_string();
    playback.clip = Some(clip);
    if !entry.sync {
        playback.time = match run.times.get(to) {
            Some(&time) if !entry.reset => time,
            _ => 0.0,
        };
    }
    playback.playing = true;
    playback.paused = false;
    match leaving {
        Some(leaving) => playback.fades.push(leaving),
        None => playback.fades.clear(),
    }
}

/// Leave the last state for `end`: the clip holds its pose, and nothing more
/// is entered until a travel or a jump.
fn finish(run: &mut MachineRun, playback: &mut Playback) {
    if !run.current.is_empty() && playback.active() {
        run.times.insert(run.current.clone(), playback.time);
    }
    run.current.clear();
    run.travel.clear();
    run.ended = true;
    playback.playing = false;
    playback.paused = false;
    playback.fades.clear();
}

/// Ask the scripts every running machine's current transitions `check`, and
/// keep the answers for this frame's steps. Before the steps, because a
/// script may read animation state the steps hold borrowed.
pub(crate) fn evaluate_checks(eng: &Engine) {
    let Some(host) = eng.script_host() else {
        return;
    };
    let asks: Vec<(Entity, Entity, Vec<String>)> = {
        let state = eng.resource::<AnimationState>();
        let state = state.borrow();
        let world = eng.world();
        state
            .machines
            .iter()
            .filter(|(_, run)| run.enabled)
            .filter_map(|(&entity, run)| {
                let machine = run.machine.as_ref()?;
                let mut methods: Vec<String> = Vec::new();
                for t in machine.transitions.iter().filter(|t| t.from == run.current) {
                    if !t.check.is_empty() && !methods.contains(&t.check) {
                        methods.push(t.check.clone());
                    }
                }
                let node = player_of(&world, entity, &run.check_node)?;
                (!methods.is_empty()).then_some((entity, node, methods))
            })
            .collect()
    };
    let answers: Vec<(Entity, String, bool)> = asks
        .into_iter()
        .flat_map(|(entity, node, methods)| {
            methods
                .into_iter()
                .map(move |method| (entity, node, method))
        })
        .map(|(entity, node, method)| {
            let answer = host.call_on(balaur_core::node_id_of(node), &method, &[]);
            (
                entity,
                method,
                answer == Some(balaur_script::Value::Bool(true)),
            )
        })
        .collect();
    let state = eng.resource::<AnimationState>();
    let mut state = state.borrow_mut();
    for run in state.machines.values_mut() {
        run.checks.clear();
    }
    for (entity, method, on) in answers {
        if let Some(run) = state.machines.get_mut(&entity) {
            run.checks.insert(method, on);
        }
    }
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

/// Head for `to` through the cheapest chain of transitions, each fading as it
/// says; a state no chain reaches is cut to directly. Before the machine has
/// loaded, the travel waits for it.
///
/// # Errors
/// When the node runs no machine, or the machine has no such state.
pub fn travel(eng: &Engine, entity: Entity, to: &str) -> Result<()> {
    with_run(eng, entity, |run| {
        // Not loaded yet, as on the frame a script turns the machine on.
        let Some(machine) = run.machine.clone() else {
            run.travel.clear();
            run.pending = Some(to.to_string());
            return Ok(());
        };
        let Some(to) = machine.entry_of(to) else {
            bail!("the state machine has no state '{to}'");
        };
        route(run, &machine, &to);
        Ok(())
    })?
}

/// Point `run` at `to` through the cheapest chain of transitions, or at a cut
/// when none reaches it.
fn route(run: &mut MachineRun, machine: &Machine, to: &str) {
    match machine.path(&run.current, to) {
        Some(path) if !run.current.is_empty() => run.travel = path,
        _ if run.current == to => run.travel.clear(),
        _ => {
            run.travel.clear();
            run.jump = Some(to.to_string());
        }
    }
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
pub fn current_state(eng: &Engine, entity: Entity) -> Option<String> {
    let state = eng.try_resource::<AnimationState>()?;
    let state = state.borrow();
    state
        .machines
        .get(&entity)
        .map(|run| run.current.clone())
        .filter(|current| !current.is_empty())
}

pub(crate) fn apply(eng: &Engine, entity: Entity, params: &toml::Value) {
    let state = eng.resource::<AnimationState>();
    let mut state = state.borrow_mut();
    let run = state.machines.entry(entity).or_default();
    let reference = text(params, k::MACHINE);
    let player = text(params, k::PLAYER);
    if run.reference != reference || run.player != player {
        *run = MachineRun::default();
        run.reference = reference;
        run.player = player;
    }
    run.enabled = params
        .get(k::ENABLED)
        .and_then(toml::Value::as_bool)
        .unwrap_or(true);
    run.check_node = text(params, k::CHECK_NODE);
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
    out.insert(k::MACHINE.into(), run.reference.clone().into());
    out.insert(k::PLAYER.into(), run.player.clone().into());
    out.insert(k::ENABLED.into(), run.enabled.into());
    out.insert(k::CHECK_NODE.into(), run.check_node.clone().into());
    Some(toml::Value::Table(out))
}
