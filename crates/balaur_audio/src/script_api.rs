//! `audio.*`: the script bindings over [`AudioState`], the bus mix and the
//! positional half.

use anyhow::{Result, anyhow};
use balaur_core::glamx::Vec3;
use balaur_core::{Engine, entity_of};
use balaur_script::{Bindings, BindingsExt, NodeId, Value};

use crate::bus::{self, Buses};
use crate::event;
use crate::spatial::Emitter;
use crate::{
    AudioState, Cue, DEFAULT_MAX_DISTANCE, DEFAULT_MIN_DISTANCE, play_on, read_sound, stop_on,
};

/// One key out of a script options table, or `None` if the table, the key or
/// its type is missing. A typo in an options table should not stop the frame.
fn opt<'a>(opts: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    match opts? {
        Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// A number from an options entry, whichever way the language spelled it.
fn number(value: Option<&Value>) -> Option<f32> {
    match value {
        Some(Value::Num(n)) => Some(*n as f32),
        Some(Value::Int(i)) => Some(*i as f32),
        _ => None,
    }
}

/// A point from a script value: a vector, or a list of two or three numbers
/// so a 2D game may write `[x, y]` and mean the plane it plays on.
fn point(value: Option<&Value>) -> Option<Vec3> {
    match value? {
        Value::Vec3([x, y, z]) => Some(Vec3::new(*x, *y, *z)),
        Value::Vec2([x, y]) => Some(Vec3::new(*x, *y, 0.0)),
        Value::List(items) if items.len() >= 2 => Some(Vec3::new(
            number(items.first())?,
            number(items.get(1))?,
            number(items.get(2)).unwrap_or(0.0),
        )),
        _ => None,
    }
}

/// Three numbers or one vector, so `set_listener(v)` and
/// `set_listener(x, y, z)` both work: the spelling `node.set_position` takes.
fn xyz(x: &Value, y: Option<&Value>, z: Option<&Value>) -> Result<Vec3> {
    if let Some(point) = point(Some(x)) {
        return Ok(point);
    }
    let axis = |value: Option<&Value>, name: &str| {
        number(value)
            .ok_or_else(|| anyhow!("expected a vector or three numbers; {name} is not a number"))
    };
    Ok(Vec3::new(axis(Some(x), "x")?, axis(y, "y")?, axis(z, "z")?))
}

/// The emitter an options table asks for, or `None` when it names no
/// `position`: which is what makes a sound flat rather than placed.
fn emitter_from(opts: Option<&Value>) -> Option<Emitter> {
    let position = point(opt(opts, "position"))?;
    Some(Emitter::new(
        position,
        number(opt(opts, "min_distance")).unwrap_or(DEFAULT_MIN_DISTANCE),
        number(opt(opts, "max_distance")).unwrap_or(DEFAULT_MAX_DISTANCE),
        number(opt(opts, "doppler")).unwrap_or(0.0),
    ))
}

/// A script-supplied handle. Negative numbers wrap to values `play` never
/// hands out, so they answer false and no-op rather than erroring.
const fn handle_of(raw: i64) -> u64 {
    raw as u64
}

