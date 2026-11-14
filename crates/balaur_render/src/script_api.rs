//! The `render` script module's bindings: the 3D and 2D cameras, the backdrop,
//! and the sprite/texture surface a node draws with. Also the `window` module.
//!
//! Split out of `lib.rs`, which keeps the plugin, its components and its
//! scene-file keys; nothing here is called from outside `RenderPlugin::build`.

use anyhow::Context;
use balaur_core::Engine;
use balaur_plugin::Registry;
use balaur_script::{Module, Value};

use crate::draw_2d::vec2_of;
use crate::vocabulary::keys;
use crate::vocabulary::words as vocab;
use crate::{
    AppIconConfig, ClearColorConfig, DEFAULT_PIXELS_PER_UNIT, DebugLineBuffer2d, DebugLineBuffer3d,
    ScreenshotRequest, ViewportSnapshot2d, ViewportSnapshot3d, WindowConfig,
};

const BOUNDS: &str = "bounds";
const CAMERA_2D: &str = "camera_2d";
const CAMERA_MATRIX: &str = "camera_matrix";
const CAMERA_POSE: &str = "camera_pose";
const DRAW_BOX_3D: &str = "draw_box_3d";
const DRAW_CAPSULE_3D: &str = "draw_capsule_3d";
const DRAW_LINE_2D: &str = "draw_line_2d";
const DRAW_LINE_3D: &str = "draw_line_3d";
const DRAW_LINES_3D: &str = "draw_lines_3d";
const DRAW_SPHERE_3D: &str = "draw_sphere_3d";
const DRAW_TEXT_2D: &str = "draw_text_2d";
const DRAW_TEXT_3D: &str = "draw_text_3d";
const HOLD_CAMERAS: &str = "hold_cameras";
const MOUSE_RAY: &str = "mouse_ray";
const MOUSE_WORLD_2D: &str = "mouse_world_2d";
const PICK_RAY: &str = "pick_ray";
const PICK_RAY_AT: &str = "pick_ray_at";
const REFRESH_RATE: &str = "refresh_rate";
const SAFE_AREA: &str = "safe_area";
const SCREENSHOT: &str = "screenshot";
const SET_APP_ICON: &str = "set_app_icon";
const SET_BACKGROUND: &str = "set_background";
const SET_CURSOR_GRAB: &str = "set_cursor_grab";
const SET_CURSOR_HIDDEN: &str = "set_cursor_hidden";
const SET_KEEP_AWAKE: &str = "set_keep_awake";
const SET_WINDOW_MODE: &str = "set_window_mode";
const SNAP_AOV: &str = "snap_aov";
const TEXT_SIZE: &str = "text_size";
const TEXTURE_PIXELS_PER_UNIT: &str = "texture_pixels_per_unit";
const TEXTURE_SIZE: &str = "texture_size";
const TRACE_TEXTURE: &str = "trace_texture";

/// The fields of the records this file answers in and reads.
const DIRECTION: &str = "direction";
const EYE: &str = "eye";
const FOV: &str = "fov";
const FROM: &str = "from";
const ON_TOP: &str = "on_top";
const SCALE_FACTOR: &str = "scale_factor";
const TARGET: &str = "target";
const TO: &str = "to";
const WIDTH: &str = "width";
const ZOOM: &str = "zoom";

/// A script's `Vec3` argument; `what` opens the error for anything else.
fn vec3_of(what: &str, value: &Value) -> anyhow::Result<[f32; 3]> {
    match value {
        Value::Vec3(v) => Ok(*v),
        other => anyhow::bail!("{what} is a Vec3, not a {}", other.type_name()),
    }
}

/// A line's colour, which it has to be given: a `Color`, alpha included.
fn line_color(value: &Value) -> anyhow::Result<[f32; 4]> {
    match value {
        Value::Color(rgba) => Ok(*rgba),
        other => anyhow::bail!("a line's colour is a Color, not a {}", other.type_name()),
    }
}

/// Queue one block of text for this frame. `pixels_per_unit` sizes the quad;
/// the rest of `opts` is the shaper's.
fn push_text(
    eng: &Engine,
    at: [f32; 3],
    text: String,
    opts: Option<Value>,
    in_3d: bool,
) -> anyhow::Result<()> {
    let number = |name: &str| match &opts {
        Some(Value::Map(entries)) => {
            entries
                .iter()
                .find(|(key, _)| key == name)
                .and_then(|(_, value)| match value {
                    Value::Num(n) => Some(*n as f32),
                    Value::Int(n) => Some(*n as f32),
                    _ => None,
                })
        }
        _ => None,
    };
    let pixels_per_unit = number(crate::vocabulary::keys::PIXELS_PER_UNIT);
    let z_index = number(crate::vocabulary::keys::Z_INDEX).map(|z| z as i32);
    let style = crate::world_text::style_of(opts)?;
    let buffer = eng.resource::<crate::world_text::TextDrawBuffer>();
    buffer.borrow_mut().items.push(crate::world_text::TextDraw {
        text,
        at,
        style,
        pixels_per_unit: pixels_per_unit.unwrap_or(DEFAULT_PIXELS_PER_UNIT).max(1.0),
        in_3d,
        z_index,
    });
    Ok(())
}

