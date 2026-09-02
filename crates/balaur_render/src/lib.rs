//! Rendering as a Balaur plugin.
//!
//! The plugin itself is backend-agnostic: it owns the `Renderable` component,
//! the `render` script module, and its scene-file component keys. Backends
//! consume `Renderable` + `GlobalTransform`. The kiss3d/wgpu backend (feature
//! `kiss3d`) owns the OS event loop; headless runs just never draw, which
//! keeps the simulation byte-for-byte identical with and without a window.

use anyhow::{anyhow, Result};
use balaur_core::entity_of;
use balaur_core::hecs::Entity;
use balaur_core::{App, Engine, Plugin, Stage};
use balaur_script::{Bindings, BindingsExt, NodeId};

mod camera;
pub mod material;
pub mod mesh;
mod particles;
mod pick;
mod polygon;
pub mod shaders;
mod shape;
mod sprite;
mod tilemap;
pub use camera::{Camera, CameraKind};
pub use particles::Particles;
pub use polygon::PolygonMesh;
pub use tilemap::{Tilemap, Tileset, TILESET_ASSET_TYPE};

#[cfg(feature = "kiss3d")]
pub mod kiss3d_backend;
#[cfg(feature = "kiss3d")]
mod shader_material;
#[cfg(feature = "kiss3d")]
mod skinned_2d;

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
    /// The image itself, not a path: a packed game's icon ships in the pack.
    pub bytes: Vec<u8>,
    /// What the script asked for, kept for the log line.
    pub name: String,
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
    Ball {
        radius: f32,
    },
    Cuboid {
        hx: f32,
        hy: f32,
        hz: f32,
    },
    /// A cylinder with hemispherical caps, principal axis on y. `height` is
    /// the cylindrical part, so the whole thing is `height + 2 * radius` tall.
    Capsule {
        radius: f32,
        height: f32,
    },
    Cylinder {
        radius: f32,
        height: f32,
    },
    Cone {
        radius: f32,
        height: f32,
    },
    /// A flat quad in the xz plane, for ground and walls.
    Plane {
        hx: f32,
        hz: f32,
    },
    /// Authored geometry. The vertices live in `Renderable::mesh`, the way a
    /// sprite's texture lives beside its quad — the enum stays `Copy`.
    Mesh,
}

/// A local axis-aligned box. A mesh is not centred on its origin, so the
/// centre is part of it.
#[derive(Clone, Copy)]
pub struct Bounds {
    pub centre: glamx::Vec3,
    pub half: glamx::Vec3,
}

