//! `light3d`: a light a scene places, and the `environment` it sits in.
//!
//! Both resolve headless. [`lights`] turns the scene tree into the world-space
//! list a backend uploads, and [`environment`] reads the scene's atmosphere,
//! so a test asserts on either with no GPU. A scene with no `light3d` keeps
//! the backend's own sun, which is what lets every example draw unchanged.

use balaur_core::GlobalTransform;
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{Component, Enum};
use balaur_core::hecs::{Entity, World};
use balaur_plugin::Registry;
use glamx::Vec3;

use crate::vocabulary::{keys as k, words};
use balaur_core::layers::Layer;

/// Which way a `light3d` throws light.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum LightKind3d {
    /// Parallel rays across the whole scene, aimed by the node's rotation.
    Directional,
    /// Radiates from the node, fading to nothing at `radius`.
    Point,
    /// A cone from the node, aimed by its rotation.
    Spot,
}

/// A 3D light placed and aimed by the node. `kind` is `directional`, `point` or `spot`; the first `light3d` in a scene retires the engine's default key light.
// The node's global pose places and aims it; `range` is in world units and
// does not follow the node's scale. [`lights`] holds every number to what a
// light can be.
#[derive(Component)]
#[component(name = "light3d")]
pub struct Light3d {
    /// A point light fades to nothing at `range`, a directional one lights the whole scene, a spot one throws a cone the node aims
    #[property(default = LightKind3d::Directional)]
    pub kind: LightKind3d,
    /// Light colour, as channel floats or #rrggbb / #rrggbbaa
    #[property(color, default = [1.0, 1.0, 1.0, 1.0])]
    pub color: [f32; 4],
    /// Brightness multiplier; over 1 blows past white
    #[property(default = 3.0, min = 0.0)]
    pub intensity: f32,
    /// How far a point or spot light reaches, in world units
    #[property(default = 30.0, min = 0.0)]
    pub range: f32,
    /// Half-angle of a spot light's full-brightness cone, in degrees
    #[property(default = 20.0, min = 0.0, max = 179.0, group = "cone")]
    pub inner_angle_degrees: f32,
    /// Half-angle a spot light fades to nothing at, in degrees
    #[property(default = 35.0, min = 0.0, max = 179.0, group = "cone")]
    pub outer_angle_degrees: f32,
    /// Whether this light casts shadows from the nodes that say they cast
    #[property(default = true, group = "shadows")]
    pub shadow_enabled: bool,
    /// Radius of the sphere the light shines from, in world units, which softens its shadows. Read by the path tracer only; the rasterizer's edge is `environment.shadow_softness`
    #[property(min = 0.0, group = "shadows")]
    pub source_radius: f32,
    /// The light layers this light reaches: a node whose `light_layers` share one; empty is every layer
    #[property(flags, group = "layers")]
    pub light_layers: Vec<Layer>,
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
    let found: Vec<Entity> = world
        .query::<(Entity, &Light3d)>()
        .iter()
        .map(|(entity, _)| entity)
        .collect();
    for entity in balaur_core::scene::tree_rank(world, root).sorted(found) {
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
            radius: light.range.max(0.0),
            inner: light.inner_angle_degrees.clamp(0.0, 179.0),
            outer: light.outer_angle_degrees.clamp(0.0, 179.0),
            shadows: light.shadow_enabled,
            layers: Layer::bits(&light.light_layers, true),
            source_radius: light.source_radius.max(0.0),
            kind: light.kind,
            enabled: visible,
        });
    }
    out
}

/// The `light3d` component. The node's position places it and its rotation
/// aims it; `visible = false` switches it off.
pub(crate) fn register_light3d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        Light3d::NAME,
        ComponentDef {
            tags: &[words::PERSPECTIVE, "render"],
            ..ComponentDef::of::<Light3d>(None)
        },
    );
}

/// How fog thickens with distance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum FogKind {
    #[option(name = "none")]
    Off,
    /// Ramps between `start` and `end`, in view-space distance.
    Linear,
    /// `1 - exp(-density * distance)`.
    Exponential,
    /// `1 - exp(-(density * distance)^2)`; a sharper onset.
    ExponentialSquared,
}

