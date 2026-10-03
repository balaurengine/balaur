//! What a material says beside its shader: the `[surface]` table every
//! material takes, and the values kiss3d's own material takes from one that
//! names no shader. Split from `material` for its length.

use anyhow::{Result, anyhow, bail};
use balaur_core::Engine;

use crate::material::{Material3d, Param, TEXTURE_SLOTS, hex_rgba, parse_param, project_path};
use crate::vocabulary::{keys as k, words};

/// How a height map is searched for the point a view ray meets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Parallax {
    /// A linear search, then the crossing interpolated. Cheaper.
    #[default]
    Occlusion,
    /// A linear search refined by `parallax_relief_steps` halvings.
    Relief,
}

/// One of kiss3d's named debug materials, drawn instead of the surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Normals,
    Uvs,
}

/// A material with no `shader`: the values kiss3d's own material takes, by
/// the names its `[params]` table writes them under.
#[derive(Clone, Debug, PartialEq)]
pub struct Builtin {
    pub metallic: f32,
    pub roughness: f32,
    pub emission_color: [f32; 4],
    pub specular_tint: [f32; 4],
    /// 0.5 is the 4% a common dielectric reflects head-on.
    pub reflectance: f32,
    pub clearcoat: f32,
    pub clearcoat_roughness: f32,
    pub anisotropy: f32,
    pub anisotropy_rotation_degrees: f32,
    pub subsurface: f32,
    pub subsurface_radius: f32,
    pub parallax_scale: f32,
    pub parallax_layers: f32,
    pub parallax_method: Parallax,
    pub parallax_relief_steps: u32,
    /// The image each of [`TEXTURE_SLOTS`] is bound to, in order.
    pub maps: Vec<Option<String>>,
    pub view: Option<View>,
}

impl Default for Builtin {
    fn default() -> Self {
        Self {
            metallic: 0.0,
            roughness: 0.5,
            emission_color: [0.0, 0.0, 0.0, 1.0],
            specular_tint: [1.0; 4],
            reflectance: 0.5,
            clearcoat: 0.0,
            clearcoat_roughness: 0.0,
            anisotropy: 0.0,
            anisotropy_rotation_degrees: 0.0,
            subsurface: 0.0,
            subsurface_radius: 0.0,
            parallax_scale: 0.1,
            parallax_layers: 16.0,
            parallax_method: Parallax::Occlusion,
            parallax_relief_steps: 8,
            maps: vec![None; TEXTURE_SLOTS.len()],
            view: None,
        }
    }
}

/// The `[params]` keys a material with no `shader` takes beside the texture
/// slots, for the error a typo gets.
const BUILTIN_KEYS: &[&str] = &[
    k::METALLIC,
    k::ROUGHNESS,
    k::EMISSION_COLOR,
    k::SPECULAR_TINT,
    k::REFLECTANCE,
    k::CLEARCOAT,
    k::CLEARCOAT_ROUGHNESS,
    k::ANISOTROPY,
    k::ANISOTROPY_ROTATION_DEGREES,
    k::SUBSURFACE,
    k::SUBSURFACE_RADIUS,
    k::PARALLAX_SCALE,
    k::PARALLAX_LAYERS,
    k::PARALLAX_METHOD,
    k::PARALLAX_RELIEF_STEPS,
];

