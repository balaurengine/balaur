//! A running match: the frame driver that steps it, and what happens
//! between ticks — who left, whether the host went, whether two machines
//! disagree.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::app::App;
use balaur_core::netsession::NetSession;
use balaur_core::rollback::{Input, PlayerId};
use balaur_core::time::Instant;
use balaur_core::transport::LinkState;
use balaur_script::Value;

use crate::lobby::Starting;
use crate::vocabulary::{EventKind, Role};
use crate::wire::Control;
use crate::{MultiplayerState, Phase, slot_value};

/// How many ticks of a leaver's inputs the host passes on with its notice.
const LAST_INPUTS: u64 = 12;

/// How many timeouts a link may stay silent from the start of play, while
/// the far end is still loading.
const FIRST_WORD: f32 = 3.0;

/// The frame driver: while a match plays, every live frame comes here
/// instead of to `App::tick`.
#[allow(
    clippy::cast_precision_loss,
    reason = "a handful of substeps as a float"
)]
pub(crate) fn drive(app: &mut App, dt: f32) -> bool {
    let Some(state) = app.engine.try_resource::<MultiplayerState>() else {
        return false;
    };
    let starting = matches!(state.borrow().phase, Phase::Starting(_));
    if starting {
        begin(app, &state);
        return true;
    }
    let taken = {
        let mut s = state.borrow_mut();
        if !matches!(s.phase, Phase::Playing) {
            return false;
        }
        s.net.take()
    };
    let Some(mut net) = taken else {
        return false;
    };
    let step = app.fixed_step();
    let most = step * balaur_core::max_substeps() as f32;
    let mut owed = (state.borrow().owed + dt).min(most);
    let mut ran = false;
    let mut playing = true;
    // A wait keeps the time owed, so a peer's jitter costs a frame of lag
    // rather than a tick; the cap above sheds it when waiting goes on.
    while playing && owed >= step {
        if net.should_wait() {
            break;
        }
        owed -= step;
        feed(&state.borrow(), &mut net);
        net.advance(app);
        ran = true;
        playing = settle(&mut state.borrow_mut(), &mut net);
    }
    if playing && !ran {
        net.poll(&app.engine);
        playing = settle(&mut state.borrow_mut(), &mut net);
    }
    if playing {
        let mut s = state.borrow_mut();
        s.net = Some(net);
        s.owed = owed;
    }
    if ran {
        app.engine.set_frame_alpha(1.0);
    }
    true
}

/// Load the match scene and open the session: the host sends the world it
/// loaded, a joiner restores the one it was sent.
#[allow(
    clippy::disallowed_methods,
    reason = "when play began times a silent link; never a simulation input"
)]
fn begin(app: &mut App, state: &Rc<RefCell<MultiplayerState>>) {
    let phase = std::mem::replace(&mut state.borrow_mut().phase, Phase::Playing);
    let Phase::Starting(Starting {
        mut links,
        scene,
        depth,
        snapshot,
    }) = phase
    else {
        state.borrow_mut().phase = phase;
        return;
    };
    if !scene.is_empty() {
        balaur_core::scene_switch::request(&app.engine, &scene);
        balaur_core::scene_switch::apply_system(&app.engine, 0.0);
    }
    let (role, local, roster) = {
        let s = state.borrow();
        (s.role, s.local.unwrap_or(0), s.roster.clone())
    };
    let slots: Vec<PlayerId> = roster.iter().map(|member| member.slot).collect();
    if let Some(snapshot) = snapshot {
        balaur_core::snapshot::restore(&app.engine, &snapshot);
    } else {
        let world = balaur_core::snapshot::capture(&app.engine);
        let start = Control::Start {
            scene,
            depth,
            roster: roster.clone(),
            snapshot: serde_json::Value::Object(world.0),
        };
        let bytes = start.bytes();
        for (_, link) in &mut links {
            let _ = link.send_reliable(&bytes);
        }
    }
    let host = role == Some(Role::Host);
    let mut net = NetSession::new(local, &slots, depth);
    let mut link_slots = Vec::new();
    for (slot, link) in links {
        let speaks_for = if host {
            vec![slot]
        } else {
            slots.iter().copied().filter(|s| *s != local).collect()
        };
        net.add_bound_peer(&app.engine, link, slot, speaks_for);
        link_slots.push(slot);
    }
    if host {
        net.set_relay(true);
        for member in roster.iter().filter(|member| member.bot) {
            net.add_local(member.slot);
        }
    }
    let mut s = state.borrow_mut();
    s.net = Some(net);
    s.link_slots = link_slots;
    s.owed = 0.0;
    s.began = Some(Instant::now());
    s.absent.clear();
    s.desync_told = false;
    s.tell(EventKind::Started, vec![("slot", slot_value(local))]);
}

