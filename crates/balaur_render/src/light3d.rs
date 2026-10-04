//! `light3d`: a light a scene places, and the `environment` it sits in.
//!
//! Both resolve headless. [`lights`] turns the scene tree into the world-space
//! list a backend uploads, and [`environment`] reads the scene's atmosphere,
//! so a test asserts on either with no GPU. A scene with no `light3d` keeps
//! the backend's own sun, which is what lets every example draw unchanged.

use anyhow::{Result, anyhow};
use balaur_core::components::{
    ComponentDef, as_f64, prop_bool, prop_f32, prop_i64, prop_str, prop_vec3,
};
use balaur_core::hecs::{Entity, World};
use balaur_core::{Engine, GlobalTransform};
use balaur_plugin::Registry;
use glamx::Vec3;

use crate::vocabulary::{keys as k, words};
use crate::{color_from_params, color_to_toml};

/// Which way a `light3d` throws light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightKind3d {
    /// Radiates from the node, fading to nothing at `radius`.
    Point,
    /// Parallel rays across the whole scene, aimed by the node's rotation.
    Directional,
    /// A cone from the node, aimed by its rotation.
    Spot,
}

/// The `light3d` component's authored state. The node's global pose places and
/// aims it; `radius` is in world units and does not follow the node's scale.
pub struct Light3d {
    pub kind: LightKind3d,
    pub color: [f32; 4],
    pub intensity: f32,
    pub radius: f32,
    /// Full brightness inside this cone, in degrees. Spot only.
    pub inner: f32,
    /// Fading to nothing at this cone, in degrees. Spot only.
    pub outer: f32,
    pub shadows: bool,
    /// Which light layers this reaches. A renderable is lit when its own
    /// layers share a bit with these.
    pub layers: u32,
    /// The emitting sphere's radius, which only the path tracer reads.
    pub source_radius: f32,
}

/// One light in world space, with everything a backend needs resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LitLight3d {
    pub position: Vec3,
    /// Where the light shines, unit length. A point light's is the vector the
    /// node aims, and nothing reads it.
    pub direction: Vec3,
    pub color: [f32; 3],
    pub intensity: f32,
    pub radius: f32,
    pub inner: f32,
    pub outer: f32,
    pub shadows: bool,
    pub layers: u32,
    pub source_radius: f32,
    pub kind: LightKind3d,
    pub enabled: bool,
}

/// The direction a node aims. At rest a `light3d` shines along -z, which is
/// what kiss3d, glTF and every camera in the tree already call forward.
fn aim(global: &GlobalTransform) -> Vec3 {
    (global.rotation * Vec3::NEG_Z)
        .try_normalize()
        .unwrap_or(Vec3::NEG_Z)
}

/// Every `light3d` under `root`, in world space and in tree order.
///
/// A hidden node's light comes back `enabled = false` rather than missing, so
/// a caller counting lights sees the same list whatever is switched off.
pub fn lights(world: &World, root: Entity) -> Vec<LitLight3d> {
    let mut out = Vec::new();
    for entity in balaur_core::scene::collect_subtree(world, root) {
        let (Ok(light), Ok(global)) = (
            world.get::<&Light3d>(entity),
            world.get::<&GlobalTransform>(entity),
        ) else {
            continue;
        };
        let visible = world
            .get::<&balaur_core::GlobalAppearance>(entity)
            .is_ok_and(|a| a.visible);
        let [r, g, b, _] = light.color;
        out.push(LitLight3d {
            position: global.position,
            direction: aim(&global),
            color: [r, g, b],
            intensity: light.intensity.max(0.0),
            radius: light.radius.max(0.0),
            inner: light.inner.clamp(0.0, 179.0),
            outer: light.outer.clamp(0.0, 179.0),
            shadows: light.shadows,
            layers: light.layers,
            source_radius: light.source_radius.max(0.0),
            kind: light.kind,
            enabled: visible,
        });
    }
    out
}

fn light_schema() -> String {
    let kinds = crate::vocabulary::options(words::LIGHT_KINDS_3D);
    let default = words::DIRECTIONAL;
    format!(
        r#"kind = {{ type = "enum", default = "{default}", options = [{kinds}], description = "A point light fades to nothing at `range`, a directional one lights the whole scene, a spot one throws a cone the node aims" }}
color = {{ type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "Light colour, as channel floats or #rrggbb / #rrggbbaa" }}
intensity = {{ type = "float", default = 3.0, min = 0.0, description = "Brightness multiplier; over 1 blows past white" }}
range = {{ type = "float", default = 30.0, min = 0.0, description = "How far a point or spot light reaches, in world units" }}
inner_angle_degrees = {{ type = "float", default = 20.0, min = 0.0, max = 179.0, description = "Half-angle of a spot light's full-brightness cone, in degrees" }}
outer_angle_degrees = {{ type = "float", default = 35.0, min = 0.0, max = 179.0, description = "Half-angle a spot light fades to nothing at, in degrees" }}
shadow_enabled = {{ type = "bool", default = true, description = "Whether this light casts shadows from the nodes that say they cast" }}
light_layers = {{ type = "int", default = -1, description = "Light-layer bitmask; a node is lit when its own `light_layers` share a bit with these. -1 is every layer" }}
source_radius = {{ type = "float", default = 0.0, min = 0.0, description = "Radius of the sphere the light shines from, in world units, which softens its shadows. Read by the path tracer only; the rasterizer's edge is `environment.shadow_softness`" }}"#
    )
}

