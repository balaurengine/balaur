//! `reflection_probe`: a box a room was captured inside, which the reflective
//! surfaces within it mirror instead of the sky.
//!
//! A sky reflects as though it were infinitely far away, which is right
//! outdoors and wrong in a room: a floor should show the walls around it, not
//! the clouds above the building. A probe is that room's own sky, captured
//! from one point, and a surface inside its box samples it aimed at where the
//! ray actually meets the box rather than at infinity.
//!
//! [`probes`] resolves the scene's boxes headless, the way
//! [`crate::light3d::lights`] resolves its lights, so a test asserts on the
//! list with no GPU. The capture itself is the backend's, and
//! `shaders/mesh.wesl` is where a surface reads one.

use balaur_core::GlobalTransform;
use balaur_core::components::derived::{Component, Enum};
use balaur_core::hecs::{Entity, World};
use balaur_plugin::Registry;
use glamx::Vec3;

use crate::vocabulary::words;
use balaur_core::layers::Layer;

/// A box the room around it was captured inside. A reflective surface within it mirrors that capture, aimed at the box, instead of the distant sky.
// The node's position places the box and its rotation turns it; `size` is in
// world units and does not follow the node's scale, the way a `light3d`'s
// range does not.
#[derive(Component)]
#[component(name = "reflection_probe", tags = [words::PERSPECTIVE, "render"])]
pub struct ReflectionProbe {
    /// The box this probe speaks for, in world units, centred on the node and turned with it. A node with a shader material still reads the box unturned
    #[property(default = Vec3::splat(10.0), min = 0.0)]
    pub size: Vec3,
    /// Brightness of what the probe reflects
    #[property(default = 1.0, min = 0.0)]
    pub intensity: f32,
    /// How wide the soft edge at the box's face is; a surface crossing it fades back to the sky
    #[property(default = 0.5, min = 0.0)]
    pub falloff: f32,
    /// Baked equirectangular image, project-relative. Empty captures the scene from the node's own position
    pub image: String,
    /// Turn of the captured map about y, in degrees
    pub image_rotation_degrees: f32,
    /// When a probe with no `image` captures the scene: once when placed or moved, or every frame. One capture draws the scene six times, at `environment.probe_capture_size_pixels` square
    #[property(default = ProbeUpdate::Once, group = "capture")]
    pub update_mode: ProbeUpdate,
    /// The layers a capture draws: the nodes whose `render_layers` share one. Leave moving nodes off it and let `ssr` reflect them; empty is every layer
    #[property(flags, group = "capture")]
    pub capture_layers: Vec<Layer>,
    /// The nearest distance a capture draws, in world units; with `capture_far` both above zero, else the camera's own planes
    #[property(min = 0.0, group = "capture")]
    pub capture_near: f32,
    /// The farthest distance a capture draws, in world units; zero takes the camera's
    #[property(min = 0.0, group = "capture")]
    pub capture_far: f32,
}

/// When a probe with no baked image captures the scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum ProbeUpdate {
    /// When it is placed, and again whenever it moves.
    Once,
    /// Every frame, so it reflects what moves inside it.
    Always,
}

/// One probe in world space, with everything a backend needs resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct LitProbe {
    pub center: Vec3,
    /// The node's turn: the box is turned with it.
    pub orientation: glamx::Quat,
    pub half_extents: Vec3,
    pub falloff: f32,
    pub intensity: f32,
    /// Radians, as a backend wants it.
    pub rotation: f32,
    pub image: String,
    pub update: ProbeUpdate,
    pub capture_layers: u32,
    /// `None` captures between the camera's own planes.
    pub clip_planes: Option<(f32, f32)>,
    pub enabled: bool,
}

/// Every `reflection_probe` under `root`, in world space and in tree order.
///
/// A hidden node's probe comes back `enabled = false` rather than missing, so
/// a caller counting probes sees the same list whatever is switched off.
pub fn probes(world: &World, root: Entity) -> Vec<LitProbe> {
    let mut out = Vec::new();
    if world.query::<&ReflectionProbe>().iter().next().is_none() {
        return out;
    }
    for entity in balaur_core::scene::collect_subtree(world, root) {
        let (Ok(probe), Ok(global)) = (
            world.get::<&ReflectionProbe>(entity),
            world.get::<&GlobalTransform>(entity),
        ) else {
            continue;
        };
        let visible = world
            .get::<&balaur_core::GlobalAppearance>(entity)
            .is_ok_and(|a| a.visible);
        let (near, far) = (probe.capture_near.max(0.0), probe.capture_far.max(0.0));
        let clip_planes = (near > 0.0 && far > near).then_some((near, far));
        out.push(LitProbe {
            center: global.position,
            orientation: global.rotation,
            // A box with no thickness influences nothing, and dividing the
            // parallax ray by it would be a surface reflecting nothing.
            half_extents: (probe.size / 2.0).max(Vec3::splat(1e-3)),
            falloff: probe.falloff.max(1e-4),
            intensity: probe.intensity.max(0.0),
            rotation: probe.image_rotation_degrees.to_radians(),
            image: probe.image.clone(),
            update: probe.update_mode,
            capture_layers: Layer::bits(&probe.capture_layers, true),
            clip_planes,
            enabled: visible,
        });
    }
    out
}

