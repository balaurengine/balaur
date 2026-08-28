//! Rendering as a Balaur plugin.
//!
//! The plugin itself is backend-agnostic: it owns the `Renderable` component,
//! the `render` Lua module, and the scene file vocabulary. Backends consume
//! `Renderable` + `GlobalTransform`. The kiss3d/wgpu backend (feature
//! `kiss3d`) owns the OS event loop; headless runs just never draw, which
//! keeps the simulation byte-for-byte identical with and without a window.

use anyhow::{anyhow, Result};
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::mlua::{self, UserDataRef};
use balaur_core::LuaModule;
use balaur_core::{App, Engine, NodeRef, Plugin};

#[cfg(feature = "kiss3d")]
pub mod kiss3d_backend;

/// Ask the windowed backend to save one frame as a PNG once `after_frame`
/// frames have been rendered (debugging, CI golden images, editor
/// thumbnails). Insert as an engine resource; the backend removes it after
/// taking the shot.
pub struct ScreenshotRequest {
    pub path: std::path::PathBuf,
    pub after_frame: u64,
}

/// The application (dock/taskbar) icon, applied by windowed backends.
pub struct AppIcon {
    pub path: std::path::PathBuf,
    pub changed: bool,
}

/// Viewport clear color, applied by windowed backends when changed.
pub struct ClearColor {
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
        GridConfig {
            enabled: false,
            step: 1.0,
            major_every: 5,
            extent: 20,
            minor_color: [0.12, 0.13, 0.15],
            major_color: [0.17, 0.19, 0.22],
        }
    }
}

/// Immediate-mode debug/overlay lines, cleared every frame after drawing.
/// Reusable for editor gizmos, physics debug, navigation debug.
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

#[derive(Default)]
pub struct DebugLines {
    pub lines: Vec<DebugLine>,
}

/// 2D counterpart of [`DebugLines`]: world-space 2D segments rendered with
/// the 2D camera, cleared every frame.
/// (a, b, color, pixel width)
pub type DebugLine2D = ([f32; 2], [f32; 2], [f32; 3], f32);

#[derive(Default)]
pub struct DebugLines2D {
    pub lines: Vec<DebugLine2D>,
}

/// Where the 2D camera looks (world center) and its zoom in logical pixels
/// per world unit. Backends apply it when changed and keep their own
/// interactive pan/zoom in between.
pub struct Camera2DConfig {
    pub center: [f32; 2],
    pub zoom: f32,
    pub changed: bool,
}

impl Default for Camera2DConfig {
    fn default() -> Self {
        Camera2DConfig {
            center: [0.0, 0.0],
            zoom: 60.0,
            changed: false,
        }
    }
}

/// The actual 2D camera state this frame, published by windowed backends
/// (zoom in logical pixels per world unit), plus the mouse in 2D world
/// coordinates for script-side picking.
#[derive(Default)]
pub struct Viewport2D {
    pub center: [f32; 2],
    pub zoom: f32,
    pub mouse_world: [f32; 2],
}

/// The actual camera pose this frame, published by windowed backends so
/// tools (editor gizmos, pickers) can do screen-space math in scripts.
#[derive(Default)]
pub struct ViewportCamera {
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

/// When false, windowed backends inhibit the camera's mouse controls
/// (editors take the pointer over for gizmo drags).
pub struct CameraInputEnabled(pub bool);

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
        CameraConfig {
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
    Circle { radius: f32 },
    Rect { hx: f32, hy: f32 },
}

/// 2D renderable, mirrored into the backend's 2D scene. The node's regular
/// `Transform` drives it: x/y translate, the z rotation spins it, x/y scale.
pub struct Renderable2d {
    pub shape: Shape2d,
    pub color: [f32; 4],
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
    fn name(&self) -> &str {
        "render"
    }

