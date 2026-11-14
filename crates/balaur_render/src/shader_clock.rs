//! What a shader's `time()` reads: the simulation's seconds, `engine::time()`,
//! so a run at a fixed tick draws the same frames and a replay draws its own.

use std::cell::Cell;

thread_local! {
    // kiss3d prepares a material with no way back to the app, so the frame
    // loop leaves the clock here before it draws.
    static SECONDS: Cell<f32> = const { Cell::new(0.0) };
}

/// Hold the engine's clock for this frame's draw.
pub(crate) fn set(eng: &balaur_core::Engine) {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a shader's clock is an f32"
    )]
    let seconds = eng.time() as f32;
    SECONDS.with(|held| held.set(seconds));
}

/// The seconds every material's `time()` reads this frame.
pub(crate) fn seconds() -> f32 {
    SECONDS.with(Cell::get)
}

#[cfg(test)]
mod tests {
    use balaur_core::{App, AppConfig};

    #[test]
    fn a_shader_reads_the_seconds_the_simulation_has_run() {
        let mut app = App::new(AppConfig::bare(".")).unwrap();
        app.tick(0.5);
        app.tick(0.25);
        super::set(&app.engine);
        assert!((super::seconds() - 0.75).abs() < 1e-6);
    }
}
