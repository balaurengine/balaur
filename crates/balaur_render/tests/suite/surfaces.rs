//! What a drawn node is dressed with, headless: textures on every shape,
//! colours and materials on the nodes that lacked them, the defaults a
//! sprite and an emitter draw at, and the emitter's new keys.

use balaur::{AppConfig, standard_app};
use balaur_core::hecs::Entity;
use balaur_core::{App, components, scene};
use balaur_render::{RenderPlugin, Renderable2d, Renderable3d};

fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(balaur_core::AppConfig::bare(dir.path().to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut RenderPlugin::default()).unwrap();
    (dir, app)
}

fn node(app: &App) -> Entity {
    let root = app.engine.root();
    scene::spawn_node(&mut app.engine.world_mut(), "Scenario", root)
}

fn add(app: &App, entity: Entity, key: &str, params: &str) {
    let params: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, key, Some(&params)).unwrap();
}

fn read(app: &App, entity: Entity, key: &str) -> toml::Value {
    components::get(&app.engine, entity, key).unwrap_or_else(|| panic!("{key} reads back"))
}

#[test]
fn every_shape_takes_a_texture_and_reads_it_back() {
    let (_dir, app) = app();
    let solid = node(&app);
    add(
        &app,
        solid,
        "shape3d",
        "kind = \"sphere\"\ntexture = \"rock.png\"",
    );
    let flat = node(&app);
    add(
        &app,
        flat,
        "shape2d",
        "kind = \"circle\"\ntexture = \"rock.png\"",
    );
    let line = node(&app);
    add(
        &app,
        line,
        "shape2d",
        "kind = \"polyline\"\ntexture = \"rope.png\"",
    );
    assert_eq!(
        app.engine
            .world()
            .get::<&Renderable3d>(solid)
            .unwrap()
            .texture,
        "rock.png"
    );
    assert_eq!(
        app.engine
            .world()
            .get::<&Renderable2d>(flat)
            .unwrap()
            .texture,
        "rock.png"
    );
    assert_eq!(
        read(&app, solid, "shape3d")["texture"].as_str(),
        Some("rock.png")
    );
    assert_eq!(
        read(&app, flat, "shape2d")["texture"].as_str(),
        Some("rock.png")
    );
    assert_eq!(
        read(&app, line, "shape2d")["texture"].as_str(),
        Some("rope.png")
    );
}

#[test]
fn a_new_texture_rebuilds_the_shape_and_a_new_tint_does_not() {
    let (_dir, app) = app();
    let flat = node(&app);
    add(&app, flat, "shape2d", "kind = \"circle\"");
    let version = || {
        app.engine
            .world()
            .get::<&Renderable2d>(flat)
            .unwrap()
            .version
    };
    let before = version();
    let tint: toml::Value = toml::from_str("color = [1.0, 0.0, 0.0, 1.0]").unwrap();
    components::patch(&app.engine, flat, "shape2d", &tint).unwrap();
    assert_eq!(version(), before, "a tint is applied in place");
    let image: toml::Value = toml::from_str("texture = \"rock.png\"").unwrap();
    components::patch(&app.engine, flat, "shape2d", &image).unwrap();
    assert!(
        version() > before,
        "an image has to be uploaded onto a new node"
    );
}

#[test]
fn a_multimesh3d_tints_its_instances_and_a_multimesh2d_takes_a_material() {
    let (_dir, mut app) = app();
    let deep = node(&app);
    add(
        &app,
        deep,
        "multimesh3d",
        "source = { type = \"multimesh\", mesh = { type = \"mesh\", kind = \"box\" }, instances = [{}] }\ncolor = [0.5, 0.5, 1.0, 1.0]",
    );
    let flat = node(&app);
    add(
        &app,
        flat,
        "multimesh2d",
        "source = { type = \"multimesh\", mesh = { type = \"mesh\", positions = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], indices = [[0, 1, 2]] }, instances = [{}] }\nmaterial = \"materials/wave.toml\"\npixels_per_unit = 32.0",
    );
    app.tick(1.0 / 60.0);
    assert!(crate::same(
        app.engine.world().get::<&Renderable3d>(deep).unwrap().color,
        [0.5, 0.5, 1.0, 1.0]
    ));
    assert_eq!(
        read(&app, deep, "multimesh3d")["color"][2].as_float(),
        Some(1.0)
    );
    let got = read(&app, flat, "multimesh2d");
    assert_eq!(got["material"].as_str(), Some("materials/wave.toml"));
    assert_eq!(got["pixels_per_unit"].as_float(), Some(32.0));
    let world = app.engine.world();
    let renderable = world.get::<&Renderable2d>(flat).unwrap();
    assert_eq!(renderable.material, "materials/wave.toml");
    assert!((renderable.polygon.as_ref().unwrap().pixels_per_unit - 32.0).abs() < 1e-6);
}

