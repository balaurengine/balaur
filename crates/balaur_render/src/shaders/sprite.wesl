// The contract a `material` shader draws 2D against: the uniforms Balaur's
// pipeline binds, and the vertex work every sprite shader would repeat.
import package::common::unpack_mat3;

struct FrameUniforms {
    view_0: vec4<f32>, view_1: vec4<f32>, view_2: vec4<f32>,
    proj_0: vec4<f32>, proj_1: vec4<f32>, proj_2: vec4<f32>,
    // x is seconds since the run began; the rest is spare.
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

// The vertex stage a shader that only changes colour writes.
fn vertex(in: VertexInput) -> VertexOutput {
    return place(in, vec2<f32>(0.0, 0.0));
}