/// The 3D camera: where it looks from, and the pose, matrix and picking ray
/// it published this frame.
pub(crate) fn install_camera_api(m: &mut Module<Engine>) {
    m.describe(&[
        (CAMERA_MATRIX, &[], "", "The camera's projection*view matrix this frame, 16 numbers column-major; all zeros with no window."),
        (CAMERA_POSE, &[], "() -> CameraPose", "The camera the renderer actually used, which is where a backend put it and not what a `camera3d` asked: its `eye` and `target`, the vertical `fov` in radians and the HiDPI `scale_factor`. Zeros and a scale of one with no window."),
        (HOLD_CAMERAS, &[], "(root: node?)", "Let no camera under a node drive the view, whatever its `current` says, until it is called with nil: an editor looking at a game through its own camera. The cameras keep what the scene asked of them."),
        (BOUNDS, &[], "(node: node) -> [vec3]?", "The box the node's geometry covers in its own space, as a centre xyz and half-extents xyz, with a z of zero for a 2D node; nil for a node that draws nothing. A solver's body reports where it is now, not where it was built; a text, the lines it was last laid out in."),
    ]);
    // The camera's exact projection*view matrix (column-major, 16
    // numbers): scripts project points precisely as the renderer does.
    m.function(CAMERA_MATRIX, |eng: &Engine, ()| {
        let cam = eng.resource::<ViewportSnapshot3d>();
        let view_proj = cam.borrow().view_proj;
        Ok(view_proj.to_vec())
    });
    m.records(&[(
        "CameraPose",
        "{ eye: Vec3, target: Vec3, fov: float, scale_factor: float }",
        "Where the renderer's 3D camera stood this frame and what it looked at, its vertical field of view in radians, and how many pixels the display draws a logical one with.",
    )]);
    // Reads the published snapshot: zeros and a scale of one with no window.
    m.function(CAMERA_POSE, |eng: &Engine, ()| {
        let cam = eng.resource::<ViewportSnapshot3d>();
        let cam = cam.borrow();
        Ok(Value::Map(vec![
            (EYE.to_string(), Value::Vec3(cam.eye)),
            (TARGET.to_string(), Value::Vec3(cam.target)),
            (FOV.to_string(), Value::Num(f64::from(cam.fov))),
            (
                SCALE_FACTOR.to_string(),
                Value::Num(f64::from(cam.scale_factor)),
            ),
        ]))
    });
    m.function(
        HOLD_CAMERAS,
        |eng: &Engine, root: Option<balaur_script::NodeId>| {
            let root = root.map(balaur_core::entity_of).transpose()?;
            eng.resource::<crate::camera::HeldCameras>().borrow_mut().0 = root;
            Ok(())
        },
    );
    // What a node covers, which an authored size cannot say for a mesh, a
    // body a solver deforms, or a sprite sized by its image.
    m.function(BOUNDS, |eng: &Engine, node: balaur_script::NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let world = eng.world();
        let found = match world.get::<&crate::Renderable3d>(entity) {
            Ok(renderable) => renderable.bounds.map(|b| (b.centre, b.half)),
            Err(_) => world
                .get::<&crate::Renderable2d>(entity)
                .ok()
                .and_then(|renderable| crate::pick::half_extents_2d(&renderable))
                .map(|(hx, hy)| (glamx::Vec3::ZERO, glamx::Vec3::new(hx, hy, 0.0)))
                .or_else(|| {
                    world
                        .get::<&crate::text_component::LaidText>(entity)
                        .ok()
                        .map(|laid| (laid.centre, laid.half))
                }),
        };
        let Some((centre, half)) = found else {
            return Ok(balaur_script::Value::Nil);
        };
        Ok(balaur_script::Value::List(vec![
            balaur_script::Value::Vec3(centre.to_array()),
            balaur_script::Value::Vec3(half.to_array()),
        ]))
    });
    install_pick_api(m);
    install_screenshot_api(m);
}

