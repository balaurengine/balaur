//! The `camera` component: a scene declares its view and the view follows
//! the node carrying it.

use anyhow::anyhow;
use balaur_core::components::ComponentDef;
use balaur_core::{Engine, GlobalTransform};
use balaur_plugin::Registry;

use crate::shape::{keys as k, options, words};
use crate::{CameraConfig, CameraConfig2d, PostConfig, color_to_toml};

/// The smallest 2D zoom, in logical pixels per world unit. Mirrors the `min`
/// the `camera` schema below states, which `render.set_camera_2d` also takes.
/// A hundredth, not one: a pixel-scale level is thousands of units across,
/// and a camera that cannot show it whole is a camera that cannot frame it.
pub(crate) const MIN_ZOOM_2D: f32 = 0.01;

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
    /// Screen-space effects the frame resolves through. Unlike `ambient` this
    /// is not per-dimension: the effects run over the whole film, so the last
    /// current camera of either kind sets them.
    pub post: Post,
}

/// One pass on a camera's chain.
///
/// A name the engine knows switches its own effect on; anything else is a
/// `material` asset drawn over the whole frame. Where the engine's own passes
/// physically run is fixed by the pipeline -- `ssao` and `ssr` feed shading,
/// `bloom` rides the tonemap -- so what the order decides is the materials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PostPass {
    Bloom,
    Ssao,
    Ssr,
    Dof,
    /// Antialiasing over the finished picture, for a frame MSAA did not
    /// smooth: it reads edges rather than geometry, so it catches the ones a
    /// shader drew.
    Fxaa,
    /// Contrast-adaptive sharpening, which puts back the definition a
    /// smoothing pass took out.
    Sharpen,
    /// Where the film becomes a picture. A material before it works in linear
    /// light and is what blooms; one after it works on the finished frame.
    /// Implicit at the head of a list that does not name it.
    Tonemap,
    /// One of the finishing passes the engine ships as a post-process
    /// material: it draws where it is listed, like any other material.
    Finish(&'static str),
    /// A `material` asset, by id.
    Material(String),
}

impl PostPass {
    fn parse(name: &str) -> Self {
        match name {
            words::BLOOM => Self::Bloom,
            words::SSAO => Self::Ssao,
            words::SSR => Self::Ssr,
            words::DOF => Self::Dof,
            words::FXAA => Self::Fxaa,
            words::SHARPEN => Self::Sharpen,
            words::TONEMAP => Self::Tonemap,
            other => match words::FINISHES.iter().find(|finish| **finish == other) {
                Some(finish) => Self::Finish(finish),
                None => Self::Material(other.to_string()),
            },
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Bloom => words::BLOOM,
            Self::Ssao => words::SSAO,
            Self::Ssr => words::SSR,
            Self::Dof => words::DOF,
            Self::Fxaa => words::FXAA,
            Self::Sharpen => words::SHARPEN,
            Self::Tonemap => words::TONEMAP,
            Self::Finish(finish) => finish,
            Self::Material(id) => id,
        }
    }
}

/// What the screen-space occlusion pass measures with.
///
/// Every one is in world units or over them, so a scene's own scale decides
/// them: a radius that reads a room reads nothing in a courtyard, and a bias
/// that stops a surface occluding itself close up stops nothing far away.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Occlusion {
    /// How far from a point the pass looks for something occluding it.
    pub radius: f32,
    /// How far a sample must be in front of the surface to count. Too small
    /// and a surface at a glancing angle occludes itself into black.
    pub bias: f32,
    pub intensity: f32,
    /// The contrast the result is raised to.
    pub power: f32,
}

impl Default for Occlusion {
    fn default() -> Self {
        Self {
            radius: 0.5,
            bias: 0.025,
            intensity: 1.2,
            power: 1.5,
        }
    }
}

impl Occlusion {
    /// The values as bits, for a comparison that treats two equal floats as
    /// equal however they were computed.
    #[must_use]
    pub fn bits(&self) -> [u32; 4] {
        [
            self.radius.to_bits(),
            self.bias.to_bits(),
            self.intensity.to_bits(),
            self.power.to_bits(),
        ]
    }
}

