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

use anyhow::anyhow;
use balaur_core::GlobalTransform;
use balaur_core::components::{ComponentDef, as_f64, prop_str};
use balaur_core::hecs::{Entity, World};
use balaur_plugin::Registry;
use glamx::Vec3;

use crate::vocabulary::{keys as k, words};

/// The `reflection_probe` component's authored state. The node's position
/// places the box; `size` is in world units and does not follow the
/// node's scale, the way a `light3d`'s radius does not.
pub struct ReflectionProbe {
    pub half_extents: Vec3,
    /// How wide the soft edge at the box's face is, in world units: a surface
    /// crossing it fades back to the sky rather than jumping.
    pub falloff: f32,
    pub intensity: f32,
    /// Turn about y, in degrees, matching `environment.sky_rotation_degrees`.
    pub rotation: f32,
    /// A baked equirectangular image, project-relative. Empty captures the
    /// scene from the probe's own position instead.
    pub image: String,
}

/// One probe in world space, with everything a backend needs resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct LitProbe {
    pub center: Vec3,
    pub half_extents: Vec3,
    pub falloff: f32,
    pub intensity: f32,
    /// Radians, as a backend wants it.
    pub rotation: f32,
    pub image: String,
    pub enabled: bool,
}

/// Every `reflection_probe` under `root`, in world space and in tree order.
///
/// A hidden node's probe comes back `enabled = false` rather than missing, so
/// a caller counting probes sees the same list whatever is switched off.
pub fn probes(world: &World, root: Entity) -> Vec<LitProbe> {
    let mut out = Vec::new();
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
        out.push(LitProbe {
            center: global.position,
            // A box with no thickness influences nothing, and dividing the
            // parallax ray by it would be a surface reflecting nothing.
            half_extents: probe.half_extents.max(Vec3::splat(1e-3)),
            falloff: probe.falloff.max(1e-4),
            intensity: probe.intensity.max(0.0),
            rotation: probe.rotation.to_radians(),
            image: probe.image.clone(),
            enabled: visible,
        });
    }
    out
}

fn probe_schema() -> String {
    r#"size = { type = "vec3", default = [10.0, 10.0, 10.0], min = 0.0, description = "The box this probe speaks for, in world units, centred on the node" }
falloff = { type = "float", default = 0.5, min = 0.0, description = "How wide the soft edge at the box's face is; a surface crossing it fades back to the sky" }
intensity = { type = "float", default = 1.0, min = 0.0, description = "Brightness of what the probe reflects" }
image_rotation_degrees = { type = "float", default = 0.0, description = "Turn of the captured map about y, in degrees" }
image = { type = "string", default = "", description = "Baked equirectangular image, project-relative. Empty captures the scene from the node's own position" }"#
        .to_string()
}

/// The `reflection_probe` component.
pub(crate) fn register_reflection_probe_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "reflection_probe",
        ComponentDef {
            warnings: None,
            doc: "A box the room around it was captured inside. A reflective surface within it mirrors that capture, aimed at the box, instead of the distant sky.",
            schema: ComponentDef::parse_schema("reflection_probe", &probe_schema()),
            tags: &[words::PERSPECTIVE, "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let num = |key: &str, default: f32| {
                    params.get(key).and_then(as_f64).unwrap_or(f64::from(default)) as f32
                };
                let extent = |i: usize, default: f32| {
                    params
                        .get(k::SIZE)
                        .and_then(toml::Value::as_array)
                        .and_then(|a| a.get(i))
                        .and_then(as_f64)
                        .unwrap_or(f64::from(default)) as f32
                        / 2.0
                };
                let next = ReflectionProbe {
                    half_extents: Vec3::new(extent(0, 10.0), extent(1, 10.0), extent(2, 10.0)),
                    falloff: num(k::FALLOFF, 0.5),
                    intensity: num(k::INTENSITY, 1.0),
                    rotation: num(k::IMAGE_ROTATION_DEGREES, 0.0),
                    image: prop_str(params, k::IMAGE).to_string(),
                };
                let mut world = eng.world_mut();
                if let Ok(mut probe) = world.get::<&mut ReflectionProbe>(entity) {
                    *probe = next;
                    return Ok(());
                }
                world
                    .insert_one(entity, next)
                    .map_err(|_| anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<ReflectionProbe>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let probe = world.get::<&ReflectionProbe>(entity).ok()?;
                let mut map = toml::map::Map::new();
                map.insert(
                    k::SIZE.into(),
                    toml::Value::Array(
                        probe
                            .half_extents
                            .to_array()
                            .iter()
                            .map(|v| toml::Value::Float(f64::from(*v * 2.0)))
                            .collect(),
                    ),
                );
                map.insert(
                    k::FALLOFF.into(),
                    toml::Value::Float(f64::from(probe.falloff)),
                );
                map.insert(
                    k::INTENSITY.into(),
                    toml::Value::Float(f64::from(probe.intensity)),
                );
                map.insert(
                    k::IMAGE_ROTATION_DEGREES.into(),
                    toml::Value::Float(f64::from(probe.rotation)),
                );
                map.insert(k::IMAGE.into(), toml::Value::String(probe.image.clone()));
                Some(toml::Value::Table(map))
            }),
        },
    );
}

/// The probes the window holds, and what they were built from.
///
/// A probe is registered once and captured once: the array it lives in is
/// allocated up front, and a capture renders the whole scene six times, which
/// is not something a frame should repeat.
#[cfg(feature = "kiss3d")]
#[derive(Default)]
pub(crate) struct ProbeSlots {
    /// What [`probes`] last resolved to, so nothing is re-registered until a
    /// scene actually moves one.
    applied: Vec<LitProbe>,
    /// How many array slots the window has handed out. The fork registers
    /// into a fixed array and never takes one back, so a probe the scene
    /// removed is shrunk to nothing rather than freed.
    registered: usize,
}

#[cfg(feature = "kiss3d")]
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
        if self.applied == resolved {
            return;
        }
        self.applied.clone_from(&resolved);
        let live: Vec<&LitProbe> = resolved.iter().filter(|probe| probe.enabled).collect();
        for (index, probe) in live.iter().enumerate() {
            let placed = kiss3d::renderer::ReflectionProbe {
                center: probe.center,
                half_extents: probe.half_extents,
                falloff: probe.falloff,
                intensity: probe.intensity,
                rotation: probe.rotation,
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

/// A baked probe map, read out of the project the way a sky is: through the
/// project reader, so a packed game carries it inside the pack.
#[cfg(feature = "kiss3d")]
fn baked(app: &balaur_core::App, path: &str) -> anyhow::Result<image::DynamicImage> {
    let files = app.engine.resource::<balaur_core::project::ProjectFiles>();
    let bytes = files.borrow().read(path)?;
    Ok(image::load_from_memory(&bytes)?)
}
