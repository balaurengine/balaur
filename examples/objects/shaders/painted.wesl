// A mesh drawn in the colours its vertices carry. `features = { vertex_color
// = true }` is what puts the attribute on the pipeline; without it the
// contract has no such input and this shader would not link.
import package::mesh::{VertexInput, VertexOutput, vertex, vertex_color, shade};

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput = vertex(in);
    out.tint = vertex_color(in);
    return out;
}

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return shade(in);
}