/// The `light3d` component. The node's position places it and its rotation
/// aims it; `visible = false` switches it off.
pub(crate) fn register_light3d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "light3d",
        ComponentDef {
            events: &[],
            warnings: None,
            doc: "A 3D light placed and aimed by the node. `kind` is `directional`, `point` or `spot`; the first `light3d` in a scene retires the engine's default key light.",
            schema: ComponentDef::parse_schema("light3d", &light_schema()),
            tags: &[words::PERSPECTIVE, "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let kind = match prop_str(params, k::KIND) {
                    words::POINT => LightKind3d::Point,
                    words::DIRECTIONAL => LightKind3d::Directional,
                    words::SPOT => LightKind3d::Spot,
                    other => return Err(anyhow!("unknown light3d kind '{other}'")),
                };
                let num = |key: &str, default: f64| {
                    params.get(key).and_then(as_f64).unwrap_or(default) as f32
                };
                let layers = params
                    .get(k::LIGHT_LAYERS)
                    .and_then(as_f64)
                    .map_or(u32::MAX, |v| v as i64 as u32);
                set_light(
                    eng,
                    entity,
                    Light3d {
                        kind,
                        color: color_from_params(params),
                        intensity: num(k::INTENSITY, 3.0).max(0.0),
                        radius: num(k::RANGE, 30.0).max(0.0),
                        inner: num(k::INNER_ANGLE_DEGREES, 20.0),
                        outer: num(k::OUTER_ANGLE_DEGREES, 35.0),
                        shadows: prop_bool(params, k::SHADOW_ENABLED),
                        layers,
                        source_radius: num(k::SOURCE_RADIUS, 0.0).max(0.0),
                    },
                )
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Light3d>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let light = world.get::<&Light3d>(entity).ok()?;
                let kind = match light.kind {
                    LightKind3d::Point => words::POINT,
                    LightKind3d::Directional => words::DIRECTIONAL,
                    LightKind3d::Spot => words::SPOT,
                };
                let mut map = toml::map::Map::new();
                map.insert(k::KIND.into(), toml::Value::String(kind.into()));
                map.insert(k::COLOR.into(), color_to_toml(light.color));
                map.insert(
                    k::INTENSITY.into(),
                    toml::Value::Float(f64::from(light.intensity)),
                );
                map.insert(k::RANGE.into(), toml::Value::Float(f64::from(light.radius)));
                map.insert(k::INNER_ANGLE_DEGREES.into(), toml::Value::Float(f64::from(light.inner)));
                map.insert(k::OUTER_ANGLE_DEGREES.into(), toml::Value::Float(f64::from(light.outer)));
                map.insert(k::SHADOW_ENABLED.into(), toml::Value::Boolean(light.shadows));
                map.insert(
                    k::LIGHT_LAYERS.into(),
                    toml::Value::Integer(i64::from(light.layers.cast_signed())),
                );
                map.insert(
                    k::SOURCE_RADIUS.into(),
                    toml::Value::Float(f64::from(light.source_radius)),
                );
                Some(toml::Value::Table(map))
            }),
        },
    );
}

fn set_light(eng: &Engine, entity: Entity, next: Light3d) -> Result<()> {
    let mut world = eng.world_mut();
    if let Ok(mut light) = world.get::<&mut Light3d>(entity) {
        *light = next;
        return Ok(());
    }
    world
        .insert_one(entity, next)
        .map_err(|_| anyhow!("node is dead"))
}

/// How fog thickens with distance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FogKind {
    Off,
    /// Ramps between `start` and `end`, in view-space distance.
    Linear,
    /// `1 - exp(-density * distance)`.
    Exponential,
    /// `1 - exp(-(density * distance)^2)`; a sharper onset.
    ExponentialSquared,
}

/// Which curve the HDR film is mapped through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tonemap {
    None,
    Aces,
    Reinhard,
    AgX,
    Neutral,
    TonyMcMapface,
}

/// How glass blurs what it refracts: kiss3d's `TransmissionBlurQuality`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlurQuality {
    Low,
    Medium,
    High,
}

/// Eye adaptation: kiss3d's auto-exposure, which replaces `exposure` while on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoExposure {
    pub enabled: bool,
    /// How fast the exposure follows the scene, per second.
    pub speed: f32,
    pub min: f32,
    pub max: f32,
    /// The mid grey the scene's average brightness is mapped to.
    pub key: f32,
}

impl Default for AutoExposure {
    fn default() -> Self {
        Self {
            enabled: false,
            speed: 3.0,
            min: 0.05,
            max: 8.0,
            key: 0.18,
        }
    }
}

/// Refractive glass: whether it is drawn, and how.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transmission {
    pub enabled: bool,
    pub blur_quality: BlurQuality,
    /// How many layers of glass seen through glass are drawn.
    pub steps: u32,
}

impl Default for Transmission {
    fn default() -> Self {
        Self {
            enabled: true,
            blur_quality: BlurQuality::High,
            steps: 1,
        }
    }
}