#[test]
fn a_polygon_and_both_texts_take_a_material() {
    let (_dir, app) = app();
    let polygon = node(&app);
    add(
        &app,
        polygon,
        "polygon",
        "material = \"materials/skin.toml\"",
    );
    assert_eq!(
        app.engine
            .world()
            .get::<&Renderable2d>(polygon)
            .unwrap()
            .material,
        "materials/skin.toml"
    );
    assert_eq!(
        read(&app, polygon, "polygon")["material"].as_str(),
        Some("materials/skin.toml")
    );
    let flat = node(&app);
    add(
        &app,
        flat,
        "text2d",
        "text = \"hi\"\nmaterial = \"materials/glow.toml\"",
    );
    assert_eq!(
        read(&app, flat, "text2d")["material"].as_str(),
        Some("materials/glow.toml")
    );
    let deep = node(&app);
    add(
        &app,
        deep,
        "text3d",
        "text = \"hi\"\nmaterial = \"materials/glow.toml\"",
    );
    assert_eq!(
        read(&app, deep, "text3d")["material"].as_str(),
        Some("materials/glow.toml")
    );
}

#[test]
fn a_tilemap_takes_a_tint() {
    let (_dir, app) = app();
    let map = node(&app);
    add(&app, map, "tilemap", "color = [1.0, 0.5, 0.25, 1.0]");
    assert_eq!(read(&app, map, "tilemap")["color"][1].as_float(), Some(0.5));
    let world = app.engine.world();
    let tilemap = world.get::<&balaur_render::Tilemap>(map).unwrap();
    assert!(crate::same(tilemap.color, [1.0, 0.5, 0.25, 1.0]));
}

/// White, as a mesh is: a tint below it would darken every texture.
#[test]
fn a_sprite_and_an_emitter_draw_white_unless_told_otherwise() {
    let (_dir, app) = app();
    let sprite = node(&app);
    add(&app, sprite, "sprite", "");
    assert!(crate::same(
        app.engine
            .world()
            .get::<&Renderable2d>(sprite)
            .unwrap()
            .color,
        [1.0; 4]
    ));
    let emitter = node(&app);
    add(&app, emitter, "particles2d", "");
    let world = app.engine.world();
    let particles = world.get::<&balaur_render::Particles>(emitter).unwrap();
    assert!(crate::same(particles.color, [1.0; 4]));
    assert!(crate::same(particles.color_end, [1.0, 1.0, 1.0, 0.0]));
}

#[test]
fn an_emitter_reads_back_its_turn_spin_sheet_and_material() {
    let (_dir, app) = app();
    let emitter = node(&app);
    add(
        &app,
        emitter,
        "particles2d",
        "rotation_degrees = 45.0\nangular_speed_degrees = -90.0\nsheet = \"sheets/smoke.toml\"\nmaterial = \"materials/soft.toml\"",
    );
    let got = read(&app, emitter, "particles2d");
    assert_eq!(got["rotation_degrees"].as_float(), Some(45.0));
    assert_eq!(got["angular_speed_degrees"].as_float(), Some(-90.0));
    assert_eq!(got["sheet"].as_str(), Some("sheets/smoke.toml"));
    assert_eq!(got["material"].as_str(), Some("materials/soft.toml"));
}

/// The request `render.snap_aov` leaves for the renderer, under a backend
/// that claims screenshots so nothing answers it first.
#[test]
fn snap_aov_asks_the_renderer_for_an_auxiliary_output() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"scenario\"\nname = \"Scenario\"\nscript = { source = \"scripts/scenario.bs\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/scenario.bs"),
        "pub class Scenario { node: Node }\n\
         impl Scenario {\n    fn init() { render::snap_aov(render::AOV_CAMERA_NORMALS, \"shots/n.png\"); }\n}\n",
    )
    .unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.engine.insert_resource(balaur_render::WindowedBackend);
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    let request = app
        .engine
        .try_resource::<balaur_render::ScreenshotRequest>()
        .expect("the script asked for a shot");
    let request = request.borrow();
    assert_eq!(request.aov, Some(balaur_render::Aov::CameraNormals));
    assert!(request.path.ends_with("shots/n.png"));
}

