//! The `post` half of a camera: the chain of passes a frame resolves through,
//! and the knobs beside it. `camera3d` and `camera2d` share every key here, so
//! a pass is spelled one way whichever view draws it.

use balaur_core::Engine;
use balaur_core::components::{prop_f32, prop_i64, prop_str, prop_vec2, prop_vec3};

use crate::PostConfig;
use crate::vocabulary::{keys as k, words};

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
    /// Where the film becomes a picture. A material before it works in linear
    /// light and is what blooms; one after it works on the finished frame.
    /// Implicit at the head of a list that does not name it.
    Tonemap,
    /// One of the passes kiss3d draws as an effect of its own (`fxaa`,
    /// `sharpen`, `crt`, ...): it draws where it is listed.
    Effect(&'static str),
    /// One of the finishing passes the engine ships as a post-process
    /// material: it draws where it is listed, like any other material.
    Finish(&'static str),
    /// A `material` asset, by id.
    Material(String),
}

impl PostPass {
    fn parse(name: &str) -> Self {
        let known = |list: &[&'static str]| list.iter().copied().find(|word| *word == name);
        match name {
            words::BLOOM => Self::Bloom,
            words::SSAO => Self::Ssao,
            words::SSR => Self::Ssr,
            words::DOF => Self::Dof,
            words::TONEMAP => Self::Tonemap,
            other => match (known(words::EFFECTS), known(words::FINISHES)) {
                (Some(effect), _) => Self::Effect(effect),
                (_, Some(finish)) => Self::Finish(finish),
                _ => Self::Material(other.to_string()),
            },
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Bloom => words::BLOOM,
            Self::Ssao => words::SSAO,
            Self::Ssr => words::SSR,
            Self::Dof => words::DOF,
            Self::Tonemap => words::TONEMAP,
            Self::Effect(name) | Self::Finish(name) => name,
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

/// kiss3d's `SsrSettings`: how the `ssr` pass marches a reflection ray.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reflections {
    pub max_steps: u32,
    /// How far behind a surface, in view space, a ray still counts as hitting it.
    pub thickness: f32,
    pub max_distance: f32,
    /// Surfaces rougher than this reflect nothing, and fade out toward it.
    pub roughness_cutoff: f32,
    /// How wide the band at the frame's edge is that reflections fade over,
    /// as a fraction of the frame.
    pub edge_fade: f32,
    pub intensity: f32,
}

impl Default for Reflections {
    fn default() -> Self {
        Self {
            max_steps: 48,
            thickness: 0.5,
            max_distance: 60.0,
            roughness_cutoff: 0.6,
            edge_fade: 0.12,
            intensity: 1.0,
        }
    }
}

impl Reflections {
    #[must_use]
    pub fn bits(&self) -> [u32; 6] {
        [
            self.max_steps,
            self.thickness.to_bits(),
            self.max_distance.to_bits(),
            self.roughness_cutoff.to_bits(),
            self.edge_fade.to_bits(),
            self.intensity.to_bits(),
        ]
    }
}

/// How the `dof` pass blurs what is out of focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusBlur {
    /// A uniform disc: sharp highlight discs.
    Bokeh,
    /// A soft falloff.
    Gaussian,
}

/// kiss3d's `DofSettings`: the thin lens the `dof` pass models.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DepthOfField {
    pub mode: FocusBlur,
    /// Distance to the plane that stays sharp, in world units.
    pub focus_distance: f32,
    /// Smaller opens the aperture and blurs more.
    pub aperture_f_stops: f32,
    /// In world units; with the camera's field of view it fixes the focal length.
    pub sensor_height: f32,
    pub max_blur_pixels: f32,
    /// Anything farther is blurred as if it were here.
    pub max_depth: f32,
    pub taps: u32,
}

impl Default for DepthOfField {
    fn default() -> Self {
        Self {
            mode: FocusBlur::Bokeh,
            focus_distance: 10.0,
            aperture_f_stops: 0.125,
            sensor_height: 0.018_66,
            max_blur_pixels: 64.0,
            max_depth: 1.0e6,
            taps: 48,
        }
    }
}

impl DepthOfField {
    #[must_use]
    pub fn bits(&self) -> [u32; 7] {
        [
            u32::from(self.mode == FocusBlur::Gaussian),
            self.focus_distance.to_bits(),
            self.aperture_f_stops.to_bits(),
            self.sensor_height.to_bits(),
            self.max_blur_pixels.to_bits(),
            self.max_depth.to_bits(),
            self.taps,
        ]
    }
}

/// Which corner of the frame the `loupe` pass draws its inset in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoupeCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl LoupeCorner {
    const ALL: [(Self, &'static str); 4] = [
        (Self::TopLeft, words::TOP_LEFT),
        (Self::TopRight, words::TOP_RIGHT),
        (Self::BottomLeft, words::BOTTOM_LEFT),
        (Self::BottomRight, words::BOTTOM_RIGHT),
    ];

    fn word(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(corner, _)| *corner == self)
            .map_or(words::BOTTOM_RIGHT, |(_, word)| word)
    }

    fn parse(word: &str) -> Self {
        Self::ALL
            .iter()
            .find(|(_, name)| *name == word)
            .map_or(Self::BottomRight, |(corner, _)| *corner)
    }
}

/// What the passes kiss3d draws on the chain are turned by. Each one is built
/// with these, so a change rebuilds the chain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Effects {
    pub fxaa_edge_threshold: f32,
    pub fxaa_edge_threshold_min: f32,
    pub sharpen_amount: f32,
    pub crt_curvature: f32,
    pub crt_aberration: f32,
    pub crt_scanline_intensity: f32,
    pub crt_scanline_count: f32,
    pub crt_vignette: f32,
    /// How sharp a change in depth the `edges` pass outlines; lower outlines more.
    pub edges_threshold: f32,
    pub loupe_zoom: f32,
    /// The magnified point, `[0, 0]` the frame's top-left corner and `[1, 1]`
    /// its bottom-right.
    pub loupe_focus: [f32; 2],
    pub loupe_corner: LoupeCorner,
    /// The inset's side as a fraction of the frame's shorter side.
    pub loupe_size: f32,
    pub loupe_border_color: [f32; 3],
    pub gi: Gi,
}

