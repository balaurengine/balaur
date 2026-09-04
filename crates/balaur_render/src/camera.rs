//! The `camera` component: a scene declares its view and the view follows
//! the node carrying it.

use anyhow::anyhow;
use balaur_core::components::ComponentDef;
use balaur_core::{App, Engine, GlobalTransform};

use crate::{color_to_toml, CameraConfig, CameraConfig2d};

/// Which view a `camera` component drives: `"3d"` in a scene file is the
/// perspective camera, `"2d"` the orthographic one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CameraKind {
    Perspective,
    Orthographic,
}

/// A colour property read by name, defaulting to black — `color_from_params`
/// reads the property called `color` and defaults to the renderable grey.
fn color_from_params_named(params: &toml::Value, key: &str) -> [f32; 4] {
    let channel = |i: usize, default: f64| {
        params
            .get(key)
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(default) as f32
    };
    [
        channel(0, 0.0),
        channel(1, 0.0),
        channel(2, 0.0),
        channel(3, 1.0),
    ]
}

/// The `camera` component's authored state. `drive_camera_system` copies
/// the current one into [`CameraConfig`] / [`CameraConfig2d`].
pub struct Camera {
    pub kind: CameraKind,
    /// The last current camera in tree-traversal order drives the view.
    pub current: bool,
    /// World point the 3D camera looks at.
    pub look_at: glamx::Vec3,
    /// 2D zoom in logical pixels per world unit.
    pub zoom: f32,
    /// Light every 2D surface gets before any `light2d`. Read from the
    /// current 2D camera only; the light map is a 2D pass.
    pub ambient: [f32; 4],
}

/// Runs in `SceneSync` after transform propagation, so the view follows the
/// node's global pose. Writes only when the pose actually differs: a still
/// camera never re-asserts itself, which leaves `changed` alone and keeps the
/// backend's interactive orbit/pan controls live between moves.
pub(crate) fn drive_camera_system(eng: &Engine, _dt: f32) {
    let (spatial, flat) = {
        let world = eng.world();
        let mut spatial = None;
        let mut flat = None;
        for entity in balaur_core::scene::collect_subtree(&world, eng.root()) {
            let Ok(cam) = world.get::<&Camera>(entity) else {
                continue;
            };
            if !cam.current {
                continue;
            }
            let Ok(global) = world.get::<&GlobalTransform>(entity) else {
                continue;
            };
            match cam.kind {
                CameraKind::Perspective => spatial = Some((global.position, cam.look_at)),
                CameraKind::Orthographic => {
                    flat = Some((
                        [global.position.x, global.position.y],
                        cam.zoom,
                        cam.ambient,
                    ));
                }
            }
        }
        (spatial, flat)
    };
    if let Some((eye, target)) = spatial {
        let config = eng.resource::<CameraConfig>();
        let mut config = config.borrow_mut();
        if config.eye != eye || config.target != target {
            config.eye = eye;
            config.target = target;
            config.changed = true;
        }
    }
    if let Some((center, zoom, ambient)) = flat {
        let config = eng.resource::<CameraConfig2d>();
        let mut config = config.borrow_mut();
        // Ambient is read every frame rather than applied on a change, so it
        // is not part of "did the view move".
        config.ambient = [ambient[0], ambient[1], ambient[2]];
        // Bit-exact "did it move": the compared values are the ones this
        // system wrote last frame, not the result of drifting arithmetic.
        let same = config.center[0].to_bits() == center[0].to_bits()
            && config.center[1].to_bits() == center[1].to_bits()
            && config.zoom.to_bits() == zoom.to_bits();
        if !same {
            config.center = center;
            config.zoom = zoom;
            config.changed = true;
        }
    }
}

/// The `camera` component. Writes a [`Camera`] on the node;
/// [`drive_camera_system`] mirrors the current one into the camera resources.
pub(crate) fn register_camera_component(app: &mut App) {
    app.register_component(
        "camera",
        ComponentDef {
            doc: "The view the scene is drawn from, following the node's global pose: `look_at` aims the 3D camera, `zoom` scales the 2D one in logical pixels per world unit. The last `current` camera of a kind, in tree order, drives that view.",
            schema: ComponentDef::parse_schema(
                "camera",
                r#"kind = { type = "enum", default = "3d", options = ["3d", "2d"], description = "Which camera this node drives" }
current = { type = "bool", default = true, description = "Whether this camera drives the view; the last current one wins" }
look_at = { type = "vec3", default = [0.0, 0.0, 0.0], description = "World point the 3D camera looks at" }
zoom = { type = "float", default = 60.0, min = 1.0, description = "2D zoom in logical pixels per world unit" }
ambient = { type = "color", default = [0.0, 0.0, 0.0, 1.0], description = "Light every 2D surface gets before any `light2d`; only a `2d` camera's is read" }"#,
            ),
            tags: &["3d", "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let kind = match params.get("kind").and_then(|v| v.as_str()).unwrap_or("3d") {
                    "3d" => CameraKind::Perspective,
                    "2d" => CameraKind::Orthographic,
                    other => return Err(anyhow!("unknown camera kind '{other}'")),
                };
                let la = |i: usize| {
                    params
                        .get("look_at")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.get(i))
                        .and_then(balaur_core::components::as_f64)
                        .unwrap_or(0.0) as f32
                };
                let zoom = params
                    .get("zoom")
                    .and_then(balaur_core::components::as_f64)
                    .unwrap_or(60.0) as f32;
                let camera = Camera {
                    kind,
                    ambient: color_from_params_named(params, "ambient"),
                    current: params
                        .get("current")
                        .and_then(toml::Value::as_bool)
                        .unwrap_or(true),
                    look_at: glamx::Vec3::new(la(0), la(1), la(2)),
                    zoom: zoom.max(1.0),
                };
                let mut world = eng.world_mut();
                if let Ok(mut c) = world.get::<&mut Camera>(entity) {
                    *c = camera;
                    return Ok(());
                }
                world
                    .insert_one(entity, camera)
                    .map_err(|_| anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Camera>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let camera = world.get::<&Camera>(entity).ok()?;
                let mut map = toml::map::Map::new();
                let kind = match camera.kind {
                    CameraKind::Perspective => "3d",
                    CameraKind::Orthographic => "2d",
                };
                map.insert("kind".into(), toml::Value::String(kind.into()));
                map.insert("current".into(), toml::Value::Boolean(camera.current));
                map.insert(
                    "look_at".into(),
                    toml::Value::Array(
                        [camera.look_at.x, camera.look_at.y, camera.look_at.z]
                            .iter()
                            .map(|c| toml::Value::Float(f64::from(*c)))
                            .collect(),
                    ),
                );
                map.insert("zoom".into(), toml::Value::Float(f64::from(camera.zoom)));
                map.insert("ambient".into(), color_to_toml(camera.ambient));
                Some(toml::Value::Table(map))
            }),
        },
    );
}
