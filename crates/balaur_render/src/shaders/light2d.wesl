// The 2D light map: lights accumulate additively into an offscreen target,
// shadow polygons mask them out through the stencil, and one full-screen
// draw multiplies the frame by what came out. `FrameUniforms` must match the
// struct of that name in `light_map.rs` field for field, and `LightInstance`
// must match the instance attributes the light pipeline declares.
import package::common::unpack_mat3;

struct FrameUniforms {
    view_0: vec4<f32>, view_1: vec4<f32>, view_2: vec4<f32>,
    proj_0: vec4<f32>, proj_1: vec4<f32>, proj_2: vec4<f32>,
    // ambient.rgb, unused
    ambient: vec4<f32>,
}
@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(1) @binding(0) var t_light: texture_2d<f32>;
@group(1) @binding(1) var s_light: sampler;

fn to_clip(world: vec2<f32>) -> vec4<f32> {
    let view = unpack_mat3(frame.view_0, frame.view_1, frame.view_2);
    let proj = unpack_mat3(frame.proj_0, frame.proj_1, frame.proj_2);
    let projected = proj * view * vec3<f32>(world, 1.0);
    return vec4<f32>(projected.xy, 0.0, 1.0);
}

// The corner of a unit quad a vertex index names, as two triangles.
fn quad_corner(index: u32) -> vec2<f32> {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    return corners[index];
}

// One light, as an instance: its quad is generated from the corner index.
struct LightInstance {
    @location(0) position: vec2<f32>,
    // radius in world units, then 0 for a point light and 1 for directional
    @location(1) radius_kind: vec2<f32>,
    @location(2) color: vec3<f32>,
    @location(3) intensity: f32,
}

struct LightVertex {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world: vec2<f32>,
    @location(1) light: vec2<f32>,
    @location(2) radius_kind: vec2<f32>,
    @location(3) radiance: vec3<f32>,
}

@vertex
fn vs_light(@builtin(vertex_index) index: u32, light: LightInstance) -> LightVertex {
    var out: LightVertex;
    let corner = quad_corner(index);
    out.light = light.position;
    out.radius_kind = light.radius_kind;
    out.radiance = light.color * light.intensity;
    if light.radius_kind.y > 0.5 {
        // A directional light reaches everywhere, so it covers the viewport
        // rather than a patch of the world.
        out.clip_position = vec4<f32>(corner, 0.0, 1.0);
        out.world = light.position;
    } else {
        let world = light.position + corner * light.radius_kind.x;
        out.clip_position = to_clip(world);
        out.world = world;
    }
    return out;
}

@fragment
fn fs_light(in: LightVertex) -> @location(0) vec4<f32> {
    if in.radius_kind.y > 0.5 {
        return vec4<f32>(in.radiance, 1.0);
    }
    let radius = max(in.radius_kind.x, 1e-6);
    let distance = length(in.world - in.light);
    // Quadratic falloff that reaches exactly zero at the radius, so a light's
    // edge has no seam where its quad ends.
    let falloff = clamp(1.0 - distance / radius, 0.0, 1.0);
    return vec4<f32>(in.radiance * falloff * falloff, 1.0);
}

// Shadow polygons write the stencil and nothing else; the light draw that
// follows is rejected wherever they landed.
@vertex
fn vs_shadow(@location(0) position: vec2<f32>) -> @builtin(position) vec4<f32> {
    return to_clip(position);
}

@fragment
fn fs_shadow() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}

struct CompositeVertex {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_composite(@builtin(vertex_index) index: u32) -> CompositeVertex {
    var out: CompositeVertex;
    let corner = quad_corner(index);
    out.clip_position = vec4<f32>(corner, 0.0, 1.0);
    // Clip space is y-up and texture space y-down.
    out.uv = vec2<f32>(corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5);
    return out;
}

@fragment
fn fs_composite(in: CompositeVertex) -> @location(0) vec4<f32> {
    let lit = textureSample(t_light, s_light, in.uv).rgb;
    return vec4<f32>(frame.ambient.rgb + lit, 1.0);
}
