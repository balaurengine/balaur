//! Rendering as a Balaur plugin.
//!
//! The plugin itself is backend-agnostic: it owns the `Renderable` component,
//! the `render` script module, and its scene-file component keys. Backends
//! consume `Renderable` + `GlobalTransform`. The kiss3d/wgpu backend (feature
//! `kiss3d`) owns the OS event loop; headless runs just never draw, which
//! keeps the simulation byte-for-byte identical with and without a window.

use anyhow::{anyhow, Result};
use balaur_core::components::ComponentDef;
use balaur_core::entity_of;
use balaur_core::hecs::Entity;
use balaur_core::{App, Engine, Plugin, Stage};
use balaur_script::{Bindings, BindingsExt, NodeId};

#[cfg(feature = "kiss3d")]
pub mod kiss3d_backend;

/// Ask a rendering backend to save one frame as a PNG once `after_frame`
/// frames have been rendered (debugging, CI golden images, editor
/// thumbnails). Insert as an engine resource; the backend removes it after
/// taking the shot.
///
/// Usually inserted by `render.screenshot(path)`; public so a Rust embedding
/// can insert one directly. Served by the windowed backend and by the
/// offscreen one — a screenshot needs a GPU, not a window. It is *not* served
/// by a headless run, which builds no renderer at all; see
/// [`warn_if_unserved`], which is why asking for one there says so instead of
/// exiting cleanly with no file.
pub struct ScreenshotRequest {
    pub path: std::path::PathBuf,
    pub after_frame: u64,
}

/// Complain about a screenshot nobody could take.
///
/// A request that no backend consumes used to leave no file, no message and a
/// zero exit code, which reads exactly like success to a script. Called by the
/// headless runner once the frame budget is spent — the one path where a
/// `render.screenshot` call can go unanswered.
pub fn warn_if_unserved(eng: &Engine) {
    let Some(request) = eng.try_resource::<ScreenshotRequest>() else {
        return;
    };
    let path = request.borrow().path.clone();
    tracing::error!(
        "no screenshot written to {}: this run has no renderer. Use --offscreen \
         (or a windowed build) — a screenshot needs a GPU, not a window.",
        path.display()
    );
}

/// Inserted by a windowed backend at startup to claim the debug-line
/// buffers: it drains them itself as it draws, so [`RenderPlugin`]'s
/// headless fallback drain stands down while this is present.
pub struct WindowedBackend;

/// The application (dock/taskbar) icon, applied by windowed backends.
pub struct AppIconConfig {
    pub path: std::path::PathBuf,
    pub changed: bool,
}

/// Fullscreen and cursor state scripts asked for, applied by windowed
/// backends when changed. Headless runs hold the values and touch nothing,
/// so a game that grabs the cursor still ticks identically in CI.
// Three independent switches plus the dirty flag — a state enum would invent
// coupling these do not have.
#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
pub struct WindowConfig {
    pub fullscreen: bool,
    pub cursor_grabbed: bool,
    pub cursor_hidden: bool,
    pub changed: bool,
}

/// Viewport clear color, applied by windowed backends when changed.
pub struct ClearColorConfig {
    pub color: [f32; 3],
    pub changed: bool,
}

/// Ground grid drawn by windowed backends: minor/major line spacing in world
/// units, extent in lines, colors from the caller (the editor themes it).
pub struct GridConfig {
    pub enabled: bool,
    pub step: f32,
    pub major_every: u32,
    pub extent: i32,
    pub minor_color: [f32; 3],
    pub major_color: [f32; 3],
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            step: 1.0,
            major_every: 5,
            extent: 20,
            minor_color: [0.12, 0.13, 0.15],
            major_color: [0.17, 0.19, 0.22],
        }
    }
}

/// Script arguments for `draw_line`: two endpoints, colour, then optional
/// width, perspective-correct width and always-on-top flags.
type DrawLineArgs = (
    f32,
    f32,
    f32,
    f32,
    f32,
    f32,
    f32,
    f32,
    f32,
    Option<f32>,
    Option<bool>,
    Option<bool>,
);

/// (a, b, color, pixel width, perspective-correct width, always-on-top)
pub type DebugLine = ([f32; 3], [f32; 3], [f32; 3], f32, bool, bool);

/// Scripts append with `render.draw_line`; the windowed backend drains it as
/// it draws, and with no window [`RenderPlugin`]'s Render-stage system
/// clears it instead, so it is empty at the start of every frame either way.
#[derive(Default)]
pub struct DebugLineBuffer {
    pub lines: Vec<DebugLine>,
}

/// 2D counterpart of [`DebugLineBuffer`]: world-space 2D segments rendered
/// with the 2D camera.
/// (a, b, color, pixel width)
pub type DebugLine2d = ([f32; 2], [f32; 2], [f32; 3], f32);

/// Appended by `render.draw_line_2d`, drained on the same terms as
/// [`DebugLineBuffer`].
#[derive(Default)]
pub struct DebugLineBuffer2d {
    pub lines: Vec<DebugLine2d>,
}

