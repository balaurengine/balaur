//! Windowed backend built on kiss3d (wgpu). kiss3d 0.46 drives an async
//! render loop, so this backend owns the main loop: per frame it pumps OS
//! events into [`balaur_input::InputSnapshot`], ticks the [`App`], then mirrors
//! renderables into the kiss3d scene graph.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use balaur_core::hecs::Entity;
use balaur_core::{App, GlobalTransform};
use balaur_input::InputSnapshot;
use glamx::Pose3;
use kiss3d::prelude::*;

use crate::{
    AppIconConfig, CameraConfig, CameraConfig2d, CameraInputConfig, ClearColorConfig,
    DebugLineBuffer, DebugLineBuffer2d, GridConfig, Renderable, Renderable2d, ScreenshotRequest,
    Shape, Shape2d, ViewportSnapshot, ViewportSnapshot2d, WindowedBackend,
};

struct Slot {
    node: SceneNode3d,
    version: u64,
}

struct Slot2d {
    node: SceneNode2d,
    version: u64,
}

/// Run the app inside a kiss3d window until the window closes or the game
/// requests quit.
pub fn run_windowed(mut app: App, title: &str) -> anyhow::Result<()> {
    // Claim the debug-line buffers: `flush_debug_lines`/`_2d` below drain
    // them as they draw, so the plugin's headless fallback stands down.
    app.engine.insert_resource(WindowedBackend);
    let title = title.to_string();
    pollster::block_on(async move {
        let mut window = Window::new_with_size(&title, 1600, 1000).await;
        let mut camera = OrbitCamera3d::default();
        let mut camera_2d = PanZoomCamera2d::default();
        let mut scene = SceneNode3d::empty();
        scene
            .add_light(Light::directional(Vec3::new(-1.0, -1.0, -0.5)))
            .set_position(Vec3::new(5.0, 10.0, 5.0));
        let mut scene_2d = SceneNode2d::empty();
        let mut slots: HashMap<Entity, Slot> = HashMap::new();
        let mut slots_2d: HashMap<Entity, Slot2d> = HashMap::new();
        let mut order_2d: Vec<Entity> = Vec::new();
        let mut last = Instant::now();
        let mut frame: u64 = 0;
        while window
            .render(
                Some(&mut scene),
                Some(&mut scene_2d),
                Some(&mut camera),
                Some(&mut camera_2d),
                None,
                None,
            )
            .await
        {
            apply_camera(&app, &mut camera);
            apply_camera_2d(&app, &mut camera_2d, &window);
            apply_app_icon(&app);
            publish_camera(&app, &camera, &window);
            publish_camera_2d(&app, &camera_2d, &window);
            apply_clear_color(&app, &mut window);
            pump_input(&app, &window);
            let now = Instant::now();
            let dt = (now - last).as_secs_f32().min(0.1);
            last = now;
            app.tick(dt);
            sync(&app, &mut scene, &mut slots);
            sync_2d(&app, &mut scene_2d, &mut slots_2d, &mut order_2d);
            draw_grid(&app, &mut window);
            flush_debug_lines(&app, &mut window);
            flush_debug_lines_2d(&app, &mut window);
            window.draw_ui(|ctx| balaur_ui::run_pass(&app.engine, ctx));
            frame += 1;
            take_screenshot_if_due(&app, &window, frame);
            if app.engine.quit_requested() {
                break;
            }
        }
    });
    Ok(())
}

/// Apply script-driven camera changes (interactive orbit controls keep
/// working in between).
fn apply_camera(app: &App, camera: &mut OrbitCamera3d) {
    let Some(config) = app.engine.try_resource::<CameraConfig>() else {
        return;
    };
    let mut config = config.borrow_mut();
    if config.changed {
        camera.look_at(config.eye, config.target);
        config.changed = false;
    }
}

/// Apply script-driven 2D camera changes. The config zoom is in logical
/// pixels per world unit; the kiss3d camera works in physical pixels.
fn apply_camera_2d(app: &App, camera: &mut PanZoomCamera2d, window: &Window) {
    let Some(config) = app.engine.try_resource::<CameraConfig2d>() else {
        return;
    };
    let mut config = config.borrow_mut();
    if config.changed {
        let scale = window.scale_factor() as f32;
        camera.look_at(
            Vec2::new(config.center[0], config.center[1]),
            config.zoom * scale,
        );
        config.changed = false;
    }
}