/// Read a shader-less material's `[params]`: the texture slots into
/// `params`, everything else into the [`Builtin`] it returns.
pub(crate) fn parse_builtin(
    value: &toml::Value,
    params: &mut Vec<(String, Param)>,
) -> Result<Builtin> {
    let mut out = Builtin::default();
    let table = value.get("params").and_then(toml::Value::as_table);
    for (name, item) in table.into_iter().flatten() {
        if TEXTURE_SLOTS.contains(&name.as_str()) {
            params.push((name.clone(), parse_param(name, item)?));
            continue;
        }
        let number = || {
            balaur_core::components::as_f64(item)
                .map(|n| n as f32)
                .ok_or_else(|| anyhow!("param `{name}` is a number, not `{item}`"))
        };
        let colour = || match parse_param(name, item)? {
            Param::Vec4(rgba) => Ok(rgba),
            Param::Vec3([r, g, b]) => Ok([r, g, b, 1.0]),
            other => bail!("param `{name}` is a colour, not a {}", other.type_name()),
        };
        match name.as_str() {
            k::METALLIC => out.metallic = number()?.clamp(0.0, 1.0),
            k::ROUGHNESS => out.roughness = number()?.clamp(0.0, 1.0),
            k::EMISSION_COLOR => out.emission_color = colour()?,
            k::SPECULAR_TINT => out.specular_tint = colour()?,
            k::REFLECTANCE => out.reflectance = number()?.clamp(0.0, 1.0),
            k::CLEARCOAT => out.clearcoat = number()?.clamp(0.0, 1.0),
            k::CLEARCOAT_ROUGHNESS => out.clearcoat_roughness = number()?.clamp(0.0, 1.0),
            k::ANISOTROPY => out.anisotropy = number()?.clamp(-1.0, 1.0),
            k::ANISOTROPY_ROTATION_DEGREES => out.anisotropy_rotation_degrees = number()?,
            k::SUBSURFACE => out.subsurface = number()?.clamp(0.0, 1.0),
            k::SUBSURFACE_RADIUS => out.subsurface_radius = number()?.max(0.0),
            k::PARALLAX_SCALE => out.parallax_scale = number()?.max(0.0),
            k::PARALLAX_LAYERS => out.parallax_layers = number()?.clamp(1.0, 64.0),
            k::PARALLAX_RELIEF_STEPS => {
                out.parallax_relief_steps = number()?.round().clamp(1.0, 64.0) as u32;
            }
            k::PARALLAX_METHOD => {
                out.parallax_method = match item.as_str() {
                    Some(words::OCCLUSION) => Parallax::Occlusion,
                    Some(words::RELIEF) => Parallax::Relief,
                    _ => bail!(
                        "param `{name}` is one of {}, not `{item}`",
                        words::PARALLAX_METHODS.join(", ")
                    ),
                };
            }
            _ => bail!(
                "a material with no `shader` takes no `{name}`; its params are {} and the texture slots {}",
                BUILTIN_KEYS.join(", "),
                TEXTURE_SLOTS.join(", ")
            ),
        }
    }
    out.view = match value.get(k::VIEW).and_then(toml::Value::as_str) {
        None => None,
        Some(words::AOV_NORMALS) => Some(View::Normals),
        Some(words::UVS) => Some(View::Uvs),
        Some(other) => bail!(
            "a material's `view` is one of {}, not `{other}`",
            words::MATERIAL_VIEWS.join(", ")
        ),
    };
    Ok(out)
}

/// The shader-less half of the material `reference` names, or `None` for
/// one with a shader, one that will not load, and no material at all.
#[must_use]
pub fn builtin_of(eng: &Engine, reference: &str) -> Option<Builtin> {
    if reference.is_empty() {
        return None;
    }
    let material = balaur_core::assets::load_typed::<Material3d>(eng, reference).ok()?;
    let mut builtin = material.builtin.clone()?;
    builtin.maps = material
        .textures()
        .into_iter()
        .map(|path| {
            path.map(|path| project_path(eng, reference, path).unwrap_or_else(|| path.to_string()))
        })
        .collect();
    Some(builtin)
}

/// How a surface's alpha is read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlphaMode {
    /// Alpha is ignored and the surface is solid.
    #[default]
    Opaque,
    /// A fragment fainter than `alpha_cutoff` is dropped rather than drawn:
    /// a leaf's outline, cut from the rectangle it was painted on.
    Mask,
    /// The surface is drawn over what is behind it, in the pass that resolves
    /// overlapping translucent surfaces without sorting them.
    Blend,
    /// As `Blend`, for a colour that already carries its alpha.
    Premultiplied,
}

/// What the path tracer takes a surface for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TraceSurface {
    #[default]
    Opaque,
    Glass,
    Metal,
    /// It shades as opaque and casts light by its emission.
    Light,
}

/// How a surface takes screen-space reflections.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is its own authored `[surface]` key, not a state machine"
)]
pub struct SurfaceSsr {
    /// Off keeps this surface out of them altogether.
    pub on: bool,
    pub intensity: f32,
    /// Count every depth crossing a ray makes as a hit, for thin geometry.
    pub infinite_thickness: bool,
    /// Fade a reflection with how far its ray travelled.
    pub distance_fade: bool,
    /// Strengthen reflections at a glancing angle.
    pub fresnel: bool,
}

impl Default for SurfaceSsr {
    fn default() -> Self {
        Self {
            on: true,
            intensity: 1.0,
            infinite_thickness: false,
            distance_fade: true,
            fresnel: false,
        }
    }
}