/// Where the 2D camera looks (world center) and its zoom in logical pixels
/// per world unit. Backends apply it when changed and keep their own
/// interactive pan/zoom in between.
pub struct CameraConfig2d {
    pub center: [f32; 2],
    pub zoom: f32,
    pub changed: bool,
}

impl Default for CameraConfig2d {
    fn default() -> Self {
        Self {
            center: [0.0, 0.0],
            zoom: 60.0,
            changed: false,
        }
    }
}

/// The actual 2D camera state this frame, published by windowed backends
/// (zoom in logical pixels per world unit), plus the mouse in 2D world
/// coordinates for script-side picking. Read-only: write
/// [`CameraConfig2d`] instead.
///
/// With no windowed backend running nothing ever publishes into it and it
/// keeps its `Default`, which is all zeros — including `zoom`, where
/// [`CameraConfig2d::default`] says 60.0. So a headless `render.camera_2d()`
/// returns `(0, 0, 0)`, and callers that divide by the zoom must clamp it.
#[derive(Default)]
pub struct ViewportSnapshot2d {
    pub center: [f32; 2],
    pub zoom: f32,
    pub mouse_world: [f32; 2],
}

/// The actual camera pose this frame, published by windowed backends so
/// tools (editor gizmos, pickers) can do screen-space math in scripts.
/// Read-only: write [`CameraConfig`] instead.
///
/// With no windowed backend running it keeps its `Default` — an all-zero
/// pose, a zero `fov`, a zero `scale_factor` and an all-zero `view_proj`,
/// which is not invertible. Headless screen-space math gets zeros, not the
/// camera the scene would have had.
#[derive(Default)]
pub struct ViewportSnapshot {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    /// Vertical field of view, radians.
    pub fov: f32,
    /// OS pixels per logical point (HiDPI factor).
    pub scale_factor: f32,
    /// The camera's exact projection*view matrix, column-major.
    pub view_proj: [f32; 16],
    /// Picking ray through the current mouse position.
    pub ray_origin: [f32; 3],
    pub ray_dir: [f32; 3],
}

/// When `enabled` is false, windowed backends inhibit the camera's mouse
/// controls (editors take the pointer over for gizmo drags).
pub struct CameraInputConfig {
    pub enabled: bool,
}

