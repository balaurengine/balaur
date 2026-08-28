//! Windowed backend built on kiss3d (wgpu). kiss3d 0.46 drives an async
//! render loop, so this backend owns the main loop: per frame it pumps OS
//! events into [`balaur_input::InputState`], ticks the [`App`], then mirrors
//! renderables into the kiss3d scene graph.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use balaur_core::hecs::Entity;
use balaur_core::{App, GlobalTransform};
use balaur_input::InputState;
use glamx::Pose3;
use kiss3d::prelude::*;

use crate::{CameraConfig, CameraInputEnabled, ClearColor, DebugLines, GridConfig, Renderable, ScreenshotRequest, Shape, ViewportCamera};

struct Slot {
    node: SceneNode3d,
    version: u64,
}

/// Run the app inside a kiss3d window until the window closes or the game
/// requests quit.
pub fn run_windowed(mut app: App, title: &str) -> anyhow::Result<()> {
    let title = title.to_string();
    pollster::block_on(async move {
        let mut window = Window::new_with_size(&title, 1600, 1000).await;
        let mut camera = OrbitCamera3d::default();
        let mut scene = SceneNode3d::empty();
        scene
            .add_light(Light::directional(Vec3::new(-1.0, -1.0, -0.5)))
            .set_position(Vec3::new(5.0, 10.0, 5.0));
        let mut slots: HashMap<Entity, Slot> = HashMap::new();
        let mut last = Instant::now();
        let mut frame: u64 = 0;
        while window.render_3d(&mut scene, &mut camera).await {
            apply_camera(&app, &mut camera);
            publish_camera(&app, &camera, &window);
            apply_clear_color(&app, &mut window);
            pump_input(&app, &window);
            let now = Instant::now();
            let dt = (now - last).as_secs_f32().min(0.1);
            last = now;
            app.tick(dt);
            sync(&app, &mut scene, &mut slots);
            draw_grid(&app, &mut window);
            flush_debug_lines(&app, &mut window);
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

fn apply_clear_color(app: &App, window: &mut Window) {
    let Some(clear) = app.engine.try_resource::<ClearColor>() else {
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
        let major = grid.major_every > 0 && i.rem_euclid(grid.major_every as i32) == 0;
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

fn flush_debug_lines(app: &App, window: &mut Window) {
    let Some(lines) = app.engine.try_resource::<DebugLines>() else {
        return;
    };
    for (a, b, c, width, perspective) in lines.borrow_mut().lines.drain(..) {
        window.draw_line(
            Vec3::new(a[0], a[1], a[2]),
            Vec3::new(b[0], b[1], b[2]),
            Color::new(c[0], c[1], c[2], 1.0),
            width,
            perspective,
        );
    }
}

/// Expose the real camera pose (the user may have orbited it) for
/// script-side screen-space math.
fn publish_camera(app: &App, camera: &OrbitCamera3d, window: &Window) {
    let Some(vp) = app.engine.try_resource::<ViewportCamera>() else {
        return;
    };
    let mut vp = vp.borrow_mut();
    let eye = camera.eye();
    let at = camera.at();
    vp.eye = [eye.x, eye.y, eye.z];
    vp.target = [at.x, at.y, at.z];
    vp.fov = std::f32::consts::FRAC_PI_4;
    vp.scale_factor = window.scale_factor() as f32;
}

/// Feed this frame's OS events into the input resource (if the input plugin
/// is installed).
fn pump_input(app: &App, window: &Window) {
    let Some(input) = app.engine.try_resource::<InputState>() else {
        return;
    };
    let camera_enabled = app
        .engine
        .try_resource::<CameraInputEnabled>()
        .map(|f| f.borrow().0)
        .unwrap_or(true);
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
                match action {
                    Action::Press => input.key_event(&format!("{key:?}"), true),
                    Action::Release => input.key_event(&format!("{key:?}"), false),
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
        for (entity, renderable, global) in world
            .query::<(Entity, &Renderable, &GlobalTransform)>()
            .iter()
        {
            let _ = renderable;
            log::debug!("renderable {entity:?} at {}", global.position);
        }
    }
    let image = window.snap_image();
    match image.save(&path) {
        Ok(()) => log::info!("saved screenshot to {}", path.display()),
        Err(err) => log::error!("screenshot failed: {err}"),
    }
    app.engine.remove_resource::<ScreenshotRequest>();
}

/// Mirror `Renderable` + `GlobalTransform` into the kiss3d scene graph.
fn sync(app: &App, scene: &mut SceneNode3d, slots: &mut HashMap<Entity, Slot>) {
    let world = app.engine.world();
    let mut seen: HashSet<Entity> = HashSet::new();
    for (entity, renderable, global) in world
        .query::<(Entity, &Renderable, &GlobalTransform)>()
        .iter()
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