pub struct Renderable {
    pub shape: Shape,
    /// The mesh's own box, measured when the asset resolves. Every other
    /// shape carries its size in `shape`, so this is `None` for those.
    pub bounds: Option<Bounds>,
    pub color: [f32; 4],
    /// The `mesh` asset this draws, present exactly when `shape` is a mesh.
    pub mesh: Option<String>,
    /// Node path to the rig a skinned mesh deforms with, relative to the
    /// node; empty means the node itself. Ignored by a mesh with no skin.
    pub skeleton: String,
    /// Image a mesh is textured with; empty draws the colour alone.
    pub texture: String,
    /// Bumped when `shape` changes so backends know to rebuild their node.
    pub version: u64,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Shape2d {
    Circle {
        radius: f32,
    },
    /// The straight part is `height`; the caps add `radius` at each end, the
    /// same meaning the `collider2d` capsule gives them.
    Capsule {
        radius: f32,
        height: f32,
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
    /// A chain of points: open it is a line, closed it is a polygon outline.
    /// The points live in `Renderable2d::polyline`, the way a sprite's texture
    /// lives beside its quad — the enum stays `Copy`.
    Polyline {
        width: f32,
        closed: bool,
    },
    /// A filled, textured polygon, deformed by a rig when its mesh carries
    /// skin weights. The geometry lives in `Renderable2d::polygon`.
    Polygon,
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
    /// Mirror the image horizontally / vertically. Like `frame`, a UV-only
    /// change: backends re-apply it without rebuilding their node.
    pub flip_x: bool,
    pub flip_y: bool,
}

/// 2D renderable, mirrored into the backend's 2D scene. The node's regular
/// `Transform` drives it: x/y translate, the z rotation spins it, x/y scale.
pub struct Renderable2d {
    pub shape: Shape2d,
    pub color: [f32; 4],
    /// Present exactly when `shape` is a sprite.
    pub sprite: Option<SpriteTexture>,
    /// The `mesh` asset a polyline draws, present exactly when `shape` is one.
    pub polyline: Option<String>,
    /// What a polygon draws, present exactly when `shape` is one.
    pub polygon: Option<std::sync::Arc<PolygonMesh>>,
    /// The `material` asset this draws with; empty means the built-in one.
    pub material: String,
    pub version: u64,
}

/// Point `entity` at a polygon, rebuilding the backend's node only when the
/// geometry, texture or rig changed — a tint alone never does.
pub(crate) fn set_polygon(
    eng: &Engine,
    entity: Entity,
    polygon: std::sync::Arc<PolygonMesh>,
) -> Result<()> {
    let mut world = eng.world_mut();
    if let Ok(mut r) = world.get::<&mut Renderable2d>(entity) {
        let rebuild = r.shape != Shape2d::Polygon || r.polygon.as_deref() != Some(&*polygon);
        r.shape = Shape2d::Polygon;
        r.polygon = Some(polygon);
        if rebuild {
            r.version += 1;
        }
        return Ok(());
    }
    world
        .insert_one(
            entity,
            Renderable2d {
                shape: Shape2d::Polygon,
                color: [1.0, 1.0, 1.0, 1.0],
                sprite: None,
                polyline: None,
                polygon: Some(polygon),
                material: String::new(),
                version: 0,
            },
        )
        .map_err(|_| anyhow!("node is dead"))
}

/// The box a mesh's vertices fill, or `None` when the asset will not load.
///
/// Measured here rather than in the picker: the definition is already being
/// resolved, and a click should not read a file.
fn mesh_bounds(eng: &Engine, source: &str) -> Option<Bounds> {
    if source.is_empty() {
        return None;
    }
    let definition =
        balaur_core::assets::load_typed::<balaur_core::mesh::MeshData>(eng, source).ok()?;
    let mut low = glamx::Vec3::splat(f32::INFINITY);
    let mut high = glamx::Vec3::splat(f32::NEG_INFINITY);
    for p in &definition.positions {
        let p = glamx::Vec3::new(p[0], p[1], p[2]);
        low = low.min(p);
        high = high.max(p);
    }
    if !low.is_finite() || !high.is_finite() {
        return None;
    }
    Some(Bounds {
        centre: (low + high) / 2.0,
        // A flat mesh has no thickness on one axis, and a slab test on zero
        // misses everything.
        half: ((high - low) / 2.0).max(glamx::Vec3::splat(1e-4)),
    })
}

/// Point `entity` at the mesh asset it draws, mirroring `set_polyline`.
pub(crate) fn set_mesh(
    eng: &Engine,
    entity: Entity,
    source: String,
    skeleton: String,
    texture: String,
) -> Result<()> {
    let bounds = mesh_bounds(eng, &source);
    let mut world = eng.world_mut();
    if let Ok(mut r) = world.get::<&mut Renderable>(entity) {
        let rebuild = r.shape != Shape::Mesh
            || r.mesh.as_deref() != Some(source.as_str())
            || r.skeleton != skeleton
            || r.texture != texture;
        r.shape = Shape::Mesh;
        r.bounds = bounds;
        r.mesh = Some(source);
        r.skeleton = skeleton;
        r.texture = texture;
        if rebuild {
            r.version += 1;
        }
        return Ok(());
    }
    world
        .insert_one(
            entity,
            Renderable {
                shape: Shape::Mesh,
                bounds,
                color: [0.8, 0.8, 0.8, 1.0],
                mesh: Some(source),
                skeleton,
                texture,
                version: 0,
            },
        )
        .map_err(|_| anyhow!("node is dead"))
}

pub(crate) fn set_shape(eng: &Engine, entity: Entity, shape: Shape) -> Result<()> {
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
                bounds: None,
                color: [0.8, 0.8, 0.8, 1.0],
                mesh: None,
                skeleton: String::new(),
                texture: String::new(),
                version: 0,
            },
        )
        .map_err(|_| anyhow!("node is dead"))
}

