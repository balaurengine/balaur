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
    shaded_normal,
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

/// What one light adds to a surface: diffuse and specular together.
fn radiance(s: Surface, light: Light, world: vec3<f32>, v: vec3<f32>) -> vec3<f32> {
    let at = light_at(light, world);
    if at.w <= 0.0 {
        return vec3<f32>(0.0, 0.0, 0.0);
    }
    let l = at.xyz;
    let n = s.normal;
    let ndotl = max(dot(n, l), 0.0);
    if ndotl <= 0.0 {
        return vec3<f32>(0.0, 0.0, 0.0);
    }
    let h = normalize(l + v);
    let ndotv = max(dot(n, v), 1e-4);
    let ndoth = max(dot(n, h), 0.0);
    let vdoth = max(dot(v, h), 0.0);
    // A metal reflects its own colour and diffuses nothing; a dielectric
    // reflects `reflectance` and diffuses the rest.
    let dielectric = 0.16 * s.reflectance * s.reflectance;
    let f0 = mix(vec3<f32>(dielectric, dielectric, dielectric), s.albedo, s.metallic);
    let f = fresnel(f0, vdoth);
    let specular = distribution(ndoth, s.roughness)
        * visibility(ndotv, ndotl, s.roughness)
        * f;
    let diffuse = (vec3<f32>(1.0, 1.0, 1.0) - f)
        * (1.0 - s.metallic)
        * s.albedo
        * INV_PI;
    let energy = light.color_intensity.rgb * light.color_intensity.a * ndotl * at.w;
    return (diffuse + specular) * energy;
}

/// Every light in the frame over one surface, plus ambient and emission.
fn lit(s: Surface, world: vec3<f32>) -> vec3<f32> {
    let v = normalize(eye() - world);
    // Ambient stands in for the light the scene bounces; occlusion is what
    // says where it would not have reached.
    var total = frame.ambient_count.rgb * s.albedo * s.occlusion;
    let count = u32(frame.ambient_count.w);
    for (var i = 0u; i < count; i = i + 1u) {
        total = total + radiance(s, frame.lights[i], world, v);
    }
    return total + s.emissive;
}

/// The whole physically based look: maps, lights, emission and fog.
fn shade_pbr(in: VertexOutput, s: Surface) -> vec4<f32> {
    return vec4<f32>(apply_fog(lit(s, in.world), in.world), s.alpha);
}

/// The look a material gets for free by importing this and calling one thing.
fn shade(in: VertexOutput) -> vec4<f32> {
    return shade_pbr(in, default_surface(in));
}