/// Picking: the ray through the mouse, and what a ray meets first.
fn install_pick_api(m: &mut Module<Engine>) {
    m.describe(&[
        (MOUSE_RAY, &[], "() -> Ray", "The picking ray through the mouse position, in world units: where it starts and which way it goes."),
        (PICK_RAY, &[], "(origin: Vec3, direction: Vec3) -> node", "The nearest node with a 3D shape that a world-space ray meets, from its origin along its direction."),
        (PICK_RAY_AT, &[], "(origin: Vec3, direction: Vec3) -> Hit?", "As `pick_ray`, but `node` and `at`, how far along the ray it was met; nil when the ray meets nothing."),
    ]);
    m.records(&[
        (
            "Hit",
            "{ node: node, at: float }",
            "The nearest node a ray met and how far along the ray it was met.",
        ),
        (
            "Ray",
            "{ origin: Vec3, direction: Vec3 }",
            "A ray in world units: where it starts and which way it goes.",
        ),
    ]);
    m.function(MOUSE_RAY, |eng: &Engine, ()| {
        let cam = eng.resource::<ViewportSnapshot3d>();
        let cam = cam.borrow();
        Ok(Value::Map(vec![
            (keys::ORIGIN.to_string(), Value::Vec3(cam.ray_origin)),
            (DIRECTION.to_string(), Value::Vec3(cam.ray_dir)),
        ]))
    });
    // What a ray meets first, or `()`. The ray is an argument rather than
    // the mouse because the viewport snapshot is empty without a window,
    // and the editor's own tests run headless.
    m.function(
        PICK_RAY,
        |eng: &Engine, (origin, direction): (Value, Value)| {
            let origin = glamx::Vec3::from_array(vec3_of("a ray's origin", &origin)?);
            let dir = glamx::Vec3::from_array(vec3_of("a ray's direction", &direction)?);
            Ok(crate::pick::along_ray(eng, origin, dir)
                .map(|(entity, _)| balaur_core::node_id_of(entity)))
        },
    );
    // The distance too, which `pick_ray` drops: a caller deciding between two
    // things under the pointer needs to know which one is in front.
    m.function(
        PICK_RAY_AT,
        |eng: &Engine, (origin, direction): (Value, Value)| {
            let origin = glamx::Vec3::from_array(vec3_of("a ray's origin", &origin)?);
            let dir = glamx::Vec3::from_array(vec3_of("a ray's direction", &direction)?);
            let Some((entity, at)) = crate::pick::along_ray(eng, origin, dir) else {
                return Ok(balaur_script::Value::Nil);
            };
            Ok(balaur_script::Value::Map(vec![
                (
                    "node".to_string(),
                    balaur_script::Value::Node(balaur_core::node_id_of(entity).0),
                ),
                ("at".to_string(), balaur_script::Value::Num(f64::from(at))),
            ]))
        },
    );
}

/// The 2D camera: its target center and zoom, the state it published this
/// frame, and the mouse unprojected through it.
pub(crate) fn install_camera_2d_api(m: &mut Module<Engine>) {
    m.describe(&[
        (CAMERA_2D, &[], "() -> Viewport2d", "The 2D camera this frame: its `center` and its `zoom` in logical pixels per world unit; all zeros with no window."),
        (MOUSE_WORLD_2D, &[], "", "The mouse position in 2D world coordinates, for picking."),
    ]);
    // Center xy, zoom, from the published snapshot rather than the config —
    // so headless answers all zeros, zoom included, not the config's default.
    m.records(&[(
        "Viewport2d",
        "{ center: Vec2, zoom: float }",
        "What the 2D camera showed this frame: the world point at the middle of the view, and how many logical pixels a world unit took.",
    )]);
    m.function(CAMERA_2D, |eng: &Engine, ()| {
        let vp = eng.resource::<ViewportSnapshot2d>();
        let vp = vp.borrow();
        Ok(Value::Map(vec![
            (vocab::CENTER.to_string(), Value::Vec2(vp.center)),
            (ZOOM.to_string(), Value::Num(f64::from(vp.zoom))),
        ]))
    });
    // The mouse position in 2D world coordinates (picking).
    m.function(MOUSE_WORLD_2D, |eng: &Engine, ()| {
        let vp = eng.resource::<ViewportSnapshot2d>();
        let vp = vp.borrow();
        Ok((vp.mouse_world[0], vp.mouse_world[1]))
    });
}

/// A script-supplied path against the project, absolute paths left alone.
pub(crate) fn resolve_project_path(eng: &Engine, path: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(path);
    if balaur_core::files::rooted(p) {
        return p.to_path_buf();
    }
    eng.try_resource::<balaur_core::project::ProjectRoot>()
        .map_or_else(|| p.to_path_buf(), |root| root.borrow().0.join(p))
}

/// `render.screenshot`: the next rendered frame, to a PNG.
fn install_screenshot_api(m: &mut Module<Engine>) {
    m.describe(&[
        (
            SCREENSHOT,
            &[],
            "",
            "Save the next rendered frame as a PNG at a project-relative path. `screenshot_saved` goes to every listener with the path once it is on disk, and `screenshot_failed` with `{ path, error }` when it cannot be, a run with no renderer included.",
        ),
        (
            SNAP_AOV,
            &[],
            "(kind: string, path: string)",
            "Render the 3D scene once more as `render.AOV_DEPTH` (nearest brightest), `AOV_NORMALS` (world space), `AOV_CAMERA_NORMALS` or `AOV_SEGMENTATION` (a colour per object) and save it as a PNG, announcing it as `screenshot` does. A read-back of the GPU, as `screenshot` is.",
        ),
    ]);
    m.function(SCREENSHOT, |eng: &Engine, path: String| {
        let full = resolve_project_path(eng, &path);
        eng.insert_resource(ScreenshotRequest {
            path: full,
            // The next rendered frame. No delay parameter on purpose: a
            // script cannot read the backend's own counter anyway.
            after_frame: 0,
            aov: None,
        });
        Ok(())
    });
    m.function(SNAP_AOV, |eng: &Engine, (kind, path): (String, String)| {
        use crate::vocabulary::words;
        let aov = match kind.as_str() {
            words::AOV_DEPTH => crate::Aov::Depth,
            words::AOV_NORMALS => crate::Aov::Normals,
            words::AOV_CAMERA_NORMALS => crate::Aov::CameraNormals,
            words::AOV_SEGMENTATION => crate::Aov::Segmentation,
            other => anyhow::bail!(
                "snap_aov takes {}, not '{other}'",
                crate::vocabulary::options(words::AOVS)
            ),
        };
        eng.insert_resource(ScreenshotRequest {
            path: resolve_project_path(eng, &path),
            after_frame: 0,
            aov: Some(aov),
        });
        Ok(())
    });
}

