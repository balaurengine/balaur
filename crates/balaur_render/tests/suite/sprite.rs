//! Textured 2D sprites, without a window.
//!
//! A sprite's size lands in a component scripts read back, so it is simulation
//! state and every number here has to come out the same headless as windowed.

use balaur_core::{App, AppConfig, components, scene};
use balaur_render::{DEFAULT_PIXELS_PER_UNIT, RenderPlugin, Renderable2d, Shape2d};

/// 200x100 px, so width and height are never confused for each other, and a
/// 4x2 sheet divides both evenly.
const FIXTURE: &str = "tests/fixtures/sprite_200x100.png";

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut RenderPlugin::default()).unwrap();
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

/// Like the frame, a flip only moves UVs, so it must ride along without
/// forcing the backend to rebuild the node.
#[test]
fn flips_round_trip_and_do_not_force_a_rebuild() {
    let app = app();
    let entity = node(&app);
    apply(&app, entity, "");
    let before = app
        .engine
        .world()
        .get::<&Renderable2d>(entity)
        .unwrap()
        .version;
    apply(&app, entity, "flip_x = true\n");
    {
        let world = app.engine.world();
        let r = world.get::<&Renderable2d>(entity).unwrap();
        assert_eq!(r.version, before, "a flip change must not rebuild");
        let sprite = r.sprite.as_ref().unwrap();
        assert!(sprite.flip_x);
        assert!(!sprite.flip_y);
    }
    let saved = components::get(&app.engine, entity, "sprite").unwrap();
    let table = saved.as_table().unwrap();
    assert!(table["flip_x"].as_bool().unwrap());
    assert!(!table["flip_y"].as_bool().unwrap());
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

/// The editor adds the component before anyone picks an image, and every
/// component has to apply with its own defaults on a bare node. So a sprite
/// with no texture is a placeholder, not an error.
#[test]
fn a_sprite_with_no_texture_is_a_placeholder() {
    let app = app();
    let entity = node(&app);
    components::add(&app.engine, entity, "sprite", None).unwrap();
    let (hx, hy) = half_extents(&app, entity);
    assert_close(hx, 0.5);
    assert_close(hy, 0.5);
    let saved = components::get(&app.engine, entity, "sprite").unwrap();
    assert_eq!(saved.as_table().unwrap()["texture"].as_str().unwrap(), "");
}

/// The size a sprite derived from its image is not the author's, so a patch
/// that moves a sheet field has to re-derive it. Reading the component back
/// once used to freeze the quad for the life of the node.
#[test]
fn patching_the_sheet_fields_resizes_a_derived_sprite() {
    let app = app();
    let entity = node(&app);
    apply(&app, entity, "");
    // The read is what used to write a resolved `half_extents` into the
    // component, which `patch` then overlaid as an explicit size.
    let read_back = components::get(&app.engine, entity, "sprite").unwrap();
    assert!(
        read_back.get("half_extents").is_none(),
        "a derived size must not be reported as the author's: {read_back:?}"
    );
    let (before, _) = half_extents(&app, entity);
    assert_close(before, 200.0 / DEFAULT_PIXELS_PER_UNIT / 2.0);

    let patch: toml::Value = toml::from_str("pixels_per_unit = 50.0").unwrap();
    components::patch(&app.engine, entity, "sprite", &patch).unwrap();
    let (after, _) = half_extents(&app, entity);
    assert_close(after, 200.0 / 50.0 / 2.0);
}

/// The other half of the same rule: a size the author stated stays stated,
/// whatever else is patched over it.
#[test]
fn patching_a_sheet_field_leaves_an_authored_size_alone() {
    let app = app();
    let entity = node(&app);
    apply(&app, entity, "half_extents = [3.0, 7.0]\n");
    let read_back = components::get(&app.engine, entity, "sprite").unwrap();
    assert!(
        read_back.get("half_extents").is_some(),
        "an authored size has to round-trip: {read_back:?}"
    );
    let patch: toml::Value = toml::from_str("pixels_per_unit = 50.0").unwrap();
    components::patch(&app.engine, entity, "sprite", &patch).unwrap();
    let (hx, hy) = half_extents(&app, entity);
    assert_close(hx, 3.0);
    assert_close(hy, 7.0);
}

/// An atlas cell states its own size: the quad is the cell, not the sheet.
#[test]
fn a_region_sizes_the_quad_from_the_cell_not_the_image() {
    let app = app();
    let entity = node(&app);
    apply(
        &app,
        entity,
        "region_origin = [10.0, 20.0]\nregion_size = [50.0, 25.0]\npixels_per_unit = 100.0",
    );
    let (hx, hy) = half_extents(&app, entity);
    assert_close(hx, 0.25);
    assert_close(hy, 0.125);
    let saved = components::get(&app.engine, entity, "sprite").unwrap();
    let origin = saved["region_origin"].as_array().unwrap();
    assert_close(origin[0].as_float().unwrap() as f32, 10.0);
    let size = saved["region_size"].as_array().unwrap();
    assert_close(size[1].as_float().unwrap() as f32, 25.0);
}
