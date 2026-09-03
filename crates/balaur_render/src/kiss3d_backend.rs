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
use kiss3d::resource::GpuMesh3d;

use crate::{
    AppIconConfig, CameraConfig, CameraConfig2d, CameraInputConfig, ClearColorConfig,
    DebugLineBuffer, DebugLineBuffer2d, GridConfig, Renderable, Renderable2d, ScreenshotRequest,
    Shape, Shape2d, SpriteTexture, ViewportSnapshot, ViewportSnapshot2d, WindowConfig,
    WindowedBackend,
};

struct Slot {
    node: SceneNode3d,
    version: u64,
    /// A skinned mesh's rest geometry and bindings, deformed on the CPU
    /// every frame and written back into the node's buffers.
    skin: Option<MeshSkinSlot>,
}

/// What a skinned 3D mesh keeps between frames: the vertices as authored,
/// which bones move them, and where the rig is.
struct MeshSkinSlot {
    positions: Vec<Vec3>,
    normals: Option<Vec<Vec3>>,
    joints: Vec<[u32; 4]>,
    weights: Vec<[f32; 4]>,
    bones: Vec<String>,
    inverse_bind: Option<Vec<glamx::Mat4>>,
    skeleton: String,
}

struct Slot2d {
    node: SceneNode2d,
    version: u64,
    /// The flip pair last written into the node's UVs: sheetless sprites only
    /// touch UVs when it changes, so un-flipping writes the identity rect once.
    flip: (bool, bool),
    /// A skinned polygon's joint palette, rewritten every frame from the rig.
    skin: Option<crate::skinned_2d::SkinHandle>,
}

/// Everything one frame of the render loop reads and writes, so the windowed
/// and offscreen runners share a body instead of keeping two copies in step.
struct Frontend {
    camera: OrbitCamera3d,
    camera_2d: PanZoomCamera2d,
    scene: SceneNode3d,
    scene_2d: SceneNode2d,
    slots: HashMap<Entity, Slot>,
    slots_2d: HashMap<Entity, Slot2d>,
    tilemap_slots: HashMap<Entity, crate::tilemap::TilemapSlot>,
    emitter_slots: HashMap<Entity, crate::particles::EmitterSlot>,
    materials: crate::shader_material::MaterialCache,
    materials_3d: crate::shader_material_3d::MaterialCache3d,
    order_2d: Vec<Entity>,
    frame: u64,
    /// Whether the on-screen keyboard was summoned last frame, so it is
    /// shown/hidden on the edge rather than re-requested every frame.
    keyboard_shown: bool,
}

impl Frontend {
    fn new() -> Self {
        let mut scene = SceneNode3d::empty();
        scene
            .add_light(Light::directional(Vec3::new(-1.0, -1.0, -0.5)))
            .set_position(Vec3::new(5.0, 10.0, 5.0));
        Self {
            camera: OrbitCamera3d::default(),
            camera_2d: PanZoomCamera2d::default(),
            scene,
            scene_2d: SceneNode2d::empty(),
            slots: HashMap::new(),
            slots_2d: HashMap::new(),
            tilemap_slots: HashMap::new(),
            emitter_slots: HashMap::new(),
            materials: crate::shader_material::MaterialCache::default(),
            materials_3d: crate::shader_material_3d::MaterialCache3d::default(),
            order_2d: Vec::new(),
            frame: 0,
            keyboard_shown: false,
        }
    }

