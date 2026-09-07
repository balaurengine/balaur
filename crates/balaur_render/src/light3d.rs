//! `light3d`: a light a scene places, and the `environment` it sits in.
//!
//! Both resolve headless. [`lights`] turns the scene tree into the world-space
//! list a backend uploads, and [`environment`] reads the scene's atmosphere,
//! so a test asserts on either with no GPU. A scene with no `light3d` keeps
//! the backend's own sun, which is what lets every example draw unchanged.

use anyhow::{Result, anyhow};
use balaur_core::components::{ComponentDef, as_f64};
use balaur_core::hecs::{Entity, World};
use balaur_core::{Engine, GlobalTransform};
use balaur_plugin::Registry;
use glamx::Vec3;

use crate::shape::{keys as k, words};
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
            kind: light.kind,
            enabled: visible,
        });
    }
    out
}

fn light_schema() -> String {
    let kinds = crate::shape::options(words::LIGHT_KINDS_3D);
    let default = words::DIRECTIONAL;
    format!(
        r#"kind = {{ type = "enum", default = "{default}", options = [{kinds}], description = "A point light fades to nothing at `radius`, a directional one lights the whole scene, a spot one throws a cone the node aims" }}
color = {{ type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "Light colour, as channel floats or #rrggbb / #rrggbbaa" }}
intensity = {{ type = "float", default = 3.0, min = 0.0, description = "Brightness multiplier; over 1 blows past white" }}
radius = {{ type = "float", default = 30.0, min = 0.0, description = "How far a point or spot light reaches, in world units" }}
inner = {{ type = "float", default = 20.0, min = 0.0, max = 179.0, description = "Half-angle of a spot light's full-brightness cone, in degrees" }}
outer = {{ type = "float", default = 35.0, min = 0.0, max = 179.0, description = "Half-angle a spot light fades to nothing at, in degrees" }}
shadows = {{ type = "bool", default = true, description = "Whether this light casts shadows from the nodes that say they cast" }}
layers = {{ type = "int", default = -1, description = "Light-layer bitmask; a node is lit when its own `layers` share a bit with these. -1 is every layer" }}"#
    )
}

