//! Windowed backend built on kiss3d (wgpu). kiss3d 0.46 drives an async
//! render loop, so this backend owns the main loop: per frame it pumps OS
//! events into [`balaur_input::InputSnapshot`], ticks the [`App`], then mirrors
//! renderables into the kiss3d scene graph.

use balaur_core::time::Instant;
use std::collections::{HashMap, HashSet};

use balaur_core::hecs::Entity;
use balaur_core::{App, GlobalAppearance, GlobalTransform};
use glamx::Pose3;
use kiss3d::prelude::*;
use kiss3d::resource::GpuMesh3d;

use crate::kiss3d_camera::{
    CameraButtons, apply_camera, apply_camera_2d, apply_camera_input, publish_camera,
    publish_camera_2d,
};
use crate::{
    ClearColorConfig, DebugLineBuffer, DebugLineBuffer2d, GridConfig, PostConfig, Renderable,
    Renderable2d, ScreenshotRequest, Shape, Shape2d, SpriteTexture, WindowConfig, WindowedBackend,
};

struct Slot {
    node: SceneNode3d,
    version: u64,
    /// A skinned mesh's rest geometry and bindings: where the palette comes
    /// from, and the vertices the CPU path deforms when there is no handle.
    skin: Option<MeshSkinSlot>,
    /// The GPU palette, when this mesh skins in the vertex shader.
    palette: Option<crate::skinned_3d::SkinHandle3d>,
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
    /// A polyline's pieces with where along the chain each sits, so a
    /// gradient can colour them every frame under the node's tint.
    pieces: Vec<(SceneNode2d, f32)>,
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
    light_map: crate::light_map::LightMap,
    order_2d: Vec<Entity>,
    /// Last frame's immediate 2D shapes, detached before this frame's are drawn.
    transients: Vec<SceneNode2d>,
    frame: u64,
    /// Whether the on-screen keyboard was summoned last frame, so it is
    /// shown/hidden on the edge rather than re-requested every frame.
    keyboard_shown: bool,
    /// The cameras' drag bindings as built: taking the pointer away from the
    /// camera unbinds them, and these are what it puts back.
    camera_buttons: CameraButtons,
    /// What the display says, measured frame by frame.
    device: crate::device::Probe,
    /// The asset generation the nodes below were built at. A saved texture,
    /// model or tileset moves it, and every node built from a file is built
    /// again — the material caches watch the same counter for their shaders.
    asset_generation: u64,
}

impl Frontend {
    fn new() -> Self {
        let mut scene = SceneNode3d::empty();
        scene
            .add_light(Light::directional(Vec3::new(-1.0, -1.0, -0.5)))
            .set_position(Vec3::new(5.0, 10.0, 5.0));
        let camera = OrbitCamera3d::default();
        let camera_2d = PanZoomCamera2d::default();
        let camera_buttons = CameraButtons {
            rotate: camera.rotate_button(),
            drag: camera.drag_button(),
            drag_2d: camera_2d.drag_button(),
        };
        Self {
            camera,
            camera_2d,
            scene,
            scene_2d: SceneNode2d::empty(),
            slots: HashMap::new(),
            slots_2d: HashMap::new(),
            tilemap_slots: HashMap::new(),
            emitter_slots: HashMap::new(),
            materials: crate::shader_material::MaterialCache::default(),
            materials_3d: crate::shader_material_3d::MaterialCache3d::default(),
            light_map: crate::light_map::LightMap::new(),
            order_2d: Vec::new(),
            transients: Vec::new(),
            frame: 0,
            keyboard_shown: false,
            camera_buttons,
            device: crate::device::Probe::default(),
            asset_generation: 0,
        }
    }

    /// Whether an asset was reloaded since the last frame drew.
    fn assets_reloaded(&mut self, app: &App) -> bool {
        let now = balaur_core::assets::generation(&app.engine);
        let moved = now != self.asset_generation;
        self.asset_generation = now;
        moved
    }