/// What a material says about how its node draws, rather than what colour it
/// comes out.
///
/// These decide which pass a node joins and how it is rasterized, so the
/// backend reads them off the material and sets them on the node, while the
/// shader reads the same numbers through its own `Params`. Everything here
/// defaults to the surface a material that says nothing already had.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub alpha: AlphaMode,
    /// The alpha a `mask` surface drops a fragment below.
    pub alpha_cutoff: f32,
    /// Whether the back of a triangle draws as well as the front.
    pub double_sided: bool,
    /// How much of the scene behind this surface comes through it. Above zero
    /// makes it glass: it draws after the opaque scene is resolved, refracting
    /// what that pass left.
    pub transmission: f32,
    /// How sharply light bends entering it. 1.0 does not bend at all; window
    /// glass is 1.5, water 1.33, diamond 2.42.
    pub ior: f32,
    /// How far light travels inside it, in world units. Zero refracts without
    /// tinting, which is what a single pane wants.
    pub thickness: f32,
    /// What is left of white light after `attenuation_distance` inside it.
    pub attenuation_color: [f32; 4],
    /// Zero takes nothing out however thick the glass.
    pub attenuation_distance: f32,
    /// Whether this surface shows the scene reflected in its own plane. The
    /// reflection is rendered from a mirrored camera, so it is sharp where a
    /// probe or the sky would only be approximate.
    pub mirror: bool,
    /// How much of the reflection shows, from nothing to all of it.
    pub mirror_intensity: f32,
    /// How fast the reflection fades as the surface turns away from its own
    /// plane. Zero keeps it even, which is what a flat mirror wants; above
    /// zero keeps a curved one reflecting only on the face that looks along
    /// the plane's normal.
    pub mirror_falloff: f32,
    /// Which way the mirror's plane faces in the node's own space. A Balaur
    /// `plane` lies in xz, so its face looks up.
    pub mirror_normal: [f32; 3],
    /// The mirror's picture as a fraction of the viewport's size.
    pub mirror_resolution_scale: f32,
    /// What the mirror draws, by `render_layers`; `None` follows the camera.
    pub mirror_render_layers: Option<u32>,
    pub trace_surface: TraceSurface,
    pub ssr: SurfaceSsr,
}

impl Default for Surface {
    fn default() -> Self {
        Self {
            alpha: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
            transmission: 0.0,
            ior: 1.5,
            thickness: 0.0,
            attenuation_color: [1.0, 1.0, 1.0, 1.0],
            attenuation_distance: 0.0,
            mirror: false,
            mirror_intensity: 1.0,
            mirror_falloff: 0.0,
            mirror_normal: [0.0, 1.0, 0.0],
            mirror_resolution_scale: 1.0,
            mirror_render_layers: None,
            trace_surface: TraceSurface::Opaque,
            ssr: SurfaceSsr::default(),
        }
    }
}

impl Surface {
    /// Whether a node drawing this joins the refraction pass rather than the
    /// opaque one.
    #[must_use]
    pub const fn refracts(&self) -> bool {
        self.transmission > 0.0
    }

    /// `attenuation_distance` as kiss3d's own material reads it: there only
    /// infinity takes nothing out, where `pbr.wesl` reads zero that way.
    #[must_use]
    pub fn absorbing_distance(&self) -> f32 {
        if self.attenuation_distance > 0.0 {
            self.attenuation_distance
        } else {
            f32::INFINITY
        }
    }
}

/// Every key a `[surface]` table may set, for the error a typo gets. A
/// `[params]` key the shader does not read is refused, and this is the one
/// table that would otherwise swallow one.
const SURFACE_KEYS: &[&str] = &[
    k::ALPHA,
    k::ALPHA_CUTOFF,
    k::DOUBLE_SIDED,
    k::TRANSMISSION,
    k::IOR,
    k::THICKNESS,
    k::ATTENUATION_COLOR,
    k::ATTENUATION_DISTANCE,
    k::MIRROR,
    k::MIRROR_INTENSITY,
    k::MIRROR_FALLOFF,
    k::MIRROR_NORMAL,
    k::MIRROR_RESOLUTION_SCALE,
    k::MIRROR_RENDER_LAYERS,
    k::TRACE_SURFACE,
    k::SSR,
    k::SSR_INTENSITY,
    k::SSR_INFINITE_THICKNESS,
    k::SSR_DISTANCE_FADE,
    k::SSR_FRESNEL,
];