pub(crate) fn register_reflection_probe_component(reg: &mut Registry<'_>) {
    reg.add_component::<ReflectionProbe>();
}

/// The probes the window holds, and what they were built from.
///
/// A probe is registered once and captured once: the array it lives in is
/// allocated up front, and a capture renders the whole scene six times, which
/// is not something a frame should repeat.
#[cfg(feature = "window")]
#[derive(Default)]
pub(crate) struct ProbeSlots {
    /// What [`probes`] last resolved to, so nothing is re-registered until a
    /// scene actually moves one.
    applied: Vec<LitProbe>,
    /// How many array slots the window has handed out. The fork registers
    /// into a fixed array and never takes one back, so a probe the scene
    /// removed is shrunk to nothing rather than freed.
    registered: usize,
    /// The capture size the maps were filled at; a new one clears them all.
    size: u32,
}

#[cfg(feature = "window")]
impl ProbeSlots {
    /// Register this scene's probes with the window, capturing the ones that
    /// name no baked image.
    ///
    /// The fork's probe array is fixed at registration, so a scene that moves
    /// a probe re-registers every one. That costs a capture, which is why it
    /// happens only when the resolved list actually differs.
    pub(crate) fn sync(&mut self, app: &balaur_core::App, window: &mut kiss3d::window::Window) {
        let resolved = {
            let world = app.engine.world();
            probes(&world, app.engine.root())
        };
        // A new size reallocates the maps black, so every probe fills again.
        let size = window.reflection_probe_size();
        if size != self.size {
            self.size = size;
            self.applied.clear();
        }
        if self.applied == resolved {
            recapture(window, &resolved);
            return;
        }
        self.applied.clone_from(&resolved);
        let live: Vec<&LitProbe> = resolved.iter().filter(|probe| probe.enabled).collect();
        for (index, probe) in live.iter().enumerate() {
            let placed = kiss3d::renderer::ReflectionProbe {
                center: probe.center,
                orientation: probe.orientation,
                half_extents: probe.half_extents,
                falloff: probe.falloff,
                intensity: probe.intensity,
                rotation: probe.rotation,
                clip_planes: probe.clip_planes,
                capture_layers: probe.capture_layers,
            };
            if index < self.registered {
                if let Some(slot) = window.reflection_probe_mut(index) {
                    *slot = placed;
                }
            } else if window.add_reflection_probe(placed).is_some() {
                self.registered += 1;
            } else {
                tracing::warn!(
                    "the scene places more reflection probes than the renderer holds; \
                     surfaces past the last one reflect the sky"
                );
                break;
            }
            match probe.image.as_str() {
                "" => window.capture_reflection_probe(index),
                path => match baked(app, path) {
                    Ok(image) => window.set_reflection_probe_image(index, &image),
                    Err(why) => {
                        tracing::warn!("reflection probe '{path}': {why:#}");
                        window.capture_reflection_probe(index);
                    }
                },
            }
        }
        // A probe the scene stopped placing keeps its array slot; shrinking
        // its box to nothing is what makes it speak for no point.
        for index in live.len()..self.registered {
            if let Some(slot) = window.reflection_probe_mut(index) {
                slot.half_extents = Vec3::splat(1e-4);
                slot.intensity = 0.0;
            }
        }
    }
}

/// Queue this frame's capture of every live probe that asks for one each
/// frame and has no baked image to show instead.
#[cfg(feature = "window")]
fn recapture(window: &mut kiss3d::window::Window, resolved: &[LitProbe]) {
    let live = resolved.iter().filter(|probe| probe.enabled);
    for (index, probe) in live.enumerate() {
        if probe.update == ProbeUpdate::Always && probe.image.is_empty() {
            window.capture_reflection_probe(index);
        }
    }
}

/// A baked probe map, read out of the project the way a sky is: through the
/// project reader, so a packed game carries it inside the pack.
#[cfg(feature = "window")]
fn baked(app: &balaur_core::App, path: &str) -> anyhow::Result<image::DynamicImage> {
    let files = app.engine.resource::<balaur_core::project::ProjectFiles>();
    let bytes = files.borrow().read(path)?;
    Ok(image::load_from_memory(&bytes)?)
}
