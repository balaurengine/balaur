//! `multiplayer.*` for scripts.

use balaur_core::Engine;
use balaur_core::rollback::{self, PlayerId};
use balaur_script::{Bindings, BindingsExt, Value};

use crate::options::Options;
use crate::vocabulary::{Status, install_constants};
use crate::wire::Control;
use crate::{MultiplayerState, Phase, lobby, slot_value};

pub(crate) fn install(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Host, join and play a rollback match. Set this machine's input with `set_input` and read every slot's with `rollback.input(slot)`; events reach every script's `on_multiplayer_event` as a map with a `kind`.",
    );
    m.describe(DOCS);
    install_constants(m);
    install_lifecycle(m);
    install_play(m);
}

const DOCS: &[balaur_script::FnDoc] = &[
    (
        "host",
        &[],
        "(options: map?)",
        "Listen and take slot 0; answers `{ url, cert_hash, transport }`, what a joiner needs. Options over `[multiplayer]`: `transport`, `address`, `players`, `scene`, `depth`, `timeout_seconds`, `name`, `token`.",
    ),
    (
        "join",
        &[],
        "(url: string, options: map?)",
        "Dial a host at a `ws://` or `https://` url; `connected` or `failed` follows. Options: `cert_hash` for a self-signed host, `name`, `token`, `timeout_seconds`.",
    ),
    (
        "start",
        &[],
        "()",
        "On a host, start with whoever is in rather than waiting for `players`; false when there is no lobby to start.",
    ),
    (
        "add_bot",
        &[],
        "(name: string?)",
        "On a host, a slot its own script plays with `set_input_for`; answers the slot, or nil when the lobby is full.",
    ),
    (
        "leave",
        &[],
        "()",
        "Say goodbye and go idle; a host leaving ends the match for everyone.",
    ),
    (
        "set_input",
        &[],
        "(value: any)",
        "What this machine's player does on the next tick, until set again; ignored while a tick re-runs.",
    ),
    (
        "set_input_for",
        &[],
        "(slot: int, value: any)",
        "The same for a bot slot this machine plays.",
    ),
    (
        "players",
        &[],
        "()",
        "Every slot as `{ slot, name, bot, local, status }`, `status` as of the tick being simulated.",
    ),
    (
        "local_player",
        &[],
        "()",
        "This machine's own slot, or nil before it has one.",
    ),
    (
        "role",
        &[],
        "()",
        "`ROLE_HOST` or `ROLE_CLIENT`, or nil when idle.",
    ),
    (
        "state",
        &[],
        "()",
        "`STATE_IDLE`, `STATE_CONNECTING`, `STATE_LOBBY` or `STATE_PLAYING`.",
    ),
    (
        "tick",
        &[],
        "()",
        "The match tick being simulated; what a match branches on instead of `engine.tick`.",
    ),
    (
        "settled_tick",
        &[],
        "()",
        "The tick before which nothing can be rolled back any more.",
    ),
    (
        "stats",
        &[],
        "(slot: int)",
        "`{ rtt_ms, loss, bytes_in, bytes_out }` for the link a slot is reached over; nil for this machine's own. An observer: never simulate from it.",
    ),
];

fn state(eng: &Engine) -> std::rc::Rc<std::cell::RefCell<MultiplayerState>> {
    eng.resource::<MultiplayerState>()
}

fn install_lifecycle(m: &mut dyn Bindings<Engine>) {
    m.function("host", |eng: &Engine, opts: Option<Value>| {
        let options = Options::read(eng, opts.as_ref())?;
        lobby::host(&mut state(eng).borrow_mut(), eng, options)
    });
    m.function(
        "join",
        |eng: &Engine, (url, opts): (String, Option<Value>)| {
            let options = Options::read(eng, opts.as_ref())?;
            lobby::join(&mut state(eng).borrow_mut(), eng, &url, options)?;
            Ok(Value::Nil)
        },
    );
    m.function("start", |eng: &Engine, (): ()| {
        Ok(Value::Bool(lobby::start(&mut state(eng).borrow_mut())))
    });
    m.function("add_bot", |eng: &Engine, name: Option<String>| {
        let name = name.unwrap_or_else(|| String::from("Bot"));
        let slot = lobby::add_bot(&mut state(eng).borrow_mut(), &name);
        Ok(slot.map_or(Value::Nil, slot_value))
    });
    m.function("leave", |eng: &Engine, (): ()| {
        leave(&mut state(eng).borrow_mut());
        Ok(Value::Nil)
    });
}