/// The `light3d` component. The node's position places it and its rotation
/// aims it; `visible = false` switches it off.
pub(crate) fn register_light3d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "light3d",
        ComponentDef {
            doc: "A 3D light: the node's position places it and its rotation aims it. A scene with no `light3d` keeps the engine's own key light, so nothing draws dark until a scene starts placing its own; the first one added retires it. Turn one off with the node's `visible`, not by deleting it.",
            schema: ComponentDef::parse_schema("light3d", &light_schema()),
            tags: &[words::PERSPECTIVE, "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let kind = match params
                    .get(k::KIND)
                    .and_then(|v| v.as_str())
                    .unwrap_or(words::DIRECTIONAL)
                {
                    words::POINT => LightKind3d::Point,
                    words::DIRECTIONAL => LightKind3d::Directional,
                    words::SPOT => LightKind3d::Spot,
                    other => return Err(anyhow!("unknown light3d kind '{other}'")),
                };
                let num = |key: &str, default: f64| {
                    params.get(key).and_then(as_f64).unwrap_or(default) as f32
                };
                let layers = params
                    .get(k::LAYERS)
                    .and_then(as_f64)
                    .map_or(u32::MAX, |v| v as i64 as u32);
                set_light(
                    eng,
                    entity,
                    Light3d {
                        kind,
                        color: color_from_params(params),
                        intensity: num(k::INTENSITY, 3.0).max(0.0),
                        radius: num(k::RADIUS, 30.0).max(0.0),
                        inner: num(k::INNER, 20.0),
                        outer: num(k::OUTER, 35.0),
                        shadows: params
                            .get(k::SHADOWS)
                            .and_then(toml::Value::as_bool)
                            .unwrap_or(true),
                        layers,
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
                map.insert(k::RADIUS.into(), toml::Value::Float(f64::from(light.radius)));
                map.insert(k::INNER.into(), toml::Value::Float(f64::from(light.inner)));
                map.insert(k::OUTER.into(), toml::Value::Float(f64::from(light.outer)));
                map.insert(k::SHADOWS.into(), toml::Value::Boolean(light.shadows));
                map.insert(
                    k::LAYERS.into(),
                    toml::Value::Integer(i64::from(light.layers.cast_signed())),
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
    /// False lights the scene from the sky without drawing it.
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
    pub shadows: bool,
    pub shadow_resolution: u32,
    pub shadow_softness: f32,
    pub shadow_distance: f32,
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
            shadows: true,
            shadow_resolution: 2048,
            shadow_softness: 1.0,
            shadow_distance: 60.0,
        }
    }
}

/// The environment a scene draws under: the last `current` one in tree order,
/// so a level can carry two and switch. `None` when no node has one, which is
/// what leaves the backend's own defaults alone.
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
    let fogs = crate::shape::options(words::FOG_KINDS);
    let tonemaps = crate::shape::options(words::TONEMAPS);
    format!(
        r#"current = {{ type = "bool", default = true, description = "Whether this is the environment the scene draws under; the last current one in tree order wins" }}
sky = {{ type = "string", default = "", description = "Equirectangular image, project-relative: .hdr, .exr or .png. It draws behind the scene and lights it. Empty is no sky" }}
sky_intensity = {{ type = "float", default = 1.0, min = 0.0, description = "Brightness of the sky, and of the light it casts" }}
sky_rotation = {{ type = "float", default = 0.0, description = "Turn of the sky about y, in degrees" }}
show_sky = {{ type = "bool", default = true, description = "False lights the scene from the sky without drawing it, leaving the background colour" }}
ambient = {{ type = "color", default = [0.125, 0.14, 0.157, 1.0], description = "Light every surface gets whatever the lights do" }}
fog = {{ type = "enum", default = "{none}", options = [{fogs}], description = "How fog thickens with distance" }}
fog_color = {{ type = "color", default = [0.624, 0.706, 0.784, 1.0], description = "What distance fades toward" }}
fog_density = {{ type = "float", default = 0.02, min = 0.0, description = "Thickness, for exponential fog" }}
fog_start = {{ type = "float", default = 10.0, min = 0.0, description = "Where linear fog begins, in world units" }}
fog_end = {{ type = "float", default = 80.0, min = 0.0, description = "Where linear fog is total, in world units" }}
fog_height_falloff = {{ type = "float", default = 0.0, min = 0.0, description = "How fast fog thins with height; zero fills the scene evenly" }}
exposure = {{ type = "float", default = 1.0, min = 0.0, description = "Linear multiplier before the tonemap" }}
tonemap = {{ type = "enum", default = "{neutral}", options = [{tonemaps}], description = "The curve the HDR film is mapped through" }}
saturation = {{ type = "float", default = 1.0, min = 0.0, description = "Colour multiplier around luminance; zero is grey" }}
contrast = {{ type = "float", default = 1.0, min = 0.0, description = "Contrast around mid grey" }}
gamma = {{ type = "float", default = 1.0, min = 0.01, description = "Gamma applied in linear space" }}
shadows = {{ type = "bool", default = true, description = "Whether any light casts shadows at all" }}
shadow_resolution = {{ type = "int", default = 2048, min = 256, description = "Side of the shadow map, in texels" }}
shadow_softness = {{ type = "float", default = 1.0, min = 0.0, description = "How far a shadow's edge is blurred" }}
shadow_distance = {{ type = "float", default = 60.0, min = 0.0, description = "How far from the camera shadows are drawn" }}"#,
        none = words::NONE,
        neutral = words::NEUTRAL,
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
        sky_rotation: num(k::SKY_ROTATION, base.sky_rotation),
        show_sky: flag(k::SHOW_SKY, true),
        ambient: crate::color_from_key(params, k::AMBIENT, base.ambient),
        fog: match params
            .get(k::FOG)
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
            other => return Err(anyhow!("unknown tonemap '{other}'")),
        },
        saturation: num(k::SATURATION, base.saturation).max(0.0),
        contrast: num(k::CONTRAST, base.contrast).max(0.0),
        gamma: num(k::GAMMA, base.gamma).max(0.01),
        shadows: flag(k::SHADOWS, true),
        shadow_resolution: num(k::SHADOW_RESOLUTION, 2048.0) as u32,
        shadow_softness: num(k::SHADOW_SOFTNESS, base.shadow_softness).max(0.0),
        shadow_distance: num(k::SHADOW_DISTANCE, base.shadow_distance).max(0.0),
    };
    Ok(next)
}

