//! The `sound` component: its schema, the reader that turns a table into a
//! [`Sound`], and the read-back.

use balaur_core::Engine;
use balaur_core::components::{ComponentDef, prop_bool, prop_f32, prop_str};
use balaur_core::hecs::Entity;

use crate::bus::Buses;
use crate::playback::{AutoGain, Effects};
use crate::spatial::Attenuation;
use crate::vocabulary::{keys as k, words as w};
use crate::{AudioState, FINISHED_EVENT, Sound, play_node};

/// The `sound` scene key: the one an editor-saved `[nodes.sound]` writes.
///
/// Takes the plugin `Registry` rather than `&mut App`: audio registers
/// through the plugin seam, and `Registry::register_component` is that
/// seam's spelling of the same operation.
pub(crate) fn register_sound_component(reg: &mut balaur_plugin::Registry<'_>) {
    let attenuation = format!(
        r#"{{ type = "enum", default = "{}", options = [{}], description = "How a positional sound falls off past `min_distance`: `inverse` halves its gain at every doubling of distance, `inverse_square` quarters it", group = "positional" }}"#,
        w::INVERSE,
        ComponentDef::options(&[w::INVERSE, w::INVERSE_SQUARE]),
    );
    // Writes the node's `Sound` in `AudioState::nodes`; a live playback takes
    // what can change while it plays, and `autoplay` starts one.
    reg.register_component(
        "sound",
        ComponentDef {
            events: &[(FINISHED_EVENT, "the handle that played out")],
            warnings: None,
            doc: "A sound on the node: `file`, `volume_linear`, `pitch_scale` and `loop`, with its `layers` mixed in and its `queue` played after it. `autoplay` starts it on load, `node.sound.play()` triggers it, `seek` and `skip` move it, `positional` plays it from the node for the `listener`, and the node announces `finished` when it plays out. Filters, distortion, reverb and automatic gain control are rodio's.",
            schema: ComponentDef::parse_schema(
                "sound",
                &ComponentDef::schema(&[
                    (k::FILE, r#"{ type = "string", default = "", description = "Audio file, project-relative; required to play" }"#),
                    (k::AUTOPLAY, r#"{ type = "bool", default = false, description = "Start playing when the node enters the scene" }"#),
                    (k::VOLUME_LINEAR, r#"{ type = "float", default = 1.0, min = 0.0, description = "Linear gain; 1 is the file's own level" }"#),
                    (k::PITCH_SCALE, r#"{ type = "float", default = 1.0, min = 0.01, description = "Playback speed multiplier" }"#),
                    (k::LOOP, r#"{ type = "bool", default = false, description = "Restart the sound when it ends; with a `queue`, each file repeats until `node.sound.skip()` moves on" }"#),
                    (k::BUS, r#"{ type = "string", default = "", description = "Audio bus this plays through; empty is `master`" }"#),
                    (k::PAUSED, r#"{ type = "bool", default = false, description = "Hold the sound where it is; `playback_time` and the count to `finished` hold with it" }"#),
                    (k::PLAYBACK_TIME, r#"{ type = "float", default = 0.0, readonly = true, description = "Seconds into the file playing now, counted on the fixed step so a run with no output device reads the same; 0 when nothing plays" }"#),
                    (k::POSITIONAL, r#"{ type = "bool", default = false, description = "Place the sound where the node is, heard from the `listener`" }"#),
                    (k::MIN_DISTANCE, r#"{ type = "float", default = 1.0, min = 0.001, description = "Full volume within this distance of the listener", group = "positional" }"#),
                    (k::MAX_DISTANCE, r#"{ type = "float", default = 50.0, min = 0.001, description = "Silent beyond this distance from the listener", group = "positional" }"#),
                    (k::DOPPLER_LEVEL, r#"{ type = "float", default = 0.0, min = 0.0, description = "How much the closing speed bends the pitch; 0 is off, 1 physical", group = "positional" }"#),
                    (k::ATTENUATION, &attenuation),
                    (k::PAN, r#"{ type = "float", default = 0.0, min = -1.0, max = 1.0, description = "Balance of a sound that is not positional: -1 left, 0 both channels at full, 1 right. A positional sound is panned by where it is" }"#),
                    (k::START_TIME, r#"{ type = "float", default = 0.0, min = 0.0, description = "Seconds into `file` each play starts from. Read when it starts", group = "playback" }"#),
                    (k::END_TIME, r#"{ type = "float", default = 0.0, min = 0.0, description = "Seconds into `file` each play stops at, and a loop turns back at; 0 plays to the end. Read when it starts", group = "playback" }"#),
                    (k::DELAY, r#"{ type = "float", default = 0.0, min = 0.0, description = "Seconds of silence before the sound starts. Read when it starts", group = "playback" }"#),
                    (k::LOOP_OFFSET, r#"{ type = "float", default = -1.0, min = -1.0, description = "Seconds into `file` every repeat after the first starts from; below 0 takes the file's own `loop_offset` import setting. Read when it starts", group = "playback" }"#),
                    (k::LAYERS, r#"{ type = "list", of = { type = "string" }, default = [], description = "Files mixed with `file` for its whole length, each at its own import level. Read when it starts", group = "playback" }"#),
                    (k::QUEUE, r#"{ type = "list", of = { type = "string" }, default = [], description = "Files played after `file`, in order and without a gap; `node.sound.skip()` moves to the next. Read when it starts", group = "playback" }"#),
                    (k::FADE_IN_TIME, r#"{ type = "float", default = 0.0, min = 0.0, description = "Seconds the sound rises from silence over when it starts", group = "fades" }"#),
                    (k::FADE_OUT_TIME, r#"{ type = "float", default = 0.0, min = 0.0, description = "Seconds the sound falls to silence over: the last ones before it plays out, and the first after `audio.stop`", group = "fades" }"#),
                    (k::CROSSFADE_TIME, r#"{ type = "float", default = 0.0, min = 0.0, description = "Seconds a playing sound fades out over when `file` changes, while the new file, if `autoplay` starts it, fades in; 0 cuts", group = "fades" }"#),
                    (k::LOW_PASS_HZ, r#"{ type = "float", default = 0.0, min = 0.0, description = "Cut what is above this frequency; 0 is off", group = "filters" }"#),
                    (k::LOW_PASS_Q, r#"{ type = "float", default = 0.5, min = 0.01, description = "How sharply the low-pass turns at its cutoff; higher rings there", group = "filters" }"#),
                    (k::HIGH_PASS_HZ, r#"{ type = "float", default = 0.0, min = 0.0, description = "Cut what is below this frequency; 0 is off", group = "filters" }"#),
                    (k::HIGH_PASS_Q, r#"{ type = "float", default = 0.5, min = 0.01, description = "How sharply the high-pass turns at its cutoff; higher rings there", group = "filters" }"#),
                    (k::DISTORTION_GAIN, r#"{ type = "float", default = 1.0, min = 0.0, description = "Gain before the distortion's clip", group = "effects" }"#),
                    (k::DISTORTION_THRESHOLD, r#"{ type = "float", default = 0.0, min = 0.0, max = 1.0, description = "Level samples are clipped at after `distortion_gain`; 0 is off", group = "effects" }"#),
                    (k::REVERB_TIME, r#"{ type = "float", default = 0.0, min = 0.0, description = "Seconds an echo of the sound trails it by; 0 is off. Read when it starts", group = "effects" }"#),
                    (k::REVERB_LEVEL, r#"{ type = "float", default = 0.0, min = 0.0, description = "Gain of that echo", group = "effects" }"#),
                    (k::AUTO_GAIN, r#"{ type = "bool", default = false, description = "Even the level out with automatic gain control. Read when it starts; turning it off while playing bypasses it", group = "auto_gain" }"#),
                    (k::AUTO_GAIN_TARGET, r#"{ type = "float", default = 1.0, min = 0.0, description = "The level it holds the sound at; 1 is the file's own", group = "auto_gain" }"#),
                    (k::AUTO_GAIN_ATTACK_TIME, r#"{ type = "float", default = 4.0, min = 0.0, max = 10.0, description = "Seconds it takes to answer the level rising", group = "auto_gain" }"#),
                    (k::AUTO_GAIN_RELEASE_TIME, r#"{ type = "float", default = 0.0, min = 0.0, max = 10.0, description = "Seconds it takes to answer the level falling", group = "auto_gain" }"#),
                    (k::AUTO_GAIN_MAX, r#"{ type = "float", default = 7.0, min = 0.0, description = "The most gain it applies", group = "auto_gain" }"#),
                    (k::AUTO_GAIN_FLOOR, r#"{ type = "float", default = 0.0, min = 0.0, description = "The least gain it applies", group = "auto_gain" }"#),
                ]),
            ),
            tags: &[balaur_core::components::tag::AUDIO],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                apply_sound(eng, entity, params);
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                remove_sound(eng, entity);
                Ok(())
            }),
            get: Box::new(sound_of),
        },
    );
}

fn files(params: &toml::Value, key: &str) -> Vec<String> {
    params
        .get(key)
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// A time key: below zero, or not a finite number, is none.
fn seconds(params: &toml::Value, key: &str) -> f32 {
    let value = prop_f32(params, key);
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn effects_of(params: &toml::Value) -> Effects {
    Effects {
        pan: prop_f32(params, k::PAN).clamp(-1.0, 1.0),
        low_pass_hz: prop_f32(params, k::LOW_PASS_HZ).max(0.0),
        low_pass_q: prop_f32(params, k::LOW_PASS_Q),
        high_pass_hz: prop_f32(params, k::HIGH_PASS_HZ).max(0.0),
        high_pass_q: prop_f32(params, k::HIGH_PASS_Q),
        reverb_time: seconds(params, k::REVERB_TIME),
        reverb_level: prop_f32(params, k::REVERB_LEVEL).max(0.0),
        distortion_gain: prop_f32(params, k::DISTORTION_GAIN).max(0.0),
        distortion_threshold: prop_f32(params, k::DISTORTION_THRESHOLD).clamp(0.0, 1.0),
        auto_gain: AutoGain {
            on: prop_bool(params, k::AUTO_GAIN),
            target: prop_f32(params, k::AUTO_GAIN_TARGET).max(0.0),
            attack_time: seconds(params, k::AUTO_GAIN_ATTACK_TIME).min(10.0),
            release_time: seconds(params, k::AUTO_GAIN_RELEASE_TIME).min(10.0),
            max: prop_f32(params, k::AUTO_GAIN_MAX).max(0.0),
            floor: prop_f32(params, k::AUTO_GAIN_FLOOR).max(0.0),
        },
    }
}

/// A node's `sound` as its table describes it, with no playback yet.
fn sound_from(params: &toml::Value) -> Sound {
    Sound {
        file: prop_str(params, k::FILE).to_string(),
        autoplay: prop_bool(params, k::AUTOPLAY),
        volume: prop_f32(params, k::VOLUME_LINEAR),
        pitch: prop_f32(params, k::PITCH_SCALE),
        looped: prop_bool(params, k::LOOP),
        bus: prop_str(params, k::BUS).to_string(),
        paused: prop_bool(params, k::PAUSED),
        positional: prop_bool(params, k::POSITIONAL),
        min_distance: prop_f32(params, k::MIN_DISTANCE),
        max_distance: prop_f32(params, k::MAX_DISTANCE),
        doppler: prop_f32(params, k::DOPPLER_LEVEL),
        attenuation: Attenuation::from_word(prop_str(params, k::ATTENUATION)),
        start_time: seconds(params, k::START_TIME),
        end_time: seconds(params, k::END_TIME),
        delay: seconds(params, k::DELAY),
        loop_offset: prop_f32(params, k::LOOP_OFFSET),
        layers: files(params, k::LAYERS),
        queue: files(params, k::QUEUE),
        fade_in_time: seconds(params, k::FADE_IN_TIME),
        fade_out_time: seconds(params, k::FADE_OUT_TIME),
        crossfade_time: seconds(params, k::CROSSFADE_TIME),
        effects: effects_of(params),
        handle: None,
    }
}

fn apply_sound(eng: &Engine, entity: Entity, params: &toml::Value) {
    let asked = sound_from(params);
    let has_file = !asked.file.trim().is_empty();
    let autoplay = asked.autoplay;
    crate::ensure_loaded(eng);
    let (start, crossfade) = {
        let buses = eng.resource::<Buses>();
        let buses = buses.borrow();
        let state = eng.resource::<AudioState>();
        let mut state = state.borrow_mut();
        let sound = state.nodes.entry(entity).or_default();
        let file_changed = sound.file != asked.file;
        let handle = sound.handle.take();
        let crossfade_time = asked.crossfade_time;
        *sound = Sound {
            // A `sound` naming another file drops the old playback.
            handle: handle.filter(|_| !file_changed),
            ..asked
        };
        let crossfade = match handle {
            Some(handle) if file_changed => {
                state.release_over(handle, crossfade_time);
                crossfade_time > 0.0
            }
            // What can change on a sound already going lands on it.
            Some(handle) => {
                if let Some(sound) = state.nodes.get(&entity).cloned() {
                    state.retune(handle, &sound, &buses);
                }
                false
            }
            None => false,
        };
        let started = state.nodes.get(&entity).is_some_and(|s| s.handle.is_some());
        (autoplay && has_file && !started, crossfade)
    };
    // Re-applying the component must not restart a sound already started:
    // the same rule the `animation` component holds for its autoplay clip.
    if start && let Err(why) = play_node(eng, entity, crossfade) {
        tracing::warn!("sound autoplay: {why:#}");
    }
}

fn remove_sound(eng: &Engine, entity: Entity) {
    let Some(state) = eng.try_resource::<AudioState>() else {
        return;
    };
    let mut state = state.borrow_mut();
    let removed = state.nodes.shift_remove(&entity);
    if let Some(handle) = removed.and_then(|sound| sound.handle) {
        state.stop(handle);
    }
}

fn sound_of(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.try_resource::<AudioState>()?;
    let state = state.borrow();
    let sound = state.nodes.get(&entity)?;
    let at = sound
        .handle
        .and_then(|handle| state.playback_time(handle))
        .unwrap_or_default();
    let list = |files: &[String]| {
        toml::Value::Array(files.iter().cloned().map(toml::Value::String).collect())
    };
    let fx = &sound.effects;
    let agc = &fx.auto_gain;
    let mut out = toml::map::Map::new();
    let mut put = |key: &str, value: toml::Value| {
        out.insert(key.into(), value);
    };
    put(k::FILE, sound.file.clone().into());
    put(k::AUTOPLAY, sound.autoplay.into());
    put(k::VOLUME_LINEAR, f64::from(sound.volume).into());
    put(k::PITCH_SCALE, f64::from(sound.pitch).into());
    put(k::LOOP, sound.looped.into());
    put(k::BUS, sound.bus.clone().into());
    put(k::PAUSED, sound.paused.into());
    put(k::PLAYBACK_TIME, at.into());
    put(k::POSITIONAL, sound.positional.into());
    put(k::MIN_DISTANCE, f64::from(sound.min_distance).into());
    put(k::MAX_DISTANCE, f64::from(sound.max_distance).into());
    put(k::DOPPLER_LEVEL, f64::from(sound.doppler).into());
    put(k::ATTENUATION, sound.attenuation.word().into());
    put(k::PAN, f64::from(fx.pan).into());
    put(k::START_TIME, f64::from(sound.start_time).into());
    put(k::END_TIME, f64::from(sound.end_time).into());
    put(k::DELAY, f64::from(sound.delay).into());
    put(k::LOOP_OFFSET, f64::from(sound.loop_offset).into());
    put(k::LAYERS, list(&sound.layers));
    put(k::QUEUE, list(&sound.queue));
    put(k::FADE_IN_TIME, f64::from(sound.fade_in_time).into());
    put(k::FADE_OUT_TIME, f64::from(sound.fade_out_time).into());
    put(k::CROSSFADE_TIME, f64::from(sound.crossfade_time).into());
    put(k::LOW_PASS_HZ, f64::from(fx.low_pass_hz).into());
    put(k::LOW_PASS_Q, f64::from(fx.low_pass_q).into());
    put(k::HIGH_PASS_HZ, f64::from(fx.high_pass_hz).into());
    put(k::HIGH_PASS_Q, f64::from(fx.high_pass_q).into());
    put(k::DISTORTION_GAIN, f64::from(fx.distortion_gain).into());
    put(
        k::DISTORTION_THRESHOLD,
        f64::from(fx.distortion_threshold).into(),
    );
    put(k::REVERB_TIME, f64::from(fx.reverb_time).into());
    put(k::REVERB_LEVEL, f64::from(fx.reverb_level).into());
    put(k::AUTO_GAIN, agc.on.into());
    put(k::AUTO_GAIN_TARGET, f64::from(agc.target).into());
    put(k::AUTO_GAIN_ATTACK_TIME, f64::from(agc.attack_time).into());
    put(
        k::AUTO_GAIN_RELEASE_TIME,
        f64::from(agc.release_time).into(),
    );
    put(k::AUTO_GAIN_MAX, f64::from(agc.max).into());
    put(k::AUTO_GAIN_FLOOR, f64::from(agc.floor).into());
    Some(toml::Value::Table(out))
}
