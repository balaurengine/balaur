// What the viewport draws instead of colour when a channel view is on. One
// entry point per channel, picked by the feature the backend links with.
import package::mesh::{VertexInput, VertexOutput, vertex, eye, sample_albedo, tint};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in);
}

// Normals into the unit cube, so -1 is black and +1 is white per axis.
@if(normals)
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(normalize(in.normal) * 0.5 + vec3<f32>(0.5, 0.5, 0.5), 1.0);
}

@if(uv)
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.uv, 0.0, 1.0);
}

// Distance from the camera, curved so near geometry keeps its contrast
// rather than crushing everything to black.
@if(depth)
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let grey = 1.0 - exp(-length(in.world - eye()) * 0.05);
    return vec4<f32>(grey, grey, grey, 1.0);
}

// The texture alone, with no lighting on it.
@if(albedo)
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return sample_albedo(in.uv) * tint();
}