/// The `environment` component: everything about a scene's atmosphere that is
/// scene-wide rather than per view. `camera.post` keeps the per-view passes.
#[derive(Clone, Debug, PartialEq)]
pub struct Environment {
    pub current: bool,
    /// Equirectangular `.hdr` or `.exr`; empty means no sky and no image-based
    /// lighting.
    pub sky: String,
    pub sky_intensity: f32,
    /// Degrees about y.
    pub sky_rotation: f32,
    /// False takes the sky away: it stops drawing and stops lighting, and
    /// `ambient` lights the scene as it does with no sky at all.
    pub show_sky: bool,
    pub ambient: [f32; 4],
    pub fog: FogKind,
    pub fog_color: [f32; 4],
    pub fog_density: f32,
    pub fog_start: f32,
    pub fog_end: f32,
    pub fog_height_falloff: f32,
    pub exposure: f32,
    pub tonemap: Tonemap,
    pub saturation: f32,
    pub contrast: f32,
    pub gamma: f32,
    /// Per-channel linear gain before the tonemap.
    pub white_balance: [f32; 3],
    /// Turn of every hue about the grey axis, in degrees.
    pub hue: f32,
    pub auto_exposure: AutoExposure,
    pub transmission: Transmission,
    pub shadows: bool,
    pub shadow_resolution: u32,
    pub shadow_softness: f32,
    pub shadow_distance: f32,
    pub shadow_cascades: u32,
    pub shadow_first_cascade_distance: f32,
    /// What the lighting shader subtracts comparing against the shadow map.
    pub shadow_bias: f32,
    /// The shadow depth pass's rasterizer bias, in depth-buffer units.
    pub shadow_constant_bias: i32,
    pub shadow_slope_bias: f32,
    /// Atlas layers every casting light shares.
    pub shadow_views: u32,
    /// An image that lights the scene instead of the drawn sky; empty lights
    /// with the sky.
    pub sky_light: String,
    /// `None` follows `sky_intensity`.
    pub sky_light_intensity: Option<f32>,
    pub probe_capture_size: u32,
    pub cluster_grid: [u32; 3],
    pub cluster_max_lights: u32,
}

impl Environment {
    /// A scene with no `environment` node: the defaults with no tonemap, so
    /// the frame shows its colours as authored. A node's own default is
    /// `neutral`.
    #[must_use]
    pub fn none() -> Self {
        Self {
            tonemap: Tonemap::None,
            ..Self::default()
        }
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            current: true,
            sky: String::new(),
            sky_intensity: 1.0,
            sky_rotation: 0.0,
            show_sky: true,
            ambient: [0.125, 0.14, 0.157, 1.0],
            fog: FogKind::Off,
            fog_color: [0.624, 0.706, 0.784, 1.0],
            fog_density: 0.02,
            fog_start: 10.0,
            fog_end: 80.0,
            fog_height_falloff: 0.0,
            exposure: 1.0,
            tonemap: Tonemap::Neutral,
            saturation: 1.0,
            contrast: 1.0,
            gamma: 1.0,
            white_balance: [1.0, 1.0, 1.0],
            hue: 0.0,
            auto_exposure: AutoExposure::default(),
            transmission: Transmission::default(),
            shadows: true,
            shadow_resolution: 2048,
            shadow_softness: 1.0,
            shadow_distance: 60.0,
            shadow_cascades: 4,
            shadow_first_cascade_distance: 12.0,
            shadow_bias: 0.0012,
            shadow_constant_bias: 1,
            shadow_slope_bias: 1.75,
            shadow_views: 16,
            sky_light: String::new(),
            sky_light_intensity: None,
            probe_capture_size: 256,
            cluster_grid: [16, 9, 24],
            cluster_max_lights: 256,
        }
    }
}

/// The environment a scene draws under: the last `current` one in tree order,
/// so a level can carry two and switch. `None` when no node has one, which is
/// what leaves the backend's own defaults alone.
/// The environment a scene draws under: its current `environment` node, or
/// [`Environment::none`] when it has none.
#[must_use]
pub fn environment_or_none(world: &World, root: Entity) -> Environment {
    environment(world, root).unwrap_or_else(Environment::none)
}

pub fn environment(world: &World, root: Entity) -> Option<Environment> {
    let mut found = None;
    for entity in balaur_core::scene::collect_subtree(world, root) {
        if let Ok(env) = world.get::<&Environment>(entity)
            && env.current
        {
            found = Some(Environment::clone(&env));
        }
    }
    found
}