/// Which curve the HDR film is mapped through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Tonemap {
    None,
    Aces,
    Reinhard,
    #[option(name = "agx")]
    AgX,
    Neutral,
    #[option(name = "tony_mcmapface")]
    TonyMcMapface,
}

/// How glass blurs what it refracts: kiss3d's `TransmissionBlurQuality`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
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

/// The scene's atmosphere: `sky`, `ambient_color`, `fog_mode`, `exposure`, `tonemap`, colour grading and how shadows fall. The last `current` one wins; per-view effects stay on `camera.post`, and what the device can afford is `[render]`.
// Everything about a scene's atmosphere that is scene-wide rather than per
// view; `camera.post` keeps the per-view passes. [`LitEnvironment`] is it
// held to kiss3d's ranges.
#[derive(Component)]
#[component(name = "environment")]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is its own property, not a state machine"
)]
pub struct Environment {
    /// Whether this is the environment the scene draws under; the last current one in tree order wins
    #[property(default = true)]
    pub current: bool,
    /// Equirectangular image, project-relative: .hdr, .exr or .png. It draws behind the scene and lights it. Empty is no sky
    pub sky: String,
    /// Light every surface gets whatever the lights do, a 2D one's before any `light2d` as a 3D one's
    #[property(color, default = [0.125, 0.14, 0.157, 1.0])]
    pub ambient_color: [f32; 4],
    /// The curve the HDR film is mapped through. A scene with no `environment` node draws untonemapped
    #[property(default = Tonemap::Neutral)]
    pub tonemap: Tonemap,
    /// Linear multiplier before the tonemap
    #[property(default = 1.0, min = 0.0)]
    pub exposure: f32,
    /// False takes the sky away: it stops drawing and stops lighting, and `ambient_color` lights the scene as it does with no sky
    #[property(default = true, group = "sky")]
    pub sky_enabled: bool,
    /// Brightness of the sky, and of the light it casts
    #[property(default = 1.0, min = 0.0, group = "sky")]
    pub sky_intensity: f32,
    /// Turn of the sky about y, in degrees
    #[property(group = "sky")]
    pub sky_rotation_degrees: f32,
    /// Equirectangular image, project-relative, that lights the scene in place of `sky`, which still draws. Empty lights with `sky`; it lights even with no `sky` drawn, and `sky_enabled = false` takes it away too
    #[property(group = "sky")]
    pub sky_light: String,
    /// Brightness of the light the sky casts, apart from how bright it draws; below zero follows `sky_intensity`
    #[property(default = -1.0, min = -1.0, group = "sky")]
    pub sky_light_intensity: f32,
    /// How fog thickens with distance
    #[property(default = FogKind::Off, group = "fog")]
    pub fog_mode: FogKind,
    /// What distance fades toward
    #[property(color, default = [0.624, 0.706, 0.784, 1.0], group = "fog")]
    pub fog_color: [f32; 4],
    /// Thickness, for exponential fog
    #[property(default = 0.02, min = 0.0, group = "fog")]
    pub fog_density: f32,
    /// Where linear fog begins, in world units
    #[property(default = 10.0, min = 0.0, group = "fog")]
    pub fog_start: f32,
    /// Where linear fog is total, in world units
    #[property(default = 80.0, min = 0.0, group = "fog")]
    pub fog_end: f32,
    /// How fast fog thins with height; zero fills the scene evenly
    #[property(min = 0.0, group = "fog")]
    pub fog_height_falloff: f32,
    /// Contrast around mid grey
    #[property(default = 1.0, min = 0.0, group = "grading")]
    pub contrast: f32,
    /// Colour multiplier around luminance; zero is grey
    #[property(default = 1.0, min = 0.0, group = "grading")]
    pub saturation: f32,
    /// Turns every hue about the grey axis before the tonemap, in degrees
    #[property(group = "grading")]
    pub hue_degrees: f32,
    /// Per-channel linear gain before the tonemap; [1, 1, 1] leaves colour alone
    #[property(default = [1.0, 1.0, 1.0], group = "grading")]
    pub white_balance: [f32; 3],
    /// Gamma applied in linear space
    #[property(default = 1.0, min = 0.01, group = "grading")]
    pub gamma: f32,
    /// Measure the frame's average brightness and adapt the exposure to it, replacing `exposure`. It adapts on the wall clock, so two captures of one fixed-step frame can differ
    #[property(group = "auto exposure")]
    pub auto_exposure_enabled: bool,
    /// The mid grey auto-exposure maps the frame's average brightness to
    #[property(default = 0.18, min = 0.0, group = "auto exposure")]
    pub auto_exposure_key: f32,
    /// The lowest exposure auto-exposure settles at, for the brightest scenes
    #[property(default = 0.05, min = 0.0, group = "auto exposure")]
    pub auto_exposure_min: f32,
    /// The highest exposure auto-exposure settles at, for the darkest scenes
    #[property(default = 8.0, min = 0.0, group = "auto exposure")]
    pub auto_exposure_max: f32,
    /// How fast auto-exposure follows the scene, per second
    #[property(default = 3.0, min = 0.0, group = "auto exposure")]
    pub auto_exposure_speed: f32,
    /// Whether any light casts shadows at all
    #[property(default = true, group = "shadows")]
    pub shadow_enabled: bool,
    /// How far along the view a directional light's cascades reach, in world units; the camera's `far` caps it
    #[property(default = 60.0, min = 0.0, group = "shadows")]
    pub shadow_distance: f32,
    /// How far a shadow's edge is blurred
    #[property(default = 1.0, min = 0.0, group = "shadows")]
    pub shadow_softness: f32,
    /// How many cascades a directional light splits its shadow range into; each takes one of `shadow_views`
    #[property(default = 4, min = 1, max = 4, group = "shadows")]
    pub shadow_cascades: i64,
    /// How far along the view the first, sharpest cascade reaches, in world units
    #[property(default = 12.0, min = 0.01, group = "shadows")]
    pub shadow_first_cascade_distance: f32,
    /// Depth the lighting pass allows before it calls a point shadowed. Raise it to cure acne; lower it to keep contact shadows attached. Not yet honoured on a node with a shader material
    #[property(default = 0.0012, min = 0.0, group = "shadows")]
    pub shadow_bias: f32,
    /// How far the shadow pass pushes a caster back per unit of its depth slope, so a surface seen edge-on by the light does not shadow itself
    #[property(default = 1.75, group = "shadows")]
    pub shadow_slope_bias: f32,
    /// Depth-buffer units the shadow pass pushes every caster back by
    #[property(default = 1, group = "shadows")]
    pub shadow_constant_bias: i32,
    /// Whether glass refracts what is behind it. Off draws a material with `transmission` as plain opaque PBR and skips the passes
    #[property(default = true, group = "transmission")]
    pub transmission_enabled: bool,
    /// How smoothly rough glass blurs what it refracts; low is cheapest and can look blocky
    #[property(default = BlurQuality::High, group = "transmission")]
    pub transmission_blur_quality: BlurQuality,
}

