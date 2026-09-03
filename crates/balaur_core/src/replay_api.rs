//! The `replay` script module: record a session, play one back, and read
//! what happened in it.
//!
//! Declared once through the seam like `debugger_api`, so the editor's
//! session dock is plain script and a tool in another language drives the
//! same recorder.

// Every declaration shares one signature so they can sit in a table of
// function pointers; several of them have nothing to fail at.
#![allow(clippy::unnecessary_wraps)]

use anyhow::{anyhow, Result};
use balaur_script::{Bindings, Value};

use crate::engine::Engine;
use crate::engine_api::{number, text, EngineOp};
use crate::replay::{self, PlayState, Recording, ReplayPlayer, Session};

pub const REPLAY_OPS: &[EngineOp] = &[
    EngineOp {
        module: "replay",
        name: "record",
        call: record,
    },
    EngineOp {
        module: "replay",
        name: "stop",
        call: stop,
    },
    EngineOp {
        module: "replay",
        name: "recording",
        call: recording,
    },
    EngineOp {
        module: "replay",
        name: "load",
        call: load,
    },
    EngineOp {
        module: "replay",
        name: "unload",
        call: unload,
    },
    EngineOp {
        module: "replay",
        name: "play",
        call: play,
    },
    EngineOp {
        module: "replay",
        name: "pause",
        call: pause,
    },
    EngineOp {
        module: "replay",
        name: "seek",
        call: seek,
    },
    EngineOp {
        module: "replay",
        name: "state",
        call: state,
    },
    EngineOp {
        module: "replay",
        name: "position",
        call: position,
    },
    EngineOp {
        module: "replay",
        name: "length",
        call: length,
    },
    EngineOp {
        module: "replay",
        name: "header",
        call: header,
    },
    EngineOp {
        module: "replay",
        name: "info",
        call: info,
    },
    EngineOp {
        module: "replay",
        name: "events",
        call: events,
    },
    EngineOp {
        module: "replay",
        name: "marks",
        call: marks,
    },
    EngineOp {
        module: "replay",
        name: "diverged",
        call: diverged,
    },
    EngineOp {
        module: "replay",
        name: "session_name",
        call: session_name,
    },
];

/// Declare the module's functions and the `STATE_*` constants `state` answers
/// with.
pub fn install_replay_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Record what a running game is fed and play it back. A recording \
         holds each tick's input, network arrivals and events, not the world \
         they produced, so a session is small and replays by re-running the \
         game against the same input. The editor's Session dock drives these, \
         and so does `balaur run --record`.",
    );
    m.describe(&[
        ("record", &[], "(path: string, options: any?)", "Start recording into a file; call it before the code whose session it records runs."),
        ("stop", &[], "(reason: string?)", "Close the recording, naming why it ended, and return the file it wrote."),
        ("recording", &[], "()", "The file being recorded into, or nil."),
        ("load", &[], "(path: string)", "Read a session and put it in front of the engine, paused before its first tick."),
        ("unload", &[], "()", "Drop the loaded session and let the game run live again."),
        ("play", &[], "()", "Run the loaded session, one recorded tick per frame."),
        ("pause", &[], "()", "Stop between ticks, holding the simulation still while the frame loop keeps drawing."),
        ("seek", &[], "(tick: int)", "Run recorded ticks until playback reaches the given tick; forward only."),
        ("state", &[], "()", "What playback is doing: `STATE_STOPPED`, `STATE_PLAYING`, `STATE_PAUSED` or `STATE_SEEKING`."),
        ("position", &[], "()", "The tick playback has reached."),
        ("length", &[], "()", "The loaded session's frame count and the ticks it spans, or nil."),
        ("header", &[], "()", "The loaded session's project, start time, script fingerprint and how it ended."),
        ("info", &[], "(path: string)", "The same summary for a session file on disk, without loading it."),
        ("events", &[], "(from: int, to: int)", "The events recorded between two ticks, each with its tick, kind, label and data."),
        ("marks", &[], "(source: string, key: string?)", "The ticks at which one replay source held a non-empty list under a key, and what it held."),
        ("diverged", &[], "()", "The first tick whose replay did not reproduce the recorded digest, or nil."),
    ]);
    for d in REPLAY_OPS {
        m.function_raw(d.name, Box::new(d.call));
    }
    m.constant(
        "STATE_STOPPED",
        Value::Str(PlayState::Stopped.name().into()),
    );
    m.constant(
        "STATE_PLAYING",
        Value::Str(PlayState::Playing.name().into()),
    );
    m.constant("STATE_PAUSED", Value::Str(PlayState::Paused.name().into()));
    m.constant(
        "STATE_SEEKING",
        Value::Str(PlayState::Seeking.name().into()),
    );
}