impl Post {
    /// The plain numbers beside `post`, paired with the key each is spelled by.
    fn knobs(&self) -> [(&'static str, f32); 9] {
        [
            (k::VIGNETTE_AMOUNT, self.finish.vignette_amount),
            (k::VIGNETTE_ROUNDNESS, self.finish.vignette_roundness),
            (k::ABERRATION_AMOUNT, self.finish.aberration_amount),
            (k::GRAIN_AMOUNT, self.finish.grain_amount),
            (k::PIXELATE_SIZE, self.finish.pixelate_size),
            (k::SSAO_RADIUS, self.occlusion.radius),
            (k::SSAO_BIAS, self.occlusion.bias),
            (k::SSAO_INTENSITY, self.occlusion.intensity),
            (k::SSAO_POWER, self.occlusion.power),
        ]
    }
}

/// What the engine's finishing passes are turned by.
///
/// They are post-process materials, so their values would be a material's
/// `[params]` — but the engine ships them and a project names them by word,
/// so the knobs sit beside `post` on the camera, as bloom's already do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Finish {
    pub vignette_amount: f32,
    pub vignette_roundness: f32,
    pub aberration_amount: f32,
    pub grain_amount: f32,
    pub pixelate_size: f32,
}

impl Default for Finish {
    fn default() -> Self {
        Self {
            vignette_amount: 0.35,
            vignette_roundness: 1.0,
            aberration_amount: 0.004,
            grain_amount: 0.06,
            pixelate_size: 4.0,
        }
    }
}

impl Finish {
    /// The values as bits, for a comparison that has to treat two equal
    /// floats as equal however they were computed.
    #[must_use]
    pub fn bits(&self) -> [u32; 5] {
        [
            self.vignette_amount.to_bits(),
            self.vignette_roundness.to_bits(),
            self.aberration_amount.to_bits(),
            self.grain_amount.to_bits(),
            self.pixelate_size.to_bits(),
        ]
    }

    /// The knob each finishing pass reads, by the name its shader's `Params`
    /// gives it.
    #[must_use]
    pub fn params(&self) -> Vec<(String, crate::material::Param)> {
        use crate::material::Param::Float;
        vec![
            (k::VIGNETTE_AMOUNT.into(), Float(self.vignette_amount)),
            (k::VIGNETTE_ROUNDNESS.into(), Float(self.vignette_roundness)),
            (k::ABERRATION_AMOUNT.into(), Float(self.aberration_amount)),
            (k::GRAIN_AMOUNT.into(), Float(self.grain_amount)),
            (k::PIXELATE_SIZE.into(), Float(self.pixelate_size)),
        ]
    }
}

/// The `post` half of a `camera`: the chain in the order it was written, and
/// the two numbers bloom is unusable without.
#[derive(Clone, Debug, PartialEq)]
pub struct Post {
    pub passes: Vec<PostPass>,
    pub bloom_threshold: f32,
    pub bloom_intensity: f32,
    pub finish: Finish,
    pub occlusion: Occlusion,
}

impl Post {
    fn holds(&self, pass: &PostPass) -> bool {
        self.passes.contains(pass)
    }

    #[must_use]
    pub fn bloom(&self) -> bool {
        self.holds(&PostPass::Bloom)
    }

    #[must_use]
    pub fn ssao(&self) -> bool {
        self.holds(&PostPass::Ssao)
    }

    #[must_use]
    pub fn ssr(&self) -> bool {
        self.holds(&PostPass::Ssr)
    }

    #[must_use]
    pub fn dof(&self) -> bool {
        self.holds(&PostPass::Dof)
    }

    /// The materials each side of the tonemap, in the order they were listed.
    /// A list that never names `tonemap` has it at the head, so a plain list
    /// of materials is a chain over the finished frame.
    #[must_use]
    pub fn materials(&self) -> (Vec<String>, Vec<String>) {
        let (mut film, mut screen) = (Vec::new(), Vec::new());
        let mut tonemapped = !self.holds(&PostPass::Tonemap);
        for pass in &self.passes {
            match pass {
                PostPass::Tonemap => tonemapped = true,
                PostPass::Material(id) if tonemapped => screen.push(id.clone()),
                PostPass::Material(id) => film.push(id.clone()),
                // Every pass the engine draws as an effect of its own goes
                // in the list by name, so what the order says is what happens.
                PostPass::Finish(name) if tonemapped => screen.push((*name).into()),
                PostPass::Finish(name) => film.push((*name).into()),
                PostPass::Fxaa if tonemapped => screen.push(words::FXAA.into()),
                PostPass::Fxaa => film.push(words::FXAA.into()),
                PostPass::Sharpen if tonemapped => screen.push(words::SHARPEN.into()),
                PostPass::Sharpen => film.push(words::SHARPEN.into()),
                _ => {}
            }
        }
        (film, screen)
    }
}

impl Default for Post {
    fn default() -> Self {
        Self {
            passes: Vec::new(),
            bloom_threshold: 1.0,
            bloom_intensity: 0.6,
            finish: Finish::default(),
            occlusion: Occlusion::default(),
        }
    }
}

