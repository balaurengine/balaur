// The physically based surface, over the lights `package::mesh` already
// collects. A material importing this gets metallic, roughness, emissive and
// reflectance instead of Lambert, and the texture slots the contract declares.
//
// Cook-Torrance: GGX distribution, Smith height-correlated visibility,
// Schlick's Fresnel. The same terms glTF's reference shading uses, so a model
// exported from Blender lands where its reference render puts it.

import package::mesh::{
    VertexOutput, Light, frame, eye, tint, apply_fog,
    sample_albedo, sample_metallic_roughness, sample_occlusion, sample_emissive,
    shaded_normal, screen_occlusion,
    environment_lit, environment_max_lod, environment_sample,
    probe_at, probe_sample, probe_max_lod,
    behind, behind_max_lod, screen_uv, mirror_at, clip,
};

const PI: f32 = 3.14159265;
const INV_PI: f32 = 0.31830988;
/// Below this a surface is a mirror, and the maths stops being stable.
const MIN_ROUGHNESS: f32 = 0.045;

/// One shaded point: what the maps and the material's own numbers agree on.
struct Surface {
    albedo: vec3<f32>,
    alpha: f32,
    normal: vec3<f32>,
    metallic: f32,
    roughness: f32,
    occlusion: f32,
    emissive: vec3<f32>,
    /// Normal-incidence reflectance of a dielectric, as glTF spells it:
    /// 0.5 is the 4% every common material has.
    reflectance: f32,
}

/// A surface with the neutral values, for a material that only sets some.
fn default_surface(in: VertexOutput) -> Surface {
    let base = sample_albedo(in.uv) * tint() * in.tint;
    let mr = sample_metallic_roughness(in.uv);
    var s: Surface;
    s.albedo = base.rgb;
    s.alpha = base.a;
    s.normal = shaded_normal(in);
    s.metallic = mr.x;
    s.roughness = max(mr.y, MIN_ROUGHNESS);
    s.occlusion = sample_occlusion(in.uv);
    s.emissive = sample_emissive(in.uv);
    s.reflectance = 0.5;
    return s;
}

/// GGX / Trowbridge-Reitz: how much of the surface faces the halfway vector.
@const fn distribution(ndoth: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let d = ndoth * ndoth * (a2 - 1.0) + 1.0;
    return a2 / max(PI * d * d, 1e-7);
}

/// Smith's height-correlated visibility, already divided by the `4 n.l n.v`
/// the specular term would otherwise carry.
@const fn visibility(ndotv: f32, ndotl: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let v = ndotl * sqrt(ndotv * ndotv * (1.0 - a2) + a2);
    let l = ndotv * sqrt(ndotl * ndotl * (1.0 - a2) + a2);
    return 0.5 / max(v + l, 1e-7);
}

/// Schlick's approximation of Fresnel reflectance at `vdoth`.
@const fn fresnel(f0: vec3<f32>, vdoth: f32) -> vec3<f32> {
    let f = pow(clamp(1.0 - vdoth, 0.0, 1.0), 5.0);
    return f0 + (vec3<f32>(1.0, 1.0, 1.0) - f0) * f;
}

/// Normal-incidence reflectance of this surface: what a dielectric reflects
/// straight on, or the metal's own colour.
fn reflectance_at_normal(s: Surface) -> vec3<f32> {
    let dielectric = 0.16 * s.reflectance * s.reflectance;
    return mix(vec3<f32>(dielectric, dielectric, dielectric), s.albedo, s.metallic);
}

/// Schlick's Fresnel with roughness folded in: a rough surface cannot brighten
/// at its edge as much as a smooth one, and plain Schlick would say it does.
@const fn fresnel_rough(f0: vec3<f32>, ndotv: f32, roughness: f32) -> vec3<f32> {
    let f = pow(clamp(1.0 - ndotv, 0.0, 1.0), 5.0);
    let ceiling = max(vec3<f32>(1.0 - roughness, 1.0 - roughness, 1.0 - roughness), f0);
    return f0 + (ceiling - f0) * f;
}

/// Karis' fit of the environment BRDF's split-sum term. A lookup table is the
/// exact answer; this is two multiplies and needs no texture.
@const fn env_brdf(f0: vec3<f32>, roughness: f32, ndotv: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * ndotv)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + vec2<f32>(r.z, r.w);
    return f0 * ab.x + vec3<f32>(ab.y, ab.y, ab.y);
}

/// The direction and attenuation one light puts on a point: the geometry half
/// of `package::mesh`'s `contribution`, kept apart so the BRDF can use it.
@const fn light_at(light: Light, world: vec3<f32>) -> vec4<f32> {
    let kind = light.position_kind.w;
    var to_light = -light.direction_radius.xyz;
    var attenuation = 1.0;
    if kind > 0.5 {
        let delta = light.position_kind.xyz - world;
        let distance = length(delta);
        let radius = max(light.direction_radius.w, 1e-4);
        if distance > radius {
            return vec4<f32>(0.0, 0.0, 0.0, 0.0);
        }
        to_light = delta / max(distance, 1e-4);
        let falloff = clamp(1.0 - distance / radius, 0.0, 1.0);
        attenuation = falloff * falloff / (1.0 + distance * distance);
        if kind > 1.5 {
            let theta = dot(-to_light, normalize(light.direction_radius.xyz));
            let inner = light.cone.x;
            let outer = light.cone.y;
            attenuation = attenuation
                * clamp((theta - outer) / max(inner - outer, 1e-4), 0.0, 1.0);
        }
    }
    return vec4<f32>(normalize(to_light), attenuation);
}

