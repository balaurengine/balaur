//! The `camera` component: a scene declares its view and the view follows
//! the node carrying it.

use anyhow::anyhow;
use balaur_core::components::ComponentDef;
use balaur_core::hecs::{self, Entity};
use balaur_core::{Engine, GlobalTransform};
use balaur_plugin::Registry;

use crate::post::{Post, drive_post, post_from_params, post_schema, post_to_map};
use crate::vocabulary::keys as k;
use crate::{CameraConfig2d, CameraConfig3d, color_to_toml};

/// The smallest 2D zoom, in logical pixels per world unit. Mirrors the `min`
/// the `camera` schema below states, which `render.set_camera_2d` also takes.
/// A hundredth, not one: a pixel-scale level is thousands of units across,
/// and a camera that cannot show it whole is a camera that cannot frame it.
pub(crate) const MIN_ZOOM_2D: f32 = 0.01;

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

/// The `camera3d` component's authored state. `drive_camera_system` copies
/// the current one into [`CameraConfig3d`].
pub struct Camera3d {
    /// The last current camera in tree-traversal order drives the view.
    pub current: bool,
    /// World point the camera looks at.
    pub look_at: glamx::Vec3,
    /// Screen-space effects the frame resolves through. Not per-dimension:
    /// the effects run over the whole film, so the last current camera of
    /// either dimension sets them.
    pub post: Post,
    pub lens: crate::lens::Lens3d,
}

/// The `camera2d` component's authored state, mirrored into
/// [`CameraConfig2d`].
pub struct Camera2d {
    /// As in 3D: the last current one in the walk drives the view.
    pub current: bool,
    /// Zoom in logical pixels per world unit.
    pub zoom: f32,
    /// Light every 2D surface gets before any `light2d`. The light map is a
    /// 2D pass, so only this dimension carries it.
    pub ambient: [f32; 4],
    /// As on [`Camera3d`], and read from whichever came last.
    pub post: Post,
    /// Whether `zoom` counts logical pixels, scaled by the display, rather
    /// than physical ones.
    pub hidpi: bool,
}

/// Runs in `SceneSync` after transform propagation, so the view follows the
/// node's global pose. Writes only when the pose actually differs: a still
/// camera never re-asserts itself, which leaves `changed` alone and keeps a
/// script's `render.set_camera` in force between moves.
pub(crate) fn drive_camera_system(eng: &Engine, _dt: f32) {
    let (spatial, flat, post, winners) = {
        let world = eng.world();
        let mut spatial = None;
        let mut flat = None;
        let mut post = None;
        let mut winners = (None, None);
        for entity in current_cameras(&world, eng.root()) {
            let Ok(global) = world.get::<&GlobalTransform>(entity) else {
                continue;
            };
            if let Ok(cam) = world.get::<&Camera3d>(entity)
                && cam.current
            {
                post = Some(cam.post.clone());
                spatial = Some((global.position, cam.look_at, cam.lens.clone()));
                winners.0 = Some(entity);
            }
            if let Ok(cam) = world.get::<&Camera2d>(entity)
                && cam.current
            {
                post = Some(cam.post.clone());
                flat = Some((
                    [global.position.x, global.position.y],
                    cam.zoom,
                    cam.ambient,
                    cam.hidpi,
                ));
                winners.1 = Some(entity);
            }
        }
        (spatial, flat, post, winners)
    };
    announce_current(eng, winners);
    if let Some(post) = post {
        drive_post(eng, &post);
    }
    let lens = spatial.as_ref().map(|s| s.2.clone()).unwrap_or_default();
    {
        let config = eng.resource::<CameraConfig3d>();
        let mut config = config.borrow_mut();
        if config.lens != lens {
            config.lens = lens;
            config.lens_changed = true;
        }
    }
    if let Some((eye, target, _)) = spatial {
        let config = eng.resource::<CameraConfig3d>();
        let mut config = config.borrow_mut();
        if config.eye != eye || config.target != target {
            config.eye = eye;
            config.target = target;
            config.changed = true;
        }
    }
    if let Some((center, zoom, ambient, hidpi)) = flat {
        let config = eng.resource::<CameraConfig2d>();
        let mut config = config.borrow_mut();
        // Ambient is read every frame rather than applied on a change, so it
        // is not part of "did the view move".
        config.ambient = [ambient[0], ambient[1], ambient[2]];
        // Bit-exact "did it move": the compared values are the ones this
        // system wrote last frame, not the result of drifting arithmetic.
        let same = config.center[0].to_bits() == center[0].to_bits()
            && config.center[1].to_bits() == center[1].to_bits()
            && config.zoom.to_bits() == zoom.to_bits()
            && config.hidpi == hidpi;
        if !same {
            config.center = center;
            config.zoom = zoom;
            config.hidpi = hidpi;
            config.changed = true;
        }
    }
}

