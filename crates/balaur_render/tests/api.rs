//! The render crate without a window: components, camera state and the debug
//! line buffers. Nothing here touches a GPU, so it runs anywhere CI does.

use balaur_core::scene;
use balaur_core::{components, App, AppConfig};
use balaur_render::{
    CameraConfig, CameraConfig2d, CameraInputConfig, ClearColorConfig, DebugLineBuffer,
    DebugLineBuffer2d, GridConfig, RenderPlugin, Renderable, Renderable2d, Shape, ViewportSnapshot,
};

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut RenderPlugin::default()).unwrap();
    app
}

fn node(app: &App) -> balaur_core::hecs::Entity {
    let root = app.engine.root();
    scene::spawn_node(&mut app.engine.world_mut(), "N", root)
}

#[test]
fn the_plugin_registers_its_components() {
    let app = app();
    let names = components::names(&app.engine);
    for expected in [
        "shape3d",
        "shape2d",
        "shape2d",
        "camera",
        "tilemap",
        "particles",
    ] {
        assert!(
            names.contains(&expected.to_string()),
            "`{expected}` is not registered"
        );
    }
}

#[test]
fn the_plugin_inserts_the_resources_a_frame_reads() {
    let app = app();
    assert!(app.engine.try_resource::<ClearColorConfig>().is_some());
    assert!(app.engine.try_resource::<GridConfig>().is_some());
    assert!(app.engine.try_resource::<CameraConfig>().is_some());
    assert!(app.engine.try_resource::<CameraConfig2d>().is_some());
    assert!(app.engine.try_resource::<CameraInputConfig>().is_some());
    assert!(app.engine.try_resource::<DebugLineBuffer>().is_some());
    assert!(app.engine.try_resource::<DebugLineBuffer2d>().is_some());
    assert!(app.engine.try_resource::<ViewportSnapshot>().is_some());
}

#[test]
fn a_shape_component_puts_a_renderable_on_the_node() {
    let app = app();
    let e = node(&app);
    let params: toml::Value = toml::from_str("kind = \"ball\"\nradius = 2.0").unwrap();
    components::add(&app.engine, e, "shape3d", Some(&params)).unwrap();

    let world = app.engine.world();
    let r = world
        .get::<&Renderable>(e)
        .expect("no Renderable was added");
    assert!(
        matches!(r.shape, Shape::Ball { .. }),
        "a ball shape produced something else"
    );
}

#[test]
fn a_2d_shape_component_puts_a_2d_renderable_on_the_node() {
    let app = app();
    let e = node(&app);
    let params: toml::Value = toml::from_str("kind = \"circle\"\nradius = 1.0").unwrap();
    components::add(&app.engine, e, "shape2d", Some(&params)).unwrap();
    assert!(app.engine.world().get::<&Renderable2d>(e).is_ok());
}

#[test]
fn every_shape_kind_the_schema_offers_is_accepted() {
    let app = app();
    for (component, kinds) in [
        ("shape3d", ["ball", "cuboid"]),
        ("shape2d", ["circle", "rect"]),
    ] {
        for kind in kinds {
            let e = node(&app);
            let params: toml::Value = toml::from_str(&format!("kind = \"{kind}\"")).unwrap();
            components::add(&app.engine, e, component, Some(&params))
                .unwrap_or_else(|err| panic!("{component} kind `{kind}` rejected: {err:#}"));
        }
    }
}

#[test]
fn a_colour_reads_back_as_it_was_set() {
    let app = app();
    let e = node(&app);
    // Colour is a property of the renderable, not a component of its own.
    let params: toml::Value =
        toml::from_str("kind = \"ball\"\ncolor = [0.25, 0.5, 0.75, 1.0]").unwrap();
    components::add(&app.engine, e, "shape3d", Some(&params)).unwrap();

    let got = components::get(&app.engine, e, "shape3d").expect("colour reads back");
    let rgba = got
        .get("color")
        .and_then(toml::Value::as_array)
        .expect("color array");
    assert!((rgba[0].as_float().unwrap() - 0.25).abs() < 1e-6);
}

