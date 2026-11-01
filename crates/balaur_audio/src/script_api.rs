//! `audio.*`: the script bindings over [`AudioState`], the bus mix and the
//! positional half.

use anyhow::{Result, anyhow};
use balaur_core::glamx::Vec3;
use balaur_core::{Engine, entity_of};
use balaur_script::{Bindings, BindingsExt, NodeId, Value};

use crate::bus::{self, Buses, Effects, Limiter};
use crate::cue;
use crate::spatial::Emitter;
use crate::vocabulary::keys as k;
use crate::{
    AudioState, Playback, play_on, publish_playback_times, read_sound, seek_on, skip_on, stop_on,
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

/// A script-supplied handle. Negative numbers wrap to values `play` never
/// hands out, so they answer false and no-op rather than erroring.
const fn handle_of(raw: i64) -> u64 {
    raw as u64
}

/// `audio.*`. Declared against the neutral seam, so it works on any backend.
pub(crate) fn install_audio_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Sound playback: `play` starts a node's `sound` component and returns a handle that `stop_playback`, `set_volume_linear`, `set_pitch_scale` and `is_playing` take; `seek` and `skip` move it. `play_cue` plays a sound named in `audio/cues.toml`, from a `position` heard from the `listener` when given one. A node's `sound` announces `finished` (`EVENT_FINISHED`) with the handle when it plays out. `devices` lists the outputs `[audio] device` can name.",
    );
    m.constant(
        "EVENT_FINISHED",
        Value::Str(crate::FINISHED_EVENT.to_string()),
    );
    m.describe(&[
        ("stop_playback", &[], "", "Silence the sound a handle names, over the `fade_out_time` its `sound` had; a finished, stopped or unknown handle is left alone."),
        ("set_volume_linear", &[], "", "Set a playing handle's linear gain, where 1 is the file's own level."),
        ("set_pitch_scale", &[], "", "Set a playing handle's speed multiplier, which carries its pitch with it."),
        ("device_ready", &[], "() -> bool", "Whether an output device is open. False on a page until the first gesture, and false for good with no sound card; playing before then hands out handles that make no sound."),
        ("is_playing", &[], "", "Whether a handle's sound is still going: false once it plays out or is stopped, on the same tick with or without an output device."),
        ("stop_all", &[], "", "Silence everything at once and clear the playback every `sound` component was holding."),
        ("play", &["sound"], "", "Start the node's own `sound` from the top, replacing what it had going, and return the new handle."),
        ("stop", &["sound"], "", "Silence what the node's `sound` started, over its `fade_out_time`; a node carrying none is left alone."),
        ("seek", &["sound"], "", "Jump the node's sound to `seconds` into the file it is playing; `playback_time` reads it back. A node with nothing playing is left alone."),
        ("skip", &["sound"], "", "Move the node's sound on to the next file in its `queue`. Past the last one it has played out, and the node announces `finished`."),
        ("devices", &[], "", "Every output device's name, as `[audio] device` takes it. Empty when the platform lists none."),
    ]);
    m.function("stop_playback", |eng: &Engine, handle: i64| {
        eng.resource::<AudioState>()
            .borrow_mut()
            .release(handle_of(handle));
        publish_playback_times(eng);
        Ok(())
    });
    m.function(
        "set_volume_linear",
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
        "set_pitch_scale",
        |eng: &Engine, (handle, pitch): (i64, f32)| {
            eng.resource::<AudioState>()
                .borrow_mut()
                .set_pitch(handle_of(handle), pitch);
            Ok(())
        },
    );
    m.function("device_ready", |eng: &Engine, ()| {
        crate::ensure_loaded(eng);
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
        publish_playback_times(eng);
        Ok(())
    });
    m.function("play", |eng: &Engine, node: NodeId| {
        play_on(eng, entity_of(node)?)
    });
    install_mixing_api(m);
    install_positional_api(m);
    m.function("stop", |eng: &Engine, node: NodeId| {
        stop_on(eng, entity_of(node)?);
        Ok(())
    });
    m.function("seek", |eng: &Engine, (node, seconds): (NodeId, f64)| {
        seek_on(eng, entity_of(node)?, seconds);
        Ok(())
    });
    m.function("skip", |eng: &Engine, node: NodeId| {
        skip_on(eng, entity_of(node)?);
        Ok(())
    });
    m.function("devices", |_: &Engine, ()| {
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
fn install_mixing_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("buses", &[], "() -> [string]", "Every audio bus, declared in `[audio.buses]` or made by setting a volume, in name order."),
        ("bus_volume_linear", &[], "(bus: string) -> float", "One bus's own gain, without its parents'."),
        ("set_bus_volume_linear", &[], "(bus: string, volume: float)", "Set one bus's gain and re-apply it to everything already playing on it: which is what a volume slider is."),
        ("bus_limit", &[], "", "One bus's limiter as `[audio.buses]` declares it: `limit`, `limit_threshold_db`, `limit_knee_db`, `limit_attack_time` and `limit_release_time`. Nil for a bus with none."),
        ("bus_effects", &[], "", "One bus's effects as `[audio.buses]` declares them, applied once to everything the bus mixes: `low_pass_hz`, `low_pass_q`, `high_pass_hz`, `high_pass_q`, `distortion_gain`, `distortion_threshold`, `reverb_time`, `reverb_level`, `auto_gain`, `auto_gain_target`, `auto_gain_attack_time`, `auto_gain_release_time`, `auto_gain_max` and `auto_gain_floor`. Built when the output device opens, as the limiter is. Nil for a bus with none switched on."),
        ("cues", &[], "() -> [string]", "Every sound named in `audio/cues.toml`, in name order."),
        ("play_cue", &[], "(name: string, options: CueOptions?) -> int?", "Play a named sound: the next of its variations in turn, at its own volume and pitch, through its own bus. A `position` in the options table places it. Nil for a name nothing declared."),
    ]);
    m.function("cues", |eng: &Engine, ()| {
        cue::ensure_loaded(eng);
        let names = eng.resource::<cue::Cues>().borrow().names();
        Ok(Value::List(names.into_iter().map(Value::text).collect()))
    });
    // The script says *what happened*; the cues file says what that sounds
    // like. Tuning one never touches the other.
    m.records(&[(
        "CueOptions",
        "{ position: any }",
        "Where `play_cue` places a sound, a `Vec3` or a `Vec2`, heard from the `listener`; unplaced when left out.",
    )]);
    m.function(
        "play_cue",
        |eng: &Engine, (name, opts): (String, Option<Value>)| {
            cue::ensure_loaded(eng);
            crate::ensure_loaded(eng);
            let played = {
                let cues = eng.resource::<cue::Cues>();
                let cues = cues.borrow();
                cues.get(&name)
                    .map(|cue| (cue.clone(), cues.next_file(&name)))
            };
            let Some((cue, Some(file))) = played else {
                tracing::warn!("audio cue '{name}' is not declared, or names no files");
                return Ok(Value::Nil);
            };
            let bytes = read_sound(eng, &file)?;
            let gain = eng.resource::<bus::Buses>().borrow().gain(&cue.bus);
            // Where an impact happened is the caller's to say; how far it
            // carries is the cues file's.
            let emitter = point(opt(opts.as_ref(), "position")).map(|position| {
                Emitter::new(
                    position,
                    cue.min_distance,
                    cue.max_distance,
                    cue.doppler_level,
                )
            });
            let handle = eng.resource::<AudioState>().borrow_mut().play_with(
                bytes,
                Playback {
                    volume: cue.volume_linear,
                    pitch: cue.pitch_scale,
                    looped: cue.looped,
                    bus: cue.bus,
                    gain,
                    emitter,
                    file: crate::FileSettings::of(eng, &file),
                    ..Playback::default()
                },
            );
            Ok(Value::Int(i64::try_from(handle).unwrap_or(i64::MAX)))
        },
    );
    m.function("buses", |eng: &Engine, ()| {
        bus::ensure_loaded(eng);
        let names = eng.resource::<bus::Buses>().borrow().names();
        Ok(Value::List(names.into_iter().map(Value::text).collect()))
    });
    m.function("bus_limit", |eng: &Engine, name: String| {
        bus::ensure_loaded(eng);
        let limit = eng.resource::<bus::Buses>().borrow().limit(&name);
        Ok(limit.map_or(Value::Nil, limit_table))
    });
    m.function("bus_effects", |eng: &Engine, name: String| {
        bus::ensure_loaded(eng);
        let effects = eng.resource::<bus::Buses>().borrow().effects(&name);
        Ok(effects.map_or(Value::Nil, |effects| effects_table(&effects)))
    });
    m.function("bus_volume_linear", |eng: &Engine, name: String| {
        bus::ensure_loaded(eng);
        let volume = eng.resource::<bus::Buses>().borrow().volume(&name);
        Ok(volume)
    });
    m.function(
        "set_bus_volume_linear",
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
fn install_positional_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("listener_position", &[], "() -> vec3", "Where the ears are: the current `listener` node's world position, else the current camera's."),
        ("emitter_position", &[], "(handle: int) -> vec3?", "Where a handle played with a `position` is; nil for a flat or unknown one."),
        ("set_emitter_position", &[], "(handle: int, x: float, y: float, z: float)", "Move what a handle plays from, so a sound follows something the script is driving; the frame takes its doppler from how far it moved."),
        ("distance_gain", &[], "(handle: int) -> float?", "The gain the distance to the listener is costing a positional handle right now: 1 up close, 0 out of range."),
        ("pan", &[], "(handle: int) -> float?", "Where a positional handle sits between the speakers: -1 hard left, 0 centred, 1 hard right."),
    ]);
    m.function("listener_position", |eng: &Engine, ()| {
        let state = eng.resource::<AudioState>();
        let position = state.borrow().listener().position;
        Ok(Value::Vec3([position.x, position.y, position.z]))
    });
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

