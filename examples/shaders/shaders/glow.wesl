// The logo with rings of light rolling out from its centre. Every step is a
// typed `let`, so the editor's gutter can draw any of them.
import package::sprite::{VertexInput, VertexOutput, vertex, sample_albedo, tint, time};

struct Params { speed: f32, rings: f32, tint: vec4<f32> }
@group(3) @binding(0) var<uniform> params: Params;

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in);
}

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv: vec2<f32> = in.uv;
    let centred: vec2<f32> = uv - vec2<f32>(0.5, 0.5);
    let dist: f32 = length(centred) * 2.0;
    let wave: f32 = 0.5 + 0.5 * sin(dist * params.rings * 6.2832 - time() * params.speed);
    let ramp: vec4<f32> = params.tint * wave * wave;
    let albedo: vec4<f32> = sample_albedo(uv) * tint(in);
    return albedo + ramp * albedo.a;
}