    /// One frame: apply what scripts asked for, tick, mirror the world into
    /// the scene graph, draw the overlays. Answers whether to keep going.
    fn step(&mut self, app: &mut App, window: &mut Window, dt: f32) -> bool {
        apply_camera(app, &mut self.camera);
        apply_camera_2d(app, &mut self.camera_2d, window);
        apply_app_icon(app);
        apply_window_config(app, window);
        publish_camera(app, &self.camera, window);
        publish_camera_2d(app, &self.camera_2d, window);
        apply_clear_color(app, window);
        pump_input(app, window);
        app.advance(dt);
        sync(
            app,
            &mut self.scene,
            &mut self.slots,
            &mut self.materials_3d,
        );
        sync_2d(
            app,
            &mut self.scene_2d,
            &mut self.slots_2d,
            &mut self.order_2d,
            &mut self.materials,
        );
        crate::tilemap::sync_tilemaps(app, &mut self.scene_2d, &mut self.tilemap_slots);
        // The step the frame actually ran, which under --fixed-tick is not
        // the measured one.
        let dt = app.engine.delta();
        crate::particles::sync_particles(app, window, &mut self.emitter_slots, dt);
        draw_grid(app, window);
        flush_debug_lines(app, window);
        flush_debug_lines_2d(app, window);
        window.draw_ui(|ctx| balaur_ui::run_pass(&app.engine, ctx));
        // On-screen keyboard follows ui keyboard focus, edge-detected after
        // the ui pass has settled focus. A no-op on desktop.
        let wants_keyboard = window.is_egui_capturing_keyboard();
        if wants_keyboard != self.keyboard_shown {
            self.keyboard_shown = wants_keyboard;
            window.set_keyboard_visible(wants_keyboard);
        }
        self.frame += 1;
        take_screenshot_if_due(app, window, self.frame);
        !app.engine.quit_requested()
    }
}

/// Run the app inside a kiss3d window until the window closes or the game
/// requests quit.
#[allow(
    clippy::disallowed_methods,
    reason = "the loop driver measures a frame; what it feeds systems is fixed_dt"
)]
pub fn run_windowed(mut app: App, title: &str) -> anyhow::Result<()> {
    // Claim the debug-line buffers: `flush_debug_lines`/`_2d` below drain
    // them as they draw, so the plugin's headless fallback stands down.
    app.engine.insert_resource(WindowedBackend);
    let title = title.to_string();
    pollster::block_on(async move {
        let mut window = Window::new_with_size(&title, 1600, 1000).await;
        let mut f = Frontend::new();
        let mut last = Instant::now();
        while window
            .render(
                Some(&mut f.scene),
                Some(&mut f.scene_2d),
                Some(&mut f.camera),
                Some(&mut f.camera_2d),
                None,
                None,
            )
            .await
        {
            let now = Instant::now();
            let dt = (now - last).as_secs_f32().min(0.1);
            last = now;
            if !f.step(&mut app, &mut window, dt) {
                break;
            }
        }
    });
    Ok(())
}

/// Run the app against a hidden window: real GPU rendering, no OS window.
///
/// The third mode, and the reason it is separate from headless: headless runs
/// no renderer at all, which is what keeps tests fast and lets the engine run
/// where there is no adapter. This one renders exactly as the windowed path
/// does — same loop body — so a screenshot taken here is what a player would
/// have seen. What it does not have is an OS window, input, or vsync.
///
/// Frames advance at a fixed `dt` rather than wall clock. There is nothing to
/// pace against without a display, and an automation client asking for frame
/// 90 should get the same frame every time it asks.
///
/// # Errors
/// If no GPU adapter is available. Offscreen still needs a device — it is a
/// window that is missing, not the GPU.
pub fn run_offscreen(mut app: App, title: &str, width: u32, height: u32) -> anyhow::Result<()> {
    app.engine.insert_resource(WindowedBackend);
    // A surface-less target has no title bar to put this in, so it goes to the
    // log instead — which is where whoever is reading a CI run will look.
    tracing::info!("rendering '{title}' offscreen at {width}x{height}");
    pollster::block_on(async move {
        // Not `new_hidden_*`: a hidden window still needs a display server.
        // Surface-less rendering runs on a CI box with no display at all.
        let mut window =
            Window::new_headless_with_setup(width, height, CanvasSetup::default()).await;
        let mut f = Frontend::new();
        // Nothing can close a target that was never shown, and there is no
        // vsync to block on, so the loop runs until the app asks to stop --
        // which `--frames` arranges by inserting a quit-after-N system.
        while window
            .render(
                Some(&mut f.scene),
                Some(&mut f.scene_2d),
                Some(&mut f.camera),
                Some(&mut f.camera_2d),
                None,
                None,
            )
            .await
        {
            if !f.step(&mut app, &mut window, OFFSCREEN_DT) {
                break;
            }
        }
    });
    Ok(())
}

/// The fixed step offscreen frames advance by, matching the physics and
/// animation tick so a screenshot lands on a whole number of simulation steps.
const OFFSCREEN_DT: f32 = balaur_core::FIXED_DT;

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