/// Light on a surface, split into what it scatters and what it mirrors.
///
/// Opaque shading adds the two straight back together. Glass keeps them
/// apart: the scene refracts through what would have scattered, while the
/// reflections and highlights stay on top of it.
struct Lobes {
    diffuse: vec3<f32>,
    specular: vec3<f32>,
}

/// What one light adds to a surface.
fn radiance(s: Surface, light: Light, world: vec3<f32>, v: vec3<f32>) -> Lobes {
    var out: Lobes;
    out.diffuse = vec3<f32>(0.0, 0.0, 0.0);
    out.specular = vec3<f32>(0.0, 0.0, 0.0);
    let at = light_at(light, world);
    if at.w <= 0.0 {
        return out;
    }
    let l = at.xyz;
    let n = s.normal;
    let ndotl = max(dot(n, l), 0.0);
    if ndotl <= 0.0 {
        return out;
    }
    let h = normalize(l + v);
    let ndotv = max(dot(n, v), 1e-4);
    let ndoth = max(dot(n, h), 0.0);
    let vdoth = max(dot(v, h), 0.0);
    // A metal reflects its own colour and diffuses nothing; a dielectric
    // reflects `reflectance` and diffuses the rest.
    let f0 = reflectance_at_normal(s);
    let f = fresnel(f0, vdoth);
    let energy = light.color_intensity.rgb * light.color_intensity.a * ndotl * at.w;
    out.specular = distribution(ndoth, s.roughness)
        * visibility(ndotv, ndotl, s.roughness)
        * f
        * energy;
    out.diffuse = (vec3<f32>(1.0, 1.0, 1.0) - f)
        * (1.0 - s.metallic)
        * s.albedo
        * INV_PI
        * energy;
    return out;
}

/// The sky and the reflection probes over one surface, dimmed by occlusion.
///
/// A sky replaces `environment.ambient` rather than adding to it: both stand
/// for the same bounced light, and a scene setting each would count it twice.
/// A probe overrides the sky by its own weight, so a surface leaving a
/// probe's box fades back to the sky rather than jumping.
fn ambient_light(s: Surface, in: VertexOutput, v: vec3<f32>, occlusion: f32) -> Lobes {
    let n = s.normal;
    let ndotv = max(dot(n, v), 1e-4);
    let mirrored = reflect(-v, n);
    var irradiance = vec3<f32>(0.0, 0.0, 0.0);
    var reflected = vec3<f32>(0.0, 0.0, 0.0);
    var found = false;
    if environment_lit() {
        let max_lod = environment_max_lod();
        irradiance = environment_sample(n, max_lod);
        reflected = environment_sample(mirrored, s.roughness * max_lod);
        found = true;
    }
    let probe = probe_at(in.world);
    if probe.x >= 0.0 {
        let i = u32(probe.x);
        let lod = probe_max_lod();
        let near_reflection = probe_sample(i, in.world, mirrored, s.roughness * lod);
        let near_irradiance = probe_sample(i, in.world, n, lod);
        if found {
            reflected = mix(reflected, near_reflection, probe.y);
            irradiance = mix(irradiance, near_irradiance, probe.y);
        } else {
            reflected = near_reflection * probe.y;
            irradiance = near_irradiance * probe.y;
        }
        found = true;
    }
    // A mirror is the same reflection rendered exactly rather than sampled
    // from a map, so it replaces what the sky or a probe would have shown
    // wherever its own picture reaches.
    let mirror = mirror_at(in, n);
    if mirror.a > 0.0 {
        reflected = mix(reflected, mirror.rgb, mirror.a);
        found = true;
    }
    var out: Lobes;
    if !found {
        out.diffuse = frame.ambient_count.rgb * s.albedo * occlusion;
        out.specular = vec3<f32>(0.0, 0.0, 0.0);
        return out;
    }
    let f0 = reflectance_at_normal(s);
    let f = fresnel_rough(f0, ndotv, s.roughness);
    let diffuse_share = (vec3<f32>(1.0, 1.0, 1.0) - f) * (1.0 - s.metallic);
    out.diffuse = diffuse_share * irradiance * s.albedo * occlusion;
    out.specular = reflected * env_brdf(f0, s.roughness, ndotv) * occlusion;
    return out;
}

/// The surface's own occlusion map times what the frame's prepass measured.
/// Both are one where nothing is bound, so this is the map alone without a
/// prepass and the prepass alone without a map.
fn occlusion_at(s: Surface, in: VertexOutput) -> f32 {
    return s.occlusion * screen_occlusion(in);
}