#[test]
fn removing_a_shape_takes_the_renderable_with_it() {
    let app = app();
    let e = node(&app);
    let params: toml::Value = toml::from_str("kind = \"ball\"").unwrap();
    components::add(&app.engine, e, "shape3d", Some(&params)).unwrap();
    components::remove(&app.engine, e, "shape3d").unwrap();
    assert!(app.engine.world().get::<&Renderable>(e).is_err());
}

#[test]
fn debug_lines_accumulate_and_can_be_cleared() {
    let app = app();
    let lines = app.engine.resource::<DebugLineBuffer>();
    assert!(lines.borrow().lines.is_empty(), "the buffer starts empty");
    lines.borrow_mut().lines.push(Default::default());
    assert_eq!(lines.borrow().lines.len(), 1);
    lines.borrow_mut().lines.clear();
    assert!(lines.borrow().lines.is_empty());

    let lines_2d = app.engine.resource::<DebugLineBuffer2d>();
    assert!(lines_2d.borrow().lines.is_empty());
}

/// With no windowed backend nothing drains the buffers as it draws, so the
/// plugin's own Render-stage system has to, or every `render.draw_line` in a
/// headless run leaks a `Vec` entry per frame.
#[test]
fn a_headless_frame_empties_the_debug_line_buffers() {
    let mut app = app();
    for _ in 0..3 {
        app.engine
            .resource::<DebugLineBuffer>()
            .borrow_mut()
            .lines
            .push(Default::default());
        app.engine
            .resource::<DebugLineBuffer2d>()
            .borrow_mut()
            .lines
            .push(Default::default());
        app.tick(1.0 / 60.0);
    }
    assert!(
        app.engine
            .resource::<DebugLineBuffer>()
            .borrow()
            .lines
            .is_empty(),
        "3D debug lines survived a headless frame"
    );
    assert!(
        app.engine
            .resource::<DebugLineBuffer2d>()
            .borrow()
            .lines
            .is_empty(),
        "2D debug lines survived a headless frame"
    );
}

/// The windowed backend drains as it draws, so the fallback must stand down
/// while it says it is present, or the frame's lines vanish before the draw.
#[test]
fn a_windowed_backend_keeps_the_fallback_off_its_buffers() {
    let mut app = app();
    app.engine.insert_resource(balaur_render::WindowedBackend);
    app.engine
        .resource::<DebugLineBuffer>()
        .borrow_mut()
        .lines
        .push(Default::default());
    app.tick(1.0 / 60.0);
    assert_eq!(
        app.engine
            .resource::<DebugLineBuffer>()
            .borrow()
            .lines
            .len(),
        1,
        "the fallback drained lines the backend was going to draw"
    );
}

#[test]
fn camera_input_can_be_switched_off() {
    let app = app();
    let config = app.engine.resource::<CameraInputConfig>();
    let before = config.borrow().enabled;
    config.borrow_mut().enabled = !before;
    assert_ne!(
        app.engine.resource::<CameraInputConfig>().borrow().enabled,
        before
    );
}

#[test]
fn ticking_a_headless_app_with_render_does_not_panic() {
    let mut app = app();
    let e = node(&app);
    let params: toml::Value = toml::from_str("kind = \"ball\"").unwrap();
    components::add(&app.engine, e, "shape3d", Some(&params)).unwrap();
    for _ in 0..10 {
        app.tick(1.0 / 60.0);
    }
}

