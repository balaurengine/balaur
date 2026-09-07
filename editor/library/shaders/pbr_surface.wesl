// The stock physically based surface. Every material in the library draws
// with this and sets only the numbers it cares about.

import package::mesh::{VertexInput, VertexOutput, vertex};
import package::pbr::{default_surface, shade_pbr};

struct Params {
    metallic: f32,
    roughness: f32,
    reflectance: f32,
    emissive: f32,
}

@group(3) @binding(0) var<uniform> params: Params;

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in);
}

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var surface = default_surface(in);
    surface.metallic = params.metallic;
    surface.roughness = max(params.roughness, 0.045);
    surface.reflectance = params.reflectance;
    surface.emissive = surface.emissive + surface.albedo * params.emissive;
    return shade_pbr(in, surface);
}
