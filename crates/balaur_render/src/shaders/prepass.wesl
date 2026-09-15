// The geometry pass every 3D material draws before the picture: where each
// surface is, which way it faces, and how it reflects.
//
// The engine owns this rather than the material, because what the pass wants
// is the shape a material happens to be painting, not the paint. A material
// that displaces its vertices still lands here correctly: the vertex work is
// `package::mesh`'s own, the same one its colour pass uses.
//
// Screen-space occlusion and depth of field read the first target, the
// refraction pass tests against the depth this writes, and screen-space
// reflections read the rest. `pipeline.rs::prepass_pipeline` declares the
// four targets in this order.

import package::mesh::{VertexInput, VertexOutput, vertex, frame};

struct PrepassOutput {
    // The point in view space, with w marking that a surface is here at all.
    @location(0) view_position: vec4<f32>,
    @location(1) normal_roughness: vec4<f32>,
    @location(2) reflectance_metallic: vec4<f32>,
    @location(3) reflection: vec4<f32>,
}

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in);
}

@fragment fn fs_main(in: VertexOutput) -> PrepassOutput {
    var out: PrepassOutput;
    out.view_position = vec4<f32>((frame.view * vec4<f32>(in.world, 1.0)).xyz, 1.0);
    // Fully rough and non-metallic: the surface's own numbers live in its
    // `Params`, which this pass does not bind. Occlusion, depth of field and
    // the refraction depth do not read them; screen-space reflections do, and
    // so read a Balaur material as a diffuse surface.
    out.normal_roughness = vec4<f32>(normalize(in.normal), 1.0);
    out.reflectance_metallic = vec4<f32>(0.04, 0.04, 0.04, 0.0);
    out.reflection = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    return out;
}