/// The `window` script module, beside `render` in the same plugin.
pub(crate) fn register_window_module(reg: &mut Registry<'_>) -> anyhow::Result<()> {
    let mut m = reg.script_module("window")?;
    m.module_doc(
        "The OS window and the display under it: mode, cursor, app icon, keep-awake, the safe area and the refresh rate.",
    );
    install_window_api(&mut m);
    Ok(())
}

/// The OS window and the display it sits on.
fn install_window_api(m: &mut Module<Engine>) {
    m.describe(&[
        (SET_APP_ICON, &[], "(path: string, opts: AppIconOptions?)", "Set the application icon, the dock or taskbar one, from a PNG or an SVG in the project. `opts.plate` colours the disc a macOS dock icon sits on, white by default. Replaces `[application] icon` for the run."),
        (SET_WINDOW_MODE, &[], "(mode: string)", "`windowed`, `maximized`, `fullscreen` (borderless) or `exclusive` (the monitor's largest video mode): the same choice as `[window] mode`."),
        (SET_CURSOR_GRAB, &[], "", "Confine the cursor to the window, for FPS-style mouse look."),
        (SET_CURSOR_HIDDEN, &[], "", "Hide or show the mouse cursor over the window."),
        (SET_KEEP_AWAKE, &[], "(on: bool)", "Keep the screen from dimming while the game runs: a page takes a wake lock, a phone its equivalent, a desktop needs nothing."),
        (SAFE_AREA, &[], "() -> SafeArea", "The display's insets in pixels, `{ left, top, right, bottom }`: what a notch or a home bar covers, read once per frame and recorded. Zero on a desktop."),
        (REFRESH_RATE, &[], "() -> float", "Frames per second the display refreshes at, as measured over the last frames; 60 with no window."),
    ]);
    m.records(&[
        (
            "AppIconOptions",
            "{ plate: Color? }",
            "The colour of the disc a macOS dock icon sits on, `plate`; white unless given.",
        ),
        (
            "SafeArea",
            "{ left: float, top: float, right: float, bottom: float }",
            "The display's insets in pixels: what a notch or a home bar covers.",
        ),
    ]);
    // OS application icon (dock icon on macOS) from a PNG in the project.
    // No reader by design: add `app_icon` when a caller needs it back.
    m.function(
        SET_APP_ICON,
        |eng: &Engine, (path, opts): (String, Option<Value>)| {
            let plate = plate_of(opts)?;
            let bytes = eng
                .resource::<balaur_core::project::ProjectFiles>()
                .borrow()
                .read(&path)?;
            eng.insert_resource(AppIconConfig {
                bytes,
                name: path,
                plate,
                changed: true,
            });
            #[cfg(feature = "window")]
            eng.remove_resource::<crate::app_icon::SettingsIconState>();
            Ok(())
        },
    );
    // No readers by design (N8): `WindowConfig` already holds what a backend
    // last applied.
    m.function(SET_WINDOW_MODE, |eng: &Engine, mode: String| {
        let Some(mode) = balaur_core::project::WindowMode::parse(&mode) else {
            anyhow::bail!("no window mode '{mode}': windowed, maximized, fullscreen or exclusive");
        };
        let config = eng.resource::<WindowConfig>();
        let mut config = config.borrow_mut();
        config.mode = mode;
        config.changed = true;
        Ok(())
    });
    // Confine the cursor to the window — FPS-style mouse capture, usually
    // paired with `set_cursor_hidden` and `input.mouse_delta`.
    m.function(SET_CURSOR_GRAB, |eng: &Engine, grab: bool| {
        let config = eng.resource::<WindowConfig>();
        let mut config = config.borrow_mut();
        config.cursor_grabbed = grab;
        config.changed = true;
        Ok(())
    });
    m.function(SET_CURSOR_HIDDEN, |eng: &Engine, hidden: bool| {
        let config = eng.resource::<WindowConfig>();
        let mut config = config.borrow_mut();
        config.cursor_hidden = hidden;
        config.changed = true;
        Ok(())
    });
    m.function(SET_KEEP_AWAKE, |eng: &Engine, on: bool| {
        let config = eng.resource::<WindowConfig>();
        let mut config = config.borrow_mut();
        config.keep_awake = on;
        config.changed = true;
        Ok(())
    });
    m.function(SAFE_AREA, |eng: &Engine, ()| {
        let [left, top, right, bottom] = balaur_core::facts::device(eng).safe_area;
        let num = |v: f32| balaur_script::Value::Num(f64::from(v));
        Ok(balaur_script::Value::Map(vec![
            ("left".into(), num(left)),
            ("top".into(), num(top)),
            ("right".into(), num(right)),
            ("bottom".into(), num(bottom)),
        ]))
    });
    m.function(REFRESH_RATE, |eng: &Engine, ()| {
        Ok(f64::from(balaur_core::facts::device(eng).refresh_rate))
    });
}

