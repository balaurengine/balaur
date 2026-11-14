//! What the emitters tell a loop about to sleep: pending while one bears
//! particles or has some left to die out, timed from its settings.

use balaur_core::hecs::Entity;
use balaur_core::{App, AppConfig, components, fixed_dt, rest, scene};

/// The name the plugin's probe answers to.
const PARTICLES: &str = "particles";

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).expect("App::new builds headless");
    balaur_plugin::load(&mut app, &mut balaur_render::RenderPlugin)
        .expect("the render plugin builds headless");
    app
}

fn pends(app: &App) -> bool {
    rest::pending_names(&app.engine).contains(&PARTICLES)
}

fn tick(app: &mut App, steps: u32) {
    for _ in 0..steps {
        app.tick(fixed_dt());
    }
}

fn emitter(app: &App, component: &str, params: &str) -> Entity {
    let node = scene::spawn_node(&mut app.engine.world_mut(), "Sparks", app.engine.root());
    let params: toml::Value = toml::from_str(params).expect("the emitter params are valid TOML");
    components::add(&app.engine, node, component, Some(&params)).expect("the emitter applies");
    node
}

fn patch(app: &App, node: Entity, component: &str, params: &str) {
    let params: toml::Value = toml::from_str(params).expect("the patch is valid TOML");
    components::patch(&app.engine, node, component, &params).expect("the patch applies");
}

#[test]
fn a_one_shot_burst_is_pending_until_its_last_particle_has_died() {
    let mut app = app();
    // All born at once and living a quarter second: fifteen steps.
    emitter(
        &app,
        "particles2d",
        "one_shot = true\nexplosiveness = 1.0\nlifetime = 0.25\nrate = 40.0",
    );
    tick(&mut app, 5);
    assert!(pends(&app), "the burst is in the air");

    tick(&mut app, 15);
    assert!(!pends(&app), "it has died out and said so");
}

#[test]
fn a_steady_emitter_is_pending_while_it_emits_and_for_one_lifetime_after() {
    let mut app = app();
    let sparks = emitter(&app, "particles3d", "lifetime = 0.25\nrate = 40.0");
    tick(&mut app, 30);
    assert!(pends(&app), "it bears particles for as long as it emits");

    patch(&app, sparks, "particles3d", "emitting = false");
    tick(&mut app, 5);
    assert!(pends(&app), "the last ones born are still alive");

    tick(&mut app, 15);
    assert!(!pends(&app), "a lifetime on, none is");
}

#[test]
fn an_emitter_that_never_emitted_is_at_rest() {
    let mut app = app();
    emitter(&app, "particles2d", "emitting = false");
    tick(&mut app, 2);
    assert!(!pends(&app));
}

#[test]
fn a_pause_holds_an_emitter_at_rest_and_a_hosted_one_is_its_tools() {
    let mut app = app();
    let sparks = emitter(&app, "particles2d", "lifetime = 0.25");
    tick(&mut app, 1);
    assert!(pends(&app));

    app.engine.set_paused(true);
    tick(&mut app, 1);
    assert!(!pends(&app), "a held emitter bears nothing");
    app.engine.set_paused(false);

    app.engine.set_debug_scope(Some(sparks));
    tick(&mut app, 1);
    assert!(
        !pends(&app),
        "an editor's opened scene does not keep it awake"
    );
}
