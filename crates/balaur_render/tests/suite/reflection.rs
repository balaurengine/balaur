//! What a surface reflects and refracts, resolved without a window.
//!
//! The capture and the refraction need a GPU; what they are driven by — the
//! boxes a scene places and the `[surface]` a material declares — is read
//! here and asserted on headless.

use balaur_core::glamx::Vec3;
use balaur_core::{App, AppConfig, Transform, components, scene};
use balaur_render::RenderPlugin;
use balaur_render::material::{AlphaMode, Surface};
use balaur_render::reflection::probes;

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut RenderPlugin::default()).unwrap();
    app
}

fn node(app: &App) -> balaur_core::hecs::Entity {
    let root = app.engine.root();
    scene::spawn_node(&mut app.engine.world_mut(), "N", root)
}

fn add(app: &App, entity: balaur_core::hecs::Entity, key: &str, params: &str) {
    let table: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, key, Some(&table)).unwrap();
}

fn place(app: &App, entity: balaur_core::hecs::Entity, position: Vec3) {
    let world = app.engine.world_mut();
    world.get::<&mut Transform>(entity).unwrap().position = position;
}

/// Advance one tick so transform propagation has run: a probe is placed by
/// its node's global pose, and nothing has one until then.
fn settle(app: &mut App) {
    app.advance(0.016);
}

fn surface(text: &str) -> Surface {
    let value: toml::Value = toml::from_str(text).unwrap();
    balaur_render::material::parse(&value).unwrap().surface
}