/// Where the camera looks from and at. Scripts drive it through
/// `render.set_camera`; windowed backends apply it whenever it changes (and
/// keep their own interactive controls, e.g. kiss3d's orbit drag, in
/// between).
pub struct CameraConfig {
    pub eye: glamx::Vec3,
    pub target: glamx::Vec3,
    pub changed: bool,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            eye: glamx::Vec3::new(8.0, 5.0, 12.0),
            target: glamx::Vec3::new(0.0, 1.0, 0.0),
            changed: true,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Shape {
    Ball { radius: f32 },
    Cuboid { hx: f32, hy: f32, hz: f32 },
}

pub struct Renderable {
    pub shape: Shape,
    pub color: [f32; 4],
    /// Bumped when `shape` changes so backends know to rebuild their node.
    pub version: u64,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Shape2d {
    Circle {
        radius: f32,
    },
    Rect {
        hx: f32,
        hy: f32,
    },
    /// A textured quad. Its half-extents are resolved when the sprite is set,
    /// so they are simulation state a script can read back, the same as a rect.
    Sprite {
        hx: f32,
        hy: f32,
    },
}

/// Pixels of texture per world unit when a sprite is sized from its image.
///
/// The 2D camera's zoom is logical pixels per world unit, so this is the other
/// half of that conversion: at the default, a 100px image is one unit across.
pub const DEFAULT_PIXELS_PER_UNIT: f32 = 100.0;

/// A grid of equally sized frames packed into one texture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SpriteSheet2d {
    pub columns: u32,
    pub rows: u32,
}

/// What a `Shape2d::Sprite` draws.
#[derive(Clone, PartialEq, Eq)]
pub struct SpriteTexture {
    /// Project-relative (or absolute) path to the image.
    pub path: String,
    /// Set when the texture holds a grid of frames rather than one image.
    pub sheet: Option<SpriteSheet2d>,
    /// Which frame of `sheet` to draw. Ignored without a sheet.
    pub frame: u32,
}

/// 2D renderable, mirrored into the backend's 2D scene. The node's regular
/// `Transform` drives it: x/y translate, the z rotation spins it, x/y scale.
pub struct Renderable2d {
    pub shape: Shape2d,
    pub color: [f32; 4],
    /// Present exactly when `shape` is a sprite.
    pub sprite: Option<SpriteTexture>,
    pub version: u64,
}

fn set_shape(eng: &Engine, entity: Entity, shape: Shape) -> Result<()> {
    let mut world = eng.world_mut();
    if let Ok(mut r) = world.get::<&mut Renderable>(entity) {
        r.shape = shape;
        r.version += 1;
        return Ok(());
    }
    world
        .insert_one(
            entity,
            Renderable {
                shape,
                color: [0.8, 0.8, 0.8, 1.0],
                version: 0,
            },
        )
        .map_err(|_| anyhow!("node is dead"))
}

fn set_shape2d(eng: &Engine, entity: Entity, shape: Shape2d) -> Result<()> {
    let mut world = eng.world_mut();
    if let Ok(mut r) = world.get::<&mut Renderable2d>(entity) {
        r.shape = shape;
        r.version += 1;
        return Ok(());
    }
    world
        .insert_one(
            entity,
            Renderable2d {
                shape,
                color: [0.8, 0.8, 0.8, 1.0],
                sprite: None,
                version: 0,
            },
        )
        .map_err(|_| anyhow!("node is dead"))
}

/// Half-extents of one frame of `path`, read from the image header.
///
/// Not behind the windowed feature: the size lands in a component a script can
/// read, so a headless run has to compute the same number a windowed one does.
fn natural_half_extents(
    path: &std::path::Path,
    sheet: Option<SpriteSheet2d>,
    ppu: f32,
) -> Result<(f32, f32)> {
    let (w, h) = image::image_dimensions(path)
        .map_err(|e| anyhow!("reading the size of {}: {e}", path.display()))?;
    let (cols, rows) = sheet.map_or((1, 1), |s| (s.columns.max(1), s.rows.max(1)));
    let ppu = if ppu > 0.0 {
        ppu
    } else {
        DEFAULT_PIXELS_PER_UNIT
    };
    Ok((
        (w as f32 / cols as f32) / ppu / 2.0,
        (h as f32 / rows as f32) / ppu / 2.0,
    ))
}

/// Point a node at a texture, sizing the quad unless the caller said otherwise.
fn set_sprite(
    eng: &Engine,
    entity: Entity,
    texture: SpriteTexture,
    half_extents: Option<(f32, f32)>,
    ppu: f32,
) -> Result<()> {
    let (hx, hy) = if let Some(explicit) = half_extents {
        explicit
    } else {
        let full = resolve_project_path(eng, &texture.path);
        natural_half_extents(&full, texture.sheet, ppu)?
    };
    let shape = Shape2d::Sprite {
        hx: hx.max(f32::EPSILON),
        hy: hy.max(f32::EPSILON),
    };
    let mut world = eng.world_mut();
    if let Ok(mut r) = world.get::<&mut Renderable2d>(entity) {
        // The frame alone changes UVs, which the backend re-applies every
        // sync; anything else means the backend's node has to be rebuilt.
        let rebuild = r.shape != shape
            || r.sprite
                .as_ref()
                .is_none_or(|s| s.path != texture.path || s.sheet != texture.sheet);
        r.shape = shape;
        r.sprite = Some(texture);
        if rebuild {
            r.version += 1;
        }
        return Ok(());
    }
    world
        .insert_one(
            entity,
            Renderable2d {
                shape,
                color: [1.0, 1.0, 1.0, 1.0],
                sprite: Some(texture),
                version: 0,
            },
        )
        .map_err(|_| anyhow!("node is dead"))
}

/// Tint whichever renderable(s) the node carries (3D and/or 2D).
fn set_color(eng: &Engine, entity: Entity, color: [f32; 4]) -> Result<()> {
    let world = eng.world_mut();
    let mut any = false;
    if let Ok(mut r) = world.get::<&mut Renderable>(entity) {
        r.color = color;
        any = true;
    }
    if let Ok(mut r) = world.get::<&mut Renderable2d>(entity) {
        r.color = color;
        any = true;
    }
    if !any {
        return Err(anyhow!("node has no shape yet"));
    }
    Ok(())
}

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn name(&self) -> &'static str {
        "render"
    }

    fn build(&mut self, app: &mut App) -> Result<()> {
        app.engine.insert_resource(CameraConfig::default());
        app.engine.insert_resource(ClearColorConfig {
            color: [0.09, 0.098, 0.11],
            changed: true,
        });
        app.engine.insert_resource(WindowConfig::default());
        app.engine.insert_resource(GridConfig::default());
        app.engine.insert_resource(DebugLineBuffer::default());
        app.engine.insert_resource(DebugLineBuffer2d::default());
        app.engine.insert_resource(CameraConfig2d::default());
        app.engine.insert_resource(ViewportSnapshot2d::default());
        app.engine.insert_resource(ViewportSnapshot::default());
        app.engine
            .insert_resource(CameraInputConfig { enabled: true });
        let mut m = app.script_module("render")?;
        install_camera_api(&mut *m);
        install_camera_2d_api(&mut *m);
        install_window_api(&mut *m);
        install_backdrop_api(&mut *m);
        install_shape_api(&mut *m);
        install_sprite_api(&mut *m);
        install_sprite_state_api(&mut *m);
        register_shape_component(app);
        register_shape2d_component(app);
        register_sprite_component(app);
        register_color_component(app);
        app.add_system(Stage::Render, clear_debug_lines_system);

        Ok(())
    }
}