/// What the `gi` pass is built with, as kiss3d's `Gi2d` setters take it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gi {
    pub rays: u32,
    /// The longest a ray marches, in world units.
    pub max_distance: f32,
    pub max_steps: u32,
    /// The irradiance field is this many times smaller than the frame.
    pub downscale: u32,
    pub temporal_blend: f32,
    /// Radiance cascades instead of a ray march per pixel.
    pub cascades: bool,
    pub cascade_count: u32,
    pub cascade_directions: u32,
    /// Occluders baked into a distance field each frame: cost independent of
    /// their count, and blind to what is off screen.
    pub screen_occluders: bool,
    pub probe_spacing: u32,
}

impl Default for Gi {
    fn default() -> Self {
        Self {
            rays: 8,
            max_distance: 2000.0,
            max_steps: 32,
            downscale: 2,
            temporal_blend: 0.85,
            cascades: false,
            cascade_count: 5,
            cascade_directions: 16,
            screen_occluders: false,
            probe_spacing: 2,
        }
    }
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            fxaa_edge_threshold: 0.125,
            fxaa_edge_threshold_min: 0.0312,
            // Half sharpness: enough to put back what a smoothing pass took
            // out, short of the ringing the full amount draws around an edge.
            sharpen_amount: 0.5,
            crt_curvature: 0.12,
            crt_aberration: 0.004,
            crt_scanline_intensity: 0.25,
            crt_scanline_count: 480.0,
            crt_vignette: 0.35,
            edges_threshold: 4.0,
            loupe_zoom: 8.0,
            loupe_focus: [0.5, 0.5],
            loupe_corner: LoupeCorner::BottomRight,
            loupe_size: 0.4,
            loupe_border_color: [1.0, 0.9, 0.2],
            gi: Gi::default(),
        }
    }
}

