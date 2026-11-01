//! What opening the window and drawing the first picture cost, for `--timings`.
//!
//! The device and the shaders are the driver's, and on a first launch they are
//! seconds, so each gets a row of its own. A pipeline first asked for after that
//! stalls the frame that asked, so those are counted apart.

use balaur_core::App;
use balaur_core::timings::{self, Phase};
use kiss3d::context::Context;

/// Start timing the window, and say this run's boot ends at a picture.
pub(crate) fn window_opening() -> Phase {
    timings::draws_pictures();
    Phase::start()
}

/// File what opening the window cost. Its shaders wait for [`note_drawn`], with
/// the ones the first frame builds.
pub(crate) fn note_window_opened(phase: &Phase) {
    let device = Context::device_wait();
    let shaders = Context::shader_builds().time;
    timings::note_boot("device", device);
    timings::note_boot("renderer", phase.elapsed().saturating_sub(device + shaders));
}

/// A frame was just drawn, `frame` being how many have stepped. The first ends the
/// boot, and the shaders built on the way to it are the boot's.
pub(crate) fn note_drawn(frame: u64) {
    if frame != 1 {
        return;
    }
    let builds = Context::take_shader_builds();
    timings::note_boot(
        &format!(
            "shaders ({} modules, {} pipelines)",
            builds.modules, builds.pipelines
        ),
        builds.time,
    );
    timings::note_first_frame();
}

/// Report the shaders built since the last frame. Before the first picture they are
/// still the boot's, which [`note_drawn`] files.
pub(crate) fn report_shader_builds(app: &App, frame: u64) {
    if frame == 0 {
        return;
    }
    let builds = Context::take_shader_builds();
    if builds.time.is_zero() {
        return;
    }
    timings::record(&app.engine, "shader build", builds.time);
    timings::note_later_shaders(builds.modules, builds.pipelines, builds.time);
}
