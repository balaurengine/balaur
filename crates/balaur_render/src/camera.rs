//! The `camera` component: a scene declares its view and the view follows
//! the node carrying it.

use anyhow::{Result, anyhow};
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{Component, Written};
use balaur_core::hecs::{self, Entity};
use balaur_core::{Engine, GlobalTransform};
use balaur_plugin::Registry;
use glamx::Vec3;

use crate::lens::{Lens3d, Projection, lens_of};
use crate::post::{drive_post, effects_of};
use crate::{CameraConfig2d, CameraConfig3d};
use balaur_core::layers::Layer;

/// The smallest 2D zoom, in logical pixels per world unit. Mirrors the `min`
/// the `camera2d` schema states, which `render.set_camera_2d` also takes.
/// A hundredth, not one: a pixel-scale level is thousands of units across,
/// and a camera that cannot show it whole is a camera that cannot frame it.
pub(crate) const MIN_ZOOM_2D: f32 = 0.01;

/// The camera the scene is drawn from. `look_at` and `up` aim it, the lens rows say how it projects and what it draws, `effects` how the frame resolves, and the last `current` camera wins.
// `drive_camera_system` copies the current one into [`CameraConfig3d`],
// through the [`Lens3d`] a write leaves beside it.
#[derive(Component)]
#[component(name = "camera3d")]
pub struct Camera3d {
    /// Whether this camera drives the view; the last current one wins
    #[property(default = true)]
    pub current: bool,
    /// Perspective, or parallel lines that stay parallel
    #[property(default = Projection::Perspective)]
    pub projection: Projection,
    /// The vertical field of view; in orthographic, how much of the distance to the point looked at the frame covers
    #[property(default = 45.0, min = 1.0, max = 179.0)]
    pub fov_degrees: f32,
    /// World point the camera looks at
    pub look_at: Vec3,
    /// The passes the frame resolves through and how each is tuned; empty draws the frame as it is
    #[property(asset = "camera_effects")]
    pub effects: String,
    /// The orthographic frame's full height in world units; zero sizes it from the distance to the point looked at and `fov_degrees`
    #[property(min = 0.0, group = "lens")]
    pub orthographic_height: f32,
    /// The nearest distance drawn, in world units
    #[property(default = 0.1, min = 0.000_001, group = "lens")]
    pub near: f32,
    /// The farthest distance drawn, in world units
    #[property(default = 1000.0, min = 0.000_001, group = "lens")]
    pub far: f32,
    /// The direction the camera keeps up
    #[property(default = Vec3::Y, group = "lens")]
    pub up: Vec3,
    /// Above zero, a stereo pair this far apart in world units: one eye in each half of the frame, both converging on the point looked at. A stereo camera ignores `projection` and keeps +y up
    #[property(min = 0.0, group = "lens")]
    pub eye_separation: f32,
    /// The layers this camera draws: a node whose `render_layers` share one; empty is every layer
    #[property(flags, group = "layers")]
    pub render_layers: Vec<Layer>,
}