#[cfg(test)]
mod tests {
    use balaur_core::{App, AppConfig, Engine};
    use balaur_script::{Bindings, BoundFn, FnDoc, Value};

    use crate::vocabulary::keys as k;

    #[derive(Default)]
    struct Recorded {
        functions: Vec<String>,
        doc: String,
    }

    impl Bindings<Engine> for Recorded {
        fn function_raw(&mut self, name: &str, _: BoundFn<Engine>) {
            self.functions.push(name.to_string());
        }
        fn constant(&mut self, _: &str, _: Value) {}
        fn module_doc(&mut self, doc: &'static str) {
            self.doc = doc.to_string();
        }
        fn describe(&mut self, _: &[FnDoc]) {}
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

    /// Every function the module registers, kept so a test can call one.
    #[derive(Default)]
    struct Callable {
        functions: Vec<(String, BoundFn<Engine>)>,
    }

    impl Bindings<Engine> for Callable {
        fn function_raw(&mut self, name: &str, func: BoundFn<Engine>) {
            self.functions.push((name.to_string(), func));
        }
        fn constant(&mut self, _: &str, _: Value) {}
    }

    impl Callable {
        fn call(&self, eng: &Engine, name: &str, args: &[Value]) -> Value {
            let (_, func) = self
                .functions
                .iter()
                .find(|(registered, _)| registered == name)
                .expect("registered");
            func(eng, args).expect("the call succeeds")
        }
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
        balaur_plugin::load(&mut app, &mut crate::AudioPlugin::default()).unwrap();
        app.load_project().unwrap();
        let mut callable = Callable::default();
        super::install_audio_api(&mut callable);
        let bus = |name: &str| callable.call(&app.engine, "bus_effects", &[Value::text(name)]);

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
        let mut recorded = Recorded::default();
        super::install_audio_api(&mut recorded);
        let sources = [
            include_str!("lib.rs"),
            include_str!("script_api.rs"),
            include_str!("spatial.rs"),
            include_str!("cue.rs"),
            include_str!("bus.rs"),
            include_str!("cache.rs"),
        ];
        for source in sources {
            for name in audio_calls(source) {
                assert!(
                    recorded.functions.contains(&name),
                    "the docs name `audio.{name}`, which the module does not register"
                );
            }
        }
        for named in [
            "play",
            "stop_playback",
            "set_volume_linear",
            "set_pitch_scale",
            "play_cue",
            "seek",
            "skip",
            "devices",
        ] {
            assert!(
                recorded.doc.contains(&format!("`{named}`")),
                "the module doc never names `{named}`"
            );
        }
    }
}
