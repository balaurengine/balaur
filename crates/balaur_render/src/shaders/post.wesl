// What a post-process material imports: the frame it reads, and the geometry
// that covers the screen with it.
//
// A pass listed on `camera.post` before `tonemap` reads the HDR film in linear
// light; one after it reads the tonemapped picture. The shader is the same
// either way -- what differs is what `frame` hands back.

struct PostInput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// A post pass has no mesh, no node and no texture of its own, so its own
// `Params` go at `@group(1)` -- not at `@group(3)`, where a material drawn on
// a node keeps them behind the frame, object and texture groups.
@group(0) @binding(0) var frame_texture: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;

// One oversized triangle rather than two: a quad's diagonal is a seam where
// interpolation differs either side of it.
fn fullscreen(index: u32) -> PostInput {
    var out: PostInput;
    let x = f32(i32(index) / 2) * 4.0 - 1.0;
    let y = f32(i32(index) & 1) * 4.0 - 1.0;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, 1.0 - (y + 1.0) * 0.5);
    return out;
}

/// The frame this pass was handed, at a texture coordinate.
fn frame(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(frame_texture, frame_sampler, uv);
}

/// The size of one texel, for a pass that reads its neighbours.
fn texel() -> vec2<f32> {
    return 1.0 / vec2<f32>(textureDimensions(frame_texture));
}