/// Leave whatever this machine is in. A match whose session the driver
/// holds right now is left between ticks.
fn leave(state: &mut MultiplayerState) {
    if !matches!(state.phase, Phase::Playing) {
        lobby::leave(state);
        if state.role.is_some() {
            state.reset();
            state.tell(
                crate::EventKind::Closed,
                vec![("reason", Value::Str(String::from("left")))],
            );
        }
        return;
    }
    let Some(mut net) = state.net.take() else {
        state.leave_asked = true;
        return;
    };
    net.broadcast_control(&Control::Bye.to_value());
    net.close_all();
    state.reset();
    state.tell(
        crate::EventKind::Closed,
        vec![("reason", Value::Str(String::from("left")))],
    );
}

fn install_play(m: &mut dyn Bindings<Engine>) {
    // Read back through `rollback.input(slot)`, on the tick it lands on.
    m.function("set_input", |eng: &Engine, value: Value| {
        if !rollback::is_resimulating(eng) {
            let state = state(eng);
            let mut state = state.borrow_mut();
            if let Some(local) = state.local {
                state.pending.insert(local, value);
            }
        }
        Ok(Value::Nil)
    });
    // Read back through `rollback.input(slot)` too.
    m.function(
        "set_input_for",
        |eng: &Engine, (slot, value): (i64, Value)| {
            if !rollback::is_resimulating(eng)
                && let Ok(slot) = PlayerId::try_from(slot)
            {
                state(eng).borrow_mut().pending.insert(slot, value);
            }
            Ok(Value::Nil)
        },
    );
    m.function("players", |eng: &Engine, (): ()| Ok(players(eng)));
    m.function("local_player", |eng: &Engine, (): ()| {
        Ok(state(eng).borrow().local.map_or(Value::Nil, slot_value))
    });
    m.function("role", |eng: &Engine, (): ()| {
        Ok(state(eng)
            .borrow()
            .role
            .map_or(Value::Nil, |role| Value::Str(role.name().into())))
    });
    m.function("state", |eng: &Engine, (): ()| {
        Ok(Value::Str(state(eng).borrow().state().name().into()))
    });
    m.function("tick", |eng: &Engine, (): ()| {
        Ok(tick_value(rollback::clock(eng).tick))
    });
    m.function("settled_tick", |eng: &Engine, (): ()| {
        let clock = rollback::clock(eng);
        Ok(tick_value(clock.settled.min(clock.tick)))
    });
    m.function("stats", |eng: &Engine, slot: i64| Ok(stats(eng, slot)));
}

fn tick_value(tick: u64) -> Value {
    Value::Int(i64::try_from(tick).unwrap_or(i64::MAX))
}

fn players(eng: &Engine) -> Value {
    let tick = rollback::clock(eng).tick;
    let state = state(eng);
    let state = state.borrow();
    let playing = matches!(state.phase, Phase::Playing);
    let list = state
        .roster
        .iter()
        .map(|member| {
            let gone = playing
                && state
                    .absent
                    .get(&member.slot)
                    .is_some_and(|from| tick >= *from);
            let status = if gone {
                Status::Absent
            } else {
                Status::Present
            };
            Value::Map(vec![
                (String::from("slot"), slot_value(member.slot)),
                (String::from("name"), Value::Str(member.name.clone())),
                (String::from("bot"), Value::Bool(member.bot)),
                (
                    String::from("local"),
                    Value::Bool(state.local == Some(member.slot)),
                ),
                (String::from("status"), Value::Str(status.name().into())),
            ])
        })
        .collect();
    Value::List(list)
}

fn stats(eng: &Engine, slot: i64) -> Value {
    let Ok(slot) = PlayerId::try_from(slot) else {
        return Value::Nil;
    };
    let state = state(eng);
    let state = state.borrow();
    let Some(link) = state.stats.get(&slot) else {
        return Value::Nil;
    };
    #[allow(clippy::cast_possible_wrap, reason = "byte counts far under i64::MAX")]
    Value::Map(vec![
        (String::from("rtt_ms"), Value::Num(f64::from(link.rtt_ms))),
        (String::from("loss"), Value::Num(f64::from(link.loss))),
        (String::from("bytes_in"), Value::Int(link.bytes_in as i64)),
        (String::from("bytes_out"), Value::Int(link.bytes_out as i64)),
    ])
}