/// The debug-line buffers' fallback owner. A windowed backend drains them
/// itself while it draws (`flush_debug_lines`, after `App::tick` returns),
/// and announces that by inserting [`WindowedBackend`]; this runs only when
/// nothing has, so the two can never both empty the same frame's lines.
///
/// Without it a headless run — `--headless`, CI, tests, the editor's
/// screenshot mode — grows both `Vec`s forever, one push per
/// `render.draw_line` per frame.
fn clear_debug_lines_system(eng: &Engine, _dt: f32) {
    if eng.try_resource::<WindowedBackend>().is_some() {
        return;
    }
    if let Some(lines) = eng.try_resource::<DebugLineBuffer>() {
        lines.borrow_mut().lines.clear();
    }
    if let Some(lines) = eng.try_resource::<DebugLineBuffer2d>() {
        lines.borrow_mut().lines.clear();
    }
}

/// The 3D camera: where it looks from, whether it takes the mouse, and the
/// pose, matrix and picking ray it published this frame.
fn install_camera_api(m: &mut dyn Bindings<Engine>) {
    // Writes `CameraConfig`; not an accessor pair with `render.camera_pose`,
    // which reads what the renderer actually did with the request.
    m.function(
        "set_camera",
        |eng: &Engine, (ex, ey, ez, tx, ty, tz): (f32, f32, f32, f32, f32, f32)| {
            let config = eng.resource::<CameraConfig>();
            let mut config = config.borrow_mut();
            config.eye = glamx::Vec3::new(ex, ey, ez);
            config.target = glamx::Vec3::new(tx, ty, tz);
            config.changed = true;
            Ok(())
        },
    );
    // No reader by design (N8): `CameraInputConfig` in the typemap already
    // holds the flag and windowed backends poll it; add `camera_input` when
    // a caller actually needs to read it back.
    m.function("set_camera_input", |eng: &Engine, enabled: bool| {
        let config = eng.resource::<CameraInputConfig>();
        config.borrow_mut().enabled = enabled;
        Ok(())
    });
    // The camera's exact projection*view matrix (column-major, 16
    // numbers): scripts project points precisely as the renderer does.
    m.function("camera_matrix", |eng: &Engine, ()| {
        let cam = eng.resource::<ViewportSnapshot>();
        let view_proj = cam.borrow().view_proj;
        Ok(view_proj.to_vec())
    });
    // Picking ray through the mouse: origin xyz, direction xyz.
    m.function("mouse_ray", |eng: &Engine, ()| {
        let cam = eng.resource::<ViewportSnapshot>();
        let cam = cam.borrow();
        Ok((
            cam.ray_origin[0],
            cam.ray_origin[1],
            cam.ray_origin[2],
            cam.ray_dir[0],
            cam.ray_dir[1],
            cam.ray_dir[2],
        ))
    });
    // Eye xyz, target xyz, fov (rad), HiDPI scale; all zeros with no windowed
    // backend. Reads the published snapshot, not what `set_camera` wrote.
    m.function("camera_pose", |eng: &Engine, ()| {
        let cam = eng.resource::<ViewportSnapshot>();
        let cam = cam.borrow();
        Ok((
            cam.eye[0],
            cam.eye[1],
            cam.eye[2],
            cam.target[0],
            cam.target[1],
            cam.target[2],
            cam.fov,
            cam.scale_factor,
        ))
    });
}

/// The 2D camera: its target center and zoom, the state it published this
/// frame, and the mouse unprojected through it.
fn install_camera_2d_api(m: &mut dyn Bindings<Engine>) {
    // World center + zoom in logical pixels per world unit. Not an accessor
    // pair with `render.camera_2d`, which reads the published snapshot.
    m.function(
        "set_camera_2d",
        |eng: &Engine, (cx, cy, zoom): (f32, f32, f32)| {
            let config = eng.resource::<CameraConfig2d>();
            let mut config = config.borrow_mut();
            config.center = [cx, cy];
            config.zoom = zoom.max(0.01);
            config.changed = true;
            Ok(())
        },
    );
    // Center xy, zoom, from the published snapshot rather than the config —
    // so headless answers all zeros, zoom included, not the config's default.
    m.function("camera_2d", |eng: &Engine, ()| {
        let vp = eng.resource::<ViewportSnapshot2d>();
        let vp = vp.borrow();
        Ok((vp.center[0], vp.center[1], vp.zoom))
    });
    // The mouse position in 2D world coordinates (picking).
    m.function("mouse_world_2d", |eng: &Engine, ()| {
        let vp = eng.resource::<ViewportSnapshot2d>();
        let vp = vp.borrow();
        Ok((vp.mouse_world[0], vp.mouse_world[1]))
    });
}

/// The OS window and its desktop presence.
/// A script-supplied path against the project, absolute paths left alone.
fn resolve_project_path(eng: &Engine, path: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    eng.try_resource::<balaur_core::project::ProjectRoot>()
        .map_or_else(|| p.to_path_buf(), |root| root.borrow().0.join(p))
}