/// A 3D renderable's own flags reach its renderable, a `debug_draw` beside
/// it is the node's own component, and both read back as written.
#[test]
fn every_3d_renderable_takes_its_flags_and_a_debug_drawing_beside_it() {
    let (_dir, app) = app();
    let keys = "segmentation_id = 9\nreceive_shadows = false";
    let drawing = "wireframe_width = 2.0\nwireframe_sizing = \"screen\"\nwireframe_color = [0.0, 1.0, 0.0, 1.0]\ndot_size = 3.0\ndraw_surface = false";
    let solid = node(&app);
    add(&app, solid, "shape3d", &format!("kind = \"box\"\n{keys}"));
    let cut = node(&app);
    add(
        &app,
        cut,
        "boolean3d",
        &format!("operation = \"union\"\n{keys}"),
    );
    let words = node(&app);
    add(&app, words, "text3d", &format!("text = \"hi\"\n{keys}"));
    for entity in [solid, cut, words] {
        add(&app, entity, "debug_draw", drawing);
    }
    {
        let world = app.engine.world();
        let overlay = world.get::<&Renderable3d>(solid).unwrap().overlay;
        assert!(!overlay.receive_shadows);
        assert_eq!(overlay.segmentation_id, 9);
        let debug = *world
            .get::<&balaur_render::debug_draw::DebugDraw>(solid)
            .unwrap();
        assert!(!debug.draw_surface);
        assert_eq!(
            debug.wireframe_sizing,
            balaur_render::debug_draw::Sizing::Screen
        );
        let text = world
            .get::<&balaur_render::world_text::TextRenderable>(words)
            .unwrap();
        assert_eq!(text.overlay_3d, overlay);
    }
    for (entity, key) in [(solid, "shape3d"), (cut, "boolean3d"), (words, "text3d")] {
        let read_back = read(&app, entity, key);
        assert_eq!(read_back["receive_shadows"].as_bool(), Some(false), "{key}");
        assert_eq!(read_back["segmentation_id"].as_integer(), Some(9), "{key}");
        let drawn = read(&app, entity, "debug_draw");
        assert!(
            (drawn["wireframe_width"].as_float().unwrap() - 2.0).abs() < 1e-6,
            "{key}"
        );
    }
}

/// How a 2D drawable blends and culls is its material's `[surface]`: an
/// inline one reaches the surface the node draws with, a node naming none
/// leaves the blend to its texture and draws both sides, and a blend that is
/// none of the words is refused.
#[test]
fn a_2d_drawable_blends_and_culls_as_its_material_says() {
    let (_dir, app) = app();
    let material = "material = { surface = { alpha = \"add\", double_sided = false } }";
    let flat = node(&app);
    add(
        &app,
        flat,
        "shape2d",
        &format!("kind = \"circle\"\n{material}"),
    );
    let quad = node(&app);
    add(&app, quad, "sprite", material);
    let named = |entity, key: &str| {
        read(&app, entity, key)["material"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    for (entity, key) in [(flat, "shape2d"), (quad, "sprite")] {
        let surface = balaur_render::material::flat_surface_of(&app.engine, &named(entity, key));
        assert_eq!(
            surface.alpha,
            balaur_render::material::AlphaMode::Add,
            "{key}"
        );
        assert!(!surface.double_sided_or(true), "{key} draws its back faces");
    }
    let plain = balaur_render::material::flat_surface_of(&app.engine, "");
    assert_eq!(plain.alpha, balaur_render::material::AlphaMode::Auto);
    assert!(plain.double_sided_or(true), "a 2D node naming none culls");
    let refused = node(&app);
    add(
        &app,
        refused,
        "shape2d",
        "kind = \"circle\"\nmaterial = { surface = { alpha = \"lighten\" } }",
    );
    let why = balaur_core::assets::load(&app.engine, &named(refused, "shape2d")).unwrap_err();
    assert!(format!("{why:#}").contains("lighten"), "{why:#}");
}