fn environment_schema() -> String {
    let fogs = crate::vocabulary::options(words::FOG_KINDS);
    let tonemaps = crate::vocabulary::options(words::TONEMAPS);
    format!(
        r#"current = {{ type = "bool", default = true, description = "Whether this is the environment the scene draws under; the last current one in tree order wins" }}
sky = {{ type = "string", default = "", description = "Equirectangular image, project-relative: .hdr, .exr or .png. It draws behind the scene and lights it. Empty is no sky" }}
sky_intensity = {{ type = "float", default = 1.0, min = 0.0, description = "Brightness of the sky, and of the light it casts" }}
sky_rotation_degrees = {{ type = "float", default = 0.0, description = "Turn of the sky about y, in degrees" }}
sky_enabled = {{ type = "bool", default = true, description = "False takes the sky away: it stops drawing and stops lighting, and `ambient_color` lights the scene as it does with no sky" }}
ambient_color = {{ type = "color", default = [0.125, 0.14, 0.157, 1.0], description = "Light every surface gets whatever the lights do" }}
fog_mode = {{ type = "enum", default = "{none}", options = [{fogs}], description = "How fog thickens with distance" }}
fog_color = {{ type = "color", default = [0.624, 0.706, 0.784, 1.0], description = "What distance fades toward" }}
fog_density = {{ type = "float", default = 0.02, min = 0.0, description = "Thickness, for exponential fog" }}
fog_start = {{ type = "float", default = 10.0, min = 0.0, description = "Where linear fog begins, in world units" }}
fog_end = {{ type = "float", default = 80.0, min = 0.0, description = "Where linear fog is total, in world units" }}
fog_height_falloff = {{ type = "float", default = 0.0, min = 0.0, description = "How fast fog thins with height; zero fills the scene evenly" }}
exposure = {{ type = "float", default = 1.0, min = 0.0, description = "Linear multiplier before the tonemap" }}
tonemap = {{ type = "enum", default = "{neutral}", options = [{tonemaps}], description = "The curve the HDR film is mapped through. A scene with no `environment` node draws untonemapped" }}
saturation = {{ type = "float", default = 1.0, min = 0.0, description = "Colour multiplier around luminance; zero is grey" }}
contrast = {{ type = "float", default = 1.0, min = 0.0, description = "Contrast around mid grey" }}
gamma = {{ type = "float", default = 1.0, min = 0.01, description = "Gamma applied in linear space" }}
white_balance = {{ type = "vec3", default = [1.0, 1.0, 1.0], description = "Per-channel linear gain before the tonemap; [1, 1, 1] leaves colour alone" }}
hue_degrees = {{ type = "float", default = 0.0, description = "Turns every hue about the grey axis before the tonemap, in degrees" }}
auto_exposure_enabled = {{ type = "bool", default = false, description = "Measure the frame's average brightness and adapt the exposure to it, replacing `exposure`. It adapts on the wall clock, so two captures of one fixed-step frame can differ" }}
auto_exposure_speed = {{ type = "float", default = 3.0, min = 0.0, description = "How fast auto-exposure follows the scene, per second" }}
auto_exposure_min = {{ type = "float", default = 0.05, min = 0.0, description = "The lowest exposure auto-exposure settles at, for the brightest scenes" }}
auto_exposure_max = {{ type = "float", default = 8.0, min = 0.0, description = "The highest exposure auto-exposure settles at, for the darkest scenes" }}
auto_exposure_key = {{ type = "float", default = 0.18, min = 0.0, description = "The mid grey auto-exposure maps the frame's average brightness to" }}
transmission_enabled = {{ type = "bool", default = true, description = "Whether glass refracts what is behind it. Off draws a material with `transmission` as plain opaque PBR and skips the passes" }}
transmission_blur_quality = {{ type = "enum", default = "{high}", options = [{qualities}], description = "How smoothly rough glass blurs what it refracts; low is cheapest and can look blocky" }}
transmission_steps = {{ type = "int", default = 1, min = 1, description = "How many layers of glass seen through glass are drawn; each costs another snapshot of the scene and another glass pass" }}
shadow_enabled = {{ type = "bool", default = true, description = "Whether any light casts shadows at all" }}
shadow_resolution = {{ type = "int", default = 2048, min = 256, description = "Side of the shadow map, in texels" }}
shadow_softness = {{ type = "float", default = 1.0, min = 0.0, description = "How far a shadow's edge is blurred" }}
shadow_distance = {{ type = "float", default = 60.0, min = 0.0, description = "How far along the view a directional light's cascades reach, in world units; the camera's `far` caps it" }}
{budget}"#,
        budget = shadow_budget_schema(),
        none = words::NONE,
        neutral = words::NEUTRAL,
        high = words::HIGH,
        qualities = crate::vocabulary::options(words::BLUR_QUALITIES),
    )
}

/// Read an `environment` table. Lifted out of the component's `apply`, which
/// is otherwise one expression per key and nothing else.
fn environment_from_params(params: &toml::Value) -> Result<Environment> {
    let base = Environment::default();
    let num = |key: &str, default: f32| {
        params
            .get(key)
            .and_then(as_f64)
            .unwrap_or(f64::from(default)) as f32
    };
    let flag = |key: &str, default: bool| {
        params
            .get(key)
            .and_then(toml::Value::as_bool)
            .unwrap_or(default)
    };
    let next = Environment {
        current: flag(k::CURRENT, true),
        sky: params
            .get(k::SKY)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        sky_intensity: num(k::SKY_INTENSITY, base.sky_intensity).max(0.0),
        sky_rotation: num(k::SKY_ROTATION_DEGREES, base.sky_rotation),
        show_sky: flag(k::SKY_ENABLED, true),
        ambient: crate::color_from_key(params, k::AMBIENT_COLOR, base.ambient),
        fog: match params
            .get(k::FOG_MODE)
            .and_then(toml::Value::as_str)
            .unwrap_or(words::NONE)
        {
            words::NONE => FogKind::Off,
            words::LINEAR => FogKind::Linear,
            words::EXPONENTIAL => FogKind::Exponential,
            words::EXPONENTIAL_SQUARED => FogKind::ExponentialSquared,
            other => return Err(anyhow!("unknown fog '{other}'")),
        },
        fog_color: crate::color_from_key(params, k::FOG_COLOR, base.fog_color),
        fog_density: num(k::FOG_DENSITY, base.fog_density).max(0.0),
        fog_start: num(k::FOG_START, base.fog_start),
        fog_end: num(k::FOG_END, base.fog_end),
        fog_height_falloff: num(k::FOG_HEIGHT_FALLOFF, 0.0).max(0.0),
        exposure: num(k::EXPOSURE, base.exposure).max(0.0),
        tonemap: match params
            .get(k::TONEMAP)
            .and_then(toml::Value::as_str)
            .unwrap_or(words::NEUTRAL)
        {
            words::NONE => Tonemap::None,
            words::ACES => Tonemap::Aces,
            words::REINHARD => Tonemap::Reinhard,
            words::AGX => Tonemap::AgX,
            words::NEUTRAL => Tonemap::Neutral,
            words::TONY_MCMAPFACE => Tonemap::TonyMcMapface,
            other => return Err(anyhow!("unknown tonemap '{other}'")),
        },
        saturation: num(k::SATURATION, base.saturation).max(0.0),
        contrast: num(k::CONTRAST, base.contrast).max(0.0),
        gamma: num(k::GAMMA, base.gamma).max(0.01),
        white_balance: prop_vec3(params, k::WHITE_BALANCE).map(|gain| gain.max(0.0)),
        hue: prop_f32(params, k::HUE_DEGREES),
        auto_exposure: AutoExposure {
            enabled: prop_bool(params, k::AUTO_EXPOSURE_ENABLED),
            speed: prop_f32(params, k::AUTO_EXPOSURE_SPEED).max(0.0),
            min: prop_f32(params, k::AUTO_EXPOSURE_MIN).max(0.0),
            max: prop_f32(params, k::AUTO_EXPOSURE_MAX).max(0.0),
            key: prop_f32(params, k::AUTO_EXPOSURE_KEY).max(0.0),
        },
        transmission: Transmission {
            enabled: prop_bool(params, k::TRANSMISSION_ENABLED),
            blur_quality: match prop_str(params, k::TRANSMISSION_BLUR_QUALITY) {
                words::LOW => BlurQuality::Low,
                words::MEDIUM => BlurQuality::Medium,
                words::HIGH => BlurQuality::High,
                other => return Err(anyhow!("unknown transmission_blur_quality '{other}'")),
            },
            steps: u32::try_from(prop_i64(params, k::TRANSMISSION_STEPS).max(1)).unwrap_or(1),
        },
        shadows: flag(k::SHADOW_ENABLED, true),
        shadow_resolution: num(k::SHADOW_RESOLUTION, 2048.0) as u32,
        shadow_softness: num(k::SHADOW_SOFTNESS, base.shadow_softness).max(0.0),
        shadow_distance: num(k::SHADOW_DISTANCE, base.shadow_distance).max(0.0),
        shadow_cascades: count(params, k::SHADOW_CASCADES).clamp(1, 4),
        shadow_first_cascade_distance: num(
            k::SHADOW_FIRST_CASCADE_DISTANCE,
            base.shadow_first_cascade_distance,
        )
        .max(0.01),
        shadow_bias: num(k::SHADOW_BIAS, base.shadow_bias).max(0.0),
        shadow_constant_bias: i32::try_from(prop_i64(params, k::SHADOW_CONSTANT_BIAS))
            .unwrap_or(base.shadow_constant_bias),
        shadow_slope_bias: num(k::SHADOW_SLOPE_BIAS, base.shadow_slope_bias),
        shadow_views: count(params, k::SHADOW_VIEWS).clamp(1, 64),
        sky_light: prop_str(params, k::SKY_LIGHT).to_string(),
        sky_light_intensity: Some(prop_f32(params, k::SKY_LIGHT_INTENSITY)).filter(|v| *v >= 0.0),
        probe_capture_size: count(params, k::PROBE_CAPTURE_SIZE_PIXELS).clamp(2, 4096),
        cluster_grid: prop_vec3(params, k::CLUSTER_GRID)
            .map(|n| n.round().clamp(1.0, 128.0) as u32),
        cluster_max_lights: count(params, k::CLUSTER_MAX_LIGHTS).max(1),
    };
    Ok(next)
}

