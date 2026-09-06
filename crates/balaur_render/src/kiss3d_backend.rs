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
    ClearColorConfig, GridConfig, PostConfig, Renderable, Renderable2d, ScreenshotRequest, Shape,
    Shape2d, WindowConfig, WindowedBackend,
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

pub(crate) struct Slot2d {
    pub(crate) node: SceneNode2d,
    pub(crate) version: u64,
    /// The flip pair last written into the node's UVs: sheetless sprites only
    /// touch UVs when it changes, so un-flipping writes the identity rect once.
    pub(crate) flip: (bool, bool),
    /// A skinned polygon's joint palette, rewritten every frame from the rig.
    pub(crate) skin: Option<crate::skinned_2d::SkinHandle>,
    /// A polygon's vertex buffer, for the frames a `polygon/deform` track
    /// has moved its vertices off the mesh's authored positions.
    pub(crate) deform: Option<crate::skinned_2d::DeformHandle>,
    /// Whether the last frame wrote a deform, so returning to rest uploads
    /// the authored positions once instead of every frame after.
    pub(crate) deformed: bool,
    /// A polyline's pieces with where along the chain each sits, so a
    /// gradient can colour them every frame under the node's tint.
    pub(crate) pieces: Vec<(SceneNode2d, f32)>,
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
    text: crate::world_text::Frame,
    frame: u64,
    /// Whether the on-screen keyboard was summoned last frame, so it is
    /// shown/hidden on the edge rather than re-requested every frame.
    keyboard_shown: bool,
    /// The cameras' drag bindings as built: taking the pointer away from the
    /// camera unbinds them, and these are what it puts back.
    camera_buttons: CameraButtons,
    /// What the display says, measured frame by frame.
    device: crate::device::Probe,
    /// One node per authored `light3d`, and the default sun they retire.
    lights: crate::light3d::LightSlots,
    /// The environment last pushed to the window, so an unchanged one costs
    /// no sky decode and no shadow-map resize.
    environment: Option<crate::light3d::Environment>,
    /// The asset generation the nodes below were built at. A saved texture,
    /// model or tileset moves it, and every node built from a file is built
    /// again — the material caches watch the same counter for their shaders.
    asset_generation: u64,
}