/// Runs in `SceneSync` after transform propagation, so the view follows the
/// node's global pose. Writes only when the pose actually differs: a still
/// camera never re-asserts itself, which leaves `changed` alone and keeps the
/// backend's interactive orbit/pan controls live between moves.
pub(crate) fn drive_camera_system(eng: &Engine, _dt: f32) {
    let (spatial, flat, post) = {
        let world = eng.world();
        let mut spatial = None;
        let mut flat = None;
        let mut post = None;
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
            post = Some(cam.post.clone());
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
        (spatial, flat, post)
    };
    if let Some(post) = post {
        drive_post(eng, &post);
    }
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

/// Mirror the current camera's effects into [`PostConfig`], raising
/// `changed` only when one actually differs: a backend rebuilds its
/// post chain when it sees that flag, and doing so every frame would
/// rebuild it every frame.
fn drive_post(eng: &Engine, post: &Post) {
    let config = eng.resource::<PostConfig>();
    let mut config = config.borrow_mut();
    let (film, screen) = post.materials();
    let same = config.bloom == post.bloom()
        && config.ssao == post.ssao()
        && config.ssr == post.ssr()
        && config.dof == post.dof()
        && config.film == film
        && config.screen == screen
        && config.bloom_threshold.to_bits() == post.bloom_threshold.to_bits()
        && config.bloom_intensity.to_bits() == post.bloom_intensity.to_bits()
        && config.finish.bits() == post.finish.bits()
        && config.occlusion.bits() == post.occlusion.bits();
    if same {
        return;
    }
    config.bloom = post.bloom();
    config.ssao = post.ssao();
    config.ssr = post.ssr();
    config.dof = post.dof();
    config.film = film;
    config.screen = screen;
    config.bloom_threshold = post.bloom_threshold;
    config.bloom_intensity = post.bloom_intensity;
    config.finish = post.finish;
    config.occlusion = post.occlusion;
    config.changed = true;
}

/// The authored camera a full property table describes.
fn camera_from_params(params: &toml::Value) -> anyhow::Result<Camera> {
    let kind = match balaur_core::components::prop_str(params, k::KIND) {
        words::PERSPECTIVE => CameraKind::Perspective,
        words::ORTHOGRAPHIC => CameraKind::Orthographic,
        other => return Err(anyhow!("unknown camera kind '{other}'")),
    };
    let num = |key: &str, default: f64| {
        params
            .get(key)
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(default) as f32
    };
    let la = |i: usize| {
        params
            .get("look_at")
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(0.0) as f32
    };
    let passes = params
        .get(k::POST)
        .and_then(toml::Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(toml::Value::as_str)
                .map(PostPass::parse)
                .collect()
        })
        .unwrap_or_default();
    let base = Finish::default();
    let occlusion = Occlusion::default();
    Ok(Camera {
        kind,
        ambient: color_from_params_named(params, "ambient"),
        post: Post {
            occlusion: Occlusion {
                radius: num(k::SSAO_RADIUS, f64::from(occlusion.radius)).max(1e-3),
                bias: num(k::SSAO_BIAS, f64::from(occlusion.bias)).max(0.0),
                intensity: num(k::SSAO_INTENSITY, f64::from(occlusion.intensity)).max(0.0),
                power: num(k::SSAO_POWER, f64::from(occlusion.power)).max(1e-3),
            },
            finish: Finish {
                vignette_amount: num(k::VIGNETTE_AMOUNT, f64::from(base.vignette_amount)),
                vignette_roundness: num(k::VIGNETTE_ROUNDNESS, f64::from(base.vignette_roundness)),
                aberration_amount: num(k::ABERRATION_AMOUNT, f64::from(base.aberration_amount)),
                grain_amount: num(k::GRAIN_AMOUNT, f64::from(base.grain_amount)),
                pixelate_size: num(k::PIXELATE_SIZE, f64::from(base.pixelate_size)),
            },
            passes,
            bloom_threshold: num(k::BLOOM_THRESHOLD, 1.0).max(0.0),
            bloom_intensity: num(k::BLOOM_INTENSITY, 0.6).max(0.0),
        },
        current: balaur_core::components::prop_bool(params, "current"),
        look_at: glamx::Vec3::new(la(0), la(1), la(2)),
        zoom: num(k::ZOOM, 60.0).max(MIN_ZOOM_2D),
    })
}

