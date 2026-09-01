//! Textured 2D sprites, without a window.
//!
//! A sprite's size lands in a component scripts read back, so it is simulation
//! state and every number here has to come out the same headless as windowed.

use balaur_core::{components, scene, App, AppConfig};
use balaur_render::{RenderPlugin, Renderable2d, Shape2d, DEFAULT_PIXELS_PER_UNIT};

/// 200x100 px, so width and height are never confused for each other, and a
/// 4x2 sheet divides both evenly.
const FIXTURE: &str = "tests/fixtures/sprite_200x100.png";

fn app() -> App {
    let mut app = App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    app.add_plugin(RenderPlugin).unwrap();
    app
}

fn node(app: &App) -> balaur_core::hecs::Entity {
    let root = app.engine.root();
    scene::spawn_node(&mut app.engine.world_mut(), "N", root)
}

fn sprite_table(extra: &str) -> toml::Value {
    let src = format!("texture = \"{FIXTURE}\"\n{extra}");
    toml::from_str(&src).unwrap()
}

fn apply(app: &App, entity: balaur_core::hecs::Entity, extra: &str) {
    components::add(&app.engine, entity, "sprite", Some(&sprite_table(extra))).unwrap();
}

/// Exact equality would be true here -- every expected value is a power of
/// two over 100 -- but the crate forbids strict float comparison, and a
/// tolerance says what the test actually means.
#[track_caller]
fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "expected {expected}, got {actual}"
    );
}

fn half_extents(app: &App, entity: balaur_core::hecs::Entity) -> (f32, f32) {
    let world = app.engine.world();
    let r = world.get::<&Renderable2d>(entity).unwrap();
    match r.shape {
        Shape2d::Sprite { hx, hy } => (hx, hy),
        _ => panic!("not a sprite"),
    }
}

/// The whole point of reading the image header: art keeps its proportions
/// without anyone typing them in, and a 200x100 image is never square.
#[test]
fn a_sprite_is_sized_from_its_image() {
    let app = app();
    let entity = node(&app);
    apply(&app, entity, "");
    let (hx, hy) = half_extents(&app, entity);
    assert_close(hx, 200.0 / DEFAULT_PIXELS_PER_UNIT / 2.0);
    assert_close(hy, 100.0 / DEFAULT_PIXELS_PER_UNIT / 2.0);
    assert!(hx > hy, "a 200x100 image must not come out square");
}

/// A sheet is sized to one cell, not to the whole texture — otherwise every
/// frame of a walk cycle would draw at eight times its intended size.
#[test]
fn a_sheet_is_sized_to_one_frame() {
    let app = app();
    let entity = node(&app);
    apply(&app, entity, "columns = 4\nrows = 2\n");
    let (hx, hy) = half_extents(&app, entity);
    assert_close(hx, (200.0 / 4.0) / DEFAULT_PIXELS_PER_UNIT / 2.0);
    assert_close(hy, (100.0 / 2.0) / DEFAULT_PIXELS_PER_UNIT / 2.0);
}

#[test]
fn explicit_half_extents_win_over_the_image() {
    let app = app();
    let entity = node(&app);
    apply(&app, entity, "half_extents = [3.0, 7.0]\n");
    let (hx, hy) = half_extents(&app, entity);
    assert_close(hx, 3.0);
    assert_close(hy, 7.0);
}

#[test]
fn pixels_per_unit_scales_the_result() {
    let app = app();
    let entity = node(&app);
    apply(&app, entity, "pixels_per_unit = 50.0\n");
    let (hx, _) = half_extents(&app, entity);
    assert_close(hx, 200.0 / 50.0 / 2.0);
}

/// Flipping frames is what an animation does every few ticks. If that bumped
/// the version the backend would tear down and rebuild the node each time,
/// re-fetching the texture, so the cheap path has to stay cheap.
#[test]
fn changing_only_the_frame_does_not_force_a_rebuild() {
    let app = app();
    let entity = node(&app);
    apply(&app, entity, "columns = 4\nrows = 2\n");
    let before = app
        .engine
        .world()
        .get::<&Renderable2d>(entity)
        .unwrap()
        .version;
    apply(&app, entity, "columns = 4\nrows = 2\nframe = 5\n");
    let world = app.engine.world();
    let r = world.get::<&Renderable2d>(entity).unwrap();
    assert_eq!(r.version, before, "a frame change must not rebuild");
    assert_eq!(r.sprite.as_ref().unwrap().frame, 5);
}

/// The opposite case: a different image cannot keep drawing the old one.
#[test]
fn changing_the_texture_forces_a_rebuild() {
    let app = app();
    let entity = node(&app);
    apply(&app, entity, "");
    let before = app
        .engine
        .world()
        .get::<&Renderable2d>(entity)
        .unwrap()
        .version;
    let table = sprite_table("half_extents = [9.0, 9.0]\n");
    components::add(&app.engine, entity, "sprite", Some(&table)).unwrap();
    assert!(
        app.engine
            .world()
            .get::<&Renderable2d>(entity)
            .unwrap()
            .version
            > before
    );
}

/// What the editor saves has to reload as the same sprite.
#[test]
fn the_component_round_trips() {
    let app = app();
    let entity = node(&app);
    apply(&app, entity, "columns = 4\nrows = 2\nframe = 3\n");
    let saved = components::get(&app.engine, entity, "sprite").unwrap();
    let table = saved.as_table().unwrap();
    assert_eq!(table["texture"].as_str().unwrap(), FIXTURE);
    assert_close(table["columns"].as_float().unwrap() as f32, 4.0);
    assert_close(table["rows"].as_float().unwrap() as f32, 2.0);
    assert_close(table["frame"].as_float().unwrap() as f32, 3.0);

    let reloaded = node(&app);
    components::add(&app.engine, reloaded, "sprite", Some(&saved)).unwrap();
    let (rhx, rhy) = half_extents(&app, reloaded);
    let (ohx, ohy) = half_extents(&app, entity);
    assert_close(rhx, ohx);
    assert_close(rhy, ohy);
}

/// A missing file must say so rather than silently drawing nothing at a size
/// nobody chose.
#[test]
fn a_missing_texture_is_an_error() {
    let app = app();
    let entity = node(&app);
    let table: toml::Value = toml::from_str("texture = \"nope/absent.png\"").unwrap();
    let err = components::add(&app.engine, entity, "sprite", Some(&table)).unwrap_err();
    // The component layer wraps it, so the filename lives in the cause chain
    // -- which is the form the log prints.
    assert!(
        format!("{err:#}").contains("absent.png"),
        "the error should name the file: {err:#}"
    );
}

/// `shape2d` describes primitives; a sprite belongs to the `sprite` component,
/// and saving it under both would write it into a scene twice.
#[test]
fn shape2d_does_not_claim_a_sprite() {
    let app = app();
    let entity = node(&app);
    apply(&app, entity, "");
    assert!(components::get(&app.engine, entity, "shape2d").is_none());
    assert!(components::get(&app.engine, entity, "sprite").is_some());
}