impl Frontend {
    fn new() -> Self {
        let mut scene = SceneNode3d::empty();
        let mut lights = crate::light3d::LightSlots::default();
        let mut sun = scene.add_light(Light::directional(Vec3::new(-1.0, -1.0, -0.5)));
        sun.set_position(Vec3::new(5.0, 10.0, 5.0));
        // Kept rather than forgotten: the first authored `light3d` hides it,
        // and a scene that removes its lights gets it back.
        lights.adopt_sun(sun);
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
            lights,
            environment: None,
            order_2d: Vec::new(),
            transients: Vec::new(),
            text: crate::world_text::Frame::default(),
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
        let input_seen = crate::kiss3d_input::pump_input(app, window);
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
        self.lights.sync(app, &mut self.scene);
        crate::light3d::sync_environment(app, window, &mut self.environment);
        crate::sync_2d::sync_2d(
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
        let tall = window.height() as f32;
        crate::world_text::draw(
            app,
            &mut self.scene_2d,
            &mut self.scene,
            &mut self.text,
            tall,
        );
        draw_grid(app, window);
        crate::debug_lines::flush_debug_lines(app, window);
        crate::debug_lines::flush_debug_lines_2d(app, window);
        // A lazy UI skips the pass; the last one's shapes are drawn again.
        if balaur_ui::wants_pass(&app.engine, window.egui_context(), input_seen) {
            window.draw_ui(|ctx| balaur_ui::run_pass(&app.engine, ctx));
        }
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
    balaur_ui::honour_lazy(&app.engine);
    // `[window]` in project.toml, or its defaults when a project says
    // nothing. Read before the window exists, so it cannot come from a
    // resource the first frame inserts.
    let window_settings = app
        .manifest()
        .map(|manifest| manifest.window.clone())
        .unwrap_or_default();
    let setup = CanvasSetup {
        canvas_id: canvas_id.unwrap_or("canvas").to_string(),
        vsync: window_settings.vsync,
        samples: NumSamples::from_u32(window_settings.msaa).unwrap_or_else(|| {
            tracing::warn!(
                "project.toml asks for msaa = {}; this renderer offers 1 or 4, using 4",
                window_settings.msaa
            );
            NumSamples::Four
        }),
        ..CanvasSetup::default()
    };
    let mut window =
        Window::new_with_setup(title, window_settings.width, window_settings.height, setup).await;
    if window_settings.fullscreen {
        // Seed the state a script's own toggle drives, so `apply_window_config`
        // puts the window up on the first frame through one path.
        app.engine.insert_resource(WindowConfig {
            fullscreen: true,
            changed: true,
            ..WindowConfig::default()
        });
    }
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
                // Built by the mesher rather than by kiss3d: the triangles a
                // collider is fitted to and a ray is picked against are the
                // ones uploaded here.
                Shape::Solid(solid) => (upload_geometry(scene, &solid.build()), None, None),
                // A boolean's result, already worked out this tick.
                Shape::Built => match renderable.built.as_deref() {
                    Some(mesh) if !mesh.indices.is_empty() => {
                        (upload_geometry(scene, mesh), None, None)
                    }
                    _ => continue,
                },
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
        // Every shape is real geometry at its authored size now, so the node
        // carries the scene's scale and nothing of the shape's.
        let scale = global.scale;
        let visible = world
            .get::<&GlobalAppearance>(entity)
            .is_ok_and(|a| a.visible);
        slot.node
            .set_pose(Pose3::from_parts(global.position, global.rotation))
            .set_local_scale(scale.x, scale.y, scale.z)
            .set_color(Color::new(r, g, b, a))
            .set_visible(visible)
            .set_casts_shadows(renderable.shadows)
            .set_light_layers(renderable.layers);
        // How far the mesh is blended towards each of its shapes, this tick.
        if let Ok(morphs) = world.get::<&crate::MorphWeights>(entity) {
            slot.node.set_morph_weights(&morphs.weights);
        }
        // A cloner above this node turns it into one draw of many copies.
        let clones = world.get::<&crate::Clones>(entity).ok();
        crate::instancing::set_instances_3d(&mut slot.node, clones.as_deref(), global);
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

/// Hand a mesh's triangles to kiss3d as a static node. Normals and UVs are
/// optional in the format; kiss3d computes normals from the faces when they
/// are absent, which is the right answer for a bare OBJ.
fn upload_geometry(scene: &mut SceneNode3d, data: &balaur_core::mesh::MeshData) -> SceneNode3d {
    let coords: Vec<Vec3> = data
        .positions
        .iter()
        .map(|p| Vec3::from_array(*p))
        .collect();
    let normals = data
        .normals
        .as_ref()
        .map(|ns| ns.iter().map(|n| Vec3::from_array(*n)).collect());
    let uvs = data
        .uvs
        .as_ref()
        .map(|us| us.iter().map(|u| Vec2::from_array(*u)).collect());
    let mut gpu = GpuMesh3d::new(coords, data.indices.clone(), normals, uvs, false);
    if let Some(colors) = &data.colors {
        gpu.set_colors(colors.clone());
    }
    scene.add_mesh(std::rc::Rc::new(std::cell::RefCell::new(gpu)), Vec3::ONE)
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
    // The shapes and the colours before the skin is moved out of the data.
    let morphs = crate::morph::targets_of(&data);
    let colors = data.colors.clone();
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
    let mut gpu = GpuMesh3d::new(coords, faces, normals, uvs, skin.is_some());
    if let Some(targets) = morphs {
        gpu.set_morph_targets(targets);
    }
    if let Some(colors) = colors {
        gpu.set_colors(colors);
    }
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
pub(crate) fn build_2d_node(
    scene: &mut SceneNode2d,
    renderable: &Renderable2d,
) -> Option<SceneNode2d> {
    Some(match renderable.shape {
        // Real geometry from the mesher, at its authored size, so a star and
        // a circle arrive by the same path.
        Shape2d::Flat(flat) => {
            let mesh = flat.build();
            let coords: Vec<Vec2> = mesh
                .positions
                .iter()
                .map(|p| Vec2::new(p[0], p[1]))
                .collect();
            let uvs = mesh
                .uvs
                .as_ref()
                .map(|us| us.iter().map(|u| Vec2::from_array(*u)).collect());
            let gpu = kiss3d::resource::GpuMesh2d::new(coords, mesh.indices, uvs, false);
            scene.add_mesh(std::rc::Rc::new(std::cell::RefCell::new(gpu)), Vec2::ONE)
        }
        // A sprite is a unit quad the caller scales, because its size comes
        // from the image and changes without rebuilding the node.
        Shape2d::Sprite { .. } => scene.add_rectangle(1.0, 1.0),
        // Built by `build_polyline_node` and `build_polygon_node`, which also
        // hand back the pieces and the palette.
        Shape2d::Polyline { .. } | Shape2d::Polygon => return None,
    })
}

/// A polyline as a group of triangle strips with round joins: one piece per
/// segment and joint, each with its place along the chain for the gradient.
/// The points come from the same mesh asset physics reads, flattened to xy.
pub(crate) fn build_polyline_node(
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
/// The joint matrices a skinned polygon deforms by this frame, resolved
/// from the rig it names. A path that resolves to nothing is logged at
/// debug and the polygon draws rigid, the same as a clip track that targets
/// no node.
pub(crate) fn polygon_palette(
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

/// A `path2d` asset sampled into points, or `None` when the reference names
/// no path -- which is a mesh reference and not a mistake.
fn sampled_path(app: &App, reference: &str) -> Option<Vec<Vec2>> {
    let path = balaur_core::assets::load_typed::<balaur_core::path::Path2d>(&app.engine, reference)
        .ok()?;
    match path.sample(balaur_core::path::TOLERANCE) {
        Ok(points) => Some(points.into_iter().map(|p| Vec2::new(p.x, p.y)).collect()),
        Err(err) => {
            tracing::error!("path '{reference}': {err:#}");
            Some(Vec::new())
        }
    }
}

/// A `mesh` asset's vertices, flattened to xy. `None` when it will not load,
/// which is logged rather than fatal.
fn mesh_points(app: &App, reference: &str) -> Option<Vec<Vec2>> {
    let loaded =
        balaur_core::assets::load_typed::<balaur_core::mesh::MeshData>(&app.engine, reference)
            .and_then(|definition| balaur_core::mesh::load_from(&app.engine, &definition));
    match loaded {
        Ok(data) => Some(
            data.positions
                .iter()
                .map(|p| Vec2::new(p[0], p[1]))
                .collect(),
        ),
        Err(err) => {
            tracing::error!("polyline '{reference}': {err:#}");
            None
        }
    }
}

/// A polyline's points from the `path2d` or `mesh` asset it names, flattened
/// to xy. Empty when neither loads, which the caller treats as nothing to draw.
fn polyline_points(app: &App, reference: Option<&str>, closed: bool) -> Vec<Vec2> {
    let Some(reference) = reference.filter(|r| !r.is_empty()) else {
        return Vec::new();
    };
    // A `path2d` first: a stroked curve names one, and a traced outline names
    // a mesh. Both end as the same chain of points.
    let Some(mut points) = sampled_path(app, reference).or_else(|| mesh_points(app, reference))
    else {
        return Vec::new();
    };
    // A closed chain repeats its first point rather than carrying a flag: the
    // renderer draws segments, and the join is just one more of them.
    if closed && points.len() > 2 {
        points.push(points[0]);
    }
    points
}