/// What is drawn behind and around the scene: clear colour, ground grid,
/// and the per-frame debug lines (3D and 2D).
/// The text calls are here rather than in a module of their own because they
/// are the same shape as the lines above them: one frame, no node, unrecorded.
#[allow(
    clippy::too_many_lines,
    reason = "one registration per call, and they belong beside the lines"
)]
/// The rgb an immediate 3D shape draws in; white when the call named none.
/// `set_app_icon`'s options: the plate a macOS dock icon sits on.
fn plate_of(opts: Option<Value>) -> anyhow::Result<[u8; 4]> {
    let entries = match opts {
        None | Some(Value::Nil) => return Ok(crate::config::WHITE_PLATE),
        Some(Value::Map(entries)) => entries,
        Some(other) => anyhow::bail!("set_app_icon's options are a table, got {other:?}"),
    };
    let mut plate = crate::config::WHITE_PLATE;
    for (key, value) in &entries {
        match key.as_str() {
            crate::vocabulary::keys::PLATE => {
                let [r, g, b, a] = crate::draw_2d::color_of(value)?;
                let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
                plate = [byte(r), byte(g), byte(b), byte(a)];
            }
            other => anyhow::bail!(
                "set_app_icon takes {}, not '{other}'",
                crate::vocabulary::keys::PLATE
            ),
        }
    }
    Ok(plate)
}

fn line_rgba(color: Option<&Value>) -> anyhow::Result<[f32; 4]> {
    let Some(value) = color else {
        return Ok([1.0; 4]);
    };
    crate::draw_2d::color_of(value)
}

/// Segments into the frame's debug lines, one pixel wide and not on top, so
/// an immediate shape sits in the scene as a mesh would.
fn push_segments(eng: &Engine, segments: &[([f32; 3], [f32; 3])], rgba: [f32; 4]) {
    let lines = eng.resource::<DebugLineBuffer3d>();
    let mut lines = lines.borrow_mut();
    for (from, to) in segments {
        lines.lines.push((*from, *to, rgba, 1.0, false, false));
    }
}

/// The twelve edges of an axis-aligned box.
fn box_edges(center: [f32; 3], half: [f32; 3]) -> Vec<([f32; 3], [f32; 3])> {
    let corner = |i: usize| {
        let sign = |bit: usize| if i & (1 << bit) == 0 { -1.0 } else { 1.0 };
        [
            center[0] + sign(0) * half[0],
            center[1] + sign(1) * half[1],
            center[2] + sign(2) * half[2],
        ]
    };
    let mut out = Vec::with_capacity(12);
    for i in 0..8u32 {
        for bit in 0..3 {
            let j = i ^ (1 << bit);
            if j > i {
                out.push((corner(i as usize), corner(j as usize)));
            }
        }
    }
    out
}

/// A ring of segments about one axis: 0 is the yz plane, 1 the xz, 2 the xy.
fn ring(center: [f32; 3], radius: f32, axis: usize, out: &mut Vec<([f32; 3], [f32; 3])>) {
    const STEPS: usize = 24;
    let point = |step: usize| {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a step index of at most 24 is exact in f32"
        )]
        let angle = std::f32::consts::TAU * step as f32 / STEPS as f32;
        let (s, c) = angle.sin_cos();
        match axis {
            0 => [center[0], center[1] + c * radius, center[2] + s * radius],
            1 => [center[0] + c * radius, center[1], center[2] + s * radius],
            _ => [center[0] + c * radius, center[1] + s * radius, center[2]],
        }
    };
    for step in 0..STEPS {
        out.push((point(step), point(step + 1)));
    }
}

