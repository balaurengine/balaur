//! `audio.*`: the script bindings over [`AudioState`], the bus mix and the
//! positional half.

use anyhow::bail;
use balaur_core::glamx::Vec3;
use balaur_core::{Engine, entity_of};
use balaur_script::{Module, NodeId, Value};

use crate::bus::{self, Buses, Effects, Limiter};
use crate::vocabulary::keys as k;
use crate::{AudioState, play_on, publish_playback_times, seek_on, skip_on, stop_on};

const BUS_EFFECTS: &str = "bus_effects";
const BUS_LIMIT: &str = "bus_limit";
const BUS_VOLUME_LINEAR: &str = "bus_volume_linear";
const BUSES: &str = "buses";
const DEVICE_READY: &str = "device_ready";
const DEVICES: &str = "devices";
const DISTANCE_GAIN: &str = "distance_gain";
const EMITTER_POSITION: &str = "emitter_position";
const IS_PLAYING: &str = "is_playing";
const LISTENER_POSITION: &str = "listener_position";
const PAN: &str = "pan";
const PLAY: &str = "play";
const PLAY_FILE: &str = "play_file";
const SEEK: &str = "seek";
const SET_BUS_VOLUME_LINEAR: &str = "set_bus_volume_linear";
const SET_EMITTER_POSITION: &str = "set_emitter_position";
const SET_PITCH_SCALE: &str = "set_pitch_scale";
const SET_VOLUME_LINEAR: &str = "set_volume_linear";
const SKIP: &str = "skip";
const SOUND: &str = "sound";
const STOP: &str = "stop";
const STOP_ALL: &str = "stop_all";
const STOP_PLAYBACK: &str = "stop_playback";

fn point(value: Option<&Value>) -> Option<Vec3> {
    match value? {
        Value::Vec3([x, y, z]) => Some(Vec3::new(*x, *y, *z)),
        _ => None,
    }
}

/// A script-supplied handle. Negative numbers wrap to values `play` never
/// hands out, so they answer false and no-op rather than erroring.
const fn handle_of(raw: i64) -> u64 {
    raw as u64
}

