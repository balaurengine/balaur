// What `balaur import` writes beside a model: the stock surface a glTF
// material becomes.
//
// glTF states every surface value as a factor times a map, where a map the
// file did not name stands at one. The contract's stand-in for an absent map
// is a neutral surface instead — mid roughness, no metal, no emission — which
// is right for a material somebody authored by hand and wrong for one whose
// numbers came out of a file. So each map is a feature: the variant the file
// named reads the map, and the variant it did not reads the factor alone.

import package::mesh::{VertexInput, VertexOutput, vertex};
import package::pbr::{default_surface, default_glass, shade_pbr, shade_glass};

struct Params {
    base_color: vec4<f32>,
    emissive: vec4<f32>,
    // rgb what is left of white light after travelling `a` units of glass.
    attenuation: vec4<f32>,
    metallic: f32,
    roughness: f32,
    reflectance: f32,
    // A fragment fainter than this is dropped rather than drawn: glTF's
    // `MASK`. Zero keeps every fragment, which is `OPAQUE` and `BLEND`.
    alpha_cutoff: f32,
    transmission: f32,
    ior: f32,
    thickness: f32,
}

@group(3) @binding(0) var<uniform> params: Params;

@if(metallic_roughness_map) @const fn mapped_metallic_roughness(from_map: vec2<f32>) -> vec2<f32> {
    return from_map;
}

@if(!metallic_roughness_map) @const fn mapped_metallic_roughness(from_map: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(1.0, 1.0);
}

@if(emissive_map) @const fn mapped_emissive(from_map: vec3<f32>) -> vec3<f32> {
    return from_map;
}

@if(!emissive_map) @const fn mapped_emissive(from_map: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(1.0, 1.0, 1.0);
}

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in);
}

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var s = default_surface(in);
    s.albedo = s.albedo * params.base_color.rgb;
    s.alpha = s.alpha * params.base_color.a;
    if s.alpha < params.alpha_cutoff {
        discard;
    }
    let mr = mapped_metallic_roughness(vec2<f32>(s.metallic, s.roughness))
        * vec2<f32>(params.metallic, params.roughness);
    s.metallic = mr.x;
    s.roughness = max(mr.y, 0.045);
    s.emissive = mapped_emissive(s.emissive) * params.emissive.rgb;
    s.reflectance = params.reflectance;
    // The same number that put this node in the refraction pass. Constant
    // for every fragment a material draws, so the branch costs nothing.
    if params.transmission > 0.0 {
        var g = default_glass();
        g.transmission = params.transmission;
        g.ior = params.ior;
        g.thickness = params.thickness;
        g.attenuation_color = params.attenuation.rgb;
        g.attenuation_distance = params.attenuation.a;
        return shade_glass(in, s, g);
    }
    return shade_pbr(in, s);
}