/// Every 3D kind the schema offers has to survive `add`, land the right
/// variant, and come back out of `get` — a shape you can write but not read
/// is a shape the editor loses.
#[test]
fn every_3d_shape_kind_round_trips() {
    for (source, expected) in [
        ("kind = \"ball\"\nradius = 2.0", "ball"),
        ("kind = \"cuboid\"", "cuboid"),
        ("kind = \"capsule\"\nradius = 0.5\nheight = 2.0", "capsule"),
        (
            "kind = \"cylinder\"\nradius = 0.5\nheight = 2.0",
            "cylinder",
        ),
        ("kind = \"cone\"\nradius = 0.5\nheight = 2.0", "cone"),
        ("kind = \"plane\"", "plane"),
    ] {
        let app = app();
        let e = node(&app);
        let params: toml::Value = toml::from_str(source).unwrap();
        components::add(&app.engine, e, "shape3d", Some(&params))
            .unwrap_or_else(|why| panic!("{expected} did not apply: {why:#}"));
        let back = components::get(&app.engine, e, "shape3d")
            .unwrap_or_else(|| panic!("{expected} produced nothing to read back"));
        assert_eq!(
            back.get("kind").and_then(toml::Value::as_str),
            Some(expected),
            "{expected} read back as something else"
        );
    }
}

/// The straight part and the caps are separate: a capsule is `height` long
/// plus a radius at each end, and the schema must not quietly halve it.
#[test]
fn a_capsule_keeps_the_height_it_was_given() {
    let app = app();
    let e = node(&app);
    let params: toml::Value =
        toml::from_str("kind = \"capsule\"\nradius = 0.25\nheight = 3.0").unwrap();
    components::add(&app.engine, e, "shape3d", Some(&params)).unwrap();
    let back = components::get(&app.engine, e, "shape3d").unwrap();
    let height = back.get("height").and_then(balaur_core::components::as_f64);
    assert!(
        height.is_some_and(|h| (h - 3.0).abs() < 1e-6),
        "expected the height back unchanged, got {height:?}"
    );
}

#[test]
fn every_2d_shape_kind_round_trips() {
    for (source, expected) in [
        ("kind = \"circle\"\nradius = 1.0", "circle"),
        ("kind = \"rect\"", "rect"),
        ("kind = \"capsule\"\nradius = 0.5\nheight = 2.0", "capsule"),
    ] {
        let app = app();
        let e = node(&app);
        let params: toml::Value = toml::from_str(source).unwrap();
        components::add(&app.engine, e, "shape2d", Some(&params))
            .unwrap_or_else(|why| panic!("{expected} did not apply: {why:#}"));
        let back = components::get(&app.engine, e, "shape2d")
            .unwrap_or_else(|| panic!("{expected} produced nothing to read back"));
        assert_eq!(
            back.get("kind").and_then(toml::Value::as_str),
            Some(expected)
        );
    }
}

/// A polyline is a `Shape2d` like any other, with its points in the sibling
/// field — the arrangement `sprite` uses, and the reason the enum stays `Copy`.
#[test]
fn a_polyline_is_a_shape2d_that_keeps_its_reference() {
    let app = app();
    let e = node(&app);
    let params: toml::Value =
        toml::from_str("kind = \"polyline\"\nmesh = \"outline\"\nwidth = 0.5\nclosed = true")
            .unwrap();
    components::add(&app.engine, e, "shape2d", Some(&params)).unwrap();

    {
        let world = app.engine.world();
        let r = world.get::<&Renderable2d>(e).expect("no Renderable2d");
        assert!(matches!(
            r.shape,
            balaur_render::Shape2d::Polyline { closed: true, .. }
        ));
        assert_eq!(r.polyline.as_deref(), Some("outline"));
    }

    let back = components::get(&app.engine, e, "shape2d").expect("nothing read back");
    assert_eq!(
        back.get("mesh").and_then(toml::Value::as_str),
        Some("outline"),
        "the reference has to survive the round trip, or a save loses the points"
    );
}

/// An unknown kind names itself rather than silently drawing a cuboid.
#[test]
fn an_unknown_shape_kind_is_refused_by_name() {
    let app = app();
    let e = node(&app);
    let params: toml::Value = toml::from_str("kind = \"dodecahedron\"").unwrap();
    // The component layer wraps the cause, so the name is in the chain --
    // which is the form the log prints.
    let err = format!(
        "{:#}",
        components::add(&app.engine, e, "shape3d", Some(&params))
            .expect_err("an unknown kind must not apply")
    );
    assert!(err.contains("dodecahedron"), "{err}");
}
