//! The resources a window and its cameras are configured through: what a
//! script asks for, and what a windowed backend applies on the next frame.

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
// Independent switches plus the dirty flag; the window's own mode is the one
// that is an enum, since its four states exclude each other.
#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
pub struct WindowConfig {
    pub mode: balaur_core::project::WindowMode,
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
    /// What the engine's own finishing passes are turned by, whichever of
    /// them the chain names.
    pub finish: crate::camera::Finish,
    /// What the occlusion pass measures with, which a scene's own scale
    /// decides.
    pub occlusion: crate::camera::Occlusion,
    /// `material` assets drawn over the whole frame before the tonemap, in the
    /// order the camera listed them: these work in linear light, so what they
    /// write is what blooms.
    pub film: Vec<String>,
    /// The same, after the tonemap, over the finished picture.
    pub screen: Vec<String>,
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
            finish: crate::camera::Finish::default(),
            occlusion: crate::camera::Occlusion::default(),
            film: Vec::new(),
            screen: Vec::new(),
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
            // Asserted at boot, as [`CameraConfig3d`] is: a scene writing the
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
/// Read-only: write [`CameraConfig3d`] instead.
///
/// With no windowed backend running it keeps its `Default` — an all-zero
/// pose, a zero `fov` and an all-zero `view_proj`, which is not invertible.
/// Headless screen-space math gets zeros, not the camera the scene would
/// have had. The scale is one: every caller divides by it.
pub struct ViewportSnapshot3d {
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
    /// The window this was drawn into, in logical points. Zero headless.
    pub width: u32,
    pub height: u32,
}

impl Default for ViewportSnapshot3d {
    fn default() -> Self {
        Self {
            eye: [0.0; 3],
            target: [0.0; 3],
            fov: 0.0,
            scale_factor: 1.0,
            view_proj: [0.0; 16],
            ray_origin: [0.0; 3],
            ray_dir: [0.0; 3],
            width: 0,
            height: 0,
        }
    }
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
pub struct CameraConfig3d {
    pub eye: glamx::Vec3,
    pub target: glamx::Vec3,
    pub changed: bool,
}

impl Default for CameraConfig3d {
    fn default() -> Self {
        Self {
            eye: glamx::Vec3::new(8.0, 5.0, 12.0),
            target: glamx::Vec3::new(0.0, 1.0, 0.0),
            changed: true,
        }
    }
}