fn install_window_api(m: &mut dyn Bindings<Engine>) {
    // PNG on the next rendered frame; a run with no renderer says so. Fire
    // and forget — the log line naming the file is the completion signal.
    m.function("screenshot", |eng: &Engine, path: String| {
        let full = resolve_project_path(eng, &path);
        eng.insert_resource(ScreenshotRequest {
            path: full,
            // The next rendered frame. No delay parameter on purpose: a
            // script cannot read the backend's own counter anyway.
            after_frame: 0,
        });
        Ok(())
    });
    // OS application icon (dock icon on macOS) from a PNG in the project.
    // No reader by design: add `app_icon` when a caller needs it back.
    m.function("set_app_icon", |eng: &Engine, path: String| {
        eng.insert_resource(AppIconConfig {
            path: resolve_project_path(eng, &path),
            changed: true,
        });
        Ok(())
    });
    // Borderless fullscreen on the current monitor. No readers by design
    // (N8): `WindowConfig` already holds what a backend last applied.
    m.function("set_fullscreen", |eng: &Engine, fullscreen: bool| {
        let config = eng.resource::<WindowConfig>();
        let mut config = config.borrow_mut();
        config.fullscreen = fullscreen;
        config.changed = true;
        Ok(())
    });
    // Confine the cursor to the window — FPS-style mouse capture, usually
    // paired with `set_cursor_hidden` and `input.mouse_delta`.
    m.function("set_cursor_grab", |eng: &Engine, grab: bool| {
        let config = eng.resource::<WindowConfig>();
        let mut config = config.borrow_mut();
        config.cursor_grabbed = grab;
        config.changed = true;
        Ok(())
    });
    m.function("set_cursor_hidden", |eng: &Engine, hidden: bool| {
        let config = eng.resource::<WindowConfig>();
        let mut config = config.borrow_mut();
        config.cursor_hidden = hidden;
        config.changed = true;
        Ok(())
    });
}

/// What is drawn behind and around the scene: clear colour, ground grid,
/// and the per-frame debug lines (3D and 2D).
fn install_backdrop_api(m: &mut dyn Bindings<Engine>) {
    // No reader by design (N8): the `ClearColorConfig` entry already holds
    // the colour; add `background` when a caller needs to read it back.
    m.function(
        "set_background",
        |eng: &Engine, (r, g, b): (f32, f32, f32)| {
            let clear = eng.resource::<ClearColorConfig>();
            let mut clear = clear.borrow_mut();
            clear.color = [r, g, b];
            clear.changed = true;
            Ok(())
        },
    );
    // No reader by design (N8): the `GridConfig` entry already holds every
    // field, and the backend polls it each frame rather than being told;
    // add `grid` when a caller needs to read it back.
    m.function(
        "set_grid",
        |eng,
         (enabled, step, major_every, extent): (
            bool,
            Option<f32>,
            Option<u32>,
            Option<i32>,
        )| {
            let grid = eng.resource::<GridConfig>();
            let mut grid = grid.borrow_mut();
            grid.enabled = enabled;
            if let Some(step) = step {
                grid.step = step;
            }
            if let Some(major) = major_every {
                grid.major_every = major;
            }
            if let Some(extent) = extent {
                grid.extent = extent;
            }
            Ok(())
        },
    );
    // No reader by design (N8): the same `GridConfig` entry holds both
    // colours; add `grid_colors` when a caller needs to read them back.
    m.function(
        "set_grid_colors",
        |eng: &Engine, (mr, mg, mb, jr, jg, jb): (f32, f32, f32, f32, f32, f32)| {
            let grid = eng.resource::<GridConfig>();
            let mut grid = grid.borrow_mut();
            grid.minor_color = [mr, mg, mb];
            grid.major_color = [jr, jg, jb];
            Ok(())
        },
    );
    // One line for one frame, world space. Width is in pixels; pass
    // perspective = true for distance-scaled width (gizmos want false).
    m.function(
        "draw_line",
        |eng: &Engine,
         (x1, y1, z1, x2, y2, z2, r, g, b, width, perspective, on_top): DrawLineArgs| {
            let lines = eng.resource::<DebugLineBuffer>();
            lines.borrow_mut().lines.push((
                [x1, y1, z1],
                [x2, y2, z2],
                [r, g, b],
                width.unwrap_or(1.0),
                perspective.unwrap_or(false),
                on_top.unwrap_or(false),
            ));
            Ok(())
        },
    );
    // One 2D world-space line for one frame; width in pixels.
    m.function(
        "draw_line_2d",
        |eng,
         (x1, y1, x2, y2, r, g, b, width): (f32, f32, f32, f32, f32, f32, f32, Option<f32>)| {
            let lines = eng.resource::<DebugLineBuffer2d>();
            lines.borrow_mut().lines.push((
                [x1, y1],
                [x2, y2],
                [r, g, b],
                width.unwrap_or(1.0),
            ));
            Ok(())
        },
    );
}

