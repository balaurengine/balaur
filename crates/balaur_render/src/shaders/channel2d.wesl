// The 2D counterpart of `channel.wesl`. A sprite has no normals, so the
// channels it can answer for are where it is and what it is textured with.
import package::sprite::{VertexInput, VertexOutput, vertex, sample_albedo, tint};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in);
}

@if(uv)
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.uv, 0.0, 1.0);
}

@if(albedo)
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return sample_albedo(in.uv) * tint(in);
}

// Nothing 2D has a normal or a distance; the node's own colour stands in so
// the view is not simply black.
@if(normals)
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return tint(in);
}

@if(depth)
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return tint(in);
}
