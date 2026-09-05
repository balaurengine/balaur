//! A `sprite` drawing frames of a `sprite_sheet`, without a window.
//!
//! The frame's rectangle sizes the quad, so it is simulation state and has
//! to come out the same headless as windowed.

use balaur_core::{App, AppConfig, components, scene};
use balaur_render::{RenderPlugin, Renderable2d, Shape2d};

/// 200x100 px; the sheet below cuts it into frames of two sizes.
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

/// A sprite naming an inline sheet: two frames, the second a different size.
fn sprite_table(extra: &str) -> toml::Value {
    toml::from_str(&format!(
        r#"{extra}
pixels_per_unit = 100.0

[sheet]
texture = "{FIXTURE}"
frames = [
  {{ rect = [0, 0, 50, 100], duration = 0.1 }},
  {{ rect = [50, 0, 150, 100], duration = 0.2 }},
]

[sheet.tags.walk]
from = 0
to = 1

[sheet.slices.hitbox]
rect = [10, 10, 20, 30]
"#
    ))
    .unwrap()
}

fn half_extents(app: &App, entity: balaur_core::hecs::Entity) -> (f32, f32) {
    let world = app.engine.world();
    let r = world.get::<&Renderable2d>(entity).unwrap();
    match r.shape {
        Shape2d::Sprite { hx, hy } => (hx, hy),
        _ => panic!("not a sprite"),
    }
}

#[track_caller]
fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn a_sheet_frame_sizes_the_quad_and_picks_its_region() {
    let app = app();
    let entity = node(&app);
    components::add(
        &app.engine,
        entity,
        "sprite",
        Some(&sprite_table("frame = 1.0")),
    )
    .unwrap();
    let (hx, hy) = half_extents(&app, entity);
    assert_close(hx, 0.75);
    assert_close(hy, 0.5);
    let world = app.engine.world();
    let r = world.get::<&Renderable2d>(entity).unwrap();
    let sprite = r.sprite.as_ref().unwrap();
    assert_eq!(sprite.region, Some([50, 0, 150, 100]));
    assert_eq!(sprite.path, FIXTURE, "the sheet's texture is drawn");
    assert!(sprite.sheet.is_none(), "an atlas is not a grid");
}

#[test]
fn a_frame_past_the_end_draws_the_last_one() {
    let app = app();
    let entity = node(&app);
    components::add(
        &app.engine,
        entity,
        "sprite",
        Some(&sprite_table("frame = 9.0")),
    )
    .unwrap();
    let (hx, _) = half_extents(&app, entity);
    assert_close(hx, 0.75);
    let saved = components::get(&app.engine, entity, "sprite").unwrap();
    assert!(
        (saved["frame"].as_float().unwrap() - 9.0).abs() < f64::EPSILON,
        "the frame reads back as written"
    );
}

#[test]
fn a_sprite_reports_its_sheet_and_not_the_region_it_derived() {
    let app = app();
    let entity = node(&app);
    components::add(
        &app.engine,
        entity,
        "sprite",
        Some(&sprite_table("frame = 0.0")),
    )
    .unwrap();
    let saved = components::get(&app.engine, entity, "sprite").unwrap();
    let sheet = saved["sheet"].as_str().unwrap();
    assert!(
        sheet.starts_with("#!"),
        "an inline sheet reads back as its reference, not {sheet}"
    );
    assert_eq!(
        saved["texture"].as_str(),
        Some(""),
        "the texture was the sheet's, not the author's"
    );
    assert!(
        saved.get("region_size").is_none(),
        "a derived region is not authored"
    );
    // Keying the frame through `patch`, as a clip does, moves the region.
    let key: toml::Value = toml::from_str("frame = 1.0").unwrap();
    components::patch(&app.engine, entity, "sprite", &key).unwrap();
    let (hx, _) = half_extents(&app, entity);
    assert_close(hx, 0.75);
}

#[test]
fn a_texture_named_beside_the_sheet_wins_and_reads_back() {
    let app = app();
    let entity = node(&app);
    components::add(
        &app.engine,
        entity,
        "sprite",
        Some(&sprite_table(&format!(
            "frame = 0.0\ntexture = \"{FIXTURE}\""
        ))),
    )
    .unwrap();
    let saved = components::get(&app.engine, entity, "sprite").unwrap();
    assert_eq!(saved["texture"].as_str(), Some(FIXTURE));
}

#[test]
fn a_sheet_that_does_not_parse_names_the_frame() {
    let app = app();
    let entity = node(&app);
    let table: toml::Value = toml::from_str(&format!(
        "frame = 0.0\n[sheet]\ntexture = \"{FIXTURE}\"\nframes = [{{ rect = [0, 0, 0, 0] }}]\n"
    ))
    .unwrap();
    let error = format!(
        "{:#}",
        components::add(&app.engine, entity, "sprite", Some(&table)).unwrap_err()
    );
    assert!(error.contains("frame 0"), "unhelpful: {error}");
}