fn option<'a>(args: &'a [Value], key: &str) -> Option<&'a Value> {
    match args.get(1) {
        Some(Value::Map(entries)) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn str_option(args: &[Value], key: &str) -> String {
    match option(args, key) {
        Some(Value::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

fn path_of(eng: &Engine, args: &[Value], i: usize) -> Result<std::path::PathBuf> {
    Ok(crate::engine_api::resolve(eng, text(args, i)?))
}

/// `replay.record(path, { digest = false, scripts = "…" })`: start recording
/// the session that is about to run.
///
/// Call it before the code it records: the engine's counters are read here,
/// and a request made before that reads a different id than the replay will.
fn record(eng: &Engine, args: &[Value]) -> Result<Value> {
    let path = path_of(eng, args, 0)?;
    let project = match str_option(args, "project") {
        name if name.is_empty() => eng
            .try_resource::<crate::project::ProjectManifest>()
            .map(|m| m.borrow().name.clone())
            .unwrap_or_default(),
        name => name,
    };
    let per_tick = matches!(option(args, "digest"), Some(Value::Bool(true)));
    replay::start_recording(eng, &path, &project, &str_option(args, "scripts"), per_tick)?;
    Ok(Value::Str(path.to_string_lossy().into_owned()))
}

/// `replay.stop("stop")`: close the recording, returning the file it wrote.
fn stop(eng: &Engine, args: &[Value]) -> Result<Value> {
    let reason = args.first().map_or("stop", |v| match v {
        Value::Str(s) => s.as_str(),
        _ => "stop",
    });
    Ok(replay::stop_recording(eng, reason)
        .map_or(Value::Nil, |p| Value::Str(p.to_string_lossy().into_owned())))
}

/// The file being recorded into, or nil.
fn recording(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(eng
        .resource::<Recording>()
        .borrow()
        .0
        .as_ref()
        .map_or(Value::Nil, |r| {
            Value::Str(r.path().to_string_lossy().into_owned())
        }))
}

/// `replay.load(path)`: put a session in front of the engine, paused on its
/// first frame. The caller rebuilds the world first — the recording holds
/// what the game was fed, not what it started from.
fn load(eng: &Engine, args: &[Value]) -> Result<Value> {
    let session = Session::read(&path_of(eng, args, 0)?)?;
    let summary = header_value(&session);
    replay::begin(eng, session);
    Ok(summary)
}

fn unload(eng: &Engine, _: &[Value]) -> Result<Value> {
    replay::end(eng);
    Ok(Value::Nil)
}

fn play(eng: &Engine, _: &[Value]) -> Result<Value> {
    let player = eng.resource::<ReplayPlayer>();
    let mut player = player.borrow_mut();
    if player.session.is_none() {
        return Err(anyhow!("no session is loaded"));
    }
    if player.remaining() > 0 {
        player.state = PlayState::Playing;
    }
    Ok(Value::Nil)
}

fn pause(eng: &Engine, _: &[Value]) -> Result<Value> {
    let player = eng.resource::<ReplayPlayer>();
    let mut player = player.borrow_mut();
    if player.session.is_some() {
        player.state = PlayState::Paused;
    }
    Ok(Value::Nil)
}

/// `replay.seek(tick)`: run frames until playback reaches `tick`. Forward
/// only — going back means rebuilding the world and seeking again, which
/// only the caller knows how to do.
fn seek(eng: &Engine, args: &[Value]) -> Result<Value> {
    let tick = number(args, 0)?.max(0.0) as u64;
    eng.resource::<ReplayPlayer>().borrow_mut().seek(tick);
    Ok(Value::Nil)
}

fn state(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Str(
        eng.resource::<ReplayPlayer>().borrow().state.name().into(),
    ))
}

fn position(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(count(eng.resource::<ReplayPlayer>().borrow().position()))
}

/// How many frames the loaded session holds, and the ticks it spans.
fn length(eng: &Engine, _: &[Value]) -> Result<Value> {
    let player = eng.resource::<ReplayPlayer>();
    let player = player.borrow();
    let Some(session) = &player.session else {
        return Ok(Value::Nil);
    };
    Ok(Value::Map(vec![
        ("frames".into(), count(session.frames.len() as u64)),
        ("first".into(), count(session.first_tick())),
        ("last".into(), count(session.last_tick())),
    ]))
}

fn header(eng: &Engine, _: &[Value]) -> Result<Value> {
    let player = eng.resource::<ReplayPlayer>();
    let player = player.borrow();
    Ok(player.session.as_ref().map_or(Value::Nil, header_value))
}

/// `replay.info(path)`: the same summary for a session on disk, without
/// loading it — what a list of sessions is drawn from.
fn info(eng: &Engine, args: &[Value]) -> Result<Value> {
    Ok(header_value(&Session::read(&path_of(eng, args, 0)?)?))
}

/// `replay.events(from, to)`: the recorded events in a tick range, each
/// `{ tick, kind, label, data }`.
fn events(eng: &Engine, args: &[Value]) -> Result<Value> {
    let player = eng.resource::<ReplayPlayer>();
    let player = player.borrow();
    let Some(session) = &player.session else {
        return Ok(Value::List(Vec::new()));
    };
    let from = number(args, 0)?.max(0.0) as u64;
    let to = number(args, 1)?.max(0.0) as u64;
    Ok(Value::List(
        session
            .events_between(from, to)
            .into_iter()
            .map(|(tick, event)| {
                Ok(Value::Map(vec![
                    ("tick".into(), count(tick)),
                    ("kind".into(), Value::Str(event.kind.clone())),
                    ("label".into(), Value::Str(event.label.clone())),
                    (
                        "data".into(),
                        event
                            .data
                            .as_ref()
                            .map_or(Ok(Value::Nil), crate::engine_api::from_json)?,
                    ),
                ]))
            })
            .collect::<Result<Vec<_>>>()?,
    ))
}

/// `replay.marks("input", "just_pressed")`: the ticks at which one replay
/// source held a non-empty list under `key`, and what it held.
///
/// The timeline's input and arrival lanes, without core learning the shape of
/// any plugin's snapshot: the caller names the source and the field, because
/// the caller is the one that knows them.
fn marks(eng: &Engine, args: &[Value]) -> Result<Value> {
    let player = eng.resource::<ReplayPlayer>();
    let player = player.borrow();
    let Some(session) = &player.session else {
        return Ok(Value::List(Vec::new()));
    };
    let source = text(args, 0)?;
    let key = args.get(1).map_or("", |v| match v {
        Value::Str(s) => s.as_str(),
        _ => "",
    });
    Ok(Value::List(
        session
            .marks(source, key)
            .into_iter()
            .map(|(tick, values)| {
                Ok(Value::Map(vec![
                    ("tick".into(), count(tick)),
                    (
                        "values".into(),
                        Value::List(
                            values
                                .iter()
                                .map(crate::engine_api::from_json)
                                .collect::<Result<Vec<_>>>()?,
                        ),
                    ),
                ]))
            })
            .collect::<Result<Vec<_>>>()?,
    ))
}

/// The first tick whose replay did not reproduce the recorded digest, or nil.
/// A session recorded without per-tick digests has nothing to compare and
/// always answers nil.
fn diverged(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(eng
        .resource::<ReplayPlayer>()
        .borrow()
        .diverged
        .map_or(Value::Nil, |d| {
            Value::Map(vec![
                ("tick".into(), count(d.tick)),
                (
                    "recorded".into(),
                    Value::Str(format!("{:016x}", d.recorded)),
                ),
                (
                    "replayed".into(),
                    Value::Str(format!("{:016x}", d.replayed)),
                ),
            ])
        }))
}

/// A file name for a session starting now: the header's timestamp with the
/// characters a Windows path refuses taken out, so it still sorts by time.
fn session_name(_: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Str(
        replay::timestamp().replace(':', "-").replace(' ', "_"),
    ))
}

fn header_value(session: &Session) -> Value {
    let h = &session.header;
    let mut out = vec![
        ("project".into(), Value::Str(h.project.clone())),
        ("started".into(), Value::Str(h.started.clone())),
        ("scripts".into(), Value::Str(h.scripts.clone())),
        ("frames".into(), count(session.frames.len() as u64)),
        ("first".into(), count(session.first_tick())),
        ("last".into(), count(session.last_tick())),
    ];
    out.push((
        "reason".into(),
        session
            .trailer
            .as_ref()
            .map_or(Value::Nil, |t| Value::Str(t.reason.clone())),
    ));
    Value::Map(out)
}

/// A tick or a count, as the script number type. Ticks outrun `i64` only
/// after a few billion years of play.
fn count(n: u64) -> Value {
    Value::Int(i64::try_from(n).unwrap_or(i64::MAX))
}