/// A whole-number key as kiss3d's counters take it; a negative one is zero.
fn count(params: &toml::Value, key: &str) -> u32 {
    u32::try_from(prop_i64(params, key).max(0)).unwrap_or(u32::MAX)
}

/// The shadow knobs past distance, the sky's own light, and the limits the
/// renderer sizes its buffers by: one of each per window, so on `environment`.
fn shadow_budget_schema() -> &'static str {
    r#"shadow_cascades = { type = "int", default = 4, min = 1, max = 4, description = "How many cascades a directional light splits its shadow range into; each takes one of `shadow_views`" }
shadow_first_cascade_distance = { type = "float", default = 12.0, min = 0.01, description = "How far along the view the first, sharpest cascade reaches, in world units" }
shadow_bias = { type = "float", default = 0.0012, min = 0.0, description = "Depth the lighting pass allows before it calls a point shadowed. Raise it to cure acne; lower it to keep contact shadows attached. Not yet honoured on a node with a shader material" }
shadow_constant_bias = { type = "int", default = 1, description = "Depth-buffer units the shadow pass pushes every caster back by" }
shadow_slope_bias = { type = "float", default = 1.75, description = "How far the shadow pass pushes a caster back per unit of its depth slope, so a surface seen edge-on by the light does not shadow itself" }
shadow_views = { type = "int", default = 16, min = 1, max = 64, description = "Shadow-map layers every casting light shares: a spot light takes one, a directional one a layer per cascade and a point one six. A light past the budget lights without a shadow. Each layer costs `shadow_resolution` squared times eight bytes" }
sky_light = { type = "string", default = "", description = "Equirectangular image, project-relative, that lights the scene in place of `sky`, which still draws. Empty lights with `sky`; it lights even with no `sky` drawn, and `sky_enabled = false` takes it away too" }
sky_light_intensity = { type = "float", default = -1.0, min = -1.0, description = "Brightness of the light the sky casts, apart from how bright it draws; below zero follows `sky_intensity`" }
probe_capture_size_pixels = { type = "int", default = 256, min = 2, max = 4096, description = "Side of each cube face a `reflection_probe` captures, and the width of every probe map, which all probes share. A change clears every map and captures the probes again" }
cluster_grid = { type = "vec3", default = [16.0, 9.0, 24.0], description = "Clusters the many-light pass cuts the view into, across x, across y and along depth, each 1 to 128. Finer culls tighter and costs memory. Not drawn on WebGL2, which has no compute" }
cluster_max_lights = { type = "int", default = 256, min = 1, description = "Lights one cluster records; past it a dense cluster drops lights. The renderer lowers it to fit the device's largest storage buffer" }"#
}