    /// One frame: apply what scripts asked for, tick, mirror the world into
    /// the scene graph, draw the overlays. Answers whether to keep going.
    fn step(&mut self, app: &mut App, window: &mut Window, dt: f32) -> bool {
        apply_camera(app, &mut self.camera);
        apply_camera_2d(app, &mut self.camera_2d, window);
        apply_camera_input(
            app,
            &mut self.camera,
            &mut self.camera_2d,
            &self.camera_buttons,
        );
        crate::app_icon::apply_app_icon(app);
        apply_window_config(app, window);
        publish_camera(app, &self.camera, window);
        publish_camera_2d(app, &self.camera_2d, window);
        apply_clear_color(app, window);
        apply_post(app, window);
        crate::kiss3d_input::pump_input(app, window);
        self.device.publish(app, window, dt);
        app.advance(dt);
        // Read once for the whole frame: three syncs ask, and each would
        // otherwise see the reload and hide it from the next.
        let reloaded = self.assets_reloaded(app);
        // Before the 2D syncs move nodes around underneath it.
        self.light_map.detach();
        sync(
            app,
            &mut self.scene,
            &mut self.slots,
            &mut self.materials_3d,
            reloaded,
        );
        sync_2d(
            app,
            &mut self.scene_2d,
            &mut self.slots_2d,
            &mut self.order_2d,
            &mut self.materials,
            reloaded,
        );
        crate::tilemap::sync_tilemaps(
            app,
            &mut self.scene_2d,
            &mut self.tilemap_slots,
            &mut self.materials,
            reloaded,
        );
        // The step the frame actually ran, which under --fixed-tick is not
        // the measured one.
        let dt = app.engine.delta();
        crate::particles::sync_particles(
            app,
            window,
            &mut self.scene_2d,
            &mut self.emitter_slots,
            dt,
        );
        // Last of the lit 2D syncs: the composite draws over everything the
        // syncs above put in the scene.
        self.light_map.sync(app, &mut self.scene_2d);
        // Immediate shapes go over the composite, unlit, like debug lines.
        crate::draw_2d::flush(app, window, &mut self.scene_2d, &mut self.transients);
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
pub fn run_windowed(app: App, title: &str) -> anyhow::Result<()> {
    pollster::block_on(run_windowed_async(app, title, None))
}

/// The windowed loop as a future. Native `run_windowed` blocks on it; a
/// browser cannot block, so its entry point spawns it onto the page's event
/// loop instead, and `canvas_id` names the `<canvas>` it draws on — kiss3d's
/// default, `"canvas"`, when `None`.
#[allow(
    clippy::disallowed_methods,
    reason = "the loop driver measures a frame; what it feeds systems is fixed_dt"
)]
pub async fn run_windowed_async(
    mut app: App,
    title: &str,
    canvas_id: Option<&str>,
) -> anyhow::Result<()> {
    // Claim the debug-line buffers: `flush_debug_lines`/`_2d` below drain
    // them as they draw, so the plugin's headless fallback stands down.
    app.engine.insert_resource(WindowedBackend);
    let setup = CanvasSetup {
        canvas_id: canvas_id.unwrap_or("canvas").to_string(),
        ..CanvasSetup::default()
    };
    let mut window = Window::new_with_setup(title, 1600, 1000, setup).await;
    window.set_ime_allowed(true);
    let mut f = Frontend::new();
    let mut last = Instant::now();
    loop {
        // A hidden tab gets no animation frame, so `render` would never
        // return: step the simulation on a timer and draw nothing, so a
        // socket's heartbeats and a fixed tick keep going behind the tab.
        #[cfg(all(target_family = "wasm", not(target_os = "emscripten")))]
        if crate::hidden_tab::is_hidden() {
            crate::kiss3d_input::pump_input(&app, &window);
            let now = Instant::now();
            let dt = (now - last).as_secs_f32().min(0.1);
            last = now;
            app.advance(dt);
            if app.engine.quit_requested() {
                break;
            }
            crate::hidden_tab::sleep().await;
            continue;
        }
        let open = window
            .render(
                Some(&mut f.scene),
                Some(&mut f.scene_2d),
                Some(&mut f.camera),
                Some(&mut f.camera_2d),
                None,
                None,
            )
            .await;
        if !open {
            break;
        }
        let now = Instant::now();
        let dt = (now - last).as_secs_f32().min(0.1);
        last = now;
        if !f.step(&mut app, &mut window, dt) {
            break;
        }
    }
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

/// Apply the screen-space effects the current `camera` asked for.
///
/// Only on the edge: kiss3d rebuilds its post chain when one of these
/// switches, so re-asserting them every frame would rebuild it every frame.
fn apply_post(app: &App, window: &mut Window) {
    let Some(post) = app.engine.try_resource::<PostConfig>() else {
        return;
    };
    let mut post = post.borrow_mut();
    if !post.changed {
        return;
    }
    post.changed = false;
    window.set_bloom_enabled(post.bloom);
    window.set_bloom(post.bloom_threshold, post.bloom_intensity);
    window.set_ssao_enabled(post.ssao);
    window.set_ssr_enabled(post.ssr);
    window.set_dof_enabled(post.dof);
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
    crate::device::keep_awake(config.keep_awake);
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
    reloaded: bool,
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
        // A reload rebuilds what was built from a file: the mesh is read
        // again and the texture uploaded under the new generation's name.
        let from_file = renderable.mesh.is_some() || !renderable.texture.is_empty();
        let rebuild = match slots.get(&entity) {
            Some(slot) => {
                slot.version != renderable.version
                    || channel_changed
                    || (relinked && !renderable.material.is_empty())
                    || (reloaded && from_file)
            }
            None => true,
        };
        if rebuild {
            if let Some(mut old) = slots.remove(&entity) {
                old.node.remove();
            }
            let (mut node, skin, geometry) = match renderable.shape {
                Shape::Ball { radius } => (scene.add_sphere(radius), None, None),
                Shape::Cuboid { hx, hy, hz } => {
                    (scene.add_cube(2.0 * hx, 2.0 * hy, 2.0 * hz), None, None)
                }
                Shape::Capsule { radius, height } => {
                    (scene.add_capsule(radius, height), None, None)
                }
                Shape::Cylinder { radius, height } => {
                    (scene.add_cylinder(radius, height), None, None)
                }
                Shape::Cone { radius, height } => (scene.add_cone(radius, height), None, None),
                // One quad, no subdivisions: a flat plane needs no interior
                // vertices, and fewer of them is fewer to transform.
                Shape::Plane { hx, hz } => (scene.add_quad(2.0 * hx, 2.0 * hz, 1, 1), None, None),
                Shape::Mesh => match upload_mesh(app, scene, renderable) {
                    Some(built) => built,
                    // Nothing to draw yet, and `upload_mesh` said why.
                    None => continue,
                },
            };
            // After the texture: a material reads it, and kiss3d's own
            // material stays on a node whose shader would not link.
            let custom = materials.for_node(app, &renderable.material, &channel);
            let mut palette = None;
            if let Some(material) = custom {
                node.set_material(material);
            } else if let Some(mesh) = geometry {
                palette = Some(crate::skinned_3d::attach(&mut node, &mesh));
            }
            slots.insert(
                entity,
                Slot {
                    node,
                    version: renderable.version,
                    skin,
                    palette,
                },
            );
        }
        // The block above inserts the slot when it is missing.
        let slot = slots.get_mut(&entity).unwrap();
        if let Some(skin) = &slot.skin {
            pose_mesh(&world, entity, skin, slot.palette.as_ref(), &mut slot.node);
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
        let visible = world
            .get::<&GlobalAppearance>(entity)
            .is_ok_and(|a| a.visible);
        slot.node
            .set_pose(Pose3::from_parts(global.position, global.rotation))
            .set_local_scale(scale.x, scale.y, scale.z)
            .set_color(Color::new(r, g, b, a))
            .set_visible(visible);
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
) -> Option<(
    SceneNode3d,
    Option<MeshSkinSlot>,
    Option<crate::skinned_3d::SkinnedMesh3d>,
)> {
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
    // The geometry the skinning material draws from, kept before the skin is
    // moved into the slot. Nothing to skin with no triangles, and an empty
    // vertex buffer is one wgpu refuses to create.
    let geometry = data
        .skin
        .as_ref()
        .filter(|_| !coords.is_empty() && !faces.is_empty())
        .map(|skin| crate::skinned_3d::SkinnedMesh3d {
            positions: coords.clone(),
            normals: normals
                .clone()
                .unwrap_or_else(|| GpuMesh3d::compute_normals_array(&coords, &faces)),
            uvs: uvs.clone().unwrap_or_default(),
            joints: skin.joints.clone(),
            weights: skin.weights.clone(),
            indices: faces.clone(),
        });
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
    crate::texture::attach_texture_3d(&app.engine, &mut node, &renderable.texture);
    Some((node, skin, geometry))
}

/// Pose a skinned mesh for this frame from the rig's joint matrices: handed
/// to the skinning material when there is one, and otherwise pushed through
/// the rest vertices on the CPU and written back into the node's buffers.
///
/// A rig or bone path that resolves to nothing is logged at debug and the
/// mesh draws at rest, the same as a clip track that targets no node.
fn pose_mesh(
    world: &balaur_core::hecs::World,
    entity: Entity,
    skin: &MeshSkinSlot,
    handle: Option<&crate::skinned_3d::SkinHandle3d>,
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
    // The vertex shader blends the same matrices, so the CPU work below is
    // exactly what the GPU path saves.
    if let Some(handle) = handle {
        handle.set(palette);
        return;
    }
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
fn build_2d_node(scene: &mut SceneNode2d, renderable: &Renderable2d) -> Option<SceneNode2d> {
    // Unit primitives: like the 3D path, dimensions live in the node's local
    // scale, which the caller updates every frame.
    Some(match renderable.shape {
        Shape2d::Circle { .. } => scene.add_circle(0.5),
        Shape2d::Capsule { radius, height } => scene.add_capsule(radius, height),
        Shape2d::Rect { .. } | Shape2d::Sprite { .. } => scene.add_rectangle(1.0, 1.0),
        // Built by `build_polyline_node` and `build_polygon_node`, which also
        // hand back the pieces and the palette.
        Shape2d::Polyline { .. } | Shape2d::Polygon => return None,
    })
}

/// A polyline as a group of triangle strips with round joins: one piece per
/// segment and joint, each with its place along the chain for the gradient.
/// The points come from the same mesh asset physics reads, flattened to xy.
fn build_polyline_node(
    app: &App,
    scene: &mut SceneNode2d,
    renderable: &Renderable2d,
    width: f32,
    closed: bool,
) -> Option<(SceneNode2d, Vec<(SceneNode2d, f32)>)> {
    let points = polyline_points(app, renderable.polyline.as_deref(), false);
    if points.len() < 2 {
        return None;
    }
    let texture = renderable
        .line
        .as_ref()
        .map(|style| style.texture.as_str())
        .unwrap_or_default();
    let mut group = scene.add_group();
    let mut pieces = Vec::new();
    for piece in crate::polyline_strip::pieces(&points, width, closed) {
        let mesh =
            kiss3d::resource::GpuMesh2d::new(piece.coords, piece.faces, Some(piece.uvs), false);
        let mut node = group.add_mesh(std::rc::Rc::new(std::cell::RefCell::new(mesh)), Vec2::ONE);
        if !texture.is_empty() {
            crate::texture::attach_texture_2d(&app.engine, &mut node, texture);
        }
        pieces.push((node, piece.along));
    }
    Some((group, pieces))
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
    crate::texture::attach_texture_2d(&app.engine, &mut node, &polygon.texture);
    Some((node, skin))
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
/// The 2D nodes in the order they draw, detaching the slots that no longer
/// sit in it so the caller rebuilds those, and only those, in the new one.
///
/// kiss3d appends children and its `detach` is a `swap_remove`, so only a
/// suffix can be re-ordered: what the old and new orders share up front keeps
/// its places, and the rest is dropped last-first and rebuilt by appending.
fn draw_order_2d(
    world: &balaur_core::hecs::World,
    root: Entity,
    slots: &mut HashMap<Entity, Slot2d>,
    order_cache: &mut Vec<Entity>,
) -> Vec<Entity> {
    let mut desired: Vec<(i32, f32, Entity)> = Vec::new();
    for entity in balaur_core::scene::collect_subtree(world, root) {
        if world.get::<&Renderable2d>(entity).is_ok() {
            let z = world
                .get::<&GlobalTransform>(entity)
                .map_or(0.0, |g| g.position.z);
            let layer = world
                .get::<&GlobalAppearance>(entity)
                .map_or(0, |a| a.z_index);
            desired.push((layer, z, entity));
        }
    }
    // A hidden node keeps its place: visibility is a flag on the kiss3d node,
    // so toggling one never reshuffles the order and rebuilds its neighbours.
    desired.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    let order: Vec<Entity> = desired.iter().map(|&(_, _, e)| e).collect();
    let kept = order
        .iter()
        .zip(order_cache.iter())
        .take_while(|(now, before)| now == before)
        .count();
    // Last first, so each detach is a pop and the kept prefix stays put.
    for entity in order_cache[kept..].iter().rev() {
        if let Some(mut slot) = slots.remove(entity) {
            slot.node.detach();
        }
    }
    order_cache.clone_from(&order);
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
    reloaded: bool,
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
        // A sprite's image and a polyline's mesh are both files; a reload
        // re-reads them, as it does in three dimensions.
        let from_file = renderable.sprite.is_some() || renderable.polyline.is_some();
        let rebuild = match slots.get(&entity) {
            Some(slot) => {
                slot.version != renderable.version
                    || channel_changed
                    || (relinked && !renderable.material.is_empty())
                    || (reloaded && from_file)
            }
            None => true,
        };
        if rebuild {
            if let Some(mut old) = slots.remove(&entity) {
                old.node.detach();
            }
            let mut pieces = Vec::new();
            let built = match renderable.shape {
                Shape2d::Polygon => build_polygon_node(app, scene, &renderable),
                Shape2d::Polyline { width, closed } => {
                    build_polyline_node(app, scene, &renderable, width, closed).map(
                        |(node, built)| {
                            pieces = built;
                            (node, None)
                        },
                    )
                }
                _ => build_2d_node(scene, &renderable).map(|node| (node, None)),
            };
            let Some((mut node, skin)) = built else {
                continue;
            };
            if let Some(sprite) = &renderable.sprite {
                crate::texture::attach_texture_2d(&app.engine, &mut node, &sprite.path);
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
                    pieces,
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
        tint_pieces(slot, &renderable);
        // Every sync, not just on rebuild: frames and flips are UV changes, so
        // an animation that flips frames must not rebuild the node.
        if let Some(sprite) = &renderable.sprite {
            let flip = (sprite.flip_x, sprite.flip_y);
            if sprite.sheet.is_some() || sprite.region.is_some() || flip != slot.flip {
                sync_sprite_uvs(&mut slot.node, sprite);
            }
            slot.flip = flip;
        }
        let (angle, _, _) = global.rotation.to_euler(glamx::EulerRot::ZYX);
        let visible = world
            .get::<&GlobalAppearance>(entity)
            .is_ok_and(|a| a.visible);
        slot.node
            .set_position(Vec2::new(global.position.x, global.position.y))
            .set_rotation(angle)
            .set_local_scale(size.x * global.scale.x, size.y * global.scale.y)
            .set_color(Color::new(r, g, b, a))
            .set_visible(visible);
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

/// A polyline's pieces carry their own colour: the tint blended toward the
/// gradient by where each sits, since a group's colour stops at the group.
fn tint_pieces(slot: &mut Slot2d, renderable: &Renderable2d) {
    let [r, g, b, a] = renderable.color;
    let end = renderable
        .line
        .as_ref()
        .and_then(|style| style.gradient)
        .unwrap_or(renderable.color);
    for (piece, along) in &mut slot.pieces {
        let t = *along;
        piece.set_color(Color::new(
            r + (end[0] - r) * t,
            g + (end[1] - g) * t,
            b + (end[2] - b) * t,
            a + (end[3] - a) * t,
        ));
    }
}

/// Remap the node's UVs to the sprite's sheet frame, with the U or V extents
/// swapped for flips.
fn sync_sprite_uvs(node: &mut SceneNode2d, sprite: &SpriteTexture) {
    let sheet = sprite
        .sheet
        .map(|s| kiss3d::scene::SpriteSheet::new(s.columns, s.rows));
    let (mut min, mut max) = sheet.map_or((Vec2::ZERO, Vec2::ONE), |s| s.frame_uv(sprite.frame));
    // A region is a rectangle of the image in pixels; it wins over a sheet.
    if let Some([x, y, w, h]) = sprite.region {
        let size = node.data().object().map(|o| o.data().texture().size);
        if let Some((tw, th)) = size.filter(|(tw, th)| *tw > 0 && *th > 0) {
            let (tw, th) = (tw as f32, th as f32);
            min = Vec2::new(x as f32 / tw, y as f32 / th);
            max = Vec2::new((x + w) as f32 / tw, (y + h) as f32 / th);
        }
    }
    if sheet.is_some() && sprite.region.is_none() {
        // The sliver keeps a nearest-sampled edge fragment from rounding into
        // the neighbouring frame, matching kiss3d's own `set_sprite_frame`.
        let size = node.data().object().map(|o| o.data().texture().size);
        if let Some((w, h)) = size
            && w > 1
            && h > 1
        {
            let inset = Vec2::new(0.05 / w as f32, 0.05 / h as f32);
            min += inset;
            max -= inset;
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