impl Effects {
    #[must_use]
    pub fn bits(&self) -> Vec<u32> {
        let [fx, fy] = self.loupe_focus;
        let [r, g, b] = self.loupe_border_color;
        let mut bits: Vec<u32> = [
            self.fxaa_edge_threshold,
            self.fxaa_edge_threshold_min,
            self.sharpen_amount,
            self.crt_curvature,
            self.crt_aberration,
            self.crt_scanline_intensity,
            self.crt_scanline_count,
            self.crt_vignette,
            self.edges_threshold,
            self.loupe_zoom,
            fx,
            fy,
            self.loupe_size,
            r,
            g,
            b,
        ]
        .iter()
        .map(|v| v.to_bits())
        .collect();
        bits.push(self.loupe_corner as u32);
        let gi = &self.gi;
        bits.extend([
            gi.rays,
            gi.max_distance.to_bits(),
            gi.max_steps,
            gi.downscale,
            gi.temporal_blend.to_bits(),
            u32::from(gi.cascades),
            gi.cascade_count,
            gi.cascade_directions,
            u32::from(gi.screen_occluders),
            gi.probe_spacing,
        ]);
        bits
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
/// the knobs each pass is turned by.
#[derive(Clone, Debug, PartialEq)]
pub struct Post {
    pub passes: Vec<PostPass>,
    pub bloom_threshold: f32,
    pub bloom_intensity: f32,
    /// How wide the soft band around the threshold is that a pixel blooms in.
    pub bloom_knee: f32,
    pub bloom_mips: u32,
    pub finish: Finish,
    pub occlusion: Occlusion,
    pub reflections: Reflections,
    pub depth_of_field: DepthOfField,
    pub effects: Effects,
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

    /// The passes each side of the tonemap, in the order they were listed.
    /// A list that never names `tonemap` has it at the head, so a plain list
    /// of materials is a chain over the finished frame.
    #[must_use]
    pub fn materials(&self) -> (Vec<String>, Vec<String>) {
        let (mut film, mut screen) = (Vec::new(), Vec::new());
        let mut tonemapped = !self.holds(&PostPass::Tonemap);
        for pass in &self.passes {
            // Every pass the engine draws as an effect of its own goes in the
            // list by name, so what the order says is what happens.
            let drawn = match pass {
                PostPass::Tonemap => {
                    tonemapped = true;
                    continue;
                }
                PostPass::Material(id) => id.clone(),
                PostPass::Finish(name) | PostPass::Effect(name) => (*name).to_string(),
                PostPass::Bloom | PostPass::Ssao | PostPass::Ssr | PostPass::Dof => continue,
            };
            if tonemapped {
                screen.push(drawn);
            } else {
                film.push(drawn);
            }
        }
        (film, screen)
    }

    /// The plain numbers beside `post`, paired with the key each is spelled by.
    fn knobs(&self) -> Vec<(&'static str, f32)> {
        let (f, o, r, d, e) = (
            &self.finish,
            &self.occlusion,
            &self.reflections,
            &self.depth_of_field,
            &self.effects,
        );
        vec![
            (k::BLOOM_THRESHOLD, self.bloom_threshold),
            (k::BLOOM_INTENSITY, self.bloom_intensity),
            (k::BLOOM_KNEE, self.bloom_knee),
            (k::VIGNETTE_AMOUNT, f.vignette_amount),
            (k::VIGNETTE_ROUNDNESS, f.vignette_roundness),
            (k::ABERRATION_AMOUNT, f.aberration_amount),
            (k::GRAIN_AMOUNT, f.grain_amount),
            (k::PIXELATE_SIZE, f.pixelate_size),
            (k::SSAO_RADIUS, o.radius),
            (k::SSAO_BIAS, o.bias),
            (k::SSAO_INTENSITY, o.intensity),
            (k::SSAO_POWER, o.power),
            (k::SSR_THICKNESS, r.thickness),
            (k::SSR_MAX_DISTANCE, r.max_distance),
            (k::SSR_ROUGHNESS_CUTOFF, r.roughness_cutoff),
            (k::SSR_EDGE_FADE, r.edge_fade),
            (k::SSR_INTENSITY, r.intensity),
            (k::DOF_FOCUS_DISTANCE, d.focus_distance),
            (k::DOF_APERTURE_F_STOPS, d.aperture_f_stops),
            (k::DOF_SENSOR_HEIGHT, d.sensor_height),
            (k::DOF_MAX_BLUR_PIXELS, d.max_blur_pixels),
            (k::DOF_MAX_DEPTH, d.max_depth),
            (k::FXAA_EDGE_THRESHOLD, e.fxaa_edge_threshold),
            (k::FXAA_EDGE_THRESHOLD_MIN, e.fxaa_edge_threshold_min),
            (k::SHARPEN_AMOUNT, e.sharpen_amount),
            (k::CRT_CURVATURE, e.crt_curvature),
            (k::CRT_ABERRATION, e.crt_aberration),
            (k::CRT_SCANLINE_INTENSITY, e.crt_scanline_intensity),
            (k::CRT_SCANLINE_COUNT, e.crt_scanline_count),
            (k::CRT_VIGNETTE, e.crt_vignette),
            (k::EDGES_THRESHOLD, e.edges_threshold),
            (k::LOUPE_ZOOM, e.loupe_zoom),
            (k::LOUPE_SIZE, e.loupe_size),
            (k::GI_MAX_DISTANCE, e.gi.max_distance),
            (k::GI_TEMPORAL_BLEND, e.gi.temporal_blend),
        ]
    }
}

impl Default for Post {
    fn default() -> Self {
        Self {
            passes: Vec::new(),
            bloom_threshold: 1.0,
            bloom_intensity: 0.6,
            bloom_knee: 0.5,
            bloom_mips: 5,
            finish: Finish::default(),
            occlusion: Occlusion::default(),
            reflections: Reflections::default(),
            depth_of_field: DepthOfField::default(),
            effects: Effects::default(),
        }
    }
}

/// Mirror the current camera's effects into [`PostConfig`], raising
/// `changed` only when one actually differs: a backend rebuilds its
/// post chain when it sees that flag, and doing so every frame would
/// rebuild it every frame.
pub(crate) fn drive_post(eng: &Engine, post: &Post) {
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
        && config.bloom_knee.to_bits() == post.bloom_knee.to_bits()
        && config.bloom_mips == post.bloom_mips
        && config.finish.bits() == post.finish.bits()
        && config.occlusion.bits() == post.occlusion.bits()
        && config.reflections.bits() == post.reflections.bits()
        && config.depth_of_field.bits() == post.depth_of_field.bits()
        && config.effects.bits() == post.effects.bits();
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
    config.bloom_knee = post.bloom_knee;
    config.bloom_mips = post.bloom_mips;
    config.finish = post.finish;
    config.occlusion = post.occlusion;
    config.reflections = post.reflections;
    config.depth_of_field = post.depth_of_field;
    config.effects = post.effects;
    config.changed = true;
}

/// A whole number at least `floor`, as kiss3d's counters take it.
fn count(params: &toml::Value, key: &str, floor: i64) -> u32 {
    u32::try_from(prop_i64(params, key).max(floor)).unwrap_or(u32::MAX)
}

/// The post chain both cameras carry, read once so the two readers differ
/// only by the properties their own view has.
pub(crate) fn post_from_params(params: &toml::Value) -> Post {
    let num = |key: &str| prop_f32(params, key);
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
    let [r, g, b] = prop_vec3(params, k::LOUPE_BORDER_COLOR);
    Post {
        occlusion: Occlusion {
            radius: num(k::SSAO_RADIUS).max(1e-3),
            bias: num(k::SSAO_BIAS).max(0.0),
            intensity: num(k::SSAO_INTENSITY).max(0.0),
            power: num(k::SSAO_POWER).max(1e-3),
        },
        finish: Finish {
            vignette_amount: num(k::VIGNETTE_AMOUNT),
            vignette_roundness: num(k::VIGNETTE_ROUNDNESS),
            aberration_amount: num(k::ABERRATION_AMOUNT),
            grain_amount: num(k::GRAIN_AMOUNT),
            pixelate_size: num(k::PIXELATE_SIZE),
        },
        reflections: Reflections {
            max_steps: count(params, k::SSR_MAX_STEPS, 1),
            thickness: num(k::SSR_THICKNESS).max(0.0),
            max_distance: num(k::SSR_MAX_DISTANCE).max(0.0),
            roughness_cutoff: num(k::SSR_ROUGHNESS_CUTOFF).clamp(0.0, 1.0),
            edge_fade: num(k::SSR_EDGE_FADE).max(0.0),
            intensity: num(k::SSR_INTENSITY).max(0.0),
        },
        depth_of_field: DepthOfField {
            mode: if prop_str(params, k::DOF_MODE) == words::GAUSSIAN {
                FocusBlur::Gaussian
            } else {
                FocusBlur::Bokeh
            },
            focus_distance: num(k::DOF_FOCUS_DISTANCE).max(0.0),
            aperture_f_stops: num(k::DOF_APERTURE_F_STOPS).max(1e-3),
            sensor_height: num(k::DOF_SENSOR_HEIGHT).max(1e-6),
            max_blur_pixels: num(k::DOF_MAX_BLUR_PIXELS).max(0.0),
            max_depth: num(k::DOF_MAX_DEPTH).max(0.0),
            taps: count(params, k::DOF_TAPS, 1),
        },
        effects: Effects {
            fxaa_edge_threshold: num(k::FXAA_EDGE_THRESHOLD).max(0.0),
            fxaa_edge_threshold_min: num(k::FXAA_EDGE_THRESHOLD_MIN).max(0.0),
            sharpen_amount: num(k::SHARPEN_AMOUNT).clamp(0.0, 1.0),
            crt_curvature: num(k::CRT_CURVATURE).max(0.0),
            crt_aberration: num(k::CRT_ABERRATION).max(0.0),
            crt_scanline_intensity: num(k::CRT_SCANLINE_INTENSITY).clamp(0.0, 1.0),
            crt_scanline_count: num(k::CRT_SCANLINE_COUNT).max(1.0),
            crt_vignette: num(k::CRT_VIGNETTE).clamp(0.0, 1.0),
            edges_threshold: num(k::EDGES_THRESHOLD).max(0.0),
            loupe_zoom: num(k::LOUPE_ZOOM).max(1.0),
            loupe_focus: prop_vec2(params, k::LOUPE_FOCUS).map(|v| v.clamp(0.0, 1.0)),
            loupe_corner: LoupeCorner::parse(prop_str(params, k::LOUPE_CORNER)),
            loupe_size: num(k::LOUPE_SIZE).clamp(0.01, 1.0),
            loupe_border_color: [r, g, b],
            gi: Gi {
                rays: count(params, k::GI_RAYS, 1),
                max_distance: num(k::GI_MAX_DISTANCE).max(0.0),
                max_steps: count(params, k::GI_MAX_STEPS, 1),
                downscale: count(params, k::GI_DOWNSCALE, 1),
                temporal_blend: num(k::GI_TEMPORAL_BLEND).clamp(0.0, 0.99),
                cascades: prop_str(params, k::GI_SOLVER) == words::CASCADES,
                cascade_count: count(params, k::GI_CASCADE_COUNT, 1).min(8),
                cascade_directions: count(params, k::GI_CASCADE_DIRECTIONS, 4),
                screen_occluders: balaur_core::components::prop_bool(
                    params,
                    k::GI_SCREEN_OCCLUDERS,
                ),
                probe_spacing: count(params, k::GI_PROBE_SPACING, 1).min(16),
            },
        },
        passes,
        bloom_threshold: num(k::BLOOM_THRESHOLD).max(0.0),
        bloom_intensity: num(k::BLOOM_INTENSITY).max(0.0),
        bloom_knee: num(k::BLOOM_KNEE).max(0.0),
        bloom_mips: count(params, k::BLOOM_MIPS, 1).min(12),
    }
}

/// The post-chain properties both cameras carry, spelled once: the two
/// schemas differ only where the two views actually differ.
pub(crate) fn post_schema() -> String {
    format!(
        r#"current = {{ type = "bool", default = true, description = "Whether this camera drives the view; the last current one wins" }}
post = {{ type = "list", of = {{ type = "string" }}, default = [], description = "The frame's passes, in order. {effects} name the engine's own -- `ssao`, `ssr` and `dof` are 3D only, `gi` is 2D only and lights the frame in place of the `light2d` light map, and where those and `bloom` run is fixed by the pipeline. Any other name is a `material` asset drawn over the whole frame. The rest run in the order given. `tonemap` is where the film becomes a picture: a pass before it works in linear light and is what blooms, one after it works on the finished frame, and a list that does not name it has it at the head" }}
bloom_threshold = {{ type = "float", default = 1.0, min = 0.0, description = "Brightness a pixel has to pass to bloom" }}
bloom_intensity = {{ type = "float", default = 0.6, min = 0.0, description = "How much of the bloom is added back over the frame" }}
bloom_knee = {{ type = "float", default = 0.5, min = 0.0, description = "Width of the soft band around `bloom_threshold` a pixel starts to bloom in; 0 is a hard cut" }}
bloom_mips = {{ type = "int", default = 5, min = 1, max = 12, description = "Levels in the bloom chain, each half the size of the last; more spreads the glow wider" }}
vignette_amount = {{ type = "float", default = 0.35, min = 0.0, max = 1.0, description = "How dark the corners go under the `vignette` pass" }}
vignette_roundness = {{ type = "float", default = 1.0, min = 0.0, max = 1.0, description = "1 darkens in a circle whatever shape the frame is; 0 follows the frame" }}
aberration_amount = {{ type = "float", default = 0.004, min = 0.0, description = "How far `aberration` slides red from blue at the frame's edge, as a fraction of it" }}
grain_amount = {{ type = "float", default = 0.06, min = 0.0, description = "How much the `grain` pass lightens and darkens a pixel" }}
pixelate_size = {{ type = "float", default = 4.0, min = 1.0, description = "The side of one block the `pixelate` pass reads the frame back in, in pixels" }}
ssao_radius = {{ type = "float", default = 0.5, min = 0.001, description = "How far the `ssao` pass looks for something occluding a point, in world units. Scale it with the scene" }}
ssao_bias = {{ type = "float", default = 0.025, min = 0.0, description = "How far in front of a surface a sample must be to occlude it. Too small and a glancing surface occludes itself into black" }}
ssao_intensity = {{ type = "float", default = 1.2, min = 0.0, description = "How strongly the `ssao` pass darkens" }}
ssao_power = {{ type = "float", default = 1.5, min = 0.001, description = "The contrast the occlusion is raised to" }}
{reflections}
{focus}
{effects_schema}"#,
        effects = words::POST_EFFECTS.join(", "),
        reflections = reflections_schema(),
        focus = focus_schema(),
        effects_schema = effects_schema(),
    )
}

fn reflections_schema() -> &'static str {
    r#"ssr_max_steps = { type = "int", default = 48, min = 1, description = "Steps the `ssr` pass marches a reflection ray before it gives up" }
ssr_thickness = { type = "float", default = 0.5, min = 0.0, description = "How far behind a surface, in view-space units, the ray still counts as hitting it" }
ssr_max_distance = { type = "float", default = 60.0, min = 0.0, description = "The longest a reflection ray travels, in view-space units" }
ssr_roughness_cutoff = { type = "float", default = 0.6, min = 0.0, max = 1.0, description = "Surfaces rougher than this reflect nothing on screen, and fade out approaching it" }
ssr_edge_fade = { type = "float", default = 0.12, min = 0.0, description = "Width of the band at the frame's edge reflections fade over, as a fraction of the frame" }
ssr_intensity = { type = "float", default = 1.0, min = 0.0, description = "Multiplier on every screen-space reflection" }"#
}

fn focus_schema() -> String {
    format!(
        r#"dof_mode = {{ type = "enum", default = "{bokeh}", options = [{modes}], description = "How the `dof` pass blurs: a uniform disc with sharp highlights, or a soft gaussian falloff" }}
dof_focus_distance = {{ type = "float", default = 10.0, min = 0.0, description = "Distance from the camera to the plane that stays sharp, in world units" }}
dof_aperture_f_stops = {{ type = "float", default = 0.125, min = 0.001, description = "The lens aperture in f-stops; smaller blurs more" }}
dof_sensor_height = {{ type = "float", default = 0.01866, min = 0.000001, description = "Sensor height in world units; with the field of view it fixes the focal length" }}
dof_max_blur_pixels = {{ type = "float", default = 64.0, min = 0.0, description = "The widest a blur circle grows, in pixels" }}
dof_max_depth = {{ type = "float", default = 1000000.0, min = 0.0, description = "Anything farther is blurred as if it were at this distance, in world units" }}
dof_taps = {{ type = "int", default = 48, min = 1, description = "Samples the blur gathers per pixel; more is smoother and costs more" }}"#,
        bokeh = words::BOKEH,
        modes = crate::vocabulary::options(words::DOF_MODES),
    )
}

fn effects_schema() -> String {
    format!(
        r#"fxaa_edge_threshold = {{ type = "float", default = 0.125, min = 0.0, description = "The `fxaa` pass smooths an edge whose contrast passes this fraction of the brightest pixel near it" }}
fxaa_edge_threshold_min = {{ type = "float", default = 0.0312, min = 0.0, description = "Contrast below this is left alone by `fxaa`, so dark noise is not smoothed" }}
sharpen_amount = {{ type = "float", default = 0.5, min = 0.0, max = 1.0, description = "How hard the `sharpen` pass sharpens" }}
crt_curvature = {{ type = "float", default = 0.12, min = 0.0, description = "How far the `crt` pass bends the frame like a tube's glass; 0 is flat" }}
crt_aberration = {{ type = "float", default = 0.004, min = 0.0, description = "How far `crt` splits the colours at the frame's edge, as a fraction of it" }}
crt_scanline_intensity = {{ type = "float", default = 0.25, min = 0.0, max = 1.0, description = "How dark the `crt` pass draws its scanlines; 0 draws none" }}
crt_scanline_count = {{ type = "float", default = 480.0, min = 1.0, description = "How many scanlines `crt` draws down the frame" }}
crt_vignette = {{ type = "float", default = 0.35, min = 0.0, max = 1.0, description = "How dark `crt` draws the corners" }}
edges_threshold = {{ type = "float", default = 4.0, min = 0.0, description = "How sharp a change in depth the `edges` pass outlines; lower outlines more" }}
loupe_zoom = {{ type = "float", default = 8.0, min = 1.0, description = "How many times the `loupe` pass magnifies" }}
loupe_focus = {{ type = "vec2", default = [0.5, 0.5], description = "The point the `loupe` magnifies, [0, 0] at the frame's top-left and [1, 1] at its bottom-right" }}
loupe_corner = {{ type = "enum", default = "{bottom_right}", options = [{corners}], description = "The corner the `loupe` draws its inset in" }}
loupe_size = {{ type = "float", default = 0.4, min = 0.01, max = 1.0, description = "The inset's side, as a fraction of the frame's shorter side" }}
loupe_border_color = {{ type = "color", default = [1.0, 0.9, 0.2, 1.0], description = "The colour of the `loupe`'s frame and of the outline round what it magnifies; alpha is ignored" }}
{gi}"#,
        bottom_right = words::BOTTOM_RIGHT,
        corners = crate::vocabulary::options(words::LOUPE_CORNERS),
        gi = gi_schema(),
    )
}

fn gi_schema() -> String {
    format!(
        r#"gi_rays = {{ type = "int", default = 8, min = 1, description = "Rays the `gi` pass casts per pixel each frame; the temporal blend adds up the frames" }}
gi_max_distance = {{ type = "float", default = 2000.0, min = 0.0, description = "The farthest a `gi` ray marches, in world units" }}
gi_max_steps = {{ type = "int", default = 32, min = 1, description = "Steps a `gi` ray takes before it gives up" }}
gi_downscale = {{ type = "int", default = 2, min = 1, description = "How many times smaller than the frame the `gi` light field is; larger is faster and softer" }}
gi_temporal_blend = {{ type = "float", default = 0.85, min = 0.0, max = 0.99, description = "How much of last frame's `gi` light is kept; higher is smoother and trails behind motion. The cascade solver ignores it" }}
gi_solver = {{ type = "enum", default = "{ray_march}", options = [{solvers}], description = "A ray march per pixel, or radiance cascades: probe grids that gather farther light for less" }}
gi_cascade_count = {{ type = "int", default = 5, min = 1, max = 8, description = "Levels of radiance cascades; more reach farther light" }}
gi_cascade_directions = {{ type = "int", default = 16, min = 4, description = "Directions the first cascade gathers from, rounded to a square of a power of two; more sharpens shadow edges" }}
gi_screen_occluders = {{ type = "bool", default = false, description = "Bake the occluders into a distance field each frame, so their count costs nothing; an occluder off screen then casts no shadow" }}
gi_probe_spacing = {{ type = "int", default = 2, min = 1, max = 16, description = "Field pixels between the first cascade's probes, rounded up to a power of two; finer resolves sharper light" }}"#,
        ray_march = words::RAY_MARCH,
        solvers = crate::vocabulary::options(words::GI_SOLVERS),
    )
}

/// The post chain read back, which both `get` closures end with.
pub(crate) fn post_to_map(post: &Post, map: &mut toml::map::Map<String, toml::Value>) {
    map.insert(
        k::POST.into(),
        toml::Value::Array(
            post.passes
                .iter()
                .map(|pass| toml::Value::String(pass.name().into()))
                .collect(),
        ),
    );
    // The inspector reads this, so the knobs beside `post` are here too
    // rather than only in the table the scene handed over.
    for (key, value) in post.knobs() {
        map.insert(key.into(), toml::Value::Float(f64::from(value)));
    }
    let (r, d, e) = (&post.reflections, &post.depth_of_field, &post.effects);
    let int = |n: u32| toml::Value::Integer(i64::from(n));
    let word = |w: &str| toml::Value::String(w.into());
    let mode = match d.mode {
        FocusBlur::Bokeh => words::BOKEH,
        FocusBlur::Gaussian => words::GAUSSIAN,
    };
    let [fx, fy] = e.loupe_focus;
    let [cr, cg, cb] = e.loupe_border_color;
    map.insert(k::BLOOM_MIPS.into(), int(post.bloom_mips));
    map.insert(k::SSR_MAX_STEPS.into(), int(r.max_steps));
    map.insert(k::DOF_TAPS.into(), int(d.taps));
    map.insert(k::DOF_MODE.into(), word(mode));
    map.insert(k::LOUPE_CORNER.into(), word(e.loupe_corner.word()));
    let solver = if e.gi.cascades {
        words::CASCADES
    } else {
        words::RAY_MARCH
    };
    map.insert(k::GI_SOLVER.into(), word(solver));
    map.insert(
        k::GI_SCREEN_OCCLUDERS.into(),
        toml::Value::Boolean(e.gi.screen_occluders),
    );
    for (key, n) in [
        (k::GI_RAYS, e.gi.rays),
        (k::GI_MAX_STEPS, e.gi.max_steps),
        (k::GI_DOWNSCALE, e.gi.downscale),
        (k::GI_CASCADE_COUNT, e.gi.cascade_count),
        (k::GI_CASCADE_DIRECTIONS, e.gi.cascade_directions),
        (k::GI_PROBE_SPACING, e.gi.probe_spacing),
    ] {
        map.insert(key.into(), int(n));
    }
    map.insert(
        k::LOUPE_FOCUS.into(),
        toml::Value::Array(
            vec![fx, fy]
                .into_iter()
                .map(|v| toml::Value::Float(f64::from(v)))
                .collect(),
        ),
    );
    map.insert(
        k::LOUPE_BORDER_COLOR.into(),
        crate::color_to_toml([cr, cg, cb, 1.0]),
    );
}