/// The `environment` component: sky, ambient, fog, exposure, tonemap, grading
/// and the shadow budget, all of which belong to the scene rather than a view.
pub(crate) fn register_environment_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "environment",
        ComponentDef {
            doc: "The scene's atmosphere: the sky it sits under and is lit by, the ambient light, fog, exposure, tonemap, colour grading and the shadow budget. The last `current` one in tree order wins, so a level can carry two and switch between them. Per-view effects stay on `camera.post`.",
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
                };
                let mut map = toml::map::Map::new();
                let mut put = |key: &str, value: toml::Value| {
                    map.insert(key.to_string(), value);
                };
                let float = |v: f32| toml::Value::Float(f64::from(v));
                put(k::CURRENT, toml::Value::Boolean(env.current));
                put(k::SKY, toml::Value::String(env.sky.clone()));
                put(k::SKY_INTENSITY, float(env.sky_intensity));
                put(k::SKY_ROTATION, float(env.sky_rotation));
                put(k::SHOW_SKY, toml::Value::Boolean(env.show_sky));
                put(k::AMBIENT, color_to_toml(env.ambient));
                put(k::FOG, toml::Value::String(fog.into()));
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
                put(k::SHADOWS, toml::Value::Boolean(env.shadows));
                put(
                    k::SHADOW_RESOLUTION,
                    toml::Value::Integer(i64::from(env.shadow_resolution)),
                );
                put(k::SHADOW_SOFTNESS, float(env.shadow_softness));
                put(k::SHADOW_DISTANCE, float(env.shadow_distance));
                Some(toml::Value::Table(map))
            }),
        },
    );
}

/// The kiss3d light one resolved `light3d` becomes.
#[cfg(feature = "kiss3d")]
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
        radius: 0.0,
        enabled: light.enabled,
        casts_shadows: light.shadows,
        layers: light.layers,
    }
}

/// One kiss3d node per authored light, kept between frames the way the mesh
/// slots are: a light rebuilt every frame would restart its shadow map.
#[cfg(feature = "kiss3d")]
#[derive(Default)]
pub(crate) struct LightSlots {
    nodes: Vec<kiss3d::scene::SceneNode3d>,
    /// The engine's own key light, dropped the first frame a scene places one.
    sun: Option<kiss3d::scene::SceneNode3d>,
}

#[cfg(feature = "kiss3d")]
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
/// film and the shadow budget. A scene with none leaves every one alone.
#[cfg(feature = "kiss3d")]
pub(crate) fn sync_environment(
    app: &balaur_core::App,
    window: &mut kiss3d::window::Window,
    applied: &mut Option<Environment>,
) {
    let found = {
        let world = app.engine.world();
        environment(&world, app.engine.root())
    };
    let Some(env) = found else { return };
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
    let hdr = window.hdr_settings_mut();
    hdr.exposure = env.exposure;
    hdr.tonemap = match env.tonemap {
        Tonemap::None => kiss3d::post_processing::Tonemap::None,
        Tonemap::Aces => kiss3d::post_processing::Tonemap::Aces,
        Tonemap::Reinhard => kiss3d::post_processing::Tonemap::Reinhard,
        Tonemap::AgX => kiss3d::post_processing::Tonemap::AgX,
        Tonemap::Neutral => kiss3d::post_processing::Tonemap::Neutral,
    };
    hdr.color_grading.saturation = env.saturation;
    hdr.color_grading.contrast = env.contrast;
    hdr.color_grading.gamma = env.gamma;
    sync_sky(app, window, &env, applied.as_ref());
    *applied = Some(env);
}

/// Load the sky when the file named changed, and re-aim it whenever the
/// orientation did. Decoding an `.hdr` is megabytes of work, so it happens
/// once per name rather than once per frame.
#[cfg(feature = "kiss3d")]
fn sync_sky(
    app: &balaur_core::App,
    window: &mut kiss3d::window::Window,
    env: &Environment,
    was: Option<&Environment>,
) {
    let changed = was.is_none_or(|old| old.sky != env.sky);
    if changed {
        if env.sky.is_empty() {
            window.clear_skybox();
        } else {
            // Bytes rather than a path, the way a texture loads: a packed
            // game carries its sky inside the pack.
            let files = app.engine.resource::<balaur_core::project::ProjectFiles>();
            let read = files.borrow().read(&env.sky);
            match read {
                Ok(bytes) => {
                    if !window.set_skybox_from_memory(&bytes) {
                        tracing::warn!("environment sky '{}' is not an image", env.sky);
                    }
                }
                Err(why) => tracing::warn!("environment sky '{}': {why:#}", env.sky),
            }
        }
    }
    if window.has_skybox() {
        let intensity = if env.show_sky { env.sky_intensity } else { 0.0 };
        window.set_skybox_orientation(env.sky_rotation.to_radians(), intensity);
    }
}