/// Shape and colour on a node, 3D and 2D.
/// The `sprite` bindings: a textured 2D quad, its sheet and its frame.
fn install_sprite_api(m: &mut dyn Bindings<Engine>) {
    // Sized from the image unless `set_sprite_size` says otherwise, so the
    // common case is one call and art keeps its authored proportions.
    m.function(
        "set_sprite",
        |eng: &Engine, (node, path): (NodeId, String)| {
            set_sprite(
                eng,
                entity_of(node)?,
                SpriteTexture {
                    path,
                    sheet: None,
                    frame: 0,
                },
                None,
                DEFAULT_PIXELS_PER_UNIT,
            )
        },
    );
    // One texture holding a `columns` x `rows` grid; the quad is sized to a
    // single frame, not to the whole sheet.
    m.function(
        "set_sprite_sheet",
        |eng: &Engine, (node, path, columns, rows): (NodeId, String, u32, u32)| {
            set_sprite(
                eng,
                entity_of(node)?,
                SpriteTexture {
                    path,
                    sheet: Some(SpriteSheet2d {
                        columns: columns.max(1),
                        rows: rows.max(1),
                    }),
                    frame: 0,
                },
                None,
                DEFAULT_PIXELS_PER_UNIT,
            )
        },
    );
}

/// Adjusting a sprite a node already has, and reading it back.
fn install_sprite_state_api(m: &mut dyn Bindings<Engine>) {
    // Frames are numbered left to right, top to bottom. Changing one only
    // moves UVs, so this is cheap enough to call every frame.
    m.function(
        "set_sprite_frame",
        |eng: &Engine, (node, frame): (NodeId, u32)| {
            let world = eng.world_mut();
            let mut r = world
                .get::<&mut Renderable2d>(entity_of(node)?)
                .map_err(|_| anyhow!("node has no sprite"))?;
            let Some(sprite) = r.sprite.as_mut() else {
                return Err(anyhow!("node has no sprite"));
            };
            sprite.frame = frame;
            Ok(())
        },
    );
    // Override the size read off the image, in half-extents like set_rect.
    m.function(
        "set_sprite_size",
        |eng: &Engine, (node, hx, hy): (NodeId, f32, f32)| {
            let world = eng.world_mut();
            let mut r = world
                .get::<&mut Renderable2d>(entity_of(node)?)
                .map_err(|_| anyhow!("node has no sprite"))?;
            if r.sprite.is_none() {
                return Err(anyhow!("node has no sprite"));
            }
            r.shape = Shape2d::Sprite {
                hx: hx.max(f32::EPSILON),
                hy: hy.max(f32::EPSILON),
            };
            r.version += 1;
            Ok(())
        },
    );
    // Returns ("", 0, 0, 0) when the node has no sprite.
    m.function("sprite", |eng: &Engine, node: NodeId| {
        let world = eng.world();
        let result = match world.get::<&Renderable2d>(entity_of(node)?) {
            Ok(r) => r.sprite.as_ref().map_or_else(
                || (String::new(), 0, 0, 0),
                |s| {
                    let (c, rows) = s.sheet.map_or((0, 0), |sh| (sh.columns, sh.rows));
                    (s.path.clone(), c, rows, s.frame)
                },
            ),
            Err(_) => (String::new(), 0, 0, 0),
        };
        Ok(result)
    });
    m.function(
        "set_circle",
        |eng: &Engine, (node, radius): (NodeId, f32)| {
            set_shape2d(eng, entity_of(node)?, Shape2d::Circle { radius })
        },
    );
    m.function(
        "set_color",
        |eng: &Engine, (node, r, g, b, a): (NodeId, f32, f32, f32, Option<f32>)| {
            set_color(eng, entity_of(node)?, [r, g, b, a.unwrap_or(1.0)])
        },
    );
    // Returns ("", 0, 0, 0) when the node has no shape.
    m.function("shape", |eng: &Engine, node: NodeId| {
        let world = eng.world();
        let result = match world.get::<&Renderable>(entity_of(node)?) {
            Ok(r) => match r.shape {
                Shape::Ball { radius } => ("ball".to_string(), radius, radius, radius),
                Shape::Cuboid { hx, hy, hz } => ("cuboid".to_string(), hx, hy, hz),
            },
            Err(_) => (String::new(), 0.0, 0.0, 0.0),
        };
        Ok(result)
    });
    // Returns ("", 0, 0) when the node has no 2D shape.
    m.function("shape2d", |eng: &Engine, node: NodeId| {
        let world = eng.world();
        let result = match world.get::<&Renderable2d>(entity_of(node)?) {
            Ok(r) => match r.shape {
                Shape2d::Circle { radius } => ("circle".to_string(), radius, radius),
                Shape2d::Rect { hx, hy } => ("rect".to_string(), hx, hy),
                Shape2d::Sprite { hx, hy } => ("sprite".to_string(), hx, hy),
            },
            Err(_) => (String::new(), 0.0, 0.0),
        };
        Ok(result)
    });
}

