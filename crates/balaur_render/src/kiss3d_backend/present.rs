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

/// Set to anything, it starts a run whose window cannot present.
pub const NO_PRESENT_VAR: &str = "BALAUR_NO_PRESENT";

/// While this is in the engine, the window's surface hands out no frame, as a
/// display that is asleep does: a test's way to run the frames a window cannot
/// present. [`NO_PRESENT_VAR`] inserts it as a run opens its window.
pub struct NoPresent;

/// Insert [`NoPresent`] when the environment asks for it.
pub(super) fn hear_no_present(app: &App) {
    if std::env::var_os(NO_PRESENT_VAR).is_some() {
        app.engine.insert_resource(NoPresent);
    }
}

/// Tell the window whether this frame may present.
pub(super) fn apply_no_present(app: &App, window: &mut Window) {
    window.set_surface_refused(app.engine.try_resource::<NoPresent>().is_some());
}

/// Whether [`NoPresent`] was asked of a kiss3d that cannot refuse its surface.
#[must_use]
pub fn no_present_ignored() -> bool {
    UNREFUSED.with(std::cell::Cell::get)
}

thread_local! {
    static UNREFUSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Stands in for `Window::set_surface_refused` while the kiss3d pin predates it:
/// an inherent method wins over a trait's, so the fork's is called once it has one.
trait RefuseSurface {
    fn set_surface_refused(&mut self, refused: bool);
}

impl RefuseSurface for Window {
    fn set_surface_refused(&mut self, refused: bool) {
        UNREFUSED.with(|ignored| ignored.set(ignored.get() || refused));
    }
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
        let settings = WindowSettings::of(&app.engine);
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

#[cfg(test)]
mod tests {
    use balaur_core::project::WindowSettings;
    use kiss3d::window::NumSamples;

    #[test]
    fn a_run_asks_its_canvas_for_the_sample_count_the_project_names() {
        let at = |msaa| {
            super::samples_of(&WindowSettings {
                msaa,
                ..WindowSettings::default()
            })
        };
        assert_eq!(at(1), NumSamples::One, "msaa = 1 is off");
        assert_eq!(at(4), NumSamples::Four);
        assert_eq!(at(8), NumSamples::Four, "a count it lacks takes four");
    }
}
