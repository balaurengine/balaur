// The contract a `material` shader draws 2D against: the uniforms Balaur's
// pipeline binds, and the vertex work every sprite shader would repeat.
import package::common::unpack_mat3;

struct FrameUniforms {
    view_0: vec4<f32>, view_1: vec4<f32>, view_2: vec4<f32>,
    proj_0: vec4<f32>, proj_1: vec4<f32>, proj_2: vec4<f32>,
    // x is seconds since the run began; y and z are the viewport in pixels.
    clock: vec4<f32>,
}
struct ObjectUniforms {
    model_0: vec4<f32>, model_1: vec4<f32>, model_2: vec4<f32>,
    scale_0: vec4<f32>, scale_1: vec4<f32>,
    color: vec4<f32>,
}
@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(1) @binding(0) var<uniform> object: ObjectUniforms;
@group(2) @binding(0) var albedo_texture: texture_2d<f32>;
@group(2) @binding(1) var albedo_sampler: sampler;
// The frame so far, in linear light, bound when the material's `features`
// has `screen`: everything drawn beneath this object this frame.
@if(screen) @group(2) @binding(2) var screen_texture: texture_2d<f32>;
@if(screen) @group(2) @binding(3) var screen_sampler: sampler;
// Images the material binds itself, under the `[params]` keys `texture_1` to
// `texture_4`; a slot it leaves out reads as opaque white.
@group(2) @binding(4) var texture_1: texture_2d<f32>;
@group(2) @binding(5) var sampler_1: sampler;
@group(2) @binding(6) var texture_2: texture_2d<f32>;
@group(2) @binding(7) var sampler_2: sampler;
@group(2) @binding(8) var texture_3: texture_2d<f32>;
@group(2) @binding(9) var sampler_3: sampler;
@group(2) @binding(10) var texture_4: texture_2d<f32>;
@group(2) @binding(11) var sampler_4: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) instance_position: vec2<f32>,
    @location(3) instance_color: vec4<f32>,
    @location(4) deformation_0: vec2<f32>,
    @location(5) deformation_1: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world: vec2<f32>,
}

// Seconds since the run began. Render time, never the simulation tick: a
// shader observes and may not feed anything back.
fn time() -> f32 {
    return frame.clock.x;
}

fn sample_albedo(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(albedo_texture, albedo_sampler, uv);
}

// Where a fragment sits on the screen, 0..1 from the top left: what
// `sample_screen` takes. `position` is the fragment's `@builtin(position)`.
fn screen_uv(position: vec4<f32>) -> vec2<f32> {
    return position.xy / frame.clock.yz;
}

// The frame so far, under this fragment or wherever `uv` points; only with
// `features = { screen = true }`.
@if(screen) fn sample_screen(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(screen_texture, screen_sampler, uv);
}

// One texel of the bound texture, in UV units: what an outline or a blur
// steps by, and what another engine spells TEXTURE_PIXEL_SIZE.
fn texture_pixel_size() -> vec2<f32> {
    return vec2<f32>(1.0, 1.0) / vec2<f32>(textureDimensions(albedo_texture));
}

// One pixel of the frame, in the units `screen_uv` answers in.
fn screen_pixel_size() -> vec2<f32> {
    return vec2<f32>(1.0, 1.0) / frame.clock.yz;
}

// The node's tint times the instance's own.
fn tint(in: VertexOutput) -> vec4<f32> {
    return object.color * in.color;
}

// Where a vertex lands, with `offset` displacing it in the node's own space
// before any of the transforms apply.
fn place(in: VertexInput, offset: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    let scale = mat2x2<f32>(object.scale_0.xy, object.scale_1.xy);
    let deformation = mat2x2<f32>(in.deformation_0, in.deformation_1);
    let deformed = deformation * (scale * (in.position + offset));
    let model = unpack_mat3(object.model_0, object.model_1, object.model_2);
    let world = vec3<f32>(in.instance_position, 0.0) + model * vec3<f32>(deformed, 1.0);
    let view = unpack_mat3(frame.view_0, frame.view_1, frame.view_2);
    let proj = unpack_mat3(frame.proj_0, frame.proj_1, frame.proj_2);
    var projected = proj * view * world;
    projected.z = 0.0;
    out.clip_position = vec4<f32>(projected, 1.0);
    out.uv = in.uv;
    out.color = in.instance_color;
    out.world = world.xy;
    return out;
}

// Where a vertex sits in its node's own space, in pixels at `ppu` to the
// unit and y down: what another engine spells VERTEX.
fn vertex_pixels(in: VertexInput, ppu: f32) -> vec2<f32> {
    let scale = vec2<f32>(object.scale_0.x, object.scale_1.y);
    return scale * in.position * vec2<f32>(ppu, -ppu);
}

// The `offset` `place` takes for a displacement measured the way
// `vertex_pixels` measures.
fn pixels_to_offset(delta: vec2<f32>, ppu: f32) -> vec2<f32> {
    let scale = vec2<f32>(object.scale_0.x, object.scale_1.y);
    return vec2<f32>(delta.x, -delta.y) / (scale * ppu);
}

// The node's place in the world in those same pixels, y down, as a 4x4:
// what another engine spells MODEL_MATRIX.
fn model_matrix_pixels(ppu: f32) -> mat4x4<f32> {
    let model = unpack_mat3(object.model_0, object.model_1, object.model_2);
    return mat4x4<f32>(
        vec4<f32>(model[0].x, -model[0].y, 0.0, 0.0),
        vec4<f32>(-model[1].x, model[1].y, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 1.0, 0.0),
        vec4<f32>(model[2].x * ppu, -model[2].y * ppu, 0.0, 1.0),
    );
}

// The vertex stage a shader that only changes colour writes.
fn vertex(in: VertexInput) -> VertexOutput {
    return place(in, vec2<f32>(0.0, 0.0));
}