/// What a camera announces when it becomes the one drawn from, `true`, and
/// when another takes over, `false`.
pub(crate) const CURRENT_CHANGED_EVENT: &str = "current_changed";

/// The 3D and the 2D camera drawn from last frame.
#[derive(Default)]
pub(crate) struct CurrentCameras {
    spatial: Option<Entity>,
    flat: Option<Entity>,
}

/// Tell a camera it became the one drawn from, and the one it took over from
/// that it stopped being, per dimension.
fn announce_current(eng: &Engine, (spatial, flat): (Option<Entity>, Option<Entity>)) {
    let changes: Vec<(Entity, bool)> = {
        let held = eng.resource::<CurrentCameras>();
        let mut held = held.borrow_mut();
        let held = &mut *held;
        let mut changes = Vec::new();
        for (was, now) in [(&mut held.spatial, spatial), (&mut held.flat, flat)] {
            if *was != now {
                changes.extend(was.map(|entity| (entity, false)));
                changes.extend(now.map(|entity| (entity, true)));
                *was = now;
            }
        }
        changes
    };
    for (entity, current) in changes {
        if eng.world().contains(entity) {
            let payload = balaur_script::Value::Bool(current);
            balaur_core::events::announce(eng, entity, CURRENT_CHANGED_EVENT, payload);
        }
    }
}

/// Every current camera in the tree, in tree order: the last one wins, and
/// which came last is answered across both kinds.
///
/// Queried rather than walked, since a frame has a camera or two and the tree
/// has thousands of nodes; the walk runs only when two cameras need ordering.
fn current_cameras(world: &hecs::World, root: Entity) -> Vec<Entity> {
    let mut current: Vec<Entity> = world
        .query::<(Entity, &Camera3d)>()
        .iter()
        .filter(|(_, cam)| cam.current)
        .map(|(e, _)| e)
        .collect();
    current.extend(
        world
            .query::<(Entity, &Camera2d)>()
            .iter()
            .filter(|(_, cam)| cam.current)
            .map(|(e, _)| e),
    );
    match current.len() {
        0 => current,
        1 if balaur_core::scene::is_under(world, current[0], root) => current,
        1 => Vec::new(),
        _ => balaur_core::scene::collect_subtree(world, root)
            .into_iter()
            .filter(|e| current.contains(e))
            .collect(),
    }
}

/// The authored 3D camera a full property table describes.
fn camera3d_from_params(params: &toml::Value) -> anyhow::Result<Camera3d> {
    let la = |i: usize| {
        params
            .get(k::LOOK_AT)
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(0.0) as f32
    };
    Ok(Camera3d {
        post: post_from_params(params),
        current: balaur_core::components::prop_bool(params, k::CURRENT),
        look_at: glamx::Vec3::new(la(0), la(1), la(2)),
        lens: crate::lens::lens_from_params(params)?,
    })
}

/// The authored 2D camera a full property table describes.
fn camera2d_from_params(params: &toml::Value) -> Camera2d {
    let zoom = params
        .get(k::PIXELS_PER_UNIT)
        .and_then(balaur_core::components::as_f64)
        .unwrap_or(60.0) as f32;
    Camera2d {
        post: post_from_params(params),
        current: balaur_core::components::prop_bool(params, k::CURRENT),
        ambient: color_from_params_named(params, k::AMBIENT_COLOR),
        zoom: zoom.max(MIN_ZOOM_2D),
        hidpi: balaur_core::components::prop_bool(params, k::HIDPI),
    }
}

/// The `camera3d` and `camera2d` components. Two rather than one with a
/// `kind`: a component's tags are what the editor files a node under, and a
/// tag is per type while a kind would be per node -- so one component could
/// only ever claim one dimension for both. Splitting also drops the property
/// that was inert either way (`look_at` on a flat camera, `pixels_per_unit`
/// on a spatial one).
pub(crate) fn register_camera_components(reg: &mut Registry<'_>) {
    register_camera3d(reg);
    register_camera2d(reg);
}