/// The `alpha` word a `[surface]` table writes.
fn alpha_of(word: &str) -> Result<AlphaMode> {
    Ok(match word {
        words::OPAQUE => AlphaMode::Opaque,
        words::MASK => AlphaMode::Mask,
        words::BLEND => AlphaMode::Blend,
        words::PREMULTIPLIED => AlphaMode::Premultiplied,
        other => bail!(
            "a material's `surface.alpha` is {}, not '{other}'",
            words::ALPHA_MODES.join(", ")
        ),
    })
}

/// The `trace_surface` word a `[surface]` table writes.
fn trace_of(word: &str) -> Result<TraceSurface> {
    Ok(match word {
        words::OPAQUE => TraceSurface::Opaque,
        words::GLASS => TraceSurface::Glass,
        words::METAL => TraceSurface::Metal,
        words::LIGHT => TraceSurface::Light,
        other => bail!(
            "a material's `surface.trace_surface` is {}, not '{other}'",
            words::TRACE_SURFACES.join(", ")
        ),
    })
}

/// Read a material's `[surface]` table.
pub(crate) fn parse_surface(value: &toml::Value) -> Result<Surface> {
    let base = Surface::default();
    let Some(table) = value.get("surface") else {
        return Ok(base);
    };
    if let Some(table) = table.as_table() {
        for name in table.keys() {
            if !SURFACE_KEYS.contains(&name.as_str()) {
                bail!(
                    "a material's `[surface]` has no `{name}`; it takes {}",
                    SURFACE_KEYS.join(", ")
                );
            }
        }
    }
    let num = |key: &str, default: f32| {
        table
            .get(key)
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(f64::from(default)) as f32
    };
    let flag = |key: &str, default: bool| {
        table
            .get(key)
            .and_then(toml::Value::as_bool)
            .unwrap_or(default)
    };
    let word = |key: &str, default: &'static str| {
        table
            .get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or(default)
            .to_string()
    };
    let ssr = SurfaceSsr {
        on: flag(k::SSR, base.ssr.on),
        intensity: num(k::SSR_INTENSITY, base.ssr.intensity).max(0.0),
        infinite_thickness: flag(k::SSR_INFINITE_THICKNESS, base.ssr.infinite_thickness),
        distance_fade: flag(k::SSR_DISTANCE_FADE, base.ssr.distance_fade),
        fresnel: flag(k::SSR_FRESNEL, base.ssr.fresnel),
    };
    Ok(Surface {
        alpha: alpha_of(&word(k::ALPHA, words::OPAQUE))?,
        alpha_cutoff: num(k::ALPHA_CUTOFF, base.alpha_cutoff).clamp(0.0, 1.0),
        double_sided: flag(k::DOUBLE_SIDED, base.double_sided),
        transmission: num(k::TRANSMISSION, 0.0).clamp(0.0, 1.0),
        ior: num(k::IOR, base.ior).max(1.0),
        thickness: num(k::THICKNESS, 0.0).max(0.0),
        attenuation_color: table
            .get(k::ATTENUATION_COLOR)
            .and_then(toml::Value::as_str)
            .and_then(hex_rgba)
            .unwrap_or(base.attenuation_color),
        attenuation_distance: num(k::ATTENUATION_DISTANCE, 0.0).max(0.0),
        mirror: flag(k::MIRROR, base.mirror),
        mirror_intensity: num(k::MIRROR_INTENSITY, base.mirror_intensity).clamp(0.0, 1.0),
        mirror_falloff: num(k::MIRROR_FALLOFF, 0.0).max(0.0),
        mirror_normal: {
            let axis = |i: usize, default: f32| {
                table
                    .get(k::MIRROR_NORMAL)
                    .and_then(toml::Value::as_array)
                    .and_then(|a| a.get(i))
                    .and_then(balaur_core::components::as_f64)
                    .unwrap_or(f64::from(default)) as f32
            };
            let normal = base.mirror_normal;
            [axis(0, normal[0]), axis(1, normal[1]), axis(2, normal[2])]
        },
        mirror_resolution_scale: num(k::MIRROR_RESOLUTION_SCALE, base.mirror_resolution_scale)
            .clamp(0.01, 4.0),
        mirror_render_layers: table
            .get(k::MIRROR_RENDER_LAYERS)
            .and_then(toml::Value::as_integer)
            .map(|mask| mask as u32),
        trace_surface: trace_of(&word(k::TRACE_SURFACE, words::OPAQUE))?,
        ssr,
    })
}