/// Publish the actual 2D camera state (zoom back in logical px per world
/// unit) and the mouse position unprojected to 2D world coordinates.
fn publish_camera_2d(app: &App, camera: &PanZoomCamera2d, window: &Window) {
    use kiss3d::camera::Camera2d;

    let Some(vp) = app.engine.try_resource::<ViewportSnapshot2d>() else {
        return;
    };
    let mut vp = vp.borrow_mut();
    let scale = window.scale_factor() as f32;
    let at = camera.at();
    vp.center = [at.x, at.y];
    vp.zoom = camera.zoom() / scale;
    if let Some(input) = app.engine.try_resource::<InputSnapshot>() {
        let (mx, my) = input.borrow().mouse_pos();
        // The 2D camera's projection is built from the framebuffer size, so
        // unproject in physical pixels.
        let size = Vec2::new(window.width() as f32, window.height() as f32);
        let world = camera.unproject(Vec2::new(mx, my), size);
        vp.mouse_world = [world.x, world.y];
    }
}

fn apply_clear_color(app: &App, window: &mut Window) {
    let Some(clear) = app.engine.try_resource::<ClearColorConfig>() else {
        return;
    };
    let mut clear = clear.borrow_mut();
    if clear.changed {
        let [r, g, b] = clear.color;
        window.set_background_color(Color::new(r, g, b, 1.0));
        clear.changed = false;
    }
}

/// Ground-plane grid, drawn as per-frame lines on the XZ plane.
fn draw_grid(app: &App, window: &mut Window) {
    let Some(grid) = app.engine.try_resource::<GridConfig>() else {
        return;
    };
    let grid = grid.borrow();
    if !grid.enabled {
        return;
    }
    let half = grid.extent as f32 * grid.step;
    let [mr, mg, mb] = grid.minor_color;
    let [jr, jg, jb] = grid.major_color;
    for i in -grid.extent..=grid.extent {
        let offset = i as f32 * grid.step;
        let major = grid.major_every > 0 && i.rem_euclid(grid.major_every.cast_signed()) == 0;
        let color = if major {
            Color::new(jr, jg, jb, 1.0)
        } else {
            Color::new(mr, mg, mb, 1.0)
        };
        let width = if i == 0 { 2.0 } else { 1.0 };
        window.draw_line(
            Vec3::new(offset, 0.0, -half),
            Vec3::new(offset, 0.0, half),
            color,
            width,
            true,
        );
        window.draw_line(
            Vec3::new(-half, 0.0, offset),
            Vec3::new(half, 0.0, offset),
            color,
            width,
            true,
        );
    }
}

fn flush_debug_lines_2d(app: &App, window: &mut Window) {
    let Some(lines) = app.engine.try_resource::<DebugLineBuffer2d>() else {
        return;
    };
    for (a, b, c, width) in lines.borrow_mut().lines.drain(..) {
        window.draw_line_2d(
            Vec2::new(a[0], a[1]),
            Vec2::new(b[0], b[1]),
            Color::new(c[0], c[1], c[2], 1.0),
            width,
        );
    }
}

fn flush_debug_lines(app: &App, window: &mut Window) {
    let Some(lines) = app.engine.try_resource::<DebugLineBuffer>() else {
        return;
    };
    for (a, b, c, width, perspective, on_top) in lines.borrow_mut().lines.drain(..) {
        let a = Vec3::new(a[0], a[1], a[2]);
        let b = Vec3::new(b[0], b[1], b[2]);
        let color = Color::new(c[0], c[1], c[2], 1.0);
        if on_top {
            // depth_bias = 1.0 collapses depth to the near plane: the line
            // renders over everything (editor rotation ball, overlays).
            let polyline = kiss3d::renderer::Polyline3d::new(vec![a, b])
                .with_color(color)
                .with_width(width)
                .with_perspective(perspective)
                .with_depth_bias(1.0);
            window.draw_polyline(&polyline);
        } else {
            window.draw_line(a, b, color, width, perspective);
        }
    }
}