/// The `environment` component: sky, ambient, fog, exposure, tonemap, grading
/// and the shadow budget, all of which belong to the scene rather than a view.
pub(crate) fn register_environment_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "environment",
        ComponentDef {
            events: &[],
            warnings: None,
            doc: "The scene's atmosphere: `sky`, `ambient_color`, `fog_mode`, `exposure`, `tonemap`, colour grading and the shadow budget. The last `current` one wins; per-view effects stay on `camera.post`.",
            schema: ComponentDef::parse_schema("environment", &environment_schema()),
            tags: &[words::PERSPECTIVE, "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let next = environment_from_params(params)?;
                let mut world = eng.world_mut();
                if let Ok(mut env) = world.get::<&mut Environment>(entity) {
                    *env = next;
                    return Ok(());
                }
                world
                    .insert_one(entity, next)
                    .map_err(|_| anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Environment>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let env = world.get::<&Environment>(entity).ok()?;
                let fog = match env.fog {
                    FogKind::Off => words::NONE,
                    FogKind::Linear => words::LINEAR,
                    FogKind::Exponential => words::EXPONENTIAL,
                    FogKind::ExponentialSquared => words::EXPONENTIAL_SQUARED,
                };
                let tonemap = match env.tonemap {
                    Tonemap::None => words::NONE,
                    Tonemap::Aces => words::ACES,
                    Tonemap::Reinhard => words::REINHARD,
                    Tonemap::AgX => words::AGX,
                    Tonemap::Neutral => words::NEUTRAL,
                    Tonemap::TonyMcMapface => words::TONY_MCMAPFACE,
                };
                let mut map = toml::map::Map::new();
                let mut put = |key: &str, value: toml::Value| {
                    map.insert(key.to_string(), value);
                };
                let float = |v: f32| toml::Value::Float(f64::from(v));
                put(k::CURRENT, toml::Value::Boolean(env.current));
                put(k::SKY, toml::Value::String(env.sky.clone()));
                put(k::SKY_INTENSITY, float(env.sky_intensity));
                put(k::SKY_ROTATION_DEGREES, float(env.sky_rotation));
                put(k::SKY_ENABLED, toml::Value::Boolean(env.show_sky));
                put(k::AMBIENT_COLOR, color_to_toml(env.ambient));
                put(k::FOG_MODE, toml::Value::String(fog.into()));
                put(k::FOG_COLOR, color_to_toml(env.fog_color));
                put(k::FOG_DENSITY, float(env.fog_density));
                put(k::FOG_START, float(env.fog_start));
                put(k::FOG_END, float(env.fog_end));
                put(k::FOG_HEIGHT_FALLOFF, float(env.fog_height_falloff));
                put(k::EXPOSURE, float(env.exposure));
                put(k::TONEMAP, toml::Value::String(tonemap.into()));
                put(k::SATURATION, float(env.saturation));
                put(k::CONTRAST, float(env.contrast));
                put(k::GAMMA, float(env.gamma));
                environment_extras_to_map(&env, &mut put);
                put(k::SHADOW_ENABLED, toml::Value::Boolean(env.shadows));
                put(
                    k::SHADOW_RESOLUTION,
                    toml::Value::Integer(i64::from(env.shadow_resolution)),
                );
                put(k::SHADOW_SOFTNESS, float(env.shadow_softness));
                put(k::SHADOW_DISTANCE, float(env.shadow_distance));
                shadow_budget_to_map(&env, &mut put);
                Some(toml::Value::Table(map))
            }),
        },
    );
}

/// The grading, eye-adaptation and glass keys read back.
fn environment_extras_to_map(env: &Environment, put: &mut impl FnMut(&str, toml::Value)) {
    let float = |v: f32| toml::Value::Float(f64::from(v));
    let [r, g, b] = env.white_balance;
    put(
        k::WHITE_BALANCE,
        toml::Value::Array(vec![float(r), float(g), float(b)]),
    );
    put(k::HUE_DEGREES, float(env.hue));
    let auto = env.auto_exposure;
    put(k::AUTO_EXPOSURE_ENABLED, toml::Value::Boolean(auto.enabled));
    put(k::AUTO_EXPOSURE_SPEED, float(auto.speed));
    put(k::AUTO_EXPOSURE_MIN, float(auto.min));
    put(k::AUTO_EXPOSURE_MAX, float(auto.max));
    put(k::AUTO_EXPOSURE_KEY, float(auto.key));
    let glass = env.transmission;
    let quality = match glass.blur_quality {
        BlurQuality::Low => words::LOW,
        BlurQuality::Medium => words::MEDIUM,
        BlurQuality::High => words::HIGH,
    };
    put(k::TRANSMISSION_ENABLED, toml::Value::Boolean(glass.enabled));
    put(
        k::TRANSMISSION_BLUR_QUALITY,
        toml::Value::String(quality.into()),
    );
    put(
        k::TRANSMISSION_STEPS,
        toml::Value::Integer(i64::from(glass.steps)),
    );
}

/// The shadow knobs, sky light and renderer limits read back.
fn shadow_budget_to_map(env: &Environment, put: &mut impl FnMut(&str, toml::Value)) {
    let float = |v: f32| toml::Value::Float(f64::from(v));
    let whole = |v: u32| toml::Value::Integer(i64::from(v));
    put(k::SHADOW_CASCADES, whole(env.shadow_cascades));
    put(
        k::SHADOW_FIRST_CASCADE_DISTANCE,
        float(env.shadow_first_cascade_distance),
    );
    put(k::SHADOW_BIAS, float(env.shadow_bias));
    put(
        k::SHADOW_CONSTANT_BIAS,
        toml::Value::Integer(i64::from(env.shadow_constant_bias)),
    );
    put(k::SHADOW_SLOPE_BIAS, float(env.shadow_slope_bias));
    put(k::SHADOW_VIEWS, whole(env.shadow_views));
    put(k::SKY_LIGHT, toml::Value::String(env.sky_light.clone()));
    put(
        k::SKY_LIGHT_INTENSITY,
        float(env.sky_light_intensity.unwrap_or(-1.0)),
    );
    put(k::PROBE_CAPTURE_SIZE_PIXELS, whole(env.probe_capture_size));
    put(
        k::CLUSTER_GRID,
        toml::Value::Array(
            env.cluster_grid
                .iter()
                .map(|n| toml::Value::Float(f64::from(*n)))
                .collect(),
        ),
    );
    put(k::CLUSTER_MAX_LIGHTS, whole(env.cluster_max_lights));
}