/// What scripts set for this machine's slots, into the next tick.
fn feed(state: &MultiplayerState, net: &mut NetSession) {
    for (&slot, value) in &state.pending {
        net.set_input_for(slot, value.clone());
    }
}

/// Everything between ticks. Answers whether the match goes on.
#[allow(
    clippy::disallowed_methods,
    reason = "how long a link has been silent is wall time; never a simulation input"
)]
fn settle(state: &mut MultiplayerState, net: &mut NetSession) -> bool {
    if std::mem::take(&mut state.leave_asked) {
        net.broadcast_control(&Control::Bye.to_value());
        end(state, net, "left");
        return false;
    }
    let host = state.role == Some(Role::Host);
    for (link, control) in net.take_controls() {
        match Control::from_value(control) {
            Some(Control::Absent {
                slot,
                from,
                first,
                values,
            }) if !host => {
                for (tick, value) in (first..).zip(values) {
                    net.submit(slot, tick, value);
                }
                mark_absent(state, net, slot, from);
            }
            Some(Control::Bye) if host => depart(state, net, link),
            Some(Control::Bye) => {
                end(state, net, "the host left");
                return false;
            }
            _ => {}
        }
    }
    let timeout = state.options.as_ref().map_or(5.0, |o| o.timeout);
    let first_word = state
        .began
        .is_some_and(|at| at.elapsed().as_secs_f32() > timeout * FIRST_WORD);
    for (link, link_state) in net.link_states().into_iter().enumerate() {
        let silent = net.silent_for(link).map_or(first_word, |s| s > timeout);
        if !silent && !matches!(link_state, LinkState::Closed(_)) {
            continue;
        }
        if !host {
            end(state, net, "lost the host");
            return false;
        }
        depart(state, net, link);
    }
    if let Some(desync) = net.desync()
        && !state.desync_told
    {
        state.desync_told = true;
        let tick = i64::try_from(desync.tick).unwrap_or(i64::MAX);
        state.tell(EventKind::Desync, vec![("tick", Value::Int(tick))]);
    }
    state.stats = state
        .roster
        .iter()
        .filter_map(|member| Some((member.slot, net.stats_of(member.slot)?)))
        .collect();
    true
}

/// A joiner's link went: pass on its last inputs and play it as absent.
fn depart(state: &mut MultiplayerState, net: &mut NetSession, link: usize) {
    let Some(&slot) = state.link_slots.get(link) else {
        return;
    };
    if state.absent.contains_key(&slot) {
        return;
    }
    let from = net.session().newest_input(slot).map_or(1, |tick| tick + 1);
    let first = from.saturating_sub(LAST_INPUTS).max(1);
    let values: Vec<Input> = (first..from)
        .map(|tick| {
            net.session()
                .arrived_input(slot, tick)
                .cloned()
                .unwrap_or(Input::Nil)
        })
        .collect();
    net.close_link(link);
    let notice = Control::Absent {
        slot,
        from,
        first,
        values,
    };
    net.broadcast_control(&notice.to_value());
    mark_absent(state, net, slot, from);
}

fn mark_absent(state: &mut MultiplayerState, net: &mut NetSession, slot: PlayerId, from: u64) {
    net.set_absent(slot, from);
    state.absent.insert(slot, from);
    let name = state.name_of(slot);
    state.tell(
        EventKind::Left,
        vec![("slot", slot_value(slot)), ("name", Value::Str(name))],
    );
}

/// This machine's match is over.
fn end(state: &mut MultiplayerState, net: &mut NetSession, reason: &str) {
    net.close_all();
    state.reset();
    state.tell(
        EventKind::Closed,
        vec![("reason", Value::Str(reason.to_string()))],
    );
}
