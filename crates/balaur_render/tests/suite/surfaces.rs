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
    scene::spawn_node(&mut app.engine.world_mut(), "N", root)
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
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"N\"\nscript = { source = \"scripts/s.rn\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.rn"),
        "pub fn init(this) {\n    render::snap_aov(render::AOV_CAMERA_NORMALS, \"shots/n.png\");\n}\n",
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

/// The wireframe, vertex and node keys reach the renderable of every 3D
/// component that draws one, and read back as written.
#[test]
fn every_3d_renderable_takes_the_overlay_keys_and_reads_them_back() {
    let (_dir, app) = app();
    let keys = "wireframe_width = 2.0\nwireframe_sizing = \"screen\"\nwireframe_color = [0.0, 1.0, 0.0, 1.0]\ndot_size = 3.0\ndraw_surface = false\nsegmentation_id = 9\nreceive_shadows = false";
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
    add(
        &app,
        words,
        "text3d",
        &format!("text = \"hi\"\ndepth_test = false\n{keys}"),
    );
    {
        let world = app.engine.world();
        let overlay = world.get::<&Renderable3d>(solid).unwrap().overlay;
        assert!(!overlay.draw_surface);
        assert!(!overlay.receive_shadows);
        assert_eq!(overlay.segmentation_id, 9);
        assert_eq!(
            overlay.wireframe_sizing,
            balaur_render::overlay::Sizing::Screen
        );
        let text = world
            .get::<&balaur_render::world_text::TextRenderable>(words)
            .unwrap();
        // Only the depth test differs: the block turned it off.
        assert_eq!(
            text.overlay_3d,
            balaur_render::overlay::Overlay3d {
                depth_test: false,
                ..overlay
            }
        );
        assert!(overlay.depth_test);
        assert!(!text.in_space.depth_test);
    }
    for (entity, key) in [(solid, "shape3d"), (cut, "boolean3d"), (words, "text3d")] {
        let read = read(&app, entity, key);
        assert!(
            (read["wireframe_width"].as_float().unwrap() - 2.0).abs() < 1e-6,
            "{key}"
        );
        assert_eq!(read["receive_shadows"].as_bool(), Some(false), "{key}");
        assert_eq!(read["segmentation_id"].as_integer(), Some(9), "{key}");
    }
}

/// The blend, wireframe, vertex and culling keys reach every 2D drawable,
/// and a blend that is none of the words is refused.
#[test]
fn every_2d_drawable_takes_the_overlay_keys_and_refuses_an_unknown_blend() {
    let (_dir, app) = app();
    let keys = "blend_mode = \"add\"\nwireframe_width = 1.5\ndot_color = [1.0, 0.0, 0.0, 1.0]\ncull_back_faces = true";
    let flat = node(&app);
    add(&app, flat, "shape2d", &format!("kind = \"circle\"\n{keys}"));
    let quad = node(&app);
    add(&app, quad, "sprite", keys);
    let sparks = node(&app);
    add(&app, sparks, "particles2d", keys);
    {
        let world = app.engine.world();
        let overlay = world.get::<&Renderable2d>(flat).unwrap().overlay;
        assert_eq!(overlay.blend, balaur_render::overlay::BlendMode::Add);
        assert!(overlay.cull_back_faces);
        assert_eq!(world.get::<&Renderable2d>(quad).unwrap().overlay, overlay);
        assert_eq!(
            world
                .get::<&balaur_render::Particles>(sparks)
                .unwrap()
                .overlay,
            overlay
        );
    }
    for (entity, key) in [(flat, "shape2d"), (quad, "sprite"), (sparks, "particles2d")] {
        let read = read(&app, entity, key);
        assert_eq!(read["blend_mode"].as_str(), Some("add"), "{key}");
        assert_eq!(read["cull_back_faces"].as_bool(), Some(true), "{key}");
    }
    let refused = node(&app);
    let table: toml::Value = toml::from_str("kind = \"circle\"\nblend_mode = \"lighten\"").unwrap();
    assert!(components::add(&app.engine, refused, "shape2d", Some(&table)).is_err());
}