/// The kiss3d light one resolved `light3d` becomes.
#[cfg(feature = "window")]
fn as_kiss3d(light: &LitLight3d) -> kiss3d::light::Light {
    use kiss3d::light::{Light, LightType};
    let light_type = match light.kind {
        LightKind3d::Point => LightType::Point {
            attenuation_radius: light.radius,
        },
        LightKind3d::Directional => LightType::Directional(light.direction),
        LightKind3d::Spot => LightType::Spot {
            inner_cone_angle: light.inner.to_radians(),
            outer_cone_angle: light.outer.to_radians(),
            attenuation_radius: light.radius,
        },
    };
    let [r, g, b] = light.color;
    Light {
        light_type,
        color: kiss3d::color::Color::new(r, g, b, 1.0),
        intensity: light.intensity,
        radius: light.source_radius,
        enabled: light.enabled,
        casts_shadows: light.shadows,
        layers: light.layers,
    }
}

/// One kiss3d node per authored light, kept between frames the way the mesh
/// slots are: a light rebuilt every frame would restart its shadow map.
#[cfg(feature = "window")]
#[derive(Default)]
pub(crate) struct LightSlots {
    nodes: Vec<kiss3d::scene::SceneNode3d>,
    /// The engine's own key light, dropped the first frame a scene places one.
    sun: Option<kiss3d::scene::SceneNode3d>,
}

#[cfg(feature = "window")]
impl LightSlots {
    /// Remember the backend's default sun, so the first authored light can
    /// retire it and a scene that removes its lights gets it back.
    pub(crate) fn adopt_sun(&mut self, sun: kiss3d::scene::SceneNode3d) {
        self.sun = Some(sun);
    }

    /// Push this frame's lights onto the scene. One node per light, reused in
    /// order, with the tail removed when a light goes.
    pub(crate) fn sync(&mut self, app: &balaur_core::App, scene: &mut kiss3d::scene::SceneNode3d) {
        let resolved = {
            let world = app.engine.world();
            lights(&world, app.engine.root())
        };
        if let Some(sun) = &mut self.sun {
            sun.set_visible(resolved.is_empty());
        }
        while self.nodes.len() > resolved.len() {
            if let Some(mut extra) = self.nodes.pop() {
                extra.remove();
            }
        }
        while self.nodes.len() < resolved.len() {
            self.nodes
                .push(scene.add_light(kiss3d::light::Light::default()));
        }
        for (node, light) in self.nodes.iter_mut().zip(resolved.iter()) {
            node.set_light(Some(as_kiss3d(light)));
            // A directional light reads its direction off the `LightType`, so
            // only the position has to reach the node.
            node.set_position(light.position);
            node.set_visible(light.enabled);
        }
    }
}

/// Push the scene's `environment` onto the window: sky, ambient, fog, the HDR
/// film and the shadow budget. A scene with none gets [`Environment::none`].
#[cfg(feature = "window")]
pub(crate) fn sync_environment(
    app: &balaur_core::App,
    window: &mut kiss3d::window::Window,
    applied: &mut Option<Environment>,
) {
    let env = {
        let world = app.engine.world();
        environment_or_none(&world, app.engine.root())
    };
    if applied.as_ref() == Some(&env) {
        return;
    }
    let [r, g, b, _] = env.ambient;
    window.set_ambient_color(kiss3d::color::Color::new(r, g, b, 1.0));
    // The fork's ambient is one scalar over the colour, and the colour already
    // carries the brightness a designer set.
    window.set_ambient(1.0);
    let [fr, fg, fb, fa] = env.fog_color;
    let fog_color = kiss3d::color::Color::new(fr, fg, fb, fa);
    let mode = match env.fog {
        FogKind::Off => kiss3d::light::FogMode::Off,
        FogKind::Linear => kiss3d::light::FogMode::Linear {
            start: env.fog_start,
            end: env.fog_end,
        },
        FogKind::Exponential => kiss3d::light::FogMode::Exponential {
            density: env.fog_density,
        },
        FogKind::ExponentialSquared => kiss3d::light::FogMode::ExponentialSquared {
            density: env.fog_density,
        },
    };
    window.set_fog(kiss3d::light::Fog {
        color: fog_color,
        mode,
        height_falloff: env.fog_height_falloff,
    });
    window.set_shadows_enabled(env.shadows);
    window.set_shadow_resolution(env.shadow_resolution);
    window.set_shadow_softness(env.shadow_softness);
    window.set_shadow_distance(env.shadow_distance);
    window.set_shadow_cascades(env.shadow_cascades);
    window.set_shadow_first_cascade_distance(env.shadow_first_cascade_distance);
    window.set_shadow_depth_bias(env.shadow_bias);
    window.set_shadow_raster_bias(env.shadow_constant_bias, env.shadow_slope_bias);
    window.set_max_shadow_views(env.shadow_views);
    window.set_reflection_probe_size(env.probe_capture_size);
    window.set_cluster_grid(env.cluster_grid);
    window.set_max_lights_per_cluster(env.cluster_max_lights);
    let hdr = window.hdr_settings_mut();
    hdr.exposure = env.exposure;
    hdr.tonemap = match env.tonemap {
        Tonemap::None => kiss3d::post_processing::Tonemap::None,
        Tonemap::Aces => kiss3d::post_processing::Tonemap::Aces,
        Tonemap::Reinhard => kiss3d::post_processing::Tonemap::Reinhard,
        Tonemap::AgX => kiss3d::post_processing::Tonemap::AgX,
        Tonemap::Neutral => kiss3d::post_processing::Tonemap::Neutral,
        Tonemap::TonyMcMapface => kiss3d::post_processing::Tonemap::TonyMcMapface,
    };
    hdr.color_grading.saturation = env.saturation;
    hdr.color_grading.contrast = env.contrast;
    hdr.color_grading.gamma = env.gamma;
    hdr.color_grading.white_balance = env.white_balance;
    hdr.color_grading.hue = env.hue.to_radians();
    hdr.auto_exposure = env.auto_exposure.enabled;
    hdr.auto_exposure_speed = env.auto_exposure.speed;
    hdr.auto_exposure_min = env.auto_exposure.min;
    hdr.auto_exposure_max = env.auto_exposure.max;
    hdr.auto_exposure_key = env.auto_exposure.key;
    sync_transmission(window, &env, applied.as_ref());
    sync_sky(app, window, &env, applied.as_ref());
    *applied = Some(env);
}

