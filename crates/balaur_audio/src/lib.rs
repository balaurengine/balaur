//! Audio as a Balaur plugin, backed by rodio.
//!
//! If no output device is available (CI, headless servers) the plugin logs a
//! warning once and every call becomes a no-op, so games and tests run
//! unchanged. Audio never feeds back into the simulation, so it has no
//! impact on determinism.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::Result;
use balaur_core::project::ProjectRoot;
use balaur_core::Engine;
use balaur_core::{App, Plugin, Stage};
use balaur_script::{Bindings, BindingsExt, Value};
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};

pub struct AudioState {
    device: Option<MixerDeviceSink>,
    players: Vec<Player>,
}

/// Resolve a script-supplied sound path against the project.
///
/// Project-relative unless absolute, and read from the `ProjectRoot` entry
/// rather than cached here: one copy of the root, owned by the engine.
fn resolve(eng: &Engine, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    eng.try_resource::<ProjectRoot>()
        .map_or_else(|| p.to_path_buf(), |root| root.borrow().0.join(p))
}

impl AudioState {
    /// Stop everything currently playing.
    pub fn stop_all(&mut self) {
        for player in self.players.drain(..) {
            player.stop();
        }
    }

    /// Start a sound from an already-resolved path — the `audio.*` bindings
    /// resolve script paths against the project root. Silent, not an error,
    /// when there is no output device: a headless run must behave like a
    /// windowed one.
    ///
    /// # Errors
    /// If the file cannot be read or decoded.
    pub fn play(&mut self, path: &Path, volume: f32, looped: bool) -> Result<()> {
        let Some(device) = &self.device else {
            return Ok(());
        };
        let file = File::open(path)?;
        let player = rodio::play(device.mixer(), BufReader::new(file))?;
        player.set_volume(volume);
        if looped {
            // rodio has no toggle on a live player; looping is requested at
            // decode time instead. Keep it simple: re-open with a looped
            // decoder.
            player.stop();
            let file = File::open(path)?;
            let decoder = rodio::Decoder::try_from(BufReader::new(file))?;
            let looped_player = Player::connect_new(device.mixer());
            looped_player.set_volume(volume);
            looped_player.append(rodio::source::Source::repeat_infinite(decoder));
            self.players.push(looped_player);
            return Ok(());
        }
        self.players.push(player);
        Ok(())
    }
}

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn name(&self) -> &'static str {
        "audio"
    }

    fn build(&mut self, app: &mut App) -> Result<()> {
        let device = match DeviceSinkBuilder::open_default_sink() {
            Ok(device) => Some(device),
            Err(err) => {
                tracing::warn!("audio disabled: {err}");
                None
            }
        };
        app.engine.insert_resource(AudioState {
            device,
            players: Vec::new(),
        });

        // Drop finished one-shot players so they do not accumulate.
        app.add_system(Stage::PostUpdate, |eng, _| {
            let state = eng.resource::<AudioState>();
            state.borrow_mut().players.retain(|p| !p.empty());
        });

        let mut m = app.script_module("audio")?;
        install_audio_api(&mut m);
        Ok(())
    }
}

/// One key out of a script options table, or `None` if the table, the key or
/// its type is missing. A typo in an options table should not stop the frame.
fn opt<'a>(opts: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    match opts? {
        Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// `audio.*`. Declared against the neutral seam, so it works on any backend.
fn install_audio_api(m: &mut dyn Bindings<Engine>) {
    // `audio.play(path, { volume = 1.0, loop = true })`. The flag lives in the
    // options table rather than in the name, so fade/pitch/bus can join it
    // without doubling the function count (N9).
    m.function(
        "play",
        |eng: &Engine, (path, opts): (String, Option<Value>)| {
            let opts = opts.as_ref();
            let volume = match opt(opts, "volume") {
                Some(Value::Num(n)) => *n as f32,
                Some(Value::Int(i)) => *i as f32,
                _ => 1.0,
            };
            let looped = matches!(opt(opts, "loop"), Some(Value::Bool(true)));
            let full = resolve(eng, &path);
            let state = eng.resource::<AudioState>();
            state.borrow_mut().play(&full, volume, looped)?;
            Ok(())
        },
    );
    m.function("stop_all", |eng: &Engine, ()| {
        eng.resource::<AudioState>().borrow_mut().stop_all();
        Ok(())
    });
}