/// Everything falling on a surface, apart from its own emission: the
/// surroundings and every light in the frame, still split.
fn shading(s: Surface, in: VertexOutput) -> Lobes {
    let v = normalize(eye() - in.world);
    var out = ambient_light(s, in, v, occlusion_at(s, in));
    let count = u32(frame.ambient_count.w);
    for (var i = 0u; i < count; i = i + 1u) {
        let from_light = radiance(s, frame.lights[i], in.world, v);
        out.diffuse = out.diffuse + from_light.diffuse;
        out.specular = out.specular + from_light.specular;
    }
    return out;
}

/// Every light in the frame over one surface, plus the surroundings and
/// emission.
fn lit(s: Surface, in: VertexOutput) -> vec3<f32> {
    let total = shading(s, in);
    return total.diffuse + total.specular + s.emissive;
}

/// The whole physically based look: maps, lights, the surroundings, emission
/// and fog.
fn shade_pbr(in: VertexOutput, s: Surface) -> vec4<f32> {
    clip(in);
    return vec4<f32>(apply_fog(lit(s, in), in.world), s.alpha);
}

/// The look a material gets for free by importing this and calling one thing.
fn shade(in: VertexOutput) -> vec4<f32> {
    return shade_pbr(in, default_surface(in));
}

/// A refractive surface: how much of the scene passes through it, how sharply
/// it bends on the way, and what its volume takes out.
///
/// The same numbers a material's `[surface]` table sets, which is what puts
/// the node in the refraction pass in the first place. A shader reading them
/// from its own `Params` and a scene setting them there must agree, or the
/// node draws as glass and shades as though it were not.
struct Glass {
    transmission: f32,
    ior: f32,
    /// How far light travels inside it, in world units. Zero refracts without
    /// tinting, which is what a single pane wants.
    thickness: f32,
    /// What is left of white light after `attenuation_distance` inside it.
    attenuation_color: vec3<f32>,
    /// Zero takes nothing out however thick the glass.
    attenuation_distance: f32,
}

/// Clear window glass: it refracts, and its volume takes nothing out.
fn default_glass() -> Glass {
    var g: Glass;
    g.transmission = 1.0;
    g.ior = 1.5;
    g.thickness = 0.0;
    g.attenuation_color = vec3<f32>(1.0, 1.0, 1.0);
    g.attenuation_distance = 0.0;
    return g;
}

/// The scene behind a refractive surface, bent by its index of refraction and
/// dimmed by the distance light travelled inside it.
///
/// The offset is anchored on the fragment's own pixel rather than on a
/// reprojected surface point, so a thickness of zero passes the scene through
/// exactly and refraction stays registered with it at any angle.
fn refracted(in: VertexOutput, s: Surface, glass: Glass) -> vec3<f32> {
    let v = normalize(eye() - in.world);
    var bent = refract(-v, s.normal, 1.0 / max(glass.ior, 1.0));
    if dot(bent, bent) < 1e-6 {
        // Past the critical angle nothing passes through, so the ray turns
        // back the way it came rather than leaving the surface undefined.
        bent = -v;
    }
    let surface_clip = frame.proj * (frame.view * vec4<f32>(in.world, 1.0));
    let exit_clip = frame.proj * (frame.view * vec4<f32>(in.world + bent * glass.thickness, 1.0));
    let at_surface = surface_clip.xy / max(surface_clip.w, 1e-4);
    let at_exit = exit_clip.xy / max(exit_clip.w, 1e-4);
    // Clip space runs up the screen and a texture runs down it, so y flips.
    let slide = vec2<f32>(at_exit.x - at_surface.x, at_surface.y - at_exit.y) * 0.5;
    var through = behind(screen_uv(in) + slide, s.roughness * behind_max_lod());
    if glass.attenuation_distance > 0.0 {
        let tint = max(glass.attenuation_color, vec3<f32>(1e-3, 1e-3, 1e-3));
        let absorbed = -log(tint) / glass.attenuation_distance;
        through = through * exp(-absorbed * glass.thickness);
    }
    return through;
}

/// The whole look of a pane: the scene refracted through it where it is
/// transmissive and seen head-on, its own reflections and highlights on top.
///
/// Raising `reflectance` brightens what it mirrors, so the sky shows on the
/// glass more; at a glancing angle Fresnel keeps the reflection and lets
/// almost nothing through, which is what a window does.
fn shade_glass(in: VertexOutput, s: Surface, glass: Glass) -> vec4<f32> {
    clip(in);
    let total = shading(s, in);
    let v = normalize(eye() - in.world);
    let ndotv = max(dot(s.normal, v), 1e-4);
    let f = fresnel_rough(reflectance_at_normal(s), ndotv, s.roughness);
    let mirrored = clamp(max(f.r, max(f.g, f.b)), 0.0, 1.0);
    let through = refracted(in, s, glass) * (1.0 - mirrored);
    let body = mix(total.diffuse, through, clamp(glass.transmission, 0.0, 1.0));
    let color = body + total.specular + s.emissive;
    // Opaque: a pane draws into the resolved scene, and what shows through it
    // is the refraction rather than the blend.
    return vec4<f32>(apply_fog(color, in.world), 1.0);
}