fn install_shape_api(m: &mut dyn Bindings<Engine>) {
    m.function("set_ball", |eng: &Engine, (node, radius): (NodeId, f32)| {
        set_shape(eng, entity_of(node)?, Shape::Ball { radius })
    });
    m.function(
        "set_cuboid",
        |eng: &Engine, (node, hx, hy, hz): (NodeId, f32, f32, f32)| {
            set_shape(eng, entity_of(node)?, Shape::Cuboid { hx, hy, hz })
        },
    );
    m.function(
        "set_rect",
        |eng: &Engine, (node, hx, hy): (NodeId, f32, f32)| {
            set_shape2d(eng, entity_of(node)?, Shape2d::Rect { hx, hy })
        },
    );
    m.function("color", |eng: &Engine, node: NodeId| {
        let world = eng.world();
        let result = if let Ok(r) = world.get::<&Renderable>(entity_of(node)?) {
            (r.color[0], r.color[1], r.color[2], r.color[3])
        } else if let Ok(r) = world.get::<&Renderable2d>(entity_of(node)?) {
            (r.color[0], r.color[1], r.color[2], r.color[3])
        } else {
            (1.0, 1.0, 1.0, 1.0)
        };
        Ok(result)
    });
}

// Components below are schema-driven, and each key doubles as a scene key.

/// The `shape` component: 3D primitives, editable from the editor.
fn register_shape_component(app: &mut App) {
    app.register_component(
        "shape",
        ComponentDef {
            schema: ComponentDef::parse_schema(
                "shape",
                r#"kind = { type = "enum", default = "cuboid", options = ["ball", "cuboid"], description = "Rendered 3D shape" }
radius = { type = "float", default = 0.5, min = 0.01, description = "Ball radius, when kind is ball" }
half_extents = { type = "vec3", default = [0.5, 0.5, 0.5], description = "Half-sizes of the cuboid, when kind is cuboid" }"#,
            ),
            apply: Box::new(|eng, entity, params| {
                let kind = params
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("cuboid");
                let radius = params
                    .get("radius")
                    .and_then(balaur_core::components::as_f64)
                    .unwrap_or(0.5) as f32;
                let he = |i: usize| {
                    params
                        .get("half_extents")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.get(i))
                        .and_then(balaur_core::components::as_f64)
                        .unwrap_or(0.5) as f32
                };
                let shape = match kind {
                    "ball" => Shape::Ball {
                        radius: radius.max(0.01),
                    },
                    "cuboid" => Shape::Cuboid {
                        hx: he(0).max(0.01),
                        hy: he(1).max(0.01),
                        hz: he(2).max(0.01),
                    },
                    other => return Err(anyhow!("unknown shape kind '{other}'")),
                };
                set_shape(eng, entity, shape)
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Renderable>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let renderable = world.get::<&Renderable>(entity).ok()?;
                let mut map = toml::map::Map::new();
                match renderable.shape {
                    Shape::Ball { radius } => {
                        map.insert("kind".into(), toml::Value::String("ball".into()));
                        map.insert("radius".into(), toml::Value::Float(f64::from(radius)));
                    }
                    Shape::Cuboid { hx, hy, hz } => {
                        map.insert("kind".into(), toml::Value::String("cuboid".into()));
                        map.insert(
                            "half_extents".into(),
                            toml::Value::Array(vec![
                                toml::Value::Float(f64::from(hx)),
                                toml::Value::Float(f64::from(hy)),
                                toml::Value::Float(f64::from(hz)),
                            ]),
                        );
                    }
                }
                Some(toml::Value::Table(map))
            }),
        },
    );
}

/// The `shape2d` component: 2D primitives.
fn register_shape2d_component(app: &mut App) {
    app.register_component(
        "shape2d",
        ComponentDef {
            schema: ComponentDef::parse_schema(
                "shape2d",
                r#"kind = { type = "enum", default = "rect", options = ["circle", "rect"], description = "Rendered 2D shape" }
radius = { type = "float", default = 0.5, min = 0.01, description = "Circle radius, when kind is circle" }
half_extents = { type = "vec2", default = [0.5, 0.5], description = "Half-sizes of the rect, when kind is rect" }"#,
            ),
            apply: Box::new(|eng, entity, params| {
                let kind = params
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("rect");
                let radius = params
                    .get("radius")
                    .and_then(balaur_core::components::as_f64)
                    .unwrap_or(0.5) as f32;
                let he = |i: usize| {
                    params
                        .get("half_extents")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.get(i))
                        .and_then(balaur_core::components::as_f64)
                        .unwrap_or(0.5) as f32
                };
                let shape = match kind {
                    "circle" => Shape2d::Circle {
                        radius: radius.max(0.01),
                    },
                    "rect" => Shape2d::Rect {
                        hx: he(0).max(0.01),
                        hy: he(1).max(0.01),
                    },
                    other => return Err(anyhow!("unknown shape2d kind '{other}'")),
                };
                set_shape2d(eng, entity, shape)
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Renderable2d>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let renderable = world.get::<&Renderable2d>(entity).ok()?;
                let mut map = toml::map::Map::new();
                match renderable.shape {
                    // A sprite is saved by the `sprite` component, not this one.
                    Shape2d::Sprite { .. } => return None,
                    Shape2d::Circle { radius } => {
                        map.insert("kind".into(), toml::Value::String("circle".into()));
                        map.insert("radius".into(), toml::Value::Float(f64::from(radius)));
                    }
                    Shape2d::Rect { hx, hy } => {
                        map.insert("kind".into(), toml::Value::String("rect".into()));
                        map.insert(
                            "half_extents".into(),
                            toml::Value::Array(vec![
                                toml::Value::Float(f64::from(hx)),
                                toml::Value::Float(f64::from(hy)),
                            ]),
                        );
                    }
                }
                Some(toml::Value::Table(map))
            }),
        },
    );
}

