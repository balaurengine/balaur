// Text with an `alpha_cutoff` in the 2D pass. kiss3d's 2D material blends
// every pixel, so the mask its 3D material draws is drawn here instead: a
// pixel below the cutoff is dropped and the rest is opaque.
import package::sprite::{VertexInput, VertexOutput, vertex, sample_albedo, tint};

struct Params { cutoff: f32 }
@group(3) @binding(0) var<uniform> params: Params;

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in);
}

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let c = sample_albedo(in.uv) * tint(in);
    if c.a < params.cutoff {
        discard;
    }
    return vec4<f32>(c.rgb, 1.0);
}
