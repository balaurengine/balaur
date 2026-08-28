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
#[derive(Default)]
pub struct DebugLines {
    /// (a, b, color, pixel width, perspective-correct width)
    pub lines: Vec<([f32; 3], [f32; 3], [f32; 3], f32, bool)>,
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

fn set_color(eng: &Engine, entity: Entity, color: [f32; 4]) -> Result<()> {
    let world = eng.world_mut();
    let mut r = world
        .get::<&mut Renderable>(entity)
        .map_err(|_| anyhow!("node has no shape yet"))?;
    r.color = color;
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
        app.engine.insert_resource(ViewportCamera::default());
        app.engine.insert_resource(CameraInputEnabled(true));
        let m = app.lua_module("render")?;
        m.function("set_camera_input", |eng, enabled: bool| {
            let flag = eng.resource::<CameraInputEnabled>();
            flag.borrow_mut().0 = enabled;
            Ok(())
        })?;
        // The actual camera pose this frame: eye xyz, target xyz, fov (rad),
        // HiDPI scale factor.
        m.function("camera_pose", |eng, ()| {
            let cam = eng.resource::<ViewportCamera>();
            let cam = cam.borrow();
            Ok((
                cam.eye[0], cam.eye[1], cam.eye[2],
                cam.target[0], cam.target[1], cam.target[2],
                cam.fov, cam.scale_factor,
            ))
        })?;
        m.function("set_background", |eng, (r, g, b): (f32, f32, f32)| {
            let clear = eng.resource::<ClearColor>();
            let mut clear = clear.borrow_mut();
            clear.color = [r, g, b];
            clear.changed = true;
            Ok(())
        })?;
        m.function(
            "set_grid",
            |eng, (enabled, step, major_every, extent): (bool, Option<f32>, Option<u32>, Option<i32>)| {
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
            |eng,
             (x1, y1, z1, x2, y2, z2, r, g, b, width, perspective): (
                f32, f32, f32, f32, f32, f32, f32, f32, f32, Option<f32>, Option<bool>,
            )| {
                let lines = eng.resource::<DebugLines>();
                lines.borrow_mut().lines.push((
                    [x1, y1, z1],
                    [x2, y2, z2],
                    [r, g, b],
                    width.unwrap_or(1.0),
                    perspective.unwrap_or(false),
                ));
                Ok(())
            },
        )?;
        // Returns ("", 0, 0, 0) when the node has no shape.
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
            let result = match world.get::<&Renderable>(node.entity) {
                Ok(r) => (r.color[0], r.color[1], r.color[2], r.color[3]),
                Err(_) => (1.0, 1.0, 1.0, 1.0),
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
        m.function(
            "set_ball",
            |eng, (node, radius): (UserDataRef<NodeRef>, f32)| {
                set_shape(eng, node.entity, Shape::Ball { radius })
                    .map_err(mlua::Error::external)
            },
        )?;
        m.function(
            "set_cuboid",
            |eng, (node, hx, hy, hz): (UserDataRef<NodeRef>, f32, f32, f32)| {
                set_shape(eng, node.entity, Shape::Cuboid { hx, hy, hz })
                    .map_err(mlua::Error::external)
            },
        )?;
        m.function(
            "set_color",
            |eng, (node, r, g, b, a): (UserDataRef<NodeRef>, f32, f32, f32, Option<f32>)| {
                set_color(eng, node.entity, [r, g, b, a.unwrap_or(1.0)])
                    .map_err(mlua::Error::external)
            },
        )?;

        // Components (schema-driven; also usable as scene keys).
        app.register_component(
            "shape",
            ComponentDef {
                schema: ComponentDef::parse_schema(
                    r#"kind = { kind = "enum", default = "cuboid", options = ["ball", "cuboid"] }
radius = { kind = "float", default = 0.5, min = 0.01 }
half_extents = { kind = "vec3", default = [0.5, 0.5, 0.5] }"#,
                ),
                apply: Box::new(|eng, entity, params| {
                    let kind = params.get("kind").and_then(|v| v.as_str()).unwrap_or("cuboid");
                    let radius = params
                        .get("radius")
                        .and_then(|v| v.as_float())
                        .unwrap_or(0.5) as f32;
                    let he = |i: usize| {
                        params
                            .get("half_extents")
                            .and_then(|v| v.as_array())
                            .and_then(|a| a.get(i))
                            .and_then(|v| v.as_float())
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
                            .and_then(|v| v.as_float())
                            .unwrap_or(default) as f32
                    };
                    set_color(eng, entity, [c(0, 0.8), c(1, 0.8), c(2, 0.8), c(3, 1.0)])
                }),
                remove: Box::new(|eng, entity| {
                    set_color(eng, entity, [0.8, 0.8, 0.8, 1.0])
                }),
                get: Box::new(|eng, entity| {
                    let world = eng.world();
                    let renderable = world.get::<&Renderable>(entity).ok()?;
                    Some(toml::Value::Table(toml::map::Map::from_iter([(
                        "rgba".to_string(),
                        toml::Value::Array(
                            renderable
                                .color
                                .iter()
                                .map(|c| toml::Value::Float(*c as f64))
                                .collect(),
                        ),
                    )])))
                }),
            },
        );
        Ok(())
    }
}