/// An [`Environment`] held to kiss3d's ranges, with the device budget
/// `[render]` sets: what a backend draws under.
#[derive(Clone, Debug, PartialEq)]
pub struct LitEnvironment {
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

impl LitEnvironment {
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

impl Default for LitEnvironment {
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

/// The environment a scene draws under: its current `environment` node, or
/// [`LitEnvironment::none`] when it has none.
#[must_use]
pub fn environment_or_none(world: &World, root: Entity) -> LitEnvironment {
    environment(world, root).unwrap_or_else(LitEnvironment::none)
}

/// The environment a scene draws under: the last `current` one in tree order,
/// so a level can carry two and switch. `None` when no node has one, which is
/// what leaves the backend's own defaults alone.
pub fn environment(world: &World, root: Entity) -> Option<LitEnvironment> {
    let current: Vec<Entity> = world
        .query::<(Entity, &Environment)>()
        .iter()
        .filter(|(_, env)| env.current)
        .map(|(entity, _)| entity)
        .collect();
    let found = *balaur_core::scene::tree_rank(world, root)
        .sorted(current)
        .last()?;
    let env = world.get::<&Environment>(found).ok()?;
    Some(LitEnvironment::of(&env))
}

impl LitEnvironment {
    /// `env` held to kiss3d's ranges, at the device budget's defaults.
    #[must_use]
    pub fn of(env: &Environment) -> Self {
        let base = Self::default();
        Self {
            current: env.current,
            sky: env.sky.clone(),
            sky_intensity: env.sky_intensity.max(0.0),
            sky_rotation: env.sky_rotation_degrees,
            show_sky: env.sky_enabled,
            ambient: env.ambient_color,
            fog: env.fog_mode,
            fog_color: env.fog_color,
            fog_density: env.fog_density.max(0.0),
            fog_start: env.fog_start,
            fog_end: env.fog_end,
            fog_height_falloff: env.fog_height_falloff.max(0.0),
            exposure: env.exposure.max(0.0),
            tonemap: env.tonemap,
            saturation: env.saturation.max(0.0),
            contrast: env.contrast.max(0.0),
            gamma: env.gamma.max(0.01),
            white_balance: env.white_balance.map(|gain| gain.max(0.0)),
            hue: env.hue_degrees,
            auto_exposure: AutoExposure {
                enabled: env.auto_exposure_enabled,
                speed: env.auto_exposure_speed.max(0.0),
                min: env.auto_exposure_min.max(0.0),
                max: env.auto_exposure_max.max(0.0),
                key: env.auto_exposure_key.max(0.0),
            },
            transmission: Transmission {
                enabled: env.transmission_enabled,
                blur_quality: env.transmission_blur_quality,
                steps: base.transmission.steps,
            },
            shadows: env.shadow_enabled,
            shadow_resolution: base.shadow_resolution,
            shadow_softness: env.shadow_softness.max(0.0),
            shadow_distance: env.shadow_distance.max(0.0),
            shadow_cascades: u32::try_from(env.shadow_cascades.clamp(1, 4)).unwrap_or(4),
            shadow_first_cascade_distance: env.shadow_first_cascade_distance.max(0.01),
            shadow_bias: env.shadow_bias.max(0.0),
            shadow_constant_bias: env.shadow_constant_bias,
            shadow_slope_bias: env.shadow_slope_bias,
            shadow_views: base.shadow_views,
            sky_light: env.sky_light.clone(),
            sky_light_intensity: Some(env.sky_light_intensity).filter(|v| *v >= 0.0),
            probe_capture_size: base.probe_capture_size,
            cluster_grid: base.cluster_grid,
            cluster_max_lights: base.cluster_max_lights,
        }
    }
}

/// The `environment` component: sky, ambient, fog, exposure, tonemap, grading
/// and how shadows fall, all of which belong to the scene rather than a view.
pub(crate) fn register_environment_component(reg: &mut Registry<'_>) {
    reg.register_component(
        Environment::NAME,
        ComponentDef {
            // Both dimensions': a 2D scene's ambient is its environment's too.
            tags: &["render"],
            ..ComponentDef::of::<Environment>(None)
        },
    );
}

/// `[render]`: what the device can afford, which a platform answers for
/// itself through `[override.<tag>.render]`, rather than what a scene's
/// atmosphere is.
pub(crate) fn budget_settings() -> String {
    balaur_core::ComponentDef::schema(&[
        (
            k::SHADOW_RESOLUTION,
            r#"{ type = "int", default = 2048, min = 256, order = 1, help = "Side of the shadow map, in texels." }"#,
        ),
        (
            k::SHADOW_VIEWS,
            r#"{ type = "int", default = 16, min = 1, max = 64, order = 2, help = "Shadow-map layers every casting light shares: a spot light takes one, a directional one a layer per cascade and a point one six. A light past the budget lights without a shadow. Each layer costs `shadow_resolution` squared times eight bytes." }"#,
        ),
        (
            k::TRANSMISSION_STEPS,
            r#"{ type = "int", default = 1, min = 1, order = 3, help = "How many layers of glass seen through glass are drawn; each costs another snapshot of the scene and another glass pass." }"#,
        ),
        (
            k::CLUSTER_GRID,
            r#"{ type = "vec3", default = [16.0, 9.0, 24.0], order = 4, help = "Clusters the many-light pass cuts the view into, across x, across y and along depth, each 1 to 128. Finer culls tighter and costs memory. Not drawn on WebGL2, which has no compute." }"#,
        ),
        (
            k::CLUSTER_MAX_LIGHTS,
            r#"{ type = "int", default = 256, min = 1, order = 5, help = "Lights one cluster records; past it a dense cluster drops lights. The renderer lowers it to fit the device's largest storage buffer." }"#,
        ),
        (
            k::PROBE_CAPTURE_SIZE_PIXELS,
            r#"{ type = "int", default = 256, min = 2, max = 4096, order = 6, help = "Side of each cube face a `reflection_probe` captures, and the width of every probe map, which all probes share. A change clears every map and captures the probes again." }"#,
        ),
    ])
}

/// The device budget `[render]` sets, over whichever environment draws.
#[cfg(feature = "window")]
fn budget_from_settings(eng: &balaur_core::Engine, env: &mut LitEnvironment) {
    let setting = |key: &str| balaur_core::settings::get(eng, &format!("render/{key}"));
    let whole = |key: &str, fallback: u32| {
        setting(key)
            .as_ref()
            .and_then(toml::Value::as_integer)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(fallback)
    };
    env.shadow_resolution = whole(k::SHADOW_RESOLUTION, env.shadow_resolution).max(256);
    env.shadow_views = whole(k::SHADOW_VIEWS, env.shadow_views).clamp(1, 64);
    env.transmission.steps = whole(k::TRANSMISSION_STEPS, env.transmission.steps).max(1);
    env.probe_capture_size =
        whole(k::PROBE_CAPTURE_SIZE_PIXELS, env.probe_capture_size).clamp(2, 4096);
    env.cluster_max_lights = whole(k::CLUSTER_MAX_LIGHTS, env.cluster_max_lights).max(1);
    if let Some(grid) = setting(k::CLUSTER_GRID)
        .as_ref()
        .and_then(toml::Value::as_array)
    {
        for (slot, cell) in grid.iter().take(3).enumerate() {
            if let Some(n) = balaur_core::components::as_f64(cell) {
                env.cluster_grid[slot] = n.round().clamp(1.0, 128.0) as u32;
            }
        }
    }
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
/// film and the shadow budget. A scene with none gets [`LitEnvironment::none`].
#[cfg(feature = "window")]
pub(crate) fn sync_environment(
    app: &balaur_core::App,
    window: &mut kiss3d::window::Window,
    applied: &mut Option<LitEnvironment>,
) {
    let mut env = {
        let world = app.engine.world();
        environment_or_none(&world, app.engine.root())
    };
    budget_from_settings(&app.engine, &mut env);
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
    window.set_first_cascade_far_bound(env.shadow_first_cascade_distance);
    window.set_shadow_depth_bias(env.shadow_bias);
    window.set_shadow_raster_bias(env.shadow_constant_bias, env.shadow_slope_bias);
    window.set_shadow_atlas_layers(env.shadow_views);
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
    env: &LitEnvironment,
    was: Option<&LitEnvironment>,
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
fn bound_sky(env: &LitEnvironment) -> Option<&str> {
    (env.show_sky && !env.sky.is_empty()).then_some(env.sky.as_str())
}

/// The image that lights the scene apart from the drawn sky: none for a sky
/// switched off, which takes its light away with it.
#[cfg(feature = "window")]
fn bound_sky_light(env: &LitEnvironment) -> Option<&str> {
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
    env: &LitEnvironment,
    was: Option<&LitEnvironment>,
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
    use super::{Environment, LitEnvironment, Tonemap, environment_or_none};

    #[test]
    fn an_environment_at_its_defaults_draws_as_the_default_one() {
        assert_eq!(
            LitEnvironment::of(&Environment::default()),
            LitEnvironment::default()
        );
    }

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
        assert_eq!(LitEnvironment::default().tonemap, Tonemap::Neutral);
        let none = LitEnvironment::none();
        assert_eq!(
            LitEnvironment {
                tonemap: Tonemap::Neutral,
                ..none
            },
            LitEnvironment::default(),
            "no environment differs from a default one by the tonemap alone"
        );
    }
}

#[cfg(all(test, feature = "window"))]
mod tests {
    use super::{LitEnvironment, bound_sky};

    #[test]
    fn a_sky_switched_off_is_unbound_so_the_ambient_colour_lights() {
        let lit = LitEnvironment {
            sky: "sky.hdr".into(),
            ..LitEnvironment::default()
        };
        assert_eq!(bound_sky(&lit), Some("sky.hdr"));
        let off = LitEnvironment {
            show_sky: false,
            ..lit
        };
        assert_eq!(bound_sky(&off), None);
        assert_eq!(
            bound_sky(&LitEnvironment::default()),
            None,
            "no file, no sky"
        );
    }
}
