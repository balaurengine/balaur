// A physically based surface: the whole BRDF comes from `package::pbr`, and
// this file only says how metal and how rough.

import package::mesh::{VertexInput, VertexOutput, vertex};
import package::pbr::{default_surface, shade_pbr};

struct Params {
    metallic: f32,
    roughness: f32,
    reflectance: f32,
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
    return shade_pbr(in, surface);
}
