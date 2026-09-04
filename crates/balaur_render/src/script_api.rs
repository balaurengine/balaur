//! The `render` script module's bindings: the 3D and 2D cameras, the OS
//! window, the backdrop, and the sprite/texture surface a node draws with.
//!
//! Split out of `lib.rs`, which keeps the plugin, its components and its
//! scene-file keys; nothing here is called from outside `RenderPlugin::build`.

use anyhow::anyhow;
use balaur_core::entity_of;
use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::{
    set_color, set_shape2d, set_sprite, AppIconConfig, CameraConfig, CameraConfig2d,
    CameraInputConfig, ClearColorConfig, DebugLineBuffer, DebugLineBuffer2d, DrawLineArgs,
    GridConfig, Renderable, Renderable2d, ScreenshotRequest, Shape, Shape2d, SpriteSheet2d,
    SpriteTexture, ViewportSnapshot, ViewportSnapshot2d, WindowConfig, DEFAULT_PIXELS_PER_UNIT,
};

/// The 3D camera: where it looks from, whether it takes the mouse, and the
/// pose, matrix and picking ray it published this frame.
pub(crate) fn install_camera_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_camera", &[], "", "Point the 3D camera: the eye position xyz, then the world point it looks at, in world units."),
        ("set_camera_input", &[], "", "Allow or inhibit the backend's own mouse camera controls, so an editor can take the pointer for a drag."),
        ("camera_matrix", &[], "", "The camera's projection*view matrix this frame, 16 numbers column-major; all zeros with no window."),
        ("mouse_ray", &[], "", "The picking ray through the mouse position: its origin xyz then its direction xyz, in world units."),
        ("pick_ray", &[], "", "The nearest node with a 3D shape that a world-space ray meets, from its origin xyz and direction xyz."),
        ("camera_pose", &[], "", "The camera the renderer actually used: eye xyz, target xyz, vertical fov in radians, HiDPI scale."),
    ]);
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
pub(crate) fn install_camera_2d_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_camera_2d", &[], "", "Point the 2D camera: the world centre xy, then the zoom in logical pixels per world unit."),
        ("camera_2d", &[], "", "The 2D camera this frame: centre xy and zoom in logical pixels per world unit; all zeros with no window."),
        ("mouse_world_2d", &[], "", "The mouse position in 2D world coordinates, for picking."),
    ]);
    // World center + zoom in logical pixels per world unit. Not an accessor
    // pair with `render.camera_2d`, which reads the published snapshot.
    m.function(
        "set_camera_2d",
        |eng: &Engine, (cx, cy, zoom): (f32, f32, f32)| {
            let config = eng.resource::<CameraConfig2d>();
            let mut config = config.borrow_mut();
            config.center = [cx, cy];
            // The floor the `camera` component's schema states, so a script
            // and a scene file cannot disagree about the same number.
            config.zoom = zoom.max(crate::camera::MIN_ZOOM_2D);
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
pub(crate) fn resolve_project_path(eng: &Engine, path: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    eng.try_resource::<balaur_core::project::ProjectRoot>()
        .map_or_else(|| p.to_path_buf(), |root| root.borrow().0.join(p))
}

pub(crate) fn install_window_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("screenshot", &[], "", "Save the next rendered frame as a PNG at a project-relative path; a run with no renderer says so."),
        ("set_app_icon", &[], "", "Set the application icon (the dock or taskbar one) from a PNG in the project, named by its path."),
        ("set_fullscreen", &[], "", "Put the window into borderless fullscreen on the current monitor, or back into a window."),
        ("set_cursor_grab", &[], "", "Confine the cursor to the window, for FPS-style mouse look."),
        ("set_cursor_hidden", &[], "", "Hide or show the mouse cursor over the window."),
    ]);
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
pub(crate) fn install_backdrop_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_background", &[], "", "Set the colour the viewport is cleared to behind everything drawn, as r, g, b channel floats."),
        ("set_grid", &[], "", "Turn the ground grid on or off, and optionally set its step in world units, major-line interval and extent."),
        ("set_grid_colors", &[], "", "Set the ground grid's minor line colour then its major line colour, as r, g, b channel floats."),
        ("draw_line", &[], "", "Draw one 3D world-space line for this frame; the width is in pixels unless perspective scales it with distance."),
        ("draw_line_2d", &[], "", "Draw one 2D world-space line for this frame; width is in pixels."),
    ]);
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
pub(crate) fn install_sprite_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_sprite", &["sprite"], "", "Draw the node as a quad textured with a project image, sized from it at 100 texture pixels per world unit."),
        ("set_sprite_sheet", &["sprite"], "", "Draw the node as one cell of a columns-by-rows sheet, sizing the quad to a single frame, not the whole image."),
    ]);
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
pub(crate) fn install_texture_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "texture_size",
        &[],
        "",
        "An image's width and height in pixels, read from the file's own header.",
    )]);
    m.function("texture_size", |eng: &Engine, path: String| {
        let bytes = eng
            .resource::<balaur_core::project::ProjectFiles>()
            .borrow()
            .read(&path)?;
        crate::texture::image_size(&bytes, &path)
    });
}

pub(crate) fn install_sprite_state_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_sprite_frame", &["sprite"], "", "Show a sheet cell, numbered left to right then top to bottom; only the UVs move, so it is cheap per frame."),
        ("set_sprite_size", &["sprite"], "", "Override the size the sprite took from its image, giving half-extents in world units instead."),
        ("sprite", &["sprite"], "", "The texture path, sheet columns and rows, and current frame; empty and zeros when the node has no sprite."),
        ("set_circle", &["shape2d"], "", "Draw the node as a circle of the given radius in world units, replacing any other 2D shape."),
        ("set_color", &["sprite", "shape2d", "shape3d", "polygon", "particles"], "", "Tint whatever the node draws, as r, g, b channel floats and an optional alpha, one meaning opaque."),
        ("shape3d", &["shape3d"], "", "The 3D shape's kind and its three dimensions in world units; empty and zeros when the node has no 3D shape."),
        ("shape2d", &["shape2d"], "", "The 2D shape's kind and its two dimensions in world units; empty and zeros when the node has no 2D shape."),
    ]);
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
            r.sized = true;
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
    install_shape_readers(m);
}

/// What a node's shape is, for a tool that has to show it: the 3D kind with
/// its dimensions, the 2D kind with its half extents.
fn install_shape_readers(m: &mut dyn Bindings<Engine>) {
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