/// Apply fullscreen and cursor state scripts asked for since the last frame.
fn apply_window_config(app: &App, window: &Window) {
    let Some(config) = app.engine.try_resource::<WindowConfig>() else {
        return;
    };
    let mut config = config.borrow_mut();
    if !config.changed {
        return;
    }
    config.changed = false;
    // Fullscreen is a window-manager idea: on a phone the app already owns the
    // screen, and kiss3d exposes no toggle there.
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    window.set_fullscreen(config.fullscreen);
    window.set_cursor_grab(config.cursor_grabbed);
    window.hide_cursor(config.cursor_hidden);
}

/// Apply a requested dock/application icon (macOS only for now).
fn apply_app_icon(app: &App) {
    let Some(icon) = app.engine.try_resource::<AppIconConfig>() else {
        return;
    };
    let (source, name) = {
        let mut icon = icon.borrow_mut();
        if !icon.changed {
            return;
        }
        icon.changed = false;
        (icon.bytes.clone(), icon.name.clone())
    };
    #[cfg(target_os = "macos")]
    {
        use objc2::AnyThread;
        use objc2_app_kit::{NSApplication, NSImage};
        use objc2_foundation::{MainThreadMarker, NSData};
        let Some(bytes) = dock_icon_png(&source) else {
            tracing::warn!("app icon not usable: {name}");
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
            tracing::info!("app icon set from {name}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (&source, &name);
    }
}

/// Build a macOS-style dock icon: the source image composited onto a white
/// rounded-rect plate (Big Sur proportions: 824-of-1024 plate, ~185 corner
/// radius) with transparent margins.
#[cfg(target_os = "macos")]
fn dock_icon_png(source: &[u8]) -> Option<Vec<u8>> {
    use image::ImageEncoder;

    const CANVAS: u32 = 1024;
    const PLATE: u32 = 824;
    const RADIUS: f32 = 185.0;
    const LOGO: u32 = 660;
    let source = image::load_from_memory(source).ok()?.to_rgba8();
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
            WindowEvent::Char(c) | WindowEvent::CharModifiers(c, _) => {
                // Control characters are key presses that produced no text.
                if !c.is_control() {
                    input.char_event(c);
                }
            }
            WindowEvent::CursorPos(x, y, _) => input.set_mouse_pos(x as f32, y as f32),
            WindowEvent::Scroll(dx, dy, _) => input.add_scroll(dx as f32, dy as f32),
            WindowEvent::Touch(id, x, y, action, _) => {
                let phase = match action {
                    TouchAction::Start => balaur_input::TouchPhase::Start,
                    TouchAction::Move => balaur_input::TouchPhase::Move,
                    TouchAction::End => balaur_input::TouchPhase::End,
                    TouchAction::Cancel => balaur_input::TouchPhase::Cancel,
                };
                input.touch_event(id, x as f32, y as f32, phase);
            }
            WindowEvent::Close => app.engine.request_quit(),
            _ => {}
        }
    }
    // Dragging a file onto the window needs a desktop with a file manager;
    // kiss3d has no such event on mobile.
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    for path in window.dropped_files() {
        input.file_drop_event(path.to_string_lossy().into_owned());
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
        Ok(()) => tracing::debug!("saved screenshot to {}", path.display()),
        Err(err) => tracing::error!("screenshot failed: {err}"),
    }
    app.engine.remove_resource::<ScreenshotRequest>();
}