    fn build(&mut self, app: &mut App) -> Result<()> {
        app.engine.insert_resource(CameraConfig::default());
        app.engine.insert_resource(ClearColor {
            color: [0.09, 0.098, 0.11],
            changed: true,
        });
        app.engine.insert_resource(GridConfig::default());
        app.engine.insert_resource(DebugLines::default());
        app.engine.insert_resource(DebugLines2D::default());
        app.engine.insert_resource(Camera2DConfig::default());
        app.engine.insert_resource(Viewport2D::default());
        app.engine.insert_resource(ViewportCamera::default());
        app.engine.insert_resource(CameraInputEnabled(true));
        let m = app.lua_module("render")?;
        install_camera_api(&m, app)?;
        install_scene_api(&m)?;
        install_shape_api(&m)?;
        install_2d_api(&m)?;
        register_shape_component(app);
        register_shape2d_component(app);
        register_color_component(app);

        Ok(())
    }
}

/// Window icon, camera input, matrices, mouse ray and camera pose.
fn install_camera_api(m: &LuaModule, app: &App) -> Result<()> {
    // Set the OS application icon (dock icon on macOS) from a PNG in
    // the project.
    m.function("set_app_icon", |eng, path: String| {
        let root = eng.resource::<balaur_core::project::ProjectRoot>();
        let full = root.borrow().0.join(path);
        eng.insert_resource(AppIcon {
            path: full,
            changed: true,
        });
        Ok(())
    })?;
    m.function("set_camera_input", |eng, enabled: bool| {
        let flag = eng.resource::<CameraInputEnabled>();
        flag.borrow_mut().0 = enabled;
        Ok(())
    })?;
    // The actual camera pose this frame: eye xyz, target xyz, fov (rad),
    // HiDPI scale factor.
    // The camera's exact projection*view matrix (column-major, 16
    // numbers): scripts project points precisely as the renderer does.
    m.table().set("camera_matrix", {
        let eng = app.engine.clone();
        app.engine
            .scripts()
            .expect("script host present")
            .lua()
            .create_function(move |lua, ()| {
                let cam = eng.resource::<ViewportCamera>();
                let cam = cam.borrow();
                let t = lua.create_table()?;
                for (i, v) in cam.view_proj.iter().enumerate() {
                    t.set(i + 1, *v)?;
                }
                Ok(t)
            })?
    })?;
    // Picking ray through the mouse: origin xyz, direction xyz.
    m.function("mouse_ray", |eng, ()| {
        let cam = eng.resource::<ViewportCamera>();
        let cam = cam.borrow();
        Ok((
            cam.ray_origin[0],
            cam.ray_origin[1],
            cam.ray_origin[2],
            cam.ray_dir[0],
            cam.ray_dir[1],
            cam.ray_dir[2],
        ))
    })?;
    m.function("camera_pose", |eng, ()| {
        let cam = eng.resource::<ViewportCamera>();
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
    })?;
    Ok(())
}

/// Background, grid, and 3D debug lines.
fn install_scene_api(m: &LuaModule) -> Result<()> {
    m.function("set_background", |eng, (r, g, b): (f32, f32, f32)| {
        let clear = eng.resource::<ClearColor>();
        let mut clear = clear.borrow_mut();
        clear.color = [r, g, b];
        clear.changed = true;
        Ok(())
    })?;
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
    )?;
    m.function(
        "set_grid_colors",
        |eng, (mr, mg, mb, jr, jg, jb): (f32, f32, f32, f32, f32, f32)| {
            let grid = eng.resource::<GridConfig>();
            let mut grid = grid.borrow_mut();
            grid.minor_color = [mr, mg, mb];
            grid.major_color = [jr, jg, jb];
            Ok(())
        },
    )?;
    // One line for one frame, world space. Width is in pixels; pass
    // perspective = true for distance-scaled width (gizmos want false).
    m.function(
        "draw_line",
        |eng, (x1, y1, z1, x2, y2, z2, r, g, b, width, perspective, on_top): DrawLineArgs| {
            let lines = eng.resource::<DebugLines>();
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
    )?;
    // Returns ("", 0, 0, 0) when the node has no shape.
    Ok(())
}

/// 3D shape and colour access.
fn install_shape_api(m: &LuaModule) -> Result<()> {
    m.function("get_shape", |eng, node: UserDataRef<NodeRef>| {
        let world = eng.world();
        let result = match world.get::<&Renderable>(node.entity) {
            Ok(r) => match r.shape {
                Shape::Ball { radius } => ("ball".to_string(), radius, radius, radius),
                Shape::Cuboid { hx, hy, hz } => ("cuboid".to_string(), hx, hy, hz),
            },
            Err(_) => (String::new(), 0.0, 0.0, 0.0),
        };
        Ok(result)
    })?;
    m.function("get_color", |eng, node: UserDataRef<NodeRef>| {
        let world = eng.world();
        let result = if let Ok(r) = world.get::<&Renderable>(node.entity) {
            (r.color[0], r.color[1], r.color[2], r.color[3])
        } else if let Ok(r) = world.get::<&Renderable2d>(node.entity) {
            (r.color[0], r.color[1], r.color[2], r.color[3])
        } else {
            (1.0, 1.0, 1.0, 1.0)
        };
        Ok(result)
    })?;
    m.function(
        "set_camera",
        |eng, (ex, ey, ez, tx, ty, tz): (f32, f32, f32, f32, f32, f32)| {
            let config = eng.resource::<CameraConfig>();
            let mut config = config.borrow_mut();
            config.eye = glamx::Vec3::new(ex, ey, ez);
            config.target = glamx::Vec3::new(tx, ty, tz);
            config.changed = true;
            Ok(())
        },
    )?;
    // 2D camera: world center + zoom in logical pixels per world unit.
    Ok(())
}

/// The 2D camera, world-space mouse, debug lines and 2D shapes.
fn install_2d_api(m: &LuaModule) -> Result<()> {
    m.function("set_camera_2d", |eng, (cx, cy, zoom): (f32, f32, f32)| {
        let config = eng.resource::<Camera2DConfig>();
        let mut config = config.borrow_mut();
        config.center = [cx, cy];
        config.zoom = zoom.max(0.01);
        config.changed = true;
        Ok(())
    })?;
    // The actual 2D camera state this frame: center xy, zoom.
    m.function("camera_2d", |eng, ()| {
        let vp = eng.resource::<Viewport2D>();
        let vp = vp.borrow();
        Ok((vp.center[0], vp.center[1], vp.zoom))
    })?;
    // The mouse position in 2D world coordinates (picking).
    m.function("mouse_world_2d", |eng, ()| {
        let vp = eng.resource::<Viewport2D>();
        let vp = vp.borrow();
        Ok((vp.mouse_world[0], vp.mouse_world[1]))
    })?;
    // One 2D world-space line for one frame; width in pixels.
    m.function(
        "draw_line_2d",
        |eng,
         (x1, y1, x2, y2, r, g, b, width): (f32, f32, f32, f32, f32, f32, f32, Option<f32>)| {
            let lines = eng.resource::<DebugLines2D>();
            lines.borrow_mut().lines.push((
                [x1, y1],
                [x2, y2],
                [r, g, b],
                width.unwrap_or(1.0),
            ));
            Ok(())
        },
    )?;
    m.function(
        "set_ball",
        |eng, (node, radius): (UserDataRef<NodeRef>, f32)| {
            set_shape(eng, node.entity, Shape::Ball { radius }).map_err(mlua::Error::external)
        },
    )?;
    m.function(
        "set_cuboid",
        |eng, (node, hx, hy, hz): (UserDataRef<NodeRef>, f32, f32, f32)| {
            set_shape(eng, node.entity, Shape::Cuboid { hx, hy, hz }).map_err(mlua::Error::external)
        },
    )?;
    m.function(
        "set_color",
        |eng, (node, r, g, b, a): (UserDataRef<NodeRef>, f32, f32, f32, Option<f32>)| {
            set_color(eng, node.entity, [r, g, b, a.unwrap_or(1.0)]).map_err(mlua::Error::external)
        },
    )?;
    m.function(
        "set_rect",
        |eng, (node, hx, hy): (UserDataRef<NodeRef>, f32, f32)| {
            set_shape2d(eng, node.entity, Shape2d::Rect { hx, hy }).map_err(mlua::Error::external)
        },
    )?;
    m.function(
        "set_circle",
        |eng, (node, radius): (UserDataRef<NodeRef>, f32)| {
            set_shape2d(eng, node.entity, Shape2d::Circle { radius }).map_err(mlua::Error::external)
        },
    )?;
    // Returns ("", 0, 0) when the node has no 2D shape.
    m.function("get_shape2d", |eng, node: UserDataRef<NodeRef>| {
        let world = eng.world();
        let result = match world.get::<&Renderable2d>(node.entity) {
            Ok(r) => match r.shape {
                Shape2d::Circle { radius } => ("circle".to_string(), radius, radius),
                Shape2d::Rect { hx, hy } => ("rect".to_string(), hx, hy),
            },
            Err(_) => (String::new(), 0.0, 0.0),
        };
        Ok(result)
    })?;

    // Components (schema-driven; also usable as scene keys).
    Ok(())
}

/// The `shape` component: 3D primitives, editable from the editor.
fn register_shape_component(app: &mut App) {
    app.register_component(
        "shape",
        ComponentDef {
            schema: ComponentDef::parse_schema(
                r#"kind = { kind = "enum", default = "cuboid", options = ["ball", "cuboid"] }
radius = { kind = "float", default = 0.5, min = 0.01 }
half_extents = { kind = "vec3", default = [0.5, 0.5, 0.5] }"#,
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
                        map.insert("radius".into(), toml::Value::Float(radius as f64));
                    }
                    Shape::Cuboid { hx, hy, hz } => {
                        map.insert("kind".into(), toml::Value::String("cuboid".into()));
                        map.insert(
                            "half_extents".into(),
                            toml::Value::Array(vec![
                                toml::Value::Float(hx as f64),
                                toml::Value::Float(hy as f64),
                                toml::Value::Float(hz as f64),
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
                r#"kind = { kind = "enum", default = "rect", options = ["circle", "rect"] }
radius = { kind = "float", default = 0.5, min = 0.01 }
half_extents = { kind = "vec2", default = [0.5, 0.5] }"#,
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
                    Shape2d::Circle { radius } => {
                        map.insert("kind".into(), toml::Value::String("circle".into()));
                        map.insert("radius".into(), toml::Value::Float(radius as f64));
                    }
                    Shape2d::Rect { hx, hy } => {
                        map.insert("kind".into(), toml::Value::String("rect".into()));
                        map.insert(
                            "half_extents".into(),
                            toml::Value::Array(vec![
                                toml::Value::Float(hx as f64),
                                toml::Value::Float(hy as f64),
                            ]),
                        );
                    }
                }
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
                r#"rgba = { kind = "color", default = [0.8, 0.8, 0.8, 1.0], shorthand = true }"#,
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
                            .map(|c| toml::Value::Float(*c as f64))
                            .collect(),
                    ),
                )])))
            }),
        },
    );
}
