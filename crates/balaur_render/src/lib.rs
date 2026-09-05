//! Rendering as a Balaur plugin.
//!
//! The plugin itself is backend-agnostic: it owns the `Renderable` component,
//! the `render` script module, and its scene-file component keys. Backends
//! consume `Renderable` + `GlobalTransform`. The kiss3d/wgpu backend (feature
//! `kiss3d`) owns the OS event loop; headless runs just never draw, which
//! keeps the simulation byte-for-byte identical with and without a window.

use anyhow::{anyhow, Result};
use balaur_core::hecs::Entity;
use balaur_core::{Engine, Stage};
use balaur_plugin::Registry;
use balaur_script::Bindings;

mod camera;
mod debug_view;
mod draw_2d;
pub mod light;
pub mod material;
pub mod mesh;
mod particles;
mod pick;
mod polygon;
#[cfg(feature = "kiss3d")]
mod polyline_strip;
pub mod preview;
#[cfg(feature = "kiss3d")]
mod probe;
mod script_api;
pub mod shaders;
mod shape;
mod sprite;
mod texture;
mod tilemap;
pub use camera::{Camera, CameraKind};
pub use debug_view::{ChannelView, PreviewRequest, ProbeReading, ProbeRequest};
pub use light::{Light2d, LightKind2d, LitLight2d, Occluder2d};
pub use particles::Particles;
pub use polygon::PolygonMesh;
pub use tilemap::{Tilemap, Tileset, TILESET_ASSET_TYPE};

#[cfg(feature = "kiss3d")]
mod device;
#[cfg(all(
    feature = "kiss3d",
    target_family = "wasm",
    not(target_os = "emscripten")
))]
mod hidden_tab;
#[cfg(feature = "kiss3d")]
mod bind_layout;
#[cfg(feature = "kiss3d")]
pub mod kiss3d_backend;
#[cfg(feature = "kiss3d")]
mod kiss3d_camera;
#[cfg(feature = "kiss3d")]
mod kiss3d_input;
#[cfg(feature = "kiss3d")]
mod light_map;
#[cfg(feature = "kiss3d")]
mod material_cache;
#[cfg(feature = "kiss3d")]
mod shader_material;
#[cfg(feature = "kiss3d")]
mod shader_material_3d;
#[cfg(feature = "kiss3d")]
mod skinned_2d;
#[cfg(feature = "kiss3d")]
mod skinned_3d;

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
    /// Keep the screen from dimming while the game runs; a page asks the
    /// browser for a wake lock, a desktop needs nothing.
    pub keep_awake: bool,
    pub changed: bool,
}

/// Screen-space effects the frame is resolved through, from the current
/// `camera`. Applied by windowed backends when changed; a headless run holds
/// the values and draws nothing, so a game that switches bloom on still ticks
/// identically in CI.
// Four independent switches plus the dirty flag, like `WindowConfig`: a state
// enum would invent coupling these do not have.
#[allow(clippy::struct_excessive_bools)]
pub struct PostConfig {
    /// Bright pixels bleed into their neighbours. The 2D light map feeds it:
    /// a light over intensity 1 is what blooms.
    pub bloom: bool,
    /// Screen-space ambient occlusion, 3D only.
    pub ssao: bool,
    /// Screen-space reflections, 3D only.
    pub ssr: bool,
    /// Depth of field, 3D only.
    pub dof: bool,
    /// Brightness a pixel blooms past, and how much of it is added back.
    pub bloom_threshold: f32,
    pub bloom_intensity: f32,
    pub changed: bool,
}