/// Mirror `Renderable` + `GlobalTransform` into the kiss3d scene graph.
fn sync(
    app: &App,
    scene: &mut SceneNode3d,
    slots: &mut HashMap<Entity, Slot>,
    materials: &mut crate::shader_material_3d::MaterialCache3d,
) {
    let world = app.engine.world();
    // A relink rebuilds the nodes holding the old pipeline; a channel view
    // rebuilds every node, whether or not it names a material.
    let channel = crate::debug_view::channel_view(&app.engine);
    let relinked = materials.refresh(app);
    let channel_changed = materials.channel_changed(&channel);
    materials.answer_probe(app);

    let mut seen: HashSet<Entity> = HashSet::new();
    for (entity, renderable, global) in
        &mut world.query::<(Entity, &Renderable, &GlobalTransform)>()
    {
        seen.insert(entity);
        let rebuild = match slots.get(&entity) {
            Some(slot) => {
                slot.version != renderable.version
                    || channel_changed
                    || (relinked && !renderable.material.is_empty())
            }
            None => true,
        };
        if rebuild {
            if let Some(mut old) = slots.remove(&entity) {
                old.node.remove();
            }
            let (mut node, skin) = match renderable.shape {
                Shape::Ball { radius } => (scene.add_sphere(radius), None),
                Shape::Cuboid { hx, hy, hz } => {
                    (scene.add_cube(2.0 * hx, 2.0 * hy, 2.0 * hz), None)
                }
                Shape::Capsule { radius, height } => (scene.add_capsule(radius, height), None),
                Shape::Cylinder { radius, height } => (scene.add_cylinder(radius, height), None),
                Shape::Cone { radius, height } => (scene.add_cone(radius, height), None),
                // One quad, no subdivisions: a flat plane needs no interior
                // vertices, and fewer of them is fewer to transform.
                Shape::Plane { hx, hz } => (scene.add_quad(2.0 * hx, 2.0 * hz, 1, 1), None),
                Shape::Mesh => match upload_mesh(app, scene, renderable) {
                    Some(built) => built,
                    // Nothing to draw yet, and `upload_mesh` said why.
                    None => continue,
                },
            };
            // After the texture: a material reads it, and kiss3d's own
            // material stays on a node whose shader would not link.
            if let Some(material) = materials.for_node(app, &renderable.material, &channel) {
                node.set_material(material);
            }
            slots.insert(
                entity,
                Slot {
                    node,
                    version: renderable.version,
                    skin,
                },
            );
        }
        // The block above inserts the slot when it is missing.
        let slot = slots.get_mut(&entity).unwrap();
        if let Some(skin) = &slot.skin {
            deform_mesh(&world, entity, skin, &mut slot.node);
        }
        let [r, g, b, a] = renderable.color;
        // kiss3d primitives encode their dimensions in the node's local
        // scale, so the node scale is (shape size) * (scene scale).
        let size = match renderable.shape {
            Shape::Ball { radius } => Vec3::splat(2.0 * radius),
            Shape::Cuboid { hx, hy, hz } => Vec3::new(2.0 * hx, 2.0 * hy, 2.0 * hz),
            Shape::Cylinder { radius, height } | Shape::Cone { radius, height } => {
                Vec3::new(2.0 * radius, height, 2.0 * radius)
            }
            // Capsule, quad and mesh are real geometry built at unit size,
            // unlike the primitives above: scaling them would double them.
            Shape::Capsule { .. } | Shape::Plane { .. } | Shape::Mesh => Vec3::ONE,
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

/// Resolve a `mesh` asset and hand its triangles to kiss3d, with the skin to
/// deform them by when the asset carries one. `None` when the asset is
/// missing or unreadable, which is logged rather than fatal: one bad model
/// must not stop the frame.
fn upload_mesh(
    app: &App,
    scene: &mut SceneNode3d,
    renderable: &Renderable,
) -> Option<(SceneNode3d, Option<MeshSkinSlot>)> {
    let reference = renderable.mesh.as_deref().filter(|r| !r.is_empty())?;
    let definition = match balaur_core::assets::load_typed::<balaur_core::mesh::MeshData>(
        &app.engine,
        reference,
    ) {
        Ok(definition) => definition,
        Err(err) => {
            tracing::error!("mesh '{reference}': {err:#}");
            return None;
        }
    };
    let data = match balaur_core::mesh::load_from(&app.engine, &definition) {
        Ok(data) => data,
        Err(err) => {
            tracing::error!("mesh '{reference}': {err:#}");
            return None;
        }
    };
    let coords: Vec<Vec3> = data
        .positions
        .iter()
        .map(|p| Vec3::new(p[0], p[1], p[2]))
        .collect();
    let faces: Vec<[u32; 3]> = data.indices.clone();
    // Normals and UVs are optional in the format; kiss3d computes normals from
    // the faces when they are absent, which is the right answer for a bare OBJ.
    let normals: Option<Vec<Vec3>> = data
        .normals
        .as_ref()
        .map(|ns| ns.iter().map(|n| Vec3::new(n[0], n[1], n[2])).collect());
    let uvs = data
        .uvs
        .as_ref()
        .map(|us| us.iter().map(|u| Vec2::new(u[0], u[1])).collect());
    let skin = data.skin.map(|skin| MeshSkinSlot {
        positions: coords.clone(),
        normals: normals.clone(),
        joints: skin.joints,
        weights: skin.weights,
        bones: skin.bones,
        inverse_bind: skin.inverse_bind,
        skeleton: renderable.skeleton.clone(),
    });
    // A skinned mesh is rewritten every frame; a rigid one is uploaded once.
    let gpu = GpuMesh3d::new(coords, faces, normals, uvs, skin.is_some());
    let mut node = scene.add_mesh(std::rc::Rc::new(std::cell::RefCell::new(gpu)), Vec3::ONE);
    if !renderable.texture.is_empty() {
        match app
            .engine
            .resource::<balaur_core::project::ProjectFiles>()
            .borrow()
            .read(&renderable.texture)
        {
            Ok(bytes) => {
                node.set_texture_from_memory(&bytes, &renderable.texture);
            }
            Err(err) => tracing::error!("{err:#}"),
        }
    }
    Some((node, skin))
}

/// Pose a skinned mesh for this frame: the palette from the rig, the rest
/// vertices through it on the CPU, and the result into the node's buffers.
/// A rig or bone path that resolves to nothing is logged at debug and the
/// mesh draws at rest, the same as a clip track that targets no node.
fn deform_mesh(
    world: &balaur_core::hecs::World,
    entity: Entity,
    skin: &MeshSkinSlot,
    node: &mut SceneNode3d,
) {
    let rig = if skin.skeleton.is_empty() {
        Some(entity)
    } else {
        balaur_core::scene::find_node(world, entity, &skin.skeleton)
    };
    let Some(rig) = rig else {
        tracing::debug!(skeleton = skin.skeleton, "mesh rig path names no node");
        return;
    };
    let bones: Vec<Option<Entity>> = skin
        .bones
        .iter()
        .map(|path| {
            let bone = balaur_core::scene::find_node(world, rig, path);
            if bone.is_none() {
                tracing::debug!(bone = path, "mesh skin names a bone that is not there");
            }
            bone
        })
        .collect();
    let palette = balaur_core::skeleton::joint_matrices_3d(
        world,
        entity,
        rig,
        &bones,
        skin.inverse_bind.as_deref(),
    );
    let positions = balaur_core::skeleton::skin_positions_3d(
        &skin.positions,
        &skin.joints,
        &skin.weights,
        &palette,
    );
    node.modify_vertices(&mut |coords: &mut Vec<Vec3>| {
        coords.clone_from(&positions);
    });
    match &skin.normals {
        Some(rest) => {
            let normals =
                balaur_core::skeleton::skin_normals_3d(rest, &skin.joints, &skin.weights, &palette);
            node.modify_normals(&mut |ns: &mut Vec<Vec3>| {
                ns.clone_from(&normals);
            });
        }
        None => node.recompute_normals(),
    }
}

/// The kiss3d node a 2D shape needs. `None` when a polyline names no usable
/// points, which is the one shape that can fail to have any.
fn build_2d_node(
    app: &App,
    scene: &mut SceneNode2d,
    renderable: &Renderable2d,
) -> Option<SceneNode2d> {
    // Unit primitives: like the 3D path, dimensions live in the node's local
    // scale, which the caller updates every frame.
    Some(match renderable.shape {
        Shape2d::Circle { .. } => scene.add_circle(0.5),
        Shape2d::Capsule { radius, height } => scene.add_capsule(radius, height),
        Shape2d::Polyline { width, closed } => {
            // The points come from the same mesh asset physics reads, flattened
            // to xy: a 2D chain is a 3D one seen from above.
            let points = polyline_points(app, renderable.polyline.as_deref(), closed);
            if points.len() < 2 {
                return None;
            }
            scene.add_polyline(points, None, width)
        }
        Shape2d::Rect { .. } | Shape2d::Sprite { .. } => scene.add_rectangle(1.0, 1.0),
        // Built by `build_polygon_node`, which also hands back the palette.
        Shape2d::Polygon => return None,
    })
}

/// A polygon's kiss3d node and, when its mesh carries a skin, the palette
/// handle the frame writes joint matrices into. `None` with nothing to draw.
fn build_polygon_node(
    app: &App,
    scene: &mut SceneNode2d,
    renderable: &Renderable2d,
) -> Option<(SceneNode2d, Option<crate::skinned_2d::SkinHandle>)> {
    let polygon = renderable.polygon.as_ref()?;
    if polygon.positions.is_empty() || polygon.indices.is_empty() {
        return None;
    }
    let (mut node, skin) = crate::skinned_2d::build(scene, polygon);
    attach_texture(app, &mut node, &polygon.texture);
    Some((node, skin))
}

/// Give a freshly built node its image. Once per rebuild: kiss3d's
/// TextureManager caches by name, so the decode happens on the first node to
/// ask for it. An empty path is an image not chosen yet, and kiss3d's default
/// white texture is the right stand-in.
fn attach_texture(app: &App, node: &mut SceneNode2d, path: &str) {
    if path.is_empty() {
        return;
    }
    // Bytes rather than a path: a packed game carries its textures inside
    // the pack, with nothing beside it on disk.
    match app
        .engine
        .resource::<balaur_core::project::ProjectFiles>()
        .borrow()
        .read(path)
    {
        Ok(bytes) => {
            node.set_texture_from_memory(&bytes, path);
        }
        // A frame is not the place to abort: say which asset is missing and
        // keep drawing the rest of the scene.
        Err(err) => tracing::error!("{err:#}"),
    }
}

/// The joint matrices a skinned polygon deforms by this frame, resolved
/// from the rig it names. A path that resolves to nothing is logged at
/// debug and the polygon draws rigid, the same as a clip track that targets
/// no node.
fn polygon_palette(
    world: &balaur_core::hecs::World,
    entity: Entity,
    polygon: &crate::PolygonMesh,
) -> Vec<glamx::Mat3> {
    let Some(skin) = polygon.skin.as_ref() else {
        return Vec::new();
    };
    let rig = if polygon.skeleton.is_empty() {
        Some(entity)
    } else {
        balaur_core::scene::find_node(world, entity, &polygon.skeleton)
    };
    let Some(rig) = rig else {
        tracing::debug!(
            skeleton = polygon.skeleton,
            "polygon rig path names no node"
        );
        return vec![glamx::Mat3::IDENTITY; skin.bones.len()];
    };
    let bones: Vec<Option<Entity>> = skin
        .bones
        .iter()
        .map(|path| {
            let bone = balaur_core::scene::find_node(world, rig, path);
            if bone.is_none() {
                tracing::debug!(bone = path, "polygon skin names a bone that is not there");
            }
            bone
        })
        .collect();
    balaur_core::skeleton::joint_matrices_2d(world, entity, rig, &bones)
}

/// A polyline's points from its `mesh` asset, flattened to xy. Empty when the
/// asset is missing or unreadable, which the caller treats as nothing to draw.
fn polyline_points(app: &App, reference: Option<&str>, closed: bool) -> Vec<Vec2> {
    let Some(reference) = reference.filter(|r| !r.is_empty()) else {
        return Vec::new();
    };
    let loaded =
        balaur_core::assets::load_typed::<balaur_core::mesh::MeshData>(&app.engine, reference)
            .and_then(|definition| balaur_core::mesh::load_from(&app.engine, &definition));
    let data = match loaded {
        Ok(data) => data,
        Err(err) => {
            tracing::error!("polyline '{reference}': {err:#}");
            return Vec::new();
        }
    };
    let mut points: Vec<Vec2> = data
        .positions
        .iter()
        .map(|p| Vec2::new(p[0], p[1]))
        .collect();
    // A closed chain repeats its first point rather than carrying a flag: the
    // renderer draws segments, and the join is just one more of them.
    if closed && points.len() > 2 {
        points.push(points[0]);
    }
    points
}

/// Mirror `Renderable2d` + `GlobalTransform` into the kiss3d 2D scene graph
/// (x/y translation, z rotation, x/y scale).
///
/// The 2D nodes in the order they draw, detaching every slot when that order
/// changed so the caller rebuilds them in the new one.
fn draw_order_2d(
    world: &balaur_core::hecs::World,
    root: Entity,
    slots: &mut HashMap<Entity, Slot2d>,
    order_cache: &mut Vec<Entity>,
) -> Vec<Entity> {
    let mut desired: Vec<(f32, Entity)> = Vec::new();
    for entity in balaur_core::scene::collect_subtree(world, root) {
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
    order
}

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
    materials: &mut crate::shader_material::MaterialCache,
) {
    let world = app.engine.world();
    let order = draw_order_2d(&world, app.engine.root(), slots, order_cache);

    // A relink rebuilds the nodes holding the old pipeline; a channel view
    // rebuilds every node, whether or not it names a material.
    let channel = crate::debug_view::channel_view(&app.engine);
    let relinked = materials.refresh(app);
    let channel_changed = materials.channel_changed(&channel);
    materials.answer_probe(app);

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
            Some(slot) => {
                slot.version != renderable.version
                    || channel_changed
                    || (relinked && !renderable.material.is_empty())
            }
            None => true,
        };
        if rebuild {
            if let Some(mut old) = slots.remove(&entity) {
                old.node.detach();
            }
            let built = if renderable.shape == Shape2d::Polygon {
                build_polygon_node(app, scene, &renderable)
            } else {
                build_2d_node(app, scene, &renderable).map(|node| (node, None))
            };
            let Some((mut node, skin)) = built else {
                continue;
            };
            if let Some(sprite) = &renderable.sprite {
                attach_texture(app, &mut node, &sprite.path);
            }
            // After the texture: a material reads it, and kiss3d's own
            // material stays on a node whose shader would not link.
            if let Some(material) = materials.for_node(app, &renderable.material, &channel) {
                node.set_material(material);
            }
            slots.insert(
                entity,
                Slot2d {
                    node,
                    version: renderable.version,
                    flip: (false, false),
                    skin,
                },
            );
        }
        // The block above inserts the slot when it is missing.
        let slot = slots.get_mut(&entity).unwrap();
        let [r, g, b, a] = renderable.color;
        let size = match renderable.shape {
            Shape2d::Circle { radius } => Vec2::splat(2.0 * radius),
            // kiss3d's 2D capsule is real geometry like its 3D one, and a
            // polyline's or polygon's points are already in world units: none
            // is a unit primitive waiting to be scaled.
            Shape2d::Capsule { .. } | Shape2d::Polyline { .. } | Shape2d::Polygon => Vec2::ONE,
            Shape2d::Rect { hx, hy } | Shape2d::Sprite { hx, hy } => Vec2::new(2.0 * hx, 2.0 * hy),
        };
        // Every frame: the rig moved even when nothing about the polygon did.
        if let (Some(handle), Some(polygon)) = (&slot.skin, renderable.polygon.as_deref()) {
            handle.set(polygon_palette(&world, entity, polygon));
        }
        // Every sync, not just on rebuild: frames and flips are UV changes, so
        // an animation that flips frames must not rebuild the node.
        if let Some(sprite) = &renderable.sprite {
            let flip = (sprite.flip_x, sprite.flip_y);
            if sprite.sheet.is_some() || flip != slot.flip {
                sync_sprite_uvs(&mut slot.node, sprite);
            }
            slot.flip = flip;
        }
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

/// Remap the node's UVs to the sprite's sheet frame, with the U or V extents
/// swapped for flips.
fn sync_sprite_uvs(node: &mut SceneNode2d, sprite: &SpriteTexture) {
    let sheet = sprite
        .sheet
        .map(|s| kiss3d::scene::SpriteSheet::new(s.columns, s.rows));
    let (mut min, mut max) = sheet.map_or((Vec2::ZERO, Vec2::ONE), |s| s.frame_uv(sprite.frame));
    if sheet.is_some() {
        // The sliver keeps a nearest-sampled edge fragment from rounding into
        // the neighbouring frame, matching kiss3d's own `set_sprite_frame`.
        let size = node.data().object().map(|o| o.data().texture().size);
        if let Some((w, h)) = size {
            if w > 1 && h > 1 {
                let inset = Vec2::new(0.05 / w as f32, 0.05 / h as f32);
                min += inset;
                max -= inset;
            }
        }
    }
    if sprite.flip_x {
        std::mem::swap(&mut min.x, &mut max.x);
    }
    if sprite.flip_y {
        std::mem::swap(&mut min.y, &mut max.y);
    }
    node.set_uv_rect(min, max);
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
