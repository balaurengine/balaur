//! `[window] msaa` and `vsync`, applied to the window when a setting moves
//! rather than only when it opens.

use balaur_core::App;
use balaur_core::project::WindowSettings;
use kiss3d::window::{NumSamples, Window};

/// The sample count `[window] msaa` asks for, in kiss3d's words; a count the
/// renderer does not offer takes four.
pub(super) fn samples_of(settings: &WindowSettings) -> NumSamples {
    NumSamples::from_u32(settings.msaa).unwrap_or_else(|| {
        tracing::warn!(
            "project.toml asks for msaa = {}; this renderer offers 1 or 4, using 4",
            settings.msaa
        );
        NumSamples::Four
    })
}

/// What the window was last told, so a frame whose settings did not move
/// reads none of them.
pub(super) struct Present {
    revision: Option<u64>,
    /// The count as asked, so an unsupported one warns once per change.
    msaa: u32,
}

impl Present {
    /// What the window was opened with.
    pub(super) fn opened_with(settings: &WindowSettings) -> Self {
        Self {
            revision: None,
            msaa: settings.msaa,
        }
    }

    /// Hand the window a changed sample count or present mode.
    pub(super) fn apply(&mut self, app: &App, window: &mut Window) {
        let revision = balaur_core::settings::revision(&app.engine);
        if self.revision == Some(revision) {
            return;
        }
        self.revision = Some(revision);
        let settings = WindowSettings::from_settings(&app.engine);
        if settings.msaa != self.msaa {
            self.msaa = settings.msaa;
            let samples = samples_of(&settings);
            if samples as u32 != window.samples() {
                window.set_samples(samples);
            }
        }
        if settings.vsync != window.vsync() {
            window.set_vsync(settings.vsync);
        }
    }
}
