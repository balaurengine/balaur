//! Any text as a scene file, on an app with physics and animation: building
//! it refuses or builds, and the frames after it run. Every component that
//! reads its table from a scene reads it here.
#![no_main]

use balaur_animation::AnimationPlugin;
use balaur_core::{App, AppConfig};
use balaur_physics::PhysicsPlugin;
use libfuzzer_sys::fuzz_target;

/// Enough for a body to settle into contact and a clip to advance.
const FRAMES: u32 = 3;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    let mut app = App::new(AppConfig::bare(".")).expect("a bare app starts");
    balaur_plugin::load(&mut app, &mut PhysicsPlugin).expect("physics loads");
    balaur_plugin::load(&mut app, &mut AnimationPlugin).expect("animation loads");
    let root = app.engine.root();
    if balaur_core::project::instantiate_scene(&app.engine, source, root, false).is_err() {
        return;
    }
    for _ in 0..FRAMES {
        app.tick(1.0 / 60.0);
    }
});