/// The `sprite` component: a textured 2D quad.
fn register_sprite_component(app: &mut App) {
    app.register_component(
        "sprite",
        ComponentDef {
            schema: ComponentDef::parse_schema(
                "sprite",
                r#"texture = { type = "string", default = "", description = "Image file, project-relative; required" }
columns = { type = "float", default = 0.0, min = 0.0, description = "Sheet grid columns for flipbook sprites; 0 means a single image" }
rows = { type = "float", default = 0.0, min = 0.0, description = "Sheet grid rows for flipbook sprites; 0 means a single image" }
frame = { type = "float", default = 0.0, min = 0.0, description = "Current sheet cell, counted left-to-right then top-to-bottom" }
pixels_per_unit = { type = "float", default = 100.0, min = 0.01, description = "Texture pixels per world unit" }
half_extents = { type = "vec2", default = [0.0, 0.0], description = "Size override in world units; [0, 0] sizes from the texture" }"#,
            ),
            apply: Box::new(|eng, entity, params| {
                let num = |key: &str| {
                    params
                        .get(key)
                        .and_then(balaur_core::components::as_f64)
                        .unwrap_or(0.0)
                };
                let texture = params
                    .get("texture")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                if texture.is_empty() {
                    return Err(anyhow!("sprite needs a texture"));
                }
                let columns = num("columns") as u32;
                let rows = num("rows") as u32;
                // A sheet needs both counts; one alone is a typo, not a grid.
                let sheet = (columns > 0 && rows > 0).then_some(SpriteSheet2d { columns, rows });
                let he = |i: usize| {
                    params
                        .get("half_extents")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.get(i))
                        .and_then(balaur_core::components::as_f64)
                        .unwrap_or(0.0) as f32
                };
                // Absent (or zero) half-extents mean "size it from the image".
                let explicit = (he(0) > 0.0 && he(1) > 0.0).then(|| (he(0), he(1)));
                let ppu = num("pixels_per_unit") as f32;
                set_sprite(
                    eng,
                    entity,
                    SpriteTexture {
                        path: texture,
                        sheet,
                        frame: num("frame") as u32,
                    },
                    explicit,
                    if ppu > 0.0 {
                        ppu
                    } else {
                        DEFAULT_PIXELS_PER_UNIT
                    },
                )
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Renderable2d>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let renderable = world.get::<&Renderable2d>(entity).ok()?;
                let sprite = renderable.sprite.as_ref()?;
                let Shape2d::Sprite { hx, hy } = renderable.shape else {
                    return None;
                };
                let mut map = toml::map::Map::new();
                map.insert("texture".into(), toml::Value::String(sprite.path.clone()));
                if let Some(sheet) = sprite.sheet {
                    map.insert(
                        "columns".into(),
                        toml::Value::Float(f64::from(sheet.columns)),
                    );
                    map.insert("rows".into(), toml::Value::Float(f64::from(sheet.rows)));
                }
                map.insert("frame".into(), toml::Value::Float(f64::from(sprite.frame)));
                // Written out resolved: a saved scene reloads to the same size
                // even if the image on disk is replaced with a different one.
                map.insert(
                    "half_extents".into(),
                    toml::Value::Array(vec![
                        toml::Value::Float(f64::from(hx)),
                        toml::Value::Float(f64::from(hy)),
                    ]),
                );
                Some(toml::Value::Table(map))
            }),
        },
    );
}

/// The `color` component, shared by 2D and 3D renderables.
fn register_color_component(app: &mut App) {
    app.register_component(
        "color",
        ComponentDef {
            schema: ComponentDef::parse_schema(
                "color",
                r#"rgba = { type = "color", default = [0.8, 0.8, 0.8, 1.0], shorthand = true, description = "The node's tint, as channel floats or #rrggbb / #rrggbbaa" }"#,
            ),
            apply: Box::new(|eng, entity, params| {
                let c = |i: usize, default: f64| {
                    params
                        .get("rgba")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.get(i))
                        .and_then(balaur_core::components::as_f64)
                        .unwrap_or(default) as f32
                };
                set_color(eng, entity, [c(0, 0.8), c(1, 0.8), c(2, 0.8), c(3, 1.0)])
            }),
            remove: Box::new(|eng, entity| set_color(eng, entity, [0.8, 0.8, 0.8, 1.0])),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let color = if let Ok(r) = world.get::<&Renderable>(entity) {
                    r.color
                } else {
                    world.get::<&Renderable2d>(entity).ok()?.color
                };
                Some(toml::Value::Table(toml::map::Map::from_iter([(
                    "rgba".to_string(),
                    toml::Value::Array(
                        color
                            .iter()
                            .map(|c| toml::Value::Float(f64::from(*c)))
                            .collect(),
                    ),
                )])))
            }),
        },
    );
}