/// Apply a requested dock/application icon (macOS only for now).
fn apply_app_icon(app: &App) {
    let Some(icon) = app.engine.try_resource::<AppIconConfig>() else {
        return;
    };
    let path = {
        let mut icon = icon.borrow_mut();
        if !icon.changed {
            return;
        }
        icon.changed = false;
        icon.path.clone()
    };
    #[cfg(target_os = "macos")]
    {
        use objc2::AnyThread;
        use objc2_app_kit::{NSApplication, NSImage};
        use objc2_foundation::{MainThreadMarker, NSData};
        let Some(bytes) = dock_icon_png(&path) else {
            tracing::warn!("app icon not usable: {}", path.display());
            return;
        };
        if let Ok(dump) = std::env::var("BALAUR_ICON_DUMP") {
            let _ = std::fs::write(&dump, &bytes);
        }
        let data = NSData::with_bytes(&bytes);
        let image = NSImage::initWithData(NSImage::alloc(), &data);
        if let (Some(image), Some(mtm)) = (image, MainThreadMarker::new()) {
            let ns_app = NSApplication::sharedApplication(mtm);
            unsafe { ns_app.setApplicationIconImage(Some(&image)) };
            tracing::info!("app icon set from {}", path.display());
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
    }
}

/// Build a macOS-style dock icon: the source image composited onto a white
/// rounded-rect plate (Big Sur proportions: 824-of-1024 plate, ~185 corner
/// radius) with transparent margins.
#[cfg(target_os = "macos")]
fn dock_icon_png(path: &std::path::Path) -> Option<Vec<u8>> {
    use image::ImageEncoder;

    const CANVAS: u32 = 1024;
    const PLATE: u32 = 824;
    const RADIUS: f32 = 185.0;
    const LOGO: u32 = 660;
    let source = image::open(path).ok()?.to_rgba8();
    let logo =
        image::imageops::resize(&source, LOGO, LOGO, image::imageops::FilterType::CatmullRom);
    let mut canvas = image::RgbaImage::new(CANVAS, CANVAS);
    let plate_min = ((CANVAS - PLATE) / 2) as f32;
    let plate_max = plate_min + PLATE as f32;
    let inside_plate = |x: f32, y: f32| -> bool {
        if x < plate_min || x > plate_max || y < plate_min || y > plate_max {
            return false;
        }
        let cx = x.clamp(plate_min + RADIUS, plate_max - RADIUS);
        let cy = y.clamp(plate_min + RADIUS, plate_max - RADIUS);
        (x - cx).powi(2) + (y - cy).powi(2) <= RADIUS * RADIUS
    };
    let logo_min = (CANVAS - LOGO) / 2;
    for (x, y, px) in canvas.enumerate_pixels_mut() {
        if inside_plate(x as f32 + 0.5, y as f32 + 0.5) {
            let mut color = [255u8, 255, 255, 255];
            if x >= logo_min && x < logo_min + LOGO && y >= logo_min && y < logo_min + LOGO {
                let lp = logo.get_pixel(x - logo_min, y - logo_min).0;
                // Alpha-over white.
                let a = f32::from(lp[3]) / 255.0;
                for c in 0..3 {
                    color[c] = (f32::from(lp[c]) * a + 255.0 * (1.0 - a)) as u8;
                }
            }
            *px = image::Rgba(color);
        }
    }
    let mut out = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut out);
    encoder
        .write_image(&canvas, CANVAS, CANVAS, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(out)
}

/// Expose the real camera state — pose, exact projection matrix, and the
/// picking ray through the current mouse position — for script-side math.
fn publish_camera(app: &App, camera: &OrbitCamera3d, window: &Window) {
    use kiss3d::camera::Camera3d;

    let Some(vp) = app.engine.try_resource::<ViewportSnapshot>() else {
        return;
    };
    let mut vp = vp.borrow_mut();
    let eye = camera.eye();
    let at = camera.at();
    vp.eye = [eye.x, eye.y, eye.z];
    vp.target = [at.x, at.y, at.z];
    vp.fov = std::f32::consts::FRAC_PI_4;
    let scale = window.scale_factor() as f32;
    vp.scale_factor = scale;
    vp.view_proj = camera.transformation().to_cols_array();
    if let Some(input) = app.engine.try_resource::<InputSnapshot>() {
        let (mx, my) = {
            let input = input.borrow();
            input.mouse_pos()
        };
        let size = Vec2::new(
            window.width() as f32 / scale,
            window.height() as f32 / scale,
        );
        let (origin, dir) = camera.unproject(Vec2::new(mx / scale, my / scale), size);
        vp.ray_origin = [origin.x, origin.y, origin.z];
        vp.ray_dir = [dir.x, dir.y, dir.z];
    }
}

/// Feed this frame's OS events into the input resource (if the input plugin
/// is installed).
fn pump_input(app: &App, window: &Window) {
    let Some(input) = app.engine.try_resource::<InputSnapshot>() else {
        return;
    };
    let camera_enabled = app
        .engine
        .try_resource::<CameraInputConfig>()
        .is_none_or(|c| c.borrow().enabled);
    let mut input = input.borrow_mut();
    input.begin_frame();
    for mut event in window.events().iter() {
        // Editors can take the pointer over for gizmo drags: inhibit the
        // camera's own mouse handling while they do.
        if !camera_enabled {
            match event.value {
                WindowEvent::MouseButton(..) | WindowEvent::CursorPos(..) => {
                    event.inhibited = true;
                }
                _ => {}
            }
        }
        match event.value {
            WindowEvent::Key(key, action, _) => {
                let name = key_name(key);
                match action {
                    Action::Press => input.key_event(&name, true),
                    Action::Release => input.key_event(&name, false),
                }
            }
            WindowEvent::MouseButton(button, action, _) => {
                let idx = button as usize;
                match action {
                    Action::Press => input.mouse_button_event(idx, true),
                    Action::Release => input.mouse_button_event(idx, false),
                }
            }
            WindowEvent::CursorPos(x, y, _) => input.set_mouse_pos(x as f32, y as f32),
            WindowEvent::Scroll(dx, dy, _) => input.add_scroll(dx as f32, dy as f32),
            WindowEvent::Close => app.engine.request_quit(),
            _ => {}
        }
    }
}

fn take_screenshot_if_due(app: &App, window: &Window, frame: u64) {
    let Some(request) = app.engine.try_resource::<ScreenshotRequest>() else {
        return;
    };
    let due = {
        let request = request.borrow();
        frame >= request.after_frame
    };
    if !due {
        return;
    }
    let path = request.borrow().path.clone();
    {
        let world = app.engine.world();
        for (entity, renderable, global) in
            &mut world.query::<(Entity, &Renderable, &GlobalTransform)>()
        {
            let _ = renderable;
            tracing::debug!("renderable {entity:?} at {}", global.position);
        }
    }
    let image = window.snap_image();
    match image.save(&path) {
        Ok(()) => tracing::info!("saved screenshot to {}", path.display()),
        Err(err) => tracing::error!("screenshot failed: {err}"),
    }
    app.engine.remove_resource::<ScreenshotRequest>();
}

/// Mirror `Renderable` + `GlobalTransform` into the kiss3d scene graph.
fn sync(app: &App, scene: &mut SceneNode3d, slots: &mut HashMap<Entity, Slot>) {
    let world = app.engine.world();
    let mut seen: HashSet<Entity> = HashSet::new();
    for (entity, renderable, global) in
        &mut world.query::<(Entity, &Renderable, &GlobalTransform)>()
    {
        seen.insert(entity);
        let rebuild = match slots.get(&entity) {
            Some(slot) => slot.version != renderable.version,
            None => true,
        };
        if rebuild {
            if let Some(mut old) = slots.remove(&entity) {
                old.node.remove();
            }
            let node = match renderable.shape {
                Shape::Ball { radius } => scene.add_sphere(radius),
                Shape::Cuboid { hx, hy, hz } => scene.add_cube(2.0 * hx, 2.0 * hy, 2.0 * hz),
            };
            slots.insert(
                entity,
                Slot {
                    node,
                    version: renderable.version,
                },
            );
        }
        // The block above inserts the slot when it is missing.
        let slot = slots.get_mut(&entity).unwrap();
        let [r, g, b, a] = renderable.color;
        // kiss3d primitives encode their dimensions in the node's local
        // scale, so the node scale is (shape size) * (scene scale).
        let size = match renderable.shape {
            Shape::Ball { radius } => Vec3::splat(2.0 * radius),
            Shape::Cuboid { hx, hy, hz } => Vec3::new(2.0 * hx, 2.0 * hy, 2.0 * hz),
        };
        let scale = size * global.scale;
        slot.node
            .set_pose(Pose3::from_parts(global.position, global.rotation))
            .set_local_scale(scale.x, scale.y, scale.z)
            .set_color(Color::new(r, g, b, a));
    }
    slots.retain(|entity, slot| {
        if seen.contains(entity) {
            true
        } else {
            slot.node.remove();
            false
        }
    });
}

/// Mirror `Renderable2d` + `GlobalTransform` into the kiss3d 2D scene graph
/// (x/y translation, z rotation, x/y scale).
///
/// kiss3d draws 2D nodes in insertion order, so draw order is made explicit
/// and deterministic: scene-tree traversal order, stably sorted by the
/// global z coordinate (z acts as a 2D layer; equal z means later-declared
/// nodes draw on top). When the order changes, the kiss3d nodes are rebuilt
/// in the new order.
fn sync_2d(
    app: &App,
    scene: &mut SceneNode2d,
    slots: &mut HashMap<Entity, Slot2d>,
    order_cache: &mut Vec<Entity>,
) {
    let world = app.engine.world();
    let root = app.engine.root();
    let mut desired: Vec<(f32, Entity)> = Vec::new();
    for entity in balaur_core::scene::collect_subtree(&world, root) {
        if world.get::<&Renderable2d>(entity).is_ok() {
            let z = world
                .get::<&GlobalTransform>(entity)
                .map_or(0.0, |g| g.position.z);
            desired.push((z, entity));
        }
    }
    desired.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let order: Vec<Entity> = desired.iter().map(|&(_, e)| e).collect();
    if order != *order_cache {
        for (_, mut slot) in slots.drain() {
            slot.node.detach();
        }
        order_cache.clone_from(&order);
    }

    let mut seen: HashSet<Entity> = HashSet::new();
    for &entity in &order {
        let Ok(renderable) = world.get::<&Renderable2d>(entity) else {
            continue;
        };
        let Ok(global) = world.get::<&GlobalTransform>(entity) else {
            continue;
        };
        seen.insert(entity);
        let rebuild = match slots.get(&entity) {
            Some(slot) => slot.version != renderable.version,
            None => true,
        };
        if rebuild {
            if let Some(mut old) = slots.remove(&entity) {
                old.node.detach();
            }
            // Unit primitives: like the 3D path, dimensions live in the
            // node's local scale, updated every frame below.
            let node = match renderable.shape {
                Shape2d::Circle { .. } => scene.add_circle(0.5),
                Shape2d::Rect { .. } => scene.add_rectangle(1.0, 1.0),
            };
            slots.insert(
                entity,
                Slot2d {
                    node,
                    version: renderable.version,
                },
            );
        }
        // The block above inserts the slot when it is missing.
        let slot = slots.get_mut(&entity).unwrap();
        let [r, g, b, a] = renderable.color;
        let size = match renderable.shape {
            Shape2d::Circle { radius } => Vec2::splat(2.0 * radius),
            Shape2d::Rect { hx, hy } => Vec2::new(2.0 * hx, 2.0 * hy),
        };
        let (angle, _, _) = global.rotation.to_euler(glamx::EulerRot::ZYX);
        slot.node
            .set_position(Vec2::new(global.position.x, global.position.y))
            .set_rotation(angle)
            .set_local_scale(size.x * global.scale.x, size.y * global.scale.y)
            .set_color(Color::new(r, g, b, a));
    }
    slots.retain(|entity, slot| {
        if seen.contains(entity) {
            true
        } else {
            slot.node.detach();
            false
        }
    });
}

/// The name this backend reports for a key.
///
/// kiss3d's `Key` debug-prints as its variant name, which is where
/// `balaur_input::KEY_NAMES` came from. If kiss3d ever renames one, the name
/// stops matching what scripts ask for and the key silently goes dead, so say
/// so the first time it happens.
fn key_name(key: kiss3d::event::Key) -> String {
    let name = format!("{key:?}");
    if !balaur_input::is_known_key(&name) {
        use std::sync::atomic::{AtomicBool, Ordering};
        static WARNED: AtomicBool = AtomicBool::new(false);
        if !WARNED.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                key = name,
                "the window backend reported a key balaur_input does not know; \
                 scripts cannot match it"
            );
        }
    }
    name
}

#[cfg(test)]
mod key_name_tests {
    use super::key_name;
    use kiss3d::event::Key;

    /// The vocabulary in `balaur_input` was copied from kiss3d's enum. Spot
    /// check that the copy still matches for the keys games actually use — a
    /// silent rename upstream would make them stop working.
    #[test]
    fn common_keys_are_names_scripts_can_ask_for() {
        for key in [
            Key::Space,
            Key::Return,
            Key::Escape,
            Key::Tab,
            Key::Left,
            Key::Right,
            Key::Up,
            Key::Down,
            Key::A,
            Key::Z,
            Key::Key0,
            Key::Key9,
            Key::LShift,
            Key::LControl,
            Key::F1,
        ] {
            let name = key_name(key);
            assert!(
                balaur_input::is_known_key(&name),
                "kiss3d reports {name:?}, which balaur_input does not know"
            );
        }
    }

    #[test]
    fn the_vocabulary_has_no_duplicates() {
        let mut seen = std::collections::BTreeSet::new();
        for name in balaur_input::KEY_NAMES {
            assert!(seen.insert(*name), "{name} is listed twice");
        }
    }
}