pub(crate) fn install_backdrop_api(m: &mut Module<Engine>) {
    m.describe(&[
        (SET_BACKGROUND, &[], "(color: Color)", "Set the colour the viewport is cleared to behind everything drawn: the colour a `#rrggbb` writes, each channel from 0 to 1. Its alpha is not read."),
        (DRAW_LINE_3D, &[], "(from: Vec3, to: Vec3, color: Color, width: float?, perspective: bool?, on_top: bool?)", "Draw one 3D world-space line for this frame; the width is in pixels unless `perspective` scales it with distance, and `on_top` draws it over the scene."),
        (DRAW_LINE_2D, &[], "(from: Vec2, to: Vec2, color: Color, width: float?, opts: DrawOptions?)", "Draw one 2D world-space line for this frame; width is in pixels, and `opts.z_index` places it among the nodes of that index."),
        (DRAW_LINES_3D, &[], "(lines: [Line3d])", "Draw many 3D lines in one call, each with its own ends, colour, width and `on_top`: a frame's worth crosses to the engine once."),
        (DRAW_BOX_3D, &[], "(center: Vec3, size: Vec3, color: Color?)", "Draw a wireframe box centred at a point, `size` across along each axis in world units, for this frame."),
        (DRAW_SPHERE_3D, &[], "(center: Vec3, radius: float, color: Color?)", "Draw a wireframe sphere centred at a point, as three rings in world units, for this frame."),
        (DRAW_CAPSULE_3D, &[], "(center: Vec3, radius: float, height: float, color: Color?)", "Draw a wireframe capsule centred at a point, `height` being the straight part along y, for this frame."),
        (DRAW_TEXT_2D, &[], "(position: Vec2, text: string, opts: TextOptions?)", "Draw a line of text in 2D world space for this frame, shaped by the engine's fonts. `opts` takes the `text2d` keys (`font_size`, `font_weight`, `font_style`, `color`, `text_align`, `markup`, `max_width`, `font`, `text_style`, …), `pixels_per_unit` and `z_index`; `font` and `text_style` name an asset."),
        (DRAW_TEXT_3D, &[], "(position: Vec3, text: string, opts: TextOptions?)", "The same in 3D world space, on a quad that faces the camera. `pixels_per_unit` sizes it, so text a metre away reads the same whatever the font size."),
        (TEXT_SIZE, &[], "(text: string, opts: TextOptions?) -> (float, float)", "The width and height `text` shapes to, in font pixels, with the project's own fonts and never a system face — so a headless run and a windowed one answer the same. A width is presentation: writing one into state puts presentation in the digest."),
    ]);
    m.records(&[(
        "Line3d",
        "{ from: Vec3, to: Vec3, color: Color, width: float?, on_top: bool? }",
        "One 3D line for `draw_lines_3d`: its two ends in world units and its colour, a `width` in pixels where the default of 1 is not wanted, and `on_top` to draw it over the scene.",
    ), (
        "TextOptions",
        "{ font_size: float?, font_weight: int?, font_style: string?, color: Color?, text_align: string?, markup: bool?, max_width: float?, max_height: float?, max_lines: int?, line_break: string?, truncate: bool?, truncate_at: string?, tab_size: int?, font: string?, text_style: string?, alpha_cutoff: float?, pixels_per_unit: float?, z_index: int? }",
        "How drawn text is shaped and placed: the `text2d` keys, a `font` or `text_style` naming an asset, `pixels_per_unit` for its size in the world and `z_index` among the nodes of that index.",
    )]);
    // No reader by design (N8): the `ClearColorConfig` entry already holds
    // the colour; add `background` when a caller needs to read it back.
    m.function(SET_BACKGROUND, |eng: &Engine, color: Value| {
        let Value::Color([r, g, b, _]) = color else {
            anyhow::bail!("a background is a Color, not a {}", color.type_name());
        };
        let clear = eng.resource::<ClearColorConfig>();
        let mut clear = clear.borrow_mut();
        clear.color = [r, g, b];
        clear.changed = true;
        Ok(())
    });
    // One line for one frame, world space. Width is in pixels; pass
    // perspective = true for distance-scaled width (gizmos want false).
    m.function(
        DRAW_LINE_3D,
        |eng: &Engine, (from, to, color, width, perspective, on_top): LineArgs3d| {
            let (from, to) = (vec3_of(LINE_START, &from)?, vec3_of(LINE_END, &to)?);
            let rgba = line_color(&color)?;
            let lines = eng.resource::<DebugLineBuffer3d>();
            lines.borrow_mut().lines.push((
                from,
                to,
                rgba,
                width.unwrap_or(crate::DEFAULT_LINE_WIDTH),
                perspective.unwrap_or(false),
                on_top.unwrap_or(false),
            ));
            Ok(())
        },
    );
    m.function(DRAW_LINES_3D, push_lines);
    install_wireframe_api(m);
    install_text_api(m);
}

/// The immediate text calls, split from [`install_backdrop_api`] under
/// `MAX_FN_LINES`: a line in either pass, and what one measures.
fn install_text_api(m: &mut Module<Engine>) {
    m.function(
        DRAW_TEXT_2D,
        |eng: &Engine, (position, text, opts): (Value, String, Option<Value>)| {
            let [x, y] = vec2_of(TEXT_POSITION, &position)?;
            push_text(eng, [x, y, 0.0], text, opts, false)
        },
    );
    m.function(
        DRAW_TEXT_3D,
        |eng: &Engine, (position, text, opts): (Value, String, Option<Value>)| {
            push_text(eng, vec3_of(TEXT_POSITION, &position)?, text, opts, true)
        },
    );
    m.function(
        TEXT_SIZE,
        |eng: &Engine, (text, opts): (String, Option<Value>)| {
            let style = crate::world_text::style_of(opts)?;
            let [w, h] = crate::world_text::measure(eng, &text, &style)?;
            Ok((w, h))
        },
    );
    // One 2D world-space line for one frame; width in pixels.
    m.function(
        DRAW_LINE_2D,
        |eng, (from, to, color, width, opts): LineArgs2d| {
            let (from, to) = (vec2_of(LINE_START, &from)?, vec2_of(LINE_END, &to)?);
            let color = line_color(&color)?;
            let width = width.unwrap_or(crate::DEFAULT_LINE_WIDTH);
            let options = crate::draw_2d::options_of(opts, false)?;
            if options.z_index.is_some() {
                let line = crate::draw_2d::Draw2d::Polyline {
                    points: vec![from, to],
                    width,
                    color,
                };
                crate::draw_2d::push_at(eng, line, options.z_index);
                return Ok(());
            }
            let lines = eng.resource::<DebugLineBuffer2d>();
            lines.borrow_mut().lines.push((from, to, color, width));
            Ok(())
        },
    );
}

/// `draw_line_2d`'s arguments: both ends, the colour, the width in pixels and
/// options.
type LineArgs2d = (Value, Value, Value, Option<f32>, Option<Value>);