#[test]
fn a_probe_resolves_to_the_box_its_node_sits_in() {
    let mut app = app();
    let entity = node(&app);
    add(
        &app,
        entity,
        "reflection_probe",
        "half_extents = [6.0, 3.0, 4.0]\nfalloff = 0.75\nintensity = 0.8\nrotation = 90.0",
    );
    place(&app, entity, Vec3::new(2.0, 1.0, -3.0));
    settle(&mut app);

    let world = app.engine.world();
    let resolved = probes(&world, app.engine.root());
    assert_eq!(resolved.len(), 1);
    let probe = &resolved[0];
    assert_eq!(probe.center, Vec3::new(2.0, 1.0, -3.0));
    assert_eq!(probe.half_extents, Vec3::new(6.0, 3.0, 4.0));
    assert!((probe.falloff - 0.75).abs() < 1e-6);
    assert!((probe.intensity - 0.8).abs() < 1e-6);
    // Authored in degrees, resolved in radians: a backend wants the latter.
    assert!(
        (probe.rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-6,
        "{}",
        probe.rotation
    );
    assert!(probe.enabled);
}

#[test]
fn a_hidden_probe_comes_back_switched_off_rather_than_missing() {
    let mut app = app();
    let entity = node(&app);
    add(&app, entity, "reflection_probe", "");
    settle(&mut app);
    {
        let world = app.engine.world_mut();
        world
            .get::<&mut balaur_core::Appearance>(entity)
            .unwrap()
            .visible = false;
    }
    settle(&mut app);

    let world = app.engine.world();
    let resolved = probes(&world, app.engine.root());
    assert_eq!(resolved.len(), 1, "a probe switched off is still a probe");
    assert!(!resolved[0].enabled);
}

/// A box with no thickness would divide the parallax ray by nothing, and a
/// soft edge of no width would divide the fade by nothing.
#[test]
fn a_probe_with_no_size_is_widened_to_something_a_ray_can_meet() {
    let mut app = app();
    let entity = node(&app);
    add(
        &app,
        entity,
        "reflection_probe",
        "half_extents = [0.0, 0.0, 0.0]\nfalloff = 0.0",
    );
    settle(&mut app);

    let world = app.engine.world();
    let probe = probes(&world, app.engine.root()).remove(0);
    assert!(probe.half_extents.min_element() > 0.0, "{probe:?}");
    assert!(probe.falloff > 0.0, "{probe:?}");
}

#[test]
fn a_material_that_says_nothing_about_its_surface_is_plain_and_opaque() {
    let plain = surface(r#"shader = "shaders/x.wesl""#);
    assert_eq!(plain, Surface::default());
    assert_eq!(plain.alpha, AlphaMode::Opaque);
    assert!(!plain.refracts());
    assert!(!plain.mirror);
    assert!(!plain.double_sided);
}

#[test]
fn a_transmissive_surface_refracts_and_carries_its_volume() {
    let glass = surface(
        r##"
shader = "shaders/glass.wesl"
[surface]
alpha = "blend"
transmission = 0.85
ior = 1.52
thickness = 0.4
attenuation_color = "#ccffee"
attenuation_distance = 3.0
"##,
    );
    assert!(glass.refracts());
    assert_eq!(glass.alpha, AlphaMode::Blend);
    assert!((glass.ior - 1.52).abs() < 1e-6);
    assert!((glass.thickness - 0.4).abs() < 1e-6);
    assert!((glass.attenuation_distance - 3.0).abs() < 1e-6);
    assert!((glass.attenuation_color[1] - 1.0).abs() < 1e-6);
}

#[test]
fn a_mirror_surface_carries_its_plane_and_how_much_of_it_shows() {
    let mirror = surface(
        r#"
shader = "shaders/floor.wesl"
[surface]
mirror = true
mirror_intensity = 0.6
mirror_falloff = 2.0
mirror_normal = [0.0, 0.0, 1.0]
"#,
    );
    assert!(mirror.mirror);
    assert!((mirror.mirror_intensity - 0.6).abs() < 1e-6);
    assert!((mirror.mirror_falloff - 2.0).abs() < 1e-6);
    assert_eq!(
        mirror.mirror_normal.map(f32::to_bits),
        [0.0f32, 0.0, 1.0].map(f32::to_bits)
    );
    // A mirror is not glass, so it keeps drawing in the opaque pass.
    assert!(!mirror.refracts());
}

/// An index of refraction below one would bend light the wrong way, and a
/// transmission above one would pass through more than arrived.
#[test]
fn a_surface_outside_what_physics_allows_is_pulled_back_into_it() {
    let odd = surface(
        r#"
shader = "shaders/x.wesl"
[surface]
transmission = 4.0
ior = 0.2
thickness = -1.0
alpha_cutoff = 9.0
"#,
    );
    assert!((odd.transmission - 1.0).abs() < 1e-6);
    assert!((odd.ior - 1.0).abs() < 1e-6);
    assert!((odd.thickness - 0.0).abs() < 1e-6);
    assert!((odd.alpha_cutoff - 1.0).abs() < 1e-6);
}

#[test]
fn a_surface_alpha_the_engine_does_not_know_says_what_it_does() {
    let value: toml::Value =
        toml::from_str("shader = \"shaders/x.wesl\"\n[surface]\nalpha = \"dither\"").unwrap();
    let err = format!("{:#}", balaur_render::material::parse(&value).unwrap_err());
    assert!(
        err.contains("dither") && err.contains("mask") && err.contains("blend"),
        "{err}"
    );
}

/// `[params]` refuses a key the shader does not read, and `[surface]` refuses
/// one the engine does not know: a silently dropped `transmision` is a pane
/// that draws solid and says nothing about why.
#[test]
fn a_surface_key_the_engine_does_not_know_is_refused() {
    let value: toml::Value =
        toml::from_str("shader = \"shaders/x.wesl\"\n[surface]\ntransmision = 0.9").unwrap();
    let err = format!("{:#}", balaur_render::material::parse(&value).unwrap_err());
    assert!(
        err.contains("transmision") && err.contains("transmission"),
        "{err}"
    );
}

/// `render.stats` counts the triangles a frame draws, and `MeshData::indices`
/// is already one entry per triangle.
///
/// Dividing that by three, as the count once did, reported a third of the
/// frame: Sponza's 262 thousand came back as 87 thousand, which is the number
/// a budget would have been set against.
#[test]
fn the_frame_counts_every_triangle_it_draws() {
    let mut app = app();
    let entity = node(&app);
    // A cuboid is twelve triangles however it is built.
    add(
        &app,
        entity,
        "shape3d",
        "kind = \"cuboid\"\nhalf_extents = [1.0, 1.0, 1.0]",
    );
    settle(&mut app);

    let counted = {
        let stats = app.engine.resource::<balaur_render::stats::Stats>();
        let stats = stats.borrow();
        stats.total().triangles
    };
    let expected = {
        let world = app.engine.world();
        let renderable = world.get::<&balaur_render::Renderable3d>(entity).unwrap();
        let solid = renderable.shape.solid().expect("a cuboid is a solid");
        let built = solid.build();
        assert_eq!(
            built.triangle_count(),
            built.indices.len(),
            "indices are triangles, not corners"
        );
        u32::try_from(built.triangle_count()).unwrap()
    };
    assert_eq!(counted, expected, "the frame reports what it drew");
    assert!(expected >= 12, "a cuboid is at least twelve triangles");
}