/// Glass on or off, and its settings once they move off kiss3d's own: asking
/// for the settings builds the snapshot target, which a scene with no glass
/// should not pay for.
#[cfg(feature = "window")]
fn sync_transmission(
    window: &mut kiss3d::window::Window,
    env: &Environment,
    was: Option<&Environment>,
) {
    window.set_transmission_enabled(env.transmission.enabled);
    let moved = was.map_or(Transmission::default(), |old| old.transmission) != env.transmission;
    if !moved {
        return;
    }
    let settings = window.transmission_settings_mut();
    settings.blur_quality = match env.transmission.blur_quality {
        BlurQuality::Low => kiss3d::renderer::TransmissionBlurQuality::Low,
        BlurQuality::Medium => kiss3d::renderer::TransmissionBlurQuality::Medium,
        BlurQuality::High => kiss3d::renderer::TransmissionBlurQuality::High,
    };
    settings.steps = env.transmission.steps;
}

/// The sky file the window should hold: none for a sky switched off.
///
/// Unbound rather than dimmed: a bound sky at zero still replaces the ambient
/// term in every shader, so the scene would go black instead of taking
/// `ambient_color`.
#[cfg(feature = "window")]
fn bound_sky(env: &Environment) -> Option<&str> {
    (env.show_sky && !env.sky.is_empty()).then_some(env.sky.as_str())
}

/// The image that lights the scene apart from the drawn sky: none for a sky
/// switched off, which takes its light away with it.
#[cfg(feature = "window")]
fn bound_sky_light(env: &Environment) -> Option<&str> {
    (env.show_sky && !env.sky_light.is_empty()).then_some(env.sky_light.as_str())
}

/// An equirectangular image read through the project, the way a texture
/// loads: a packed game carries it inside the pack.
#[cfg(feature = "window")]
fn sky_image(app: &balaur_core::App, path: &str) -> Option<image::DynamicImage> {
    let files = app.engine.resource::<balaur_core::project::ProjectFiles>();
    let read = files.borrow().read(path);
    match read.map(|bytes| image::load_from_memory(&bytes)) {
        Ok(Ok(image)) => Some(image),
        Ok(Err(why)) => {
            tracing::warn!("environment sky '{path}' is not an image: {why}");
            None
        }
        Err(why) => {
            tracing::warn!("environment sky '{path}': {why:#}");
            None
        }
    }
}

/// Load the sky and its light when the file bound changed, and re-aim them
/// whenever the orientation did. Decoding an `.hdr` is megabytes of work, so
/// it happens when the sky changes or comes back rather than once per frame.
#[cfg(feature = "window")]
fn sync_sky(
    app: &balaur_core::App,
    window: &mut kiss3d::window::Window,
    env: &Environment,
    was: Option<&Environment>,
) {
    let wanted = bound_sky(env);
    if was.is_none_or(|old| bound_sky(old) != wanted) {
        match wanted.and_then(|path| sky_image(app, path)) {
            Some(image) => window.set_skybox_image(&image),
            None => window.clear_skybox(),
        }
    }
    let light = bound_sky_light(env);
    if was.is_none_or(|old| bound_sky_light(old) != light) {
        let image = light.and_then(|path| sky_image(app, path));
        window.set_sky_lighting_image(image.as_ref());
    }
    // The light image turns with the sky's rotation, drawn sky or none.
    window.set_skybox_orientation(env.sky_rotation.to_radians(), env.sky_intensity);
    window.set_sky_lighting_intensity(env.sky_light_intensity);
}

#[cfg(test)]
mod environment_tests {
    use super::{Environment, Tonemap, environment_or_none};

    #[test]
    fn a_scene_with_no_environment_is_not_tonemapped() {
        let app = balaur_core::App::new(balaur_core::AppConfig::bare(".")).unwrap();
        let root = app.engine.root();
        assert_eq!(
            environment_or_none(&app.engine.world(), root).tonemap,
            Tonemap::None
        );
    }

    #[test]
    fn an_environment_node_keeps_its_own_defaults() {
        assert_eq!(Environment::default().tonemap, Tonemap::Neutral);
        let none = Environment::none();
        assert_eq!(
            Environment {
                tonemap: Tonemap::Neutral,
                ..none
            },
            Environment::default(),
            "no environment differs from a default one by the tonemap alone"
        );
    }
}

#[cfg(all(test, feature = "window"))]
mod tests {
    use super::{Environment, bound_sky};

    #[test]
    fn a_sky_switched_off_is_unbound_so_the_ambient_colour_lights() {
        let lit = Environment {
            sky: "sky.hdr".into(),
            ..Environment::default()
        };
        assert_eq!(bound_sky(&lit), Some("sky.hdr"));
        let off = Environment {
            show_sky: false,
            ..lit
        };
        assert_eq!(bound_sky(&off), None);
        assert_eq!(bound_sky(&Environment::default()), None, "no file, no sky");
    }
}