/// `draw_line_3d`'s arguments: both ends, the colour, then the width, whether
/// perspective scales it and whether it draws over the scene.
type LineArgs3d = (Value, Value, Value, Option<f32>, Option<bool>, Option<bool>);

const LINE_START: &str = "a line's start";
const LINE_END: &str = "a line's end";
const TEXT_POSITION: &str = "a text's position";
const SHAPE_CENTER: &str = "a shape's centre";

/// The 3D immediate primitives, split from [`install_backdrop_api`] under
/// `MAX_FN_LINES`: a box, a sphere and a capsule as wireframes, the shapes
/// the 2D side fills.
fn install_wireframe_api(m: &mut Module<Engine>) {
    m.function(
        DRAW_BOX_3D,
        |eng: &Engine, (center, size, color): (Value, Value, Option<Value>)| {
            let center = vec3_of(SHAPE_CENTER, &center)?;
            let half = vec3_of("a box's size", &size)?.map(|side| side.abs() / 2.0);
            let rgba = line_rgba(color.as_ref())?;
            push_segments(eng, &box_edges(center, half), rgba);
            Ok(())
        },
    );
    m.function(
        DRAW_SPHERE_3D,
        |eng: &Engine, (center, radius, color): (Value, f32, Option<Value>)| {
            let center = vec3_of(SHAPE_CENTER, &center)?;
            let rgba = line_rgba(color.as_ref())?;
            let mut out = Vec::new();
            for axis in 0..3 {
                ring(center, radius.abs(), axis, &mut out);
            }
            push_segments(eng, &out, rgba);
            Ok(())
        },
    );
    m.function(
        DRAW_CAPSULE_3D,
        |eng: &Engine, (center, radius, height, color): (Value, f32, f32, Option<Value>)| {
            let [x, y, z] = vec3_of(SHAPE_CENTER, &center)?;
            let rgba = line_rgba(color.as_ref())?;
            let (r, half) = (radius.abs(), height.abs() / 2.0);
            let mut out = Vec::new();
            ring([x, y + half, z], r, 1, &mut out);
            ring([x, y - half, z], r, 1, &mut out);
            for (dx, dz) in [(r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r)] {
                out.push(([x + dx, y - half, z + dz], [x + dx, y + half, z + dz]));
            }
            push_segments(eng, &out, rgba);
            Ok(())
        },
    );
    // A block of text for one frame, in either pass. `pixels_per_unit` is
    // the only key the shaper does not read, so it is taken out separately.
}

/// Shape3d and colour on a node, 3D and 2D.
/// The `sprite` bindings: a textured 2D quad, its sheet and its frame.
pub(crate) fn install_sprite_api(m: &mut Module<Engine>) {
    m.describe(&[]);
}

/// What `trace_texture` was asked for, with the defaults a Trace button uses.
struct TraceOpts {
    /// Alpha counted as opaque, `0..1`.
    threshold: f32,
    /// How many pixels of detail the simplifier may drop.
    tolerance: f32,
    pixels_per_unit: f32,
    /// Whether the loops inside the shape come back too.
    holes: bool,
}

impl TraceOpts {
    fn of(opts: Option<&Value>) -> Self {
        let mut out = Self {
            threshold: 0.5,
            tolerance: 2.0,
            pixels_per_unit: crate::DEFAULT_PIXELS_PER_UNIT,
            holes: false,
        };
        let Some(Value::Map(entries)) = opts else {
            return out;
        };
        let number = |v: &Value| match v {
            Value::Num(n) => Some(*n as f32),
            Value::Int(n) => Some(*n as f32),
            _ => None,
        };
        for (key, value) in entries {
            match key.as_str() {
                vocab::options::THRESHOLD => {
                    out.threshold = number(value).unwrap_or(out.threshold).clamp(0.0, 1.0);
                }
                vocab::options::TOLERANCE => {
                    out.tolerance = number(value).unwrap_or(out.tolerance).max(0.0);
                }
                vocab::options::PIXELS_PER_UNIT => {
                    out.pixels_per_unit = number(value).unwrap_or(out.pixels_per_unit).max(0.01);
                }
                vocab::options::HOLES => out.holes = matches!(value, Value::Bool(true)),
                _ => {}
            }
        }
        out
    }
}

