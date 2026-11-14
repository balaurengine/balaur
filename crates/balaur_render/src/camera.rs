//! The `camera` component: a scene declares its view and the view follows
//! the node carrying it.

use anyhow::{Result, anyhow};
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
/// the `camera2d` schema states.
/// A hundredth, not one: a pixel-scale level is thousands of units across,
/// and a camera that cannot show it whole is a camera that cannot frame it.
pub(crate) const MIN_ZOOM_2D: f32 = 0.01;

/// The nearest a 3D camera's focus sits. Mirrors the `min` the `camera3d`
/// schema states.
const MIN_FOCUS: f32 = 0.001;

/// The camera the scene is drawn from. Its node places and aims it, looking along the node's -z as a `light3d` shines; the lens rows say how it projects and what it draws, `effects` how the frame resolves, and the last `current` camera wins.
// `drive_camera_system` copies the current one into [`CameraConfig3d`],
// through the [`Lens3d`] a write leaves beside it.
#[derive(Component)]
#[component(
    name = "camera3d",
    on_remove = camera3d_removed,
    events = &[( CURRENT_CHANGED_EVENT, "whether it is the camera drawn from now", )],
    tags = [balaur_core::components::tag::DIM_3D, "render"],
    after_write = camera3d_written,
)]
pub struct Camera3d {
    /// Whether this camera drives the view; the last current one wins
    #[property(default = true)]
    pub current: bool,
    /// Perspective, or parallel lines that stay parallel
    #[property(default = Projection::Perspective)]
    pub projection: Projection,
    /// The vertical field of view; in orthographic, how much of `focus_distance` the frame covers
    #[property(default = 45.0, min = 1.0, max = 179.0)]
    pub fov_degrees: f32,
    /// The passes the frame resolves through and how each is tuned; empty draws the frame as it is
    #[property(asset = "camera_effects")]
    pub effects: String,
    /// The orthographic frame's full height in world units; zero sizes it from `focus_distance` and `fov_degrees`
    #[property(min = 0.0, group = "lens")]
    pub orthographic_height: f32,
    /// The nearest distance drawn, in world units
    #[property(default = 0.1, min = 0.000_001, group = "lens")]
    pub near: f32,
    /// The farthest distance drawn, in world units
    #[property(default = 1000.0, min = 0.000_001, group = "lens")]
    pub far: f32,
    /// How far ahead the camera's focus sits, in world units: an orthographic frame with no `orthographic_height` is sized from it, and a stereo pair converges there
    #[property(default = 10.0, min = 0.001, group = "lens")]
    pub focus_distance: f32,
    /// Above zero, a stereo pair this far apart in world units: one eye in each half of the frame, both converging `focus_distance` ahead. A stereo camera ignores `projection` and keeps +y up
    #[property(min = 0.0, group = "lens")]
    pub stereo_separation: f32,
    /// The layers this camera draws: a node whose `render_layers` share one; empty is every layer
    #[property(flags, group = "layers")]
    pub render_layers: Vec<Layer>,
}

/// The orthographic camera a flat scene is drawn from. `pixels_per_unit` scales it, and the last `current` camera wins; the scene's `environment` lights it. Only its node moves it.
// Mirrored into [`CameraConfig2d`].
#[derive(Component)]
#[component(
    name = "camera2d",
    events = &[( CURRENT_CHANGED_EVENT, "whether it is the camera drawn from now", )],
    tags = [balaur_core::components::tag::DIM_2D, "render"],
)]
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
/// camera never re-asserts itself, which leaves `changed` alone.
pub(crate) fn drive_camera_system(eng: &Engine, _dt: f32) {
    let (spatial, flat, post, winners, view) = {
        let world = eng.world();
        let mut spatial = None;
        let mut flat = None;
        let mut post = None;
        let mut winners = (None, None);
        // A 3D camera over a 2D one: ears go where the depth is.
        let mut view = (None, None);
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
                let ahead = crate::light3d::aim(&global) * cam.focus_distance.max(MIN_FOCUS);
                spatial = Some((
                    [
                        global.position,
                        global.position + ahead,
                        global.rotation * Vec3::Y,
                    ],
                    Lens3d::clone(&lens),
                ));
                winners.0 = Some(entity);
                view.0 = Some((global.position, global.rotation * Vec3::X));
            }
            if let Ok(cam) = world.get::<&Camera2d>(entity)
                && cam.current
            {
                post = Some(cam.effects.clone());
                let zoom = cam.pixels_per_unit.max(MIN_ZOOM_2D);
                flat = Some(([global.position.x, global.position.y], zoom, cam.hidpi));
                winners.1 = Some(entity);
                view.1 = Some((global.position, global.rotation * glamx::Vec3::X));
            }
        }
        (spatial, flat, post, winners, view.0.or(view.1))
    };
    if let Some((position, right)) = view {
        balaur_core::viewpoint::set(eng, position, right);
    }
    announce_current(eng, winners);
    if let Some(effects) = post {
        drive_post(eng, &effects_of(eng, &effects));
    }
    let lens = spatial.as_ref().map(|s| s.1.clone()).unwrap_or_default();
    {
        let config = eng.resource::<CameraConfig3d>();
        let mut config = config.borrow_mut();
        if config.lens != lens {
            config.lens = lens;
            config.lens_changed = true;
        }
    }
    if let Some(([eye, target, up], _)) = spatial {
        let config = eng.resource::<CameraConfig3d>();
        let mut config = config.borrow_mut();
        if config.eye != eye || config.target != target || config.up != up {
            config.eye = eye;
            config.target = target;
            config.up = up;
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

/// What a removed `Camera3d` leaves behind, taken with it.
fn camera3d_removed(eng: &Engine, entity: Entity) {
    let _ = eng.remove_one::<Lens3d>(entity);
}

/// The `camera3d` and `camera2d` components. Two rather than one with a
/// `kind`: a component's tags are what the editor files a node under, and a
/// tag is per type while a kind would be per node -- so one component could
/// only ever claim one dimension for both. Splitting also drops the property
/// that was inert either way (`focus_distance` on a flat camera,
/// `pixels_per_unit` on a spatial one).
pub(crate) fn register_camera_components(reg: &mut Registry<'_>) {
    reg.add_component::<Camera3d>();
    reg.add_component::<Camera2d>();
}