/// Point `entity` at the mesh asset its polyline draws, and set the shape.
pub(crate) fn set_polyline(
    eng: &Engine,
    entity: Entity,
    source: String,
    shape: Shape2d,
) -> Result<()> {
    let mut world = eng.world_mut();
    if let Ok(mut r) = world.get::<&mut Renderable2d>(entity) {
        let rebuild = r.shape != shape || r.polyline.as_deref() != Some(source.as_str());
        r.shape = shape;
        r.polyline = Some(source);
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
                sprite: None,
                polyline: Some(source),
                polygon: None,
                material: String::new(),
                version: 0,
            },
        )
        .map_err(|_| anyhow!("node is dead"))
}

pub(crate) fn set_shape2d(eng: &Engine, entity: Entity, shape: Shape2d) -> Result<()> {
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
                polyline: None,
                polygon: None,
                material: String::new(),
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
    bytes: &[u8],
    name: &str,
    sheet: Option<SpriteSheet2d>,
    ppu: f32,
) -> Result<(f32, f32)> {
    let (w, h) = image::load_from_memory(bytes)
        .map(|image| (image.width(), image.height()))
        .map_err(|e| anyhow!("reading the size of {name}: {e}"))?;
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
    } else if texture.path.is_empty() {
        // A sprite with no texture yet: the editor adds the component first
        // and picks the image after. It draws an untextured quad rather than
        // refusing to exist, at the same default size as a `rect`.
        (0.5, 0.5)
    } else {
        let bytes = eng
            .resource::<balaur_core::project::ProjectFiles>()
            .borrow()
            .read(&texture.path)?;
        natural_half_extents(&bytes, &texture.path, texture.sheet, ppu)?
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
                polyline: None,
                polygon: None,
                material: String::new(),
                version: 0,
            },
        )
        .map_err(|_| anyhow!("node is dead"))
}

/// The `color` property of a renderable component's params.
///
/// A property rather than a component of its own: a tint has no meaning
/// without something to tint, and as a separate component it was the only one
/// that owned no storage and refused to apply on a bare node.
pub(crate) fn color_from_params(params: &toml::Value) -> [f32; 4] {
    let c = |i: usize, default: f64| {
        params
            .get("color")
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(default) as f32
    };
    [c(0, 0.8), c(1, 0.8), c(2, 0.8), c(3, 1.0)]
}

/// The `color` property, for a component's `get`.
pub(crate) fn color_to_toml(color: [f32; 4]) -> toml::Value {
    toml::Value::Array(
        color
            .iter()
            .map(|c| toml::Value::Float(f64::from(*c)))
            .collect(),
    )
}

/// Tint whichever renderable(s) the node carries (3D, 2D and/or particles).
/// Point `entity` at the `material` asset it draws with; empty is the
/// built-in material.
///
/// A change bumps `version`, which rebuilds the backend's node: a material
/// owns its pipeline, so it cannot be swapped onto a node already built
/// against a different one.
pub(crate) fn set_material_2d(eng: &Engine, entity: Entity, reference: &str) -> Result<()> {
    let world = eng.world_mut();
    let mut renderable = world
        .get::<&mut Renderable2d>(entity)
        .map_err(|_| anyhow!("node has no 2D shape yet"))?;
    if renderable.material != reference {
        renderable.material = reference.to_string();
        renderable.version += 1;
    }
    Ok(())
}

pub(crate) fn set_color(eng: &Engine, entity: Entity, color: [f32; 4]) -> Result<()> {
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
    if let Ok(mut p) = world.get::<&mut Particles>(entity) {
        p.color = color;
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
        m.drives(&["camera", "sprite", "shape2d", "shape3d"]);
        install_camera_api(&mut *m);
        install_camera_2d_api(&mut *m);
        install_window_api(&mut *m);
        install_backdrop_api(&mut *m);
        material::install_material_check(&mut *m);
        shape::install_shape_api(&mut *m);
        install_sprite_api(&mut *m);
        install_sprite_state_api(&mut *m);
        install_texture_api(&mut *m);
        shape::register_shape_component(app);
        shape::register_shape2d_component(app);
        register_render_presets(app)?;
        sprite::register_sprite_component(app);
        polygon::register_polygon_component(app);
        camera::register_camera_component(app);
        mesh::register_mesh_component(app);
        material::register_material_asset(app);
        tilemap::register_tileset_asset(app);
        tilemap::register_tilemap_component(app);
        particles::register_particles_component(app);
        // SceneSync, and after the core propagation system registered at
        // `App::new`: the camera follows the node's settled global pose.
        app.add_system(Stage::SceneSync, camera::drive_camera_system);
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
    // What a ray meets first, or `()`. The ray is an argument rather than
    // the mouse because the viewport snapshot is empty without a window,
    // and the editor's own tests run headless.
    m.function(
        "pick_ray",
        |eng: &Engine, (ox, oy, oz, dx, dy, dz): (f64, f64, f64, f64, f64, f64)| {
            let origin = glamx::Vec3::new(ox as f32, oy as f32, oz as f32);
            let dir = glamx::Vec3::new(dx as f32, dy as f32, dz as f32);
            let world = eng.world();
            Ok(crate::pick::along_ray(&world, origin, dir)
                .map(|(entity, _)| balaur_core::node_id_of(entity)))
        },
    );
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
        let bytes = eng
            .resource::<balaur_core::project::ProjectFiles>()
            .borrow()
            .read(&path)?;
        eng.insert_resource(AppIconConfig {
            bytes,
            name: path,
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
                    flip_x: false,
                    flip_y: false,
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
                    flip_x: false,
                    flip_y: false,
                },
                None,
                DEFAULT_PIXELS_PER_UNIT,
            )
        },
    );
}