/// The orthographic camera a flat scene is drawn from. `pixels_per_unit` scales it, and the last `current` camera wins; the scene's `environment` lights it. Only its node moves it.
// Mirrored into [`CameraConfig2d`].
#[derive(Component)]
#[component(name = "camera2d")]
pub struct Camera2d {
    /// Whether this camera drives the view; the last current one wins
    #[property(default = true)]
    pub current: bool,
    /// Zoom in logical pixels per world unit
    #[property(default = 60.0, min = 0.01)]
    pub pixels_per_unit: f32,
    /// The passes the frame resolves through and how each is tuned; empty draws the frame as it is
    #[property(asset = "camera_effects")]
    pub effects: String,
    /// Count `pixels_per_unit` in logical pixels, multiplied by the display's scale; off counts physical pixels, for pixel art that should land on the screen's own grid
    #[property(default = true)]
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
        let held = eng.resource::<HeldCameras>().borrow().0;
        let current = current_cameras(&world, eng.root())
            .into_iter()
            .filter(|&entity| {
                held.is_none_or(|root| !balaur_core::scene::is_within(&world, entity, root))
            });
        for entity in current {
            let Ok(global) = world.get::<&GlobalTransform>(entity) else {
                continue;
            };
            if let Ok(cam) = world.get::<&Camera3d>(entity)
                && cam.current
                && let Ok(lens) = world.get::<&Lens3d>(entity)
            {
                post = Some(cam.effects.clone());
                spatial = Some((global.position, cam.look_at, Lens3d::clone(&lens)));
                winners.0 = Some(entity);
            }
            if let Ok(cam) = world.get::<&Camera2d>(entity)
                && cam.current
            {
                post = Some(cam.effects.clone());
                let zoom = cam.pixels_per_unit.max(MIN_ZOOM_2D);
                flat = Some(([global.position.x, global.position.y], zoom, cam.hidpi));
                winners.1 = Some(entity);
            }
        }
        (spatial, flat, post, winners)
    };
    announce_current(eng, winners);
    if let Some(effects) = post {
        drive_post(eng, &effects_of(eng, &effects));
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
    if let Some((center, zoom, hidpi)) = flat {
        // The scene's one ambient, the `environment`'s, lights a 2D surface
        // before any `light2d` as it lights a 3D one. Read every frame rather
        // than applied on a change, so it is not part of "did the view move".
        let ambient = {
            let world = eng.world();
            crate::light3d::environment_or_none(&world, eng.root()).ambient
        };
        let config = eng.resource::<CameraConfig2d>();
        let mut config = config.borrow_mut();
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

/// The subtree whose cameras drive no view while a caller holds it: an
/// editor looking at a game through its own camera.
#[derive(Default)]
pub(crate) struct HeldCameras(pub Option<Entity>);

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
        1 if balaur_core::scene::is_within(world, current[0], root) => current,
        1 => Vec::new(),
        _ => balaur_core::scene::collect_subtree(world, root)
            .into_iter()
            .filter(|e| current.contains(e))
            .collect(),
    }
}

/// A camera's lens, worked out again on every write: a lens kiss3d could not
/// draw refuses the write, and one that refuses being added takes the camera
/// back off, so the node is left as it was.
fn camera3d_written(eng: &Engine, entity: Entity, _written: Written) -> Result<()> {
    let lens = {
        let world = eng.world();
        let Ok(camera) = world.get::<&Camera3d>(entity) else {
            return Ok(());
        };
        lens_of(&camera)
    };
    lens.and_then(|lens| {
        eng.insert_one(entity, lens)
            .map_err(|_| anyhow!("node is dead"))
    })
}

/// The `camera3d` and `camera2d` components. Two rather than one with a
/// `kind`: a component's tags are what the editor files a node under, and a
/// tag is per type while a kind would be per node -- so one component could
/// only ever claim one dimension for both. Splitting also drops the property
/// that was inert either way (`look_at` on a flat camera, `pixels_per_unit`
/// on a spatial one).
pub(crate) fn register_camera_components(reg: &mut Registry<'_>) {
    reg.register_component(
        Camera3d::NAME,
        ComponentDef {
            events: &[(
                CURRENT_CHANGED_EVENT,
                "whether it is the camera drawn from now",
            )],
            tags: &[balaur_core::components::tag::DIM_3D, "render"],
            remove: Box::new(|eng, entity| {
                let _ = eng.remove_one::<Camera3d>(entity);
                let _ = eng.remove_one::<Lens3d>(entity);
                Ok(())
            }),
            ..ComponentDef::of::<Camera3d>(Some(camera3d_written))
        },
    );
    reg.register_component(
        Camera2d::NAME,
        ComponentDef {
            events: &[(
                CURRENT_CHANGED_EVENT,
                "whether it is the camera drawn from now",
            )],
            tags: &[balaur_core::components::tag::DIM_2D, "render"],
            ..ComponentDef::of::<Camera2d>(None)
        },
    );
}
