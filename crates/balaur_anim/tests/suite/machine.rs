//! State machines: a start state, `auto` transitions on conditions and at a
//! clip's end, travel through the states between, and the fade each
//! transition asks for.

use balaur_anim::{AnimationPlugin, machine};
use balaur_core::hecs::Entity;
use balaur_core::{App, AppConfig, assets, components, scene};

fn app() -> App {
    let mut app = App::new(AppConfig::bare(std::path::PathBuf::from("tests/fixtures"))).unwrap();
    balaur_plugin::load(&mut app, &mut AnimationPlugin::default()).unwrap();
    app
}

fn tick(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
    }
}

/// A clip that holds the node at `x`, for `length` seconds.
fn hold(x: f64, length: f64, wrap: &str) -> toml::Value {
    toml::from_str(&format!(
        r#"
        length = {length}
        loop = "{wrap}"
        [[tracks]]
        property = "position"
        interp = "linear"
        keys = [{{ t = 0.0, value = [{x}, 0.0, 0.0] }}, {{ t = {length}, value = [{x}, 0.0, 0.0] }}]
        "#
    ))
    .unwrap()
}

/// A node playing `idle` (x = 0, looping), `walk` (x = 1, looping) and
/// `jump` (x = 2, half a second, once), run by `machine`.
fn rig(app: &App, machine: &str) -> Entity {
    let eng = &app.engine;
    let root = eng.root();
    let entity = scene::spawn_node(&mut eng.world_mut(), "Hero", root);
    components::add(eng, entity, "animation", None).unwrap();
    balaur_anim::define(eng, entity, "idle", hold(0.0, 1.0, "loop")).unwrap();
    balaur_anim::define(eng, entity, "walk", hold(1.0, 1.0, "loop")).unwrap();
    balaur_anim::define(eng, entity, "jump", hold(2.0, 0.5, "none")).unwrap();
    let body: toml::Value = toml::from_str(machine).unwrap();
    let reference = assets::define_inline(eng, machine::MACHINE_ASSET_TYPE, body).unwrap();
    let params = toml::Value::Table(toml::map::Map::from_iter([(
        "machine".to_string(),
        toml::Value::String(reference.to_string()),
    )]));
    components::add(eng, entity, "state_machine", Some(&params)).unwrap();
    entity
}

fn x(app: &App, entity: Entity) -> f32 {
    app.engine
        .world()
        .get::<&scene::Transform>(entity)
        .map_or(f32::NAN, |t| t.position.x)
}

const STATES: &str = r#"
start = "idle"
[states]
idle = "idle"
walk = "walk"
jump = "jump"
"#;

#[test]
fn a_machine_enters_its_start_and_crosses_when_a_condition_comes_on() {
    let mut app = app();
    let hero = rig(
        &app,
        &format!(
            "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"walk\"\nadvance = \"auto\"\ncondition = \"moving\"\nfade = 0.5\n"
        ),
    );
    tick(&mut app, 10);
    assert_eq!(machine::state(&app.engine, hero).as_deref(), Some("idle"));
    assert_eq!(balaur_anim::current(&app.engine, hero).as_deref(), Some("idle"));
    assert!(x(&app, hero).abs() < 1e-4);

    machine::set_condition(&app.engine, hero, "moving", true).unwrap();
    tick(&mut app, 15);
    assert_eq!(machine::state(&app.engine, hero).as_deref(), Some("walk"));
    // A quarter of a second into a half-second fade: between the two.
    let mid = x(&app, hero);
    assert!(mid > 0.2 && mid < 0.8, "mid-fade the node sits between the clips, at {mid}");
    tick(&mut app, 30);
    assert!((x(&app, hero) - 1.0).abs() < 1e-4, "the fade has run");
}

#[test]
fn an_at_end_transition_waits_for_the_clip_to_finish() {
    let mut app = app();
    let hero = rig(
        &app,
        &(STATES.replace("start = \"idle\"", "start = \"jump\"")
            + "\n[[transitions]]\nfrom = \"jump\"\nto = \"idle\"\nadvance = \"auto\"\nswitch = \"at_end\"\n"),
    );
    tick(&mut app, 20);
    assert_eq!(machine::state(&app.engine, hero).as_deref(), Some("jump"));
    tick(&mut app, 20);
    assert_eq!(machine::state(&app.engine, hero).as_deref(), Some("idle"));
    assert_eq!(balaur_anim::current(&app.engine, hero).as_deref(), Some("idle"));
}

#[test]
fn travel_passes_through_the_states_between_and_cuts_to_one_it_cannot_reach() {
    let mut app = app();
    let hero = rig(
        &app,
        &format!(
            "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"walk\"\n\n[[transitions]]\nfrom = \"walk\"\nto = \"jump\"\n"
        ),
    );
    tick(&mut app, 3);
    machine::travel(&app.engine, hero, "jump").unwrap();
    tick(&mut app, 1);
    assert_eq!(machine::state(&app.engine, hero).as_deref(), Some("walk"));
    tick(&mut app, 1);
    assert_eq!(machine::state(&app.engine, hero).as_deref(), Some("jump"));

    // Nothing leads back from `jump`, so the machine cuts straight to `idle`.
    machine::travel(&app.engine, hero, "idle").unwrap();
    tick(&mut app, 1);
    assert_eq!(machine::state(&app.engine, hero).as_deref(), Some("idle"));
}

#[test]
fn a_transition_to_a_state_the_machine_lacks_is_refused() {
    let body: toml::Value = toml::from_str(&format!(
        "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"swim\"\n"
    ))
    .unwrap();
    let why = machine::parse(&body).unwrap_err();
    assert!(format!("{why:#}").contains("swim"), "{why:#}");
}