/// Adjusting a sprite a node already has, and reading it back.
/// The image's pixel size, from its header: what a tool placing UVs over a
/// texture needs, in every build.
fn install_texture_api(m: &mut dyn Bindings<Engine>) {
    m.function("texture_size", |eng: &Engine, path: String| {
        let bytes = eng
            .resource::<balaur_core::project::ProjectFiles>()
            .borrow()
            .read(&path)?;
        let (w, h) = image::load_from_memory(&bytes)
            .map(|image| (image.width(), image.height()))
            .map_err(|e| anyhow!("reading the size of {path}: {e}"))?;
        Ok((w, h))
    });
}

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
    m.function("shape3d", |eng: &Engine, node: NodeId| {
        let world = eng.world();
        let result = match world.get::<&Renderable>(entity_of(node)?) {
            Ok(r) => match r.shape {
                Shape::Ball { radius } => ("ball".to_string(), radius, radius, radius),
                Shape::Cuboid { hx, hy, hz } => ("cuboid".to_string(), hx, hy, hz),
                // Radius, height, radius — the same order the dimensions
                // occupy in space, so x and z always mean the same thing.
                Shape::Capsule { radius, height } => {
                    ("capsule".to_string(), radius, height, radius)
                }
                Shape::Cylinder { radius, height } => {
                    ("cylinder".to_string(), radius, height, radius)
                }
                Shape::Cone { radius, height } => ("cone".to_string(), radius, height, radius),
                Shape::Plane { hx, hz } => ("plane".to_string(), hx, 0.0, hz),
                Shape::Mesh => ("mesh".to_string(), 0.0, 0.0, 0.0),
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
                Shape2d::Capsule { radius, height } => ("capsule".to_string(), radius, height),
                Shape2d::Polyline { width, .. } => ("polyline".to_string(), width, width),
                Shape2d::Polygon => ("polygon".to_string(), 0.0, 0.0),
                Shape2d::Rect { hx, hy } => ("rect".to_string(), hx, hy),
                Shape2d::Sprite { hx, hy } => ("sprite".to_string(), hx, hy),
            },
            Err(_) => (String::new(), 0.0, 0.0),
        };
        Ok(result)
    });
}

/// Presets over the render components. `2d` is terminal and lowercase; 3D
/// names carry no marker (N/D5).
fn register_render_presets(app: &mut App) -> Result<()> {
    app.register_preset(
        "sprite2d",
        balaur_core::presets::preset("A textured 2D quad", &["2d", "render"], &[("sprite", None)])?,
    );
    app.register_preset(
        "rect2d",
        balaur_core::presets::preset(
            "An untextured 2D rectangle",
            &["2d", "render"],
            &[("shape2d", Some("kind = \"rect\""))],
        )?,
    );
    app.register_preset(
        "tilemap2d",
        balaur_core::presets::preset(
            "A grid of tiles from one tileset",
            &["2d", "render"],
            &[("tilemap", None)],
        )?,
    );
    app.register_preset(
        "polygon2d",
        balaur_core::presets::preset(
            "A textured polygon that a rig can deform",
            &["2d", "render", "animation"],
            &[("polygon", None)],
        )?,
    );
    Ok(())
}