impl Default for PostConfig {
    fn default() -> Self {
        Self {
            bloom: false,
            ssao: false,
            ssr: false,
            dof: false,
            bloom_threshold: 1.0,
            bloom_intensity: 0.6,
            changed: false,
        }
    }
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

/// The debug-line buffers, which live in `balaur_core` because physics and the
/// editor fill them too and a producer must not depend on the renderer to draw
/// a line. Re-exported: `balaur::render::DebugLineBuffer` is published API.
pub use balaur_core::debug_lines::{DebugLine, DebugLine2d, DebugLineBuffer, DebugLineBuffer2d};
pub use draw_2d::{Draw2d, DrawBuffer2d};

/// Where the 2D camera looks (world center) and its zoom in logical pixels
/// per world unit. Backends apply it when changed and keep their own
/// interactive pan/zoom in between.
pub struct CameraConfig2d {
    pub center: [f32; 2],
    pub zoom: f32,
    /// Light every 2D surface gets before any `light2d`, from the current 2D
    /// camera. Read every frame rather than applied on a change, so it is not
    /// what `changed` is about.
    pub ambient: [f32; 3],
    pub changed: bool,
}

impl Default for CameraConfig2d {
    fn default() -> Self {
        Self {
            center: [0.0, 0.0],
            zoom: 60.0,
            ambient: [0.0, 0.0, 0.0],
            // Asserted at boot, as [`CameraConfig`] is: a scene writing the
            // schema's own default of 60 raises no change and is never applied.
            changed: true,
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
    /// The `material` asset this draws with; empty means the built-in one.
    pub material: String,
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
    /// One rectangle of the image, in texture pixels as `[x, y, w, h]`: an
    /// atlas cell. `None` draws the whole image, or the sheet's frame.
    pub region: Option<[u32; 4]>,
}

/// How a polyline is dressed beyond its colour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineStyle {
    /// The colour at the chain's end, blended from `color` along it.
    pub gradient: Option<[f32; 4]>,
    /// An image drawn along the chain, `u` in world units along it.
    pub texture: String,
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
    /// A polyline's gradient and texture, present exactly when `shape` is one.
    pub line: Option<LineStyle>,
    /// What a polygon draws, present exactly when `shape` is one.
    pub polygon: Option<std::sync::Arc<PolygonMesh>>,
    /// The `material` asset this draws with; empty means the built-in one.
    pub material: String,
    /// Whether the author stated the size, rather than a sprite deriving it
    /// from its image: a derived one re-derives when the sheet or
    /// `pixels_per_unit` moves, and an authored one is left alone.
    pub sized: bool,
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
                line: None,
                polygon: Some(polygon),
                material: String::new(),
                sized: false,
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
                material: String::new(),
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
                material: String::new(),
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
    style: LineStyle,
) -> Result<()> {
    let mut world = eng.world_mut();
    if let Ok(mut r) = world.get::<&mut Renderable2d>(entity) {
        let rebuild = r.shape != shape
            || r.polyline.as_deref() != Some(source.as_str())
            || r.line.as_ref() != Some(&style);
        r.shape = shape;
        r.polyline = Some(source);
        r.line = Some(style);
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
                line: Some(style),
                polygon: None,
                material: String::new(),
                sized: false,
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
                line: None,
                polygon: None,
                material: String::new(),
                sized: false,
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
    region: Option<[u32; 4]>,
    ppu: f32,
) -> Result<(f32, f32)> {
    let ppu = if ppu > 0.0 {
        ppu
    } else {
        DEFAULT_PIXELS_PER_UNIT
    };
    // A region states its own size; the image is only read when nothing does.
    if let Some([_, _, w, h]) = region {
        return Ok((w as f32 / ppu / 2.0, h as f32 / ppu / 2.0));
    }
    let (w, h) = texture::image_size(bytes, name)?;
    let (cols, rows) = sheet.map_or((1, 1), |s| (s.columns.max(1), s.rows.max(1)));
    Ok((
        (w as f32 / cols as f32) / ppu / 2.0,
        (h as f32 / rows as f32) / ppu / 2.0,
    ))
}

/// Point a node at a texture, sizing the quad unless the caller said otherwise.
pub(crate) fn set_sprite(
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
        natural_half_extents(&bytes, &texture.path, texture.sheet, texture.region, ppu)?
    };
    let shape = Shape2d::Sprite {
        hx: hx.max(f32::EPSILON),
        hy: hy.max(f32::EPSILON),
    };
    // A size the caller stated is the author's; one read off the image is
    // this call's, and `read_sprite` must not report it back as authored.
    let sized = half_extents.is_some();
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
        r.sized = sized;
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
                line: None,
                polygon: None,
                material: String::new(),
                sized,
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

pub struct RenderPlugin {
    manifest: balaur_plugin::Manifest,
}

impl Default for RenderPlugin {
    fn default() -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("render", env!("CARGO_PKG_VERSION")),
        }
    }
}

impl balaur_plugin::Plugin for RenderPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        reg.insert_resource(CameraConfig::default());
        reg.insert_resource(ClearColorConfig {
            color: [0.09, 0.098, 0.11],
            changed: true,
        });
        reg.insert_resource(WindowConfig::default());
        reg.insert_resource(GridConfig::default());
        reg.insert_resource(DebugLineBuffer::default());
        reg.insert_resource(DebugLineBuffer2d::default());
        reg.insert_resource(DrawBuffer2d::default());
        reg.insert_resource(CameraConfig2d::default());
        reg.insert_resource(PostConfig::default());
        reg.insert_resource(ViewportSnapshot2d::default());
        reg.insert_resource(ViewportSnapshot::default());
        reg.insert_resource(CameraInputConfig { enabled: true });
        let mut m = reg.script_module("render")?;
        m.module_doc(
            "What a frame is made of: the shape, sprite, mesh or emitter a \
             node draws, the 2D and 3D cameras, the OS window, and the \
             backdrop and debug lines drawn around the scene.",
        );
        script_api::install_camera_api(&mut *m);
        script_api::install_camera_2d_api(&mut *m);
        script_api::install_window_api(&mut *m);
        debug_view::install_debug_view_api(&mut *m);
        script_api::install_backdrop_api(&mut *m);
        material::install_material_check(&mut *m);
        material::install_material_params(&mut *m);
        shape::install_shape_api(&mut *m);
        light::install_occluder_api(&mut *m);
        script_api::install_sprite_api(&mut *m);
        script_api::install_sprite_state_api(&mut *m);
        script_api::install_texture_api(&mut *m);
        draw_2d::install_draw_2d_api(&mut *m);
        tilemap::install_tilemap_api(&mut *m);
        shape::register_shape_component(reg);
        shape::register_shape2d_component(reg);
        register_render_presets(reg)?;
        sprite::register_sprite_component(reg);
        polygon::register_polygon_component(reg);
        camera::register_camera_component(reg);
        light::register_light2d_component(reg);
        light::register_occluder2d_component(reg);
        mesh::register_mesh_component(reg);
        material::register_material_asset(reg);
        tilemap::register_tileset_asset(reg);
        tilemap::register_tilemap_component(reg);
        particles::register_particles_component(reg);
        // SceneSync, and after the core propagation system registered at
        // `App::new`: the camera follows the node's settled global pose.
        reg.add_system(Stage::SceneSync, camera::drive_camera_system);
        // Same stage, after the camera: an outline follows the collider or
        // shape the node has settled on this tick.
        reg.add_system(Stage::SceneSync, light::resolve_occluders_system);
        reg.add_system(Stage::Render, clear_debug_lines_system);

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
    if let Some(shapes) = eng.try_resource::<DrawBuffer2d>() {
        shapes.borrow_mut().shapes.clear();
    }
}

/// Presets over the render components. `2d` is terminal and lowercase; 3D
/// names carry no marker (N/D5).
fn register_render_presets(reg: &mut Registry<'_>) -> Result<()> {
    reg.register_preset(
        "sprite2d",
        balaur_core::presets::preset("A textured 2D quad", &["2d", "render"], &[("sprite", None)])?,
    );
    reg.register_preset(
        "rect2d",
        balaur_core::presets::preset(
            "An untextured 2D rectangle",
            &["2d", "render"],
            &[("shape2d", Some("kind = \"rect\""))],
        )?,
    );
    reg.register_preset(
        "tilemap2d",
        balaur_core::presets::preset(
            "A grid of tiles from one tileset",
            &["2d", "render"],
            &[("tilemap", None)],
        )?,
    );
    reg.register_preset(
        "polygon2d",
        balaur_core::presets::preset(
            "A textured polygon that a rig can deform",
            &["2d", "render", "animation"],
            &[("polygon", None)],
        )?,
    );
    Ok(())
}
