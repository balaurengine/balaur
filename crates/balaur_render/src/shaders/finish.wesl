// The finishing passes the engine ships: vignette, chromatic aberration,
// grain and pixelation. One shader, one variant per pass, picked by the
// feature the pass name sets.
//
// Each is a dozen lines over the resolved frame, which is what makes them the
// first `camera.post` materials rather than passes in the pipeline. A project
// that wants a fifth writes it the same way; nothing here is privileged.

import package::post::{PostInput, fullscreen, frame, size, time};

struct Params {
    // How dark the corners go, and how round the darkening is.
    vignette_amount: f32,
    vignette_roundness: f32,
    // How far the red and blue channels slide apart at the frame's edge, as
    // a fraction of the frame.
    aberration_amount: f32,
    // How much the grain lightens and darkens a pixel.
    grain_amount: f32,
    // The side of one block, in pixels. Below one leaves the frame alone.
    pixelate_size: f32,
}

@group(1) @binding(0) var<uniform> params: Params;

// A value in 0..1 from a point, with no pattern a viewer can read. The
// constants are the usual ones for this hash; nothing about them is derived.
@const fn hash(at: vec2<f32>) -> f32 {
    return fract(sin(dot(at, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

// Darker toward the corners. `roundness` at one is a circle whatever shape
// the frame is; at zero it follows the frame, so a wide screen gets a wide
// oval rather than a circle with dark bands down the sides.
@if(vignette) fn finish(in: PostInput) -> vec4<f32> {
    let dimensions = size();
    let shape = mix(
        vec2<f32>(dimensions.x / dimensions.y, 1.0),
        vec2<f32>(1.0, 1.0),
        clamp(params.vignette_roundness, 0.0, 1.0),
    );
    let offset = (in.uv - 0.5) * 2.0 * shape;
    let falloff = smoothstep(0.5, 1.4, length(offset));
    let color = frame(in.uv);
    return vec4<f32>(color.rgb * (1.0 - params.vignette_amount * falloff), color.a);
}

// Red and blue pulled apart along the line from the centre, the way a lens
// that does not focus every wavelength at the same place pulls them.
@if(aberration) fn finish(in: PostInput) -> vec4<f32> {
    let slide = (in.uv - 0.5) * params.aberration_amount;
    let middle = frame(in.uv);
    return vec4<f32>(
        frame(in.uv - slide).r,
        middle.g,
        frame(in.uv + slide).b,
        middle.a,
    );
}

// Film grain: one hashed value per pixel per second, added around zero so it
// darkens as much as it lightens and the picture keeps its brightness.
@if(grain) fn finish(in: PostInput) -> vec4<f32> {
    let color = frame(in.uv);
    let at = in.uv * size() + fract(time()) * 1000.0;
    let noise = hash(at) - 0.5;
    return vec4<f32>(color.rgb + noise * params.grain_amount, color.a);
}

// The frame read back one block at a time, sampled at each block's middle so
// a block takes one colour rather than smearing across its edge.
@if(pixelate) fn finish(in: PostInput) -> vec4<f32> {
    let side = max(params.pixelate_size, 1.0);
    let dimensions = size();
    let block = (floor(in.uv * dimensions / side) + 0.5) * side;
    return frame(block / dimensions);
}

@vertex fn vs_main(@builtin(vertex_index) index: u32) -> PostInput {
    return fullscreen(index);
}

@fragment fn fs_main(in: PostInput) -> @location(0) vec4<f32> {
    return finish(in);
}
