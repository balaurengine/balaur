//! Audio as a Balaur plugin, backed by rodio.
//!
//! If no output device is available (CI, headless servers) the plugin logs a
//! warning once and every call becomes a no-op, so games and tests run
//! unchanged. Audio never feeds back into the simulation, so it has no
//! impact on determinism.

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use anyhow::Result;
use balaur_core::mlua;
use balaur_core::{App, Plugin, Stage};
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};

pub struct AudioState {
    device: Option<MixerDeviceSink>,
    players: Vec<Player>,
    project_root: PathBuf,
}

impl AudioState {
    fn play(&mut self, path: &str, volume: f32, looped: bool) -> Result<()> {
        let Some(device) = &self.device else {
            return Ok(());
        };
        let file = File::open(self.project_root.join(path))?;
        let player = rodio::play(device.mixer(), BufReader::new(file))?;
        player.set_volume(volume);
        if looped {
            // rodio has no toggle on a live player; looping is requested at
            // decode time instead. Keep it simple: re-open with a looped
            // decoder.
            player.stop();
            let file = File::open(self.project_root.join(path))?;
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
            project_root: app.project_root().to_path_buf(),
        });

        // Drop finished one-shot players so they do not accumulate.
        app.add_system(Stage::PostUpdate, |eng, _| {
            let state = eng.resource::<AudioState>();
            state.borrow_mut().players.retain(|p| !p.empty());
        });

        let m = app.lua_module("audio")?;
        m.function("play", |eng, (path, volume): (String, Option<f32>)| {
            let state = eng.resource::<AudioState>();
            let result = state
                .borrow_mut()
                .play(&path, volume.unwrap_or(1.0), false)
                .map_err(mlua::Error::external);
            result
        })?;
        m.function(
            "play_looping",
            |eng, (path, volume): (String, Option<f32>)| {
                let state = eng.resource::<AudioState>();
                let result = state
                    .borrow_mut()
                    .play(&path, volume.unwrap_or(1.0), true)
                    .map_err(mlua::Error::external);
                result
            },
        )?;
        m.function("stop_all", |eng, ()| {
            let state = eng.resource::<AudioState>();
            let mut state = state.borrow_mut();
            for player in state.players.drain(..) {
                player.stop();
            }
            Ok(())
        })?;
        Ok(())
    }
}
