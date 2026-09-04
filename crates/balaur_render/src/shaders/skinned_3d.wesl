// GPU skinning for 3D meshes, drawn against the same contract a project's
// material uses (`package::mesh`), so a skinned mesh is lit and fogged the
// way every other one is. `MAX_JOINTS` must match `skinned_3d.rs`.
import package::mesh::{VertexOutput, place_deformed, shade};

const MAX_JOINTS: u32 = 128u;

struct SkinUniforms {
    // Padded with identities, so a joint the rig never filled leaves the
    // vertex where it was authored — the same answer the CPU path gives.
    joints: array<mat4x4<f32>, 128>,
}
@group(3) @binding(0) var<uniform> skin: SkinUniforms;

struct SkinnedVertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) joints: vec4<u32>,
    @location(4) weights: vec4<f32>,
}

// Blends four joint matrices by their weights, the way
// `balaur_core::skeleton::blend_3d` does: weights summing to zero leave the
// value alone, which is what an unskinned vertex wants.
@const fn blend(
    a: mat4x4<f32>,
    b: mat4x4<f32>,
    c: mat4x4<f32>,
    d: mat4x4<f32>,
    weights: vec4<f32>,
    value: vec4<f32>,
) -> vec4<f32> {
    let total = weights.x + weights.y + weights.z + weights.w;
    if total <= 0.0 {
        return value;
    }
    return weights.x * (a * value)
        + weights.y * (b * value)
        + weights.z * (c * value)
        + weights.w * (d * value);
}

fn joint(index: u32) -> mat4x4<f32> {
    return skin.joints[min(index, MAX_JOINTS - 1u)];
}

@vertex
fn vs_main(in: SkinnedVertex) -> VertexOutput {
    let a = joint(in.joints.x);
    let b = joint(in.joints.y);
    let c = joint(in.joints.z);
    let d = joint(in.joints.w);
    let position = blend(a, b, c, d, in.weights, vec4<f32>(in.position, 1.0)).xyz;
    // w = 0 drops the translation, so a normal is rotated and not moved.
    let normal = blend(a, b, c, d, in.weights, vec4<f32>(in.normal, 0.0)).xyz;
    // `normalize_or_zero`, as the CPU path spells it: a collapsed normal is
    // zero rather than NaN.
    let square = dot(normal, normal);
    let unit = select(vec3<f32>(0.0, 0.0, 0.0), normal * inverseSqrt(max(square, 1e-20)), square > 0.0);
    return place_deformed(position, unit, in.uv);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return shade(in);
}