/// `audio.*`. Declared against the neutral seam, so it works on any backend.
pub(crate) fn install_audio_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Sound playback: a file plays under an integer handle, with `volume`, \
         `pitch` and `loop` options, and the `sound` component gives a node a \
         sound of its own. Give a `play` a `position` and it is heard from \
         where the `listener` is. With no output device every call still \
         works and nothing is heard.",
    );
    m.describe(&[
        ("play", &[], "", "Start the audio file at a path and return the handle `stop`, `set_volume`, `set_pitch` and `is_playing` take. The options table takes `volume`, `pitch`, `loop`, `bus`, and a `position` with `min_distance`, `max_distance` and `doppler`."),
        ("stop", &[], "", "Silence the sound a handle names; a finished, stopped or unknown handle is left alone."),
        ("set_volume", &[], "", "Set a playing handle's linear gain, where 1 is the file's own level."),
        ("set_pitch", &[], "", "Set a playing handle's speed multiplier, which carries its pitch with it."),
        ("ready", &[], "()", "Whether an output device is open. False on a page until the first gesture, and false for good with no sound card; playing before then hands out handles that make no sound."),
        ("is_playing", &[], "", "Whether a handle's sound is still audible; false once it ends, and always false with no output device."),
        ("stop_all", &[], "", "Silence everything at once and clear the playback every `sound` component was holding."),
        ("play_on", &["sound"], "", "Start the node's own `sound` from the top, replacing what it had going, and return the new handle."),
        ("stop_on", &["sound"], "", "Silence what the node's `sound` started; a node carrying none is left alone."),
    ]);
    // `audio.play(path, { volume = 1.0, pitch = 1.0, loop = true })` hands
    // back the handle the other functions take. Flags live in the options
    // table rather than in the name, so fade/bus can join them (N9).
    m.function(
        "play",
        |eng: &Engine, (path, opts): (String, Option<Value>)| {
            let opts = opts.as_ref();
            let cue = Cue {
                volume: number(opt(opts, "volume")).unwrap_or(1.0),
                pitch: number(opt(opts, "pitch")).unwrap_or(1.0),
                looped: matches!(opt(opts, "loop"), Some(Value::Bool(true))),
                bus: match opt(opts, "bus") {
                    Some(Value::Str(name)) => name.clone(),
                    _ => String::new(),
                },
                gain: 1.0,
                emitter: emitter_from(opts),
            };
            let bytes = read_sound(eng, &path)?;
            bus::ensure_loaded(eng);
            let gain = eng.resource::<bus::Buses>().borrow().gain(&cue.bus);
            let state = eng.resource::<AudioState>();
            let handle = state.borrow_mut().play_cue(bytes, Cue { gain, ..cue });
            Ok(handle)
        },
    );
    m.function("stop", |eng: &Engine, handle: i64| {
        eng.resource::<AudioState>()
            .borrow_mut()
            .stop(handle_of(handle));
        Ok(())
    });
    m.function(
        "set_volume",
        |eng: &Engine, (handle, volume): (i64, f32)| {
            bus::ensure_loaded(eng);
            let buses = eng.resource::<Buses>();
            eng.resource::<AudioState>().borrow_mut().set_volume(
                handle_of(handle),
                volume,
                &buses.borrow(),
            );
            Ok(())
        },
    );
    m.function("set_pitch", |eng: &Engine, (handle, pitch): (i64, f32)| {
        eng.resource::<AudioState>()
            .borrow_mut()
            .set_pitch(handle_of(handle), pitch);
        Ok(())
    });
    m.function("ready", |eng: &Engine, ()| {
        let state = eng.resource::<AudioState>();
        let mut state = state.borrow_mut();
        state.open_if_needed();
        Ok(state.device.is_some())
    });
    m.function("is_playing", |eng: &Engine, handle: i64| {
        Ok(eng
            .resource::<AudioState>()
            .borrow()
            .is_playing(handle_of(handle)))
    });
    m.function("stop_all", |eng: &Engine, ()| {
        eng.resource::<AudioState>().borrow_mut().stop_all();
        Ok(())
    });
    m.function("play_on", |eng: &Engine, node: NodeId| {
        play_on(eng, entity_of(node)?)
    });
    install_mixing_api(m);
    install_positional_api(m);
    m.function("stop_on", |eng: &Engine, node: NodeId| {
        stop_on(eng, entity_of(node)?);
        Ok(())
    });
}

/// `audio.*` covers the mix: which bus a sound plays through, and the sounds a
/// project names rather than spells out.
///
/// Its own group because the rest of `audio` is about one playback at a time
/// and this is about all of them at once.
fn install_mixing_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("buses", &[], "()", "Every audio bus, declared in `[audio.buses]` or made by setting a volume, in name order."),
        ("bus_volume", &[], "(bus: string)", "One bus's own gain, without its parents'."),
        ("set_bus_volume", &[], "(bus: string, volume: float)", "Set one bus's gain and re-apply it to everything already playing on it: which is what a volume slider is."),
        ("events", &[], "()", "Every sound named in `audio/events.toml`, in name order."),
        ("play_event", &[], "(name: string, options: map)", "Play a named sound: the next of its variations in turn, at its own volume and pitch, through its own bus. A `position` in the options table places it. Nil for a name nothing declared."),
    ]);
    m.function("events", |eng: &Engine, ()| {
        event::ensure_loaded(eng);
        let names = eng.resource::<event::Events>().borrow().names();
        Ok(Value::List(names.into_iter().map(Value::Str).collect()))
    });
    // The script says *what happened*; the events file says what that sounds
    // like. Tuning one never touches the other.
    m.function(
        "play_event",
        |eng: &Engine, (name, opts): (String, Option<Value>)| {
            event::ensure_loaded(eng);
            bus::ensure_loaded(eng);
            let played = {
                let events = eng.resource::<event::Events>();
                let events = events.borrow();
                events
                    .get(&name)
                    .map(|event| (event.clone(), events.next_file(&name)))
            };
            let Some((event, Some(file))) = played else {
                tracing::warn!("audio event '{name}' is not declared, or names no files");
                return Ok(Value::Nil);
            };
            let bytes = read_sound(eng, &file)?;
            let gain = eng.resource::<bus::Buses>().borrow().gain(&event.bus);
            // Where an impact happened is the caller's to say; how far it
            // carries is the events file's.
            let emitter = point(opt(opts.as_ref(), "position")).map(|position| {
                Emitter::new(
                    position,
                    event.min_distance,
                    event.max_distance,
                    event.doppler,
                )
            });
            let handle = eng.resource::<AudioState>().borrow_mut().play_cue(
                bytes,
                Cue {
                    volume: event.volume,
                    pitch: event.pitch,
                    looped: event.looped,
                    bus: event.bus,
                    gain,
                    emitter,
                },
            );
            Ok(Value::Int(i64::try_from(handle).unwrap_or(i64::MAX)))
        },
    );
    m.function("buses", |eng: &Engine, ()| {
        bus::ensure_loaded(eng);
        let names = eng.resource::<bus::Buses>().borrow().names();
        Ok(Value::List(names.into_iter().map(Value::Str).collect()))
    });
    m.function("bus_volume", |eng: &Engine, name: String| {
        bus::ensure_loaded(eng);
        let volume = eng.resource::<bus::Buses>().borrow().volume(&name);
        Ok(volume)
    });
    m.function(
        "set_bus_volume",
        |eng: &Engine, (name, volume): (String, f32)| {
            bus::ensure_loaded(eng);
            let buses = eng.resource::<bus::Buses>();
            buses.borrow_mut().set_volume(&name, volume);
            // Everything already sounding through that bus moves too, which
            // is the difference between a mixer and a default.
            eng.resource::<AudioState>()
                .borrow_mut()
                .reroute(&buses.borrow(), &name);
            Ok(())
        },
    );
}