/// `audio.*`. Declared against the neutral seam, so it works on any backend.
pub(crate) fn install_audio_api(m: &mut Module<Engine>) {
    m.module_doc(
        "Sound playback: `play` starts a node's `sound` component and returns a handle that `stop_playback`, `set_volume_linear`, `set_pitch_scale` and `is_playing` take; `seek` and `skip` move it. `play_file` plays a file no node carries, from a `position` heard from the `listener` when given one. A node's `sound` announces `finished` (`EVENT_FINISHED`) with the handle when it plays out. `devices` lists the outputs `[audio] device` can name.",
    );
    m.constant(
        "EVENT_FINISHED",
        Value::Str(crate::FINISHED_EVENT.to_string()),
    );
    m.describe(&[
        (STOP_PLAYBACK, &[], "", "Silence the sound a handle names, over the `fade_out_time` its `sound` had; a finished, stopped or unknown handle is left alone."),
        (SET_VOLUME_LINEAR, &[], "", "Set a playing handle's linear gain, where 1 is the file's own level."),
        (SET_PITCH_SCALE, &[], "", "Set a playing handle's speed multiplier, which carries its pitch with it."),
        (DEVICE_READY, &[], "() -> bool", "Whether an output device is open. False on a page until the first gesture, and false for good with no sound card; playing before then hands out handles that make no sound."),
        (IS_PLAYING, &[], "", "Whether a handle's sound is still going: false once it plays out or is stopped, on the same tick with or without an output device."),
        (STOP_ALL, &[], "", "Silence everything at once and clear the playback every `sound` component was holding."),
        (PLAY, &[SOUND], "", "Start the node's own `sound` from the top, replacing what it had going, and return the new handle."),
        (STOP, &[SOUND], "", "Silence what the node's `sound` started, over its `fade_out_time`; a node carrying none is left alone."),
        (SEEK, &[SOUND], "", "Jump the node's sound to `seconds` into the file it is playing; `playback_time` reads it back. A node with nothing playing is left alone."),
        (SKIP, &[SOUND], "", "Move the node's sound on to the next file in its `queue`. Past the last one it has played out, and the node announces `finished`."),
        (DEVICES, &[], "() -> [string]", "Every output device's name, as `[audio] device` takes it. Empty when the platform lists none."),
    ]);
    m.function(STOP_PLAYBACK, |eng: &Engine, handle: i64| {
        eng.resource::<AudioState>()
            .borrow_mut()
            .release(handle_of(handle));
        publish_playback_times(eng);
        Ok(())
    });
    m.function(
        SET_VOLUME_LINEAR,
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
    m.function(
        SET_PITCH_SCALE,
        |eng: &Engine, (handle, pitch): (i64, f32)| {
            eng.resource::<AudioState>()
                .borrow_mut()
                .set_pitch(handle_of(handle), pitch);
            Ok(())
        },
    );
    m.function(DEVICE_READY, |eng: &Engine, ()| {
        crate::ensure_loaded(eng);
        let state = eng.resource::<AudioState>();
        let mut state = state.borrow_mut();
        state.open_if_needed();
        Ok(state.device.is_some())
    });
    m.function(IS_PLAYING, |eng: &Engine, handle: i64| {
        Ok(eng
            .resource::<AudioState>()
            .borrow()
            .is_playing(handle_of(handle)))
    });
    m.function(STOP_ALL, |eng: &Engine, ()| {
        eng.resource::<AudioState>().borrow_mut().stop_all();
        publish_playback_times(eng);
        Ok(())
    });
    m.function(PLAY, |eng: &Engine, node: NodeId| {
        play_on(eng, entity_of(node)?)
    });
    install_mixing_api(m);
    install_positional_api(m);
    m.function(STOP, |eng: &Engine, node: NodeId| {
        stop_on(eng, entity_of(node)?);
        Ok(())
    });
    m.function(SEEK, |eng: &Engine, (node, seconds): (NodeId, f64)| {
        seek_on(eng, entity_of(node)?, seconds);
        Ok(())
    });
    m.function(SKIP, |eng: &Engine, node: NodeId| {
        skip_on(eng, entity_of(node)?);
        Ok(())
    });
    m.function(DEVICES, |_: &Engine, ()| {
        Ok(Value::List(
            crate::devices().into_iter().map(Value::text).collect(),
        ))
    });
}

/// `audio.*` covers the mix: which bus a sound plays through, and the sounds a
/// project names rather than spells out.
///
/// Its own group because the rest of `audio` is about one playback at a time
/// and this is about all of them at once.
fn install_mixing_api(m: &mut Module<Engine>) {
    m.describe(&[
        (BUSES, &[], "() -> [string]", "Every audio bus, declared in `[audio.buses]` or made by setting a volume, in name order."),
        (BUS_VOLUME_LINEAR, &[], "(bus: string) -> float", "One bus's own gain, without its parents'."),
        (SET_BUS_VOLUME_LINEAR, &[], "(bus: string, volume: float)", "Set one bus's gain and re-apply it to everything already playing on it: which is what a volume slider is."),
        (BUS_LIMIT, &[], "(bus: string) -> BusLimit?", "One bus's limiter as `[audio.buses]` declares it. Nil for a bus with none."),
        (BUS_EFFECTS, &[], "(bus: string) -> BusEffects?", "One bus's effects as `[audio.buses]` declares them, applied once to everything the bus mixes. Built when the output device opens, as the limiter is. Nil for a bus with none switched on."),
        (PLAY_FILE, &[], "(path: string, options: PlayFileOptions?) -> int", "Play a sound file no node carries, through `bus` at `volume_linear` and `pitch_scale`, looping when `loop` is true. Returns the handle `stop_playback` takes."),
    ]);
    m.records(&[
        (
            "PlayFileOptions",
            "{ bus: string?, volume_linear: float?, pitch_scale: float?, loop: bool?, position: Vec3?, min_distance: float?, max_distance: float?, doppler_level: float? }",
            "How `play_file` plays, under the `sound` component's keys: a `position` places it, heard from the `listener` over `min_distance` to `max_distance`.",
        ),
        (
            "BusLimit",
            "{ limit: bool, limit_threshold_db: float, limit_knee_db: float, limit_attack_time: float, limit_release_time: float }",
            "A bus's limiter, under the keys `[audio.buses]` spells.",
        ),
        (
            "BusEffects",
            "{ low_pass_hz: float, low_pass_q: float, high_pass_hz: float, high_pass_q: float, distortion_gain: float, distortion_threshold: float, reverb_time: float, reverb_level: float, auto_gain: bool, auto_gain_target: float, auto_gain_attack_time: float, auto_gain_release_time: float, auto_gain_max: float, auto_gain_floor: float }",
            "A bus's effects, under the keys `[audio.buses]` spells.",
        ),
    ]);
    m.function(
        PLAY_FILE,
        |eng: &Engine, (path, opts): (String, Option<Value>)| {
            let options: crate::play_file::FileOptions = match opts {
                Some(opts) => balaur_core::node_api::to_toml(&opts)?.try_into()?,
                None => crate::play_file::FileOptions::default(),
            };
            let handle = crate::play_file::play_file(eng, &path, &options)?;
            Ok(Value::Int(i64::try_from(handle).unwrap_or(i64::MAX)))
        },
    );
    install_bus_readers(m);
}

fn install_bus_readers(m: &mut Module<Engine>) {
    m.function(BUSES, |eng: &Engine, ()| {
        bus::ensure_loaded(eng);
        let names = eng.resource::<bus::Buses>().borrow().names();
        Ok(Value::List(names.into_iter().map(Value::text).collect()))
    });
    m.function(BUS_LIMIT, |eng: &Engine, name: String| {
        bus::ensure_loaded(eng);
        let limit = eng.resource::<bus::Buses>().borrow().limit(&name);
        Ok(limit.map_or(Value::Nil, limit_table))
    });
    m.function(BUS_EFFECTS, |eng: &Engine, name: String| {
        bus::ensure_loaded(eng);
        let effects = eng.resource::<bus::Buses>().borrow().effects(&name);
        Ok(effects.map_or(Value::Nil, |effects| effects_table(&effects)))
    });
    m.function(BUS_VOLUME_LINEAR, |eng: &Engine, name: String| {
        bus::ensure_loaded(eng);
        let volume = eng.resource::<bus::Buses>().borrow().volume(&name);
        Ok(volume)
    });
    m.function(
        SET_BUS_VOLUME_LINEAR,
        |eng: &Engine, (name, volume): (String, f32)| {
            // A mix being set is audio to come: the device starts opening.
            crate::ensure_loaded(eng);
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

/// A table under the keys `[audio.buses]` spells, so a script reads a bus
/// back in the words the project wrote it in.
fn table(entries: Vec<(&str, Value)>) -> Value {
    Value::Map(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

fn float(value: f32) -> Value {
    Value::Num(f64::from(value))
}

/// What `audio.bus_limit` hands back.
fn limit_table(limit: Limiter) -> Value {
    table(vec![
        (k::LIMIT, Value::Bool(true)),
        (k::LIMIT_THRESHOLD_DB, float(limit.threshold_db)),
        (k::LIMIT_KNEE_DB, float(limit.knee_db)),
        (k::LIMIT_ATTACK_TIME, float(limit.attack_time)),
        (k::LIMIT_RELEASE_TIME, float(limit.release_time)),
    ])
}

/// What `audio.bus_effects` hands back.
fn effects_table(effects: &Effects) -> Value {
    let auto_gain = &effects.auto_gain;
    table(vec![
        (k::LOW_PASS_HZ, float(effects.low_pass_hz)),
        (k::LOW_PASS_Q, float(effects.low_pass_q)),
        (k::HIGH_PASS_HZ, float(effects.high_pass_hz)),
        (k::HIGH_PASS_Q, float(effects.high_pass_q)),
        (k::DISTORTION_GAIN, float(effects.distortion_gain)),
        (k::DISTORTION_THRESHOLD, float(effects.distortion_threshold)),
        (k::REVERB_TIME, float(effects.reverb_time)),
        (k::REVERB_LEVEL, float(effects.reverb_level)),
        (k::AUTO_GAIN, Value::Bool(auto_gain.on)),
        (k::AUTO_GAIN_TARGET, float(auto_gain.target)),
        (k::AUTO_GAIN_ATTACK_TIME, float(auto_gain.attack_time)),
        (k::AUTO_GAIN_RELEASE_TIME, float(auto_gain.release_time)),
        (k::AUTO_GAIN_MAX, float(auto_gain.max)),
        (k::AUTO_GAIN_FLOOR, float(auto_gain.floor)),
    ])
}

/// `audio.*`: where a sound is and where it is heard from.
///
/// Its own group because the rest of `audio` is about what plays, and this
/// is about where: the `listener` node's own half of the pair, and the
/// emitter behind a handle that was played with a `position`.
fn install_positional_api(m: &mut Module<Engine>) {
    m.describe(&[
        (LISTENER_POSITION, &[], "() -> vec3", "Where the ears are: the current `listener` node's world position, else the current camera's."),
        (EMITTER_POSITION, &[], "(handle: int) -> vec3?", "Where a handle played with a `position` is; nil for a flat or unknown one."),
        (SET_EMITTER_POSITION, &[], "(handle: int, position: Vec3)", "Move what a handle plays from, so a sound follows something the script is driving; the frame takes its doppler from how far it moved."),
        (DISTANCE_GAIN, &[], "(handle: int) -> float?", "The gain the distance to the listener is costing a positional handle right now: 1 up close, 0 out of range."),
        (PAN, &[], "(handle: int) -> float?", "Where a positional handle sits between the speakers: -1 hard left, 0 centred, 1 hard right."),
    ]);
    m.function(LISTENER_POSITION, |eng: &Engine, ()| {
        let state = eng.resource::<AudioState>();
        let position = state.borrow().listener().position;
        Ok(Value::Vec3([position.x, position.y, position.z]))
    });
    m.function(EMITTER_POSITION, |eng: &Engine, handle: i64| {
        let state = eng.resource::<AudioState>();
        let position = state.borrow().emitter_position(handle_of(handle));
        Ok(position.map_or(Value::Nil, |at| Value::Vec3([at.x, at.y, at.z])))
    });
    m.function(
        SET_EMITTER_POSITION,
        |eng: &Engine, (handle, position): (i64, Value)| {
            let Some(position) = point(Some(&position)) else {
                bail!(
                    "an emitter's position is a Vec3, not a {}",
                    position.type_name()
                );
            };
            eng.resource::<AudioState>()
                .borrow_mut()
                .set_emitter_position(handle_of(handle), position);
            Ok(())
        },
    );
    // The two halves of a placement, so a script can show what the mix is
    // doing: a debug overlay, or a subtitle only for a sound near enough to
    // hear.
    m.function(DISTANCE_GAIN, |eng: &Engine, handle: i64| {
        let state = eng.resource::<AudioState>();
        let placement = state.borrow().placement_of(handle_of(handle));
        Ok(placement.map_or(Value::Nil, |placed| Value::Num(f64::from(placed.gain))))
    });
    m.function(PAN, |eng: &Engine, handle: i64| {
        let state = eng.resource::<AudioState>();
        let placement = state.borrow().placement_of(handle_of(handle));
        Ok(placement.map_or(Value::Nil, |placed| Value::Num(f64::from(placed.pan))))
    });
}

#[cfg(test)]
mod tests {
    use balaur_core::{App, AppConfig, Engine};
    use std::cell::RefCell;
    use std::rc::Rc;

    use balaur_script::{Call, Declared, Module, Value, Values};

    use crate::vocabulary::keys as k;

    /// What the module declares, as a backend would read it.
    fn declared() -> Rc<RefCell<Declared<Engine>>> {
        let declared: Rc<RefCell<Declared<Engine>>> = Rc::default();
        super::install_audio_api(&mut Module::new("audio", declared.clone()));
        declared
    }

    /// Every function name a source spells after the module's name and a dot.
    fn audio_calls(source: &str) -> Vec<String> {
        source
            .split("audio.")
            .skip(1)
            .map(|rest| {
                rest.chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect::<String>()
            })
            .filter(|name| !name.is_empty())
            .collect()
    }

    /// `audio.bus_effects` hands back the keys `[audio.buses]` spells, so a
    /// script reads a bus's effects by the names the project wrote them in.
    #[test]
    fn bus_effects_reads_back_a_buses_keys_and_nil_without_any() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("project.toml"),
            "[application]\nname = \"a\"\nmain_scene = \"main.toml\"\n\n[audio.buses]\ncave = { low_pass_hz = 800.0, reverb_time = 0.25, auto_gain = true }\nsfx = { volume_linear = 0.5 }\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("main.toml"), "").unwrap();
        let mut app = App::new(AppConfig::bare(dir.path().to_path_buf())).unwrap();
        balaur_plugin::load(&mut app, &mut crate::AudioPlugin).unwrap();
        app.load_project().unwrap();
        let declared = declared();
        let bus = |name: &str| {
            let args = [Value::text(name)];
            let held = declared.borrow();
            match held.modules["audio"]["bus_effects"].call.as_ref().unwrap() {
                Call::Raw(func) => func(&app.engine, &args),
                Call::Typed(func) => func(&app.engine, &Values(&args)),
            }
            .expect("the call succeeds")
        };

        let Value::Map(entries) = bus("cave") else {
            panic!("a bus with effects reads back a map");
        };
        let entry = |key: &str| {
            entries
                .iter()
                .find(|(named, _)| named == key)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(entry(k::LOW_PASS_HZ), Some(Value::Num(800.0)));
        assert_eq!(entry(k::REVERB_TIME), Some(Value::Num(0.25)));
        assert_eq!(entry(k::AUTO_GAIN), Some(Value::Bool(true)));
        assert_eq!(entry(k::HIGH_PASS_HZ), Some(Value::Num(0.0)));
        assert_eq!(entries.len(), 14, "every effect key, set or not");
        assert_eq!(bus("sfx"), Value::Nil, "a bus with none switched on");
        assert_eq!(bus("nobody"), Value::Nil, "a name nobody declared");
    }

    #[test]
    fn every_audio_call_the_crate_docs_name_is_registered() {
        let declared = declared();
        let declared = declared.borrow();
        let sources = [
            include_str!("lib.rs"),
            include_str!("script_api.rs"),
            include_str!("spatial.rs"),
            include_str!("play_file.rs"),
            include_str!("bus.rs"),
            include_str!("cache.rs"),
        ];
        for source in sources {
            for name in audio_calls(source) {
                assert!(
                    declared.modules["audio"].contains_key(&name),
                    "the docs name `audio.{name}`, which the module does not register"
                );
            }
        }
        for named in [
            "play",
            "stop_playback",
            "set_volume_linear",
            "set_pitch_scale",
            "play_file",
            "seek",
            "skip",
            "devices",
        ] {
            assert!(
                declared.module_docs["audio"].contains(&format!("`{named}`")),
                "the module doc never names `{named}`"
            );
        }
    }
}