fn register_camera3d(reg: &mut Registry<'_>) {
    reg.register_component(
        "camera3d",
        ComponentDef {
            events: &[(CURRENT_CHANGED_EVENT, "whether it is the camera drawn from now")],
            warnings: None,
            doc: "The camera the scene is drawn from. `look_at` aims it, the lens rows say how it projects and what it draws, the control rows say how a mouse orbits, pans and zooms it, and the last `current` camera wins.",
            schema: ComponentDef::parse_schema(
                "camera3d",
                &[
                    balaur_core::components::ComponentDef::schema(&[(
                        k::LOOK_AT,
                        r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "World point the camera looks at" }"#,
                    )]),
                    crate::lens::lens_schema(),
                    post_schema(),
                ]
                .join("\n"),
            ),
            tags: &[balaur_core::components::tag::DIM_3D, "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let camera = camera3d_from_params(params)?;
                let mut world = eng.world_mut();
                if let Ok(mut c) = world.get::<&mut Camera3d>(entity) {
                    *c = camera;
                    return Ok(());
                }
                world
                    .insert_one(entity, camera)
                    .map_err(|_| anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Camera3d>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let camera = world.get::<&Camera3d>(entity).ok()?;
                let mut map = toml::map::Map::new();
                map.insert(k::CURRENT.into(), toml::Value::Boolean(camera.current));
                map.insert(
                    k::LOOK_AT.into(),
                    toml::Value::Array(
                        [camera.look_at.x, camera.look_at.y, camera.look_at.z]
                            .iter()
                            .map(|c| toml::Value::Float(f64::from(*c)))
                            .collect(),
                    ),
                );
                crate::lens::lens_to_map(&camera.lens, &mut map);
                post_to_map(&camera.post, &mut map);
                Some(toml::Value::Table(map))
            }),
        },
    );
}

fn register_camera2d(reg: &mut Registry<'_>) {
    reg.register_component(
        "camera2d",
        ComponentDef {
            events: &[(CURRENT_CHANGED_EVENT, "whether it is the camera drawn from now")],
            warnings: None,
            doc: "The orthographic camera a flat scene is drawn from. `pixels_per_unit` scales it, `ambient_color` lights every 2D surface, and the last `current` camera wins. Only its node moves it.",
            schema: ComponentDef::parse_schema(
                "camera2d",
                &[
                    balaur_core::components::ComponentDef::schema(&[
                        (k::PIXELS_PER_UNIT, r#"{ type = "float", default = 60.0, min = 0.01, description = "Zoom in logical pixels per world unit" }"#),
                        (k::AMBIENT_COLOR, r#"{ type = "color", default = [0.0, 0.0, 0.0, 1.0], description = "Light every 2D surface gets before any `light2d`; its alpha is ignored" }"#),
                        (k::HIDPI, r#"{ type = "bool", default = true, description = "Count `pixels_per_unit` in logical pixels, multiplied by the display's scale; off counts physical pixels, for pixel art that should land on the screen's own grid" }"#),
                    ]),
                    post_schema(),
                ]
                .join("\n"),
            ),
            tags: &[balaur_core::components::tag::DIM_2D, "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let camera = camera2d_from_params(params);
                let mut world = eng.world_mut();
                if let Ok(mut c) = world.get::<&mut Camera2d>(entity) {
                    *c = camera;
                    return Ok(());
                }
                world
                    .insert_one(entity, camera)
                    .map_err(|_| anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Camera2d>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let camera = world.get::<&Camera2d>(entity).ok()?;
                let mut map = toml::map::Map::new();
                map.insert(k::CURRENT.into(), toml::Value::Boolean(camera.current));
                map.insert(k::PIXELS_PER_UNIT.into(), toml::Value::Float(f64::from(camera.zoom)));
                map.insert(k::AMBIENT_COLOR.into(), color_to_toml(camera.ambient));
                map.insert(k::HIDPI.into(), toml::Value::Boolean(camera.hidpi));
                post_to_map(&camera.post, &mut map);
                Some(toml::Value::Table(map))
            }),
        },
    );
}