/// `audio.*`: where a sound is and where it is heard from.
///
/// Its own group because the rest of `audio` is about what plays, and this
/// is about where: the `listener` node's own half of the pair, and the
/// emitter behind a handle that was played with a `position`.
fn install_positional_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("listener", &[], "()", "Where the ears are: the current `listener` node's world position, or what `set_listener` last put there."),
        ("set_listener", &[], "(x: float, y: float, z: float)", "Put the ears at a point by hand, for a game whose view is not a node; a `listener` node in the scene takes it back on the next frame."),
        ("emitter_position", &[], "(handle: int)", "Where a handle played with a `position` is; nil for a flat or unknown one."),
        ("set_emitter_position", &[], "(handle: int, x: float, y: float, z: float)", "Move what a handle plays from, so a sound follows something the script is driving; the frame takes its doppler from how far it moved."),
        ("distance_gain", &[], "(handle: int)", "The gain the distance to the listener is costing a positional handle right now: 1 up close, 0 out of range."),
        ("pan", &[], "(handle: int)", "Where a positional handle sits between the speakers: -1 hard left, 0 centred, 1 hard right."),
    ]);
    m.function("listener", |eng: &Engine, ()| {
        let state = eng.resource::<AudioState>();
        let position = state.borrow().listener().position;
        Ok(Value::Vec3([position.x, position.y, position.z]))
    });
    m.function(
        "set_listener",
        |eng: &Engine, (x, y, z): (Value, Option<Value>, Option<Value>)| {
            let position = xyz(&x, y.as_ref(), z.as_ref())?;
            eng.resource::<AudioState>()
                .borrow_mut()
                .set_listener(position);
            Ok(())
        },
    );
    m.function("emitter_position", |eng: &Engine, handle: i64| {
        let state = eng.resource::<AudioState>();
        let position = state.borrow().emitter_position(handle_of(handle));
        Ok(position.map_or(Value::Nil, |at| Value::Vec3([at.x, at.y, at.z])))
    });
    m.function(
        "set_emitter_position",
        |eng: &Engine, (handle, x, y, z): (i64, Value, Option<Value>, Option<Value>)| {
            let position = xyz(&x, y.as_ref(), z.as_ref())?;
            eng.resource::<AudioState>()
                .borrow_mut()
                .set_emitter_position(handle_of(handle), position);
            Ok(())
        },
    );
    // The two halves of a placement, so a script can show what the mix is
    // doing: a debug overlay, or a subtitle only for a sound near enough to
    // hear.
    m.function("distance_gain", |eng: &Engine, handle: i64| {
        let state = eng.resource::<AudioState>();
        let placement = state.borrow().placement_of(handle_of(handle));
        Ok(placement.map_or(Value::Nil, |placed| Value::Num(f64::from(placed.gain))))
    });
    m.function("pan", |eng: &Engine, handle: i64| {
        let state = eng.resource::<AudioState>();
        let placement = state.borrow().placement_of(handle_of(handle));
        Ok(placement.map_or(Value::Nil, |placed| Value::Num(f64::from(placed.pan))))
    });
}