/// Adjusting a sprite a node already has, and reading it back.
/// The image's pixel size, from its header: what a tool placing UVs over a
/// texture needs, in every build.
pub(crate) fn install_texture_api(m: &mut Module<Engine>) {
    m.describe(&[
        (
            TEXTURE_SIZE,
            &[],
            "(path: string) -> Vec2",
            "An image's width and height in pixels, read from the file's own header.",
        ),
        (
            TEXTURE_PIXELS_PER_UNIT,
            &[],
            "(path: string) -> float",
            "How many of an image's pixels make a world unit when a sprite says 0: its own \
             `pixels_per_unit` import setting, else 100.",
        ),
        (
            TRACE_TEXTURE,
            &[],
            "(path: string, opts: TraceOptions?) -> [[vec2]]",
            "The outline of an image's opaque pixels, as `[x, y]` points in a node's own space, \
             ready to be a polygon's `positions`. `opts` takes `threshold` (alpha counted as \
             opaque, 0 to 1, default 0.5), `tolerance` (how many pixels of detail to drop, \
             default 2), `pixels_per_unit` (default 100) and `holes` (every loop, largest first: \
             each other island counter-clockwise and each hole clockwise; default false keeps \
             the largest alone). Counter-clockwise with y up, centred on the origin, the way a \
             sprite at the same `pixels_per_unit` is drawn.",
        ),
    ]);
    m.records(&[(
        "TraceOptions",
        "{ threshold: float?, tolerance: float?, pixels_per_unit: float?, holes: bool? }",
        "How `trace_texture` reads an image: the alpha counted as opaque, the pixels of detail dropped, the pixels in a world unit, and whether every loop is kept.",
    )]);
    m.function(TEXTURE_SIZE, |eng: &Engine, path: String| {
        let (width, height) = crate::texture::size_of(eng, &path)?;
        Ok(Value::Vec2([width as f32, height as f32]))
    });
    m.function(TEXTURE_PIXELS_PER_UNIT, |eng: &Engine, path: String| {
        Ok(crate::texture::pixels_per_unit(eng, &path))
    });
    m.function(
        TRACE_TEXTURE,
        |eng: &Engine, (path, opts): (String, Option<Value>)| {
            let opts = TraceOpts::of(opts.as_ref());
            let source = balaur_asset::texture_asset::source(eng, &path)?;
            let bytes = eng
                .resource::<balaur_core::project::ProjectFiles>()
                .borrow()
                .read(&source.path)?;
            // Only alpha is traced, so nothing is bled into the clear texels.
            let alpha = balaur_asset::pixels::Alpha::Premultiplied;
            let image = balaur_asset::pixels::decode(&bytes, &source.settings.settings, alpha)
                .with_context(|| format!("decoding the image {path}"))?;
            let (w, h) = (image.width() as usize, image.height() as usize);
            // The file may be a smaller copy of what was drawn; the outline is
            // in the pixels the artist counted, like a sprite over it.
            let (dw, dh) = crate::texture::size_of(eng, &path)
                .map_or((w as f32, h as f32), |(a, b)| (a as f32, b as f32));
            let (sx, sy) = (dw / w.max(1) as f32, dh / h.max(1) as f32);
            let cut = opts.threshold * 255.0;
            let mask: Vec<bool> = image.pixels().map(|p| f32::from(p.0[3]) >= cut).collect();
            let ppu = opts.pixels_per_unit;
            let loops = balaur_geometry::geometry2d::trace(&mask, w, h);
            let keep = if opts.holes { loops.len() } else { 1 };
            // Pixel corners are y-down and measured from the top left; a
            // polygon's points are y-up and centred, the same frame
            // `default_uv` maps a texture in.
            let local = |p: glamx::Vec2| {
                Value::Vec2([(p.x * sx - dw / 2.0) / ppu, (dh / 2.0 - p.y * sy) / ppu])
            };
            Ok(Value::List(
                loops
                    .into_iter()
                    .take(keep)
                    .map(|outline| {
                        // `local`'s y flip turns a traced outline clockwise; reversed, an
                        // outline winds counter-clockwise and a hole clockwise.
                        let simplified =
                            balaur_geometry::geometry2d::simplify(&outline, opts.tolerance);
                        Value::List(simplified.into_iter().rev().map(local).collect())
                    })
                    .collect(),
            ))
        },
    );
}

pub(crate) fn install_sprite_state_api(m: &mut Module<Engine>) {
    m.describe(&[]);
}

/// A list of `Line3d` records. One call for a frame's worth of lines, so a
/// tool drawing hundreds of them crosses the script boundary once rather
/// than once a line.
fn push_lines(eng: &Engine, lines: Value) -> anyhow::Result<()> {
    let Value::List(items) = lines else {
        anyhow::bail!(
            "draw_lines_3d takes a list of Line3d, not a {}",
            lines.type_name()
        );
    };
    let mut read = Vec::with_capacity(items.len());
    for item in &items {
        let Value::Map(fields) = item else {
            anyhow::bail!(
                "draw_lines_3d takes a list of Line3d, and one is a {}",
                item.type_name()
            );
        };
        let field = |name: &str| fields.iter().find(|(key, _)| key == name).map(|(_, v)| v);
        let missing = |name: &str| anyhow::anyhow!("a Line3d needs its `{name}`");
        let from = field(FROM).ok_or_else(|| missing(FROM))?;
        let to = field(TO).ok_or_else(|| missing(TO))?;
        let color = field(keys::COLOR).ok_or_else(|| missing(keys::COLOR))?;
        let width = match field(WIDTH) {
            Some(Value::Num(width)) => *width as f32,
            Some(Value::Int(width)) => *width as f32,
            _ => crate::DEFAULT_LINE_WIDTH,
        };
        read.push((
            vec3_of(LINE_START, from)?,
            vec3_of(LINE_END, to)?,
            line_color(color)?,
            width,
            false,
            matches!(field(ON_TOP), Some(Value::Bool(true))),
        ));
    }
    eng.resource::<DebugLineBuffer3d>()
        .borrow_mut()
        .lines
        .extend(read);
    Ok(())
}