/// The `camera` component. Writes a [`Camera`] on the node;
/// [`drive_camera_system`] mirrors the current one into the camera resources.
pub(crate) fn register_camera_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "camera",
        ComponentDef {
            doc: "The camera the scene is drawn from. `kind` is `3d` or `2d`; `look_at` aims the 3D one, `zoom` scales the 2D one, the last `current` camera wins.",
            schema: ComponentDef::parse_schema(
                "camera",
                &balaur_core::components::ComponentDef::schema(&[
                    (k::KIND, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Which camera this node drives" }}"#, words::PERSPECTIVE, options(words::CAMERA_KINDS))),
                    (k::CURRENT, r#"{ type = "bool", default = true, description = "Whether this camera drives the view; the last current one wins" }"#),
                    (k::LOOK_AT, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "World point the 3D camera looks at" }"#),
                    (k::ZOOM, r#"{ type = "float", default = 60.0, min = 0.01, description = "2D zoom in logical pixels per world unit" }"#),
                    (k::AMBIENT, r#"{ type = "color", default = [0.0, 0.0, 0.0, 1.0], description = "Light every 2D surface gets before any `light2d`; only a `2d` camera's is read" }"#),
                    (k::POST, &format!(r#"{{ type = "strings", default = [], description = "The frame's passes, in order. {} name the engine's own -- `ssao`, `ssr` and `dof` are 3D only, and where each physically runs is fixed by the pipeline. Any other name is a `material` asset drawn over the whole frame, and those run in the order given. `tonemap` is where the film becomes a picture: a material before it works in linear light and is what blooms, one after it works on the finished frame, and a list that does not name it has it at the head" }}"#, words::POST_EFFECTS.join(", "))),
                    (k::BLOOM_THRESHOLD, r#"{ type = "float", default = 1.0, min = 0.0, description = "Brightness a pixel has to pass to bloom" }"#),
                    (k::BLOOM_INTENSITY, r#"{ type = "float", default = 0.6, min = 0.0, description = "How much of the bloom is added back over the frame" }"#),
                    (k::VIGNETTE_AMOUNT, r#"{ type = "float", default = 0.35, min = 0.0, max = 1.0, description = "How dark the corners go under the `vignette` pass" }"#),
                    (k::VIGNETTE_ROUNDNESS, r#"{ type = "float", default = 1.0, min = 0.0, max = 1.0, description = "1 darkens in a circle whatever shape the frame is; 0 follows the frame" }"#),
                    (k::ABERRATION_AMOUNT, r#"{ type = "float", default = 0.004, min = 0.0, description = "How far `aberration` slides red from blue at the frame's edge, as a fraction of it" }"#),
                    (k::GRAIN_AMOUNT, r#"{ type = "float", default = 0.06, min = 0.0, description = "How much the `grain` pass lightens and darkens a pixel" }"#),
                    (k::PIXELATE_SIZE, r#"{ type = "float", default = 4.0, min = 1.0, description = "The side of one block the `pixelate` pass reads the frame back in, in pixels" }"#),
                    (k::SSAO_RADIUS, r#"{ type = "float", default = 0.5, min = 0.001, description = "How far the `ssao` pass looks for something occluding a point, in world units. Scale it with the scene" }"#),
                    (k::SSAO_BIAS, r#"{ type = "float", default = 0.025, min = 0.0, description = "How far in front of a surface a sample must be to occlude it. Too small and a glancing surface occludes itself into black" }"#),
                    (k::SSAO_INTENSITY, r#"{ type = "float", default = 1.2, min = 0.0, description = "How strongly the `ssao` pass darkens" }"#),
                    (k::SSAO_POWER, r#"{ type = "float", default = 1.5, min = 0.001, description = "The contrast the occlusion is raised to" }"#),
                ]),
            ),
            tags: &[words::PERSPECTIVE, "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let camera = camera_from_params(params)?;
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
                    CameraKind::Perspective => words::PERSPECTIVE,
                    CameraKind::Orthographic => words::ORTHOGRAPHIC,
                };
                map.insert(k::KIND.into(), toml::Value::String(kind.into()));
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
                map.insert(k::ZOOM.into(), toml::Value::Float(f64::from(camera.zoom)));
                map.insert("ambient".into(), color_to_toml(camera.ambient));
                map.insert(
                    k::POST.into(),
                    toml::Value::Array(
                        camera
                            .post
                            .passes
                            .iter()
                            .map(|pass| toml::Value::String(pass.name().into()))
                            .collect(),
                    ),
                );
                map.insert(
                    k::BLOOM_THRESHOLD.into(),
                    toml::Value::Float(f64::from(camera.post.bloom_threshold)),
                );
                map.insert(
                    k::BLOOM_INTENSITY.into(),
                    toml::Value::Float(f64::from(camera.post.bloom_intensity)),
                );
                // The inspector reads this, so the knobs beside `post` are
                // here too rather than only in the table the scene handed over.
                for (key, value) in camera.post.knobs() {
                    map.insert(key.into(), toml::Value::Float(f64::from(value)));
                }
                Some(toml::Value::Table(map))
            }),
        },
    );
}
