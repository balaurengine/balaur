// The contract a `material` shader draws 3D against: the uniforms Balaur's
// mesh pipeline binds, and the shading every such shader would repeat.
// `MAX_LIGHTS` must match `shader_material_3d.rs`.

const MAX_LIGHTS: u32 = 16u;

// Lambert's normalisation. A light's intensity is radiance, and the engine's
// own default is 3.0, which reads as white without this.
const INV_PI: f32 = 0.31830988;

struct Light {
    // xyz position, w kind: 0 directional, 1 point, 2 spot.
    position_kind: vec4<f32>,
    // xyz direction, w the distance past which the light contributes nothing.
    direction_radius: vec4<f32>,
    // rgb colour, a intensity.
    color_intensity: vec4<f32>,
    // cos(inner cone), cos(outer cone), then spare.
    cone: vec4<f32>,
}

struct FrameUniforms {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    // xyz the camera, w seconds since the run began.
    eye_clock: vec4<f32>,
    // rgb ambient light, w how many of `lights` are live.
    ambient_count: vec4<f32>,
    fog_color: vec4<f32>,
    // mode (0 off, 1 linear, 2 exponential, 3 exponential squared), then
    // start/density, end, and the height falloff.
    fog: vec4<f32>,
    lights: array<Light, 16>,
}

struct ObjectUniforms {
    model: mat4x4<f32>,
    // The model's inverse transpose, so a scaled node's normals stay normal.
    normal_matrix: mat4x4<f32>,
    color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(1) @binding(0) var<uniform> object: ObjectUniforms;
// The six texture slots the contract declares, in the order `TEXTURE_SLOTS`
// names them. Every slot a material leaves out is bound to a one-pixel
// neutral stand-in, so nothing here branches on absence.
@group(2) @binding(0) var albedo_texture: texture_2d<f32>;
@group(2) @binding(1) var albedo_sampler: sampler;
@group(2) @binding(2) var normal_texture: texture_2d<f32>;
@group(2) @binding(3) var normal_sampler: sampler;
@group(2) @binding(4) var metallic_roughness_texture: texture_2d<f32>;
@group(2) @binding(5) var metallic_roughness_sampler: sampler;
@group(2) @binding(6) var occlusion_texture: texture_2d<f32>;
@group(2) @binding(7) var occlusion_sampler: sampler;
@group(2) @binding(8) var emissive_texture: texture_2d<f32>;
@group(2) @binding(9) var emissive_sampler: sampler;
@group(2) @binding(10) var height_texture: texture_2d<f32>;
@group(2) @binding(11) var height_sampler: sampler;

struct VertexInput {
    // Which copy this is, when something is drawing the mesh many times.
    @builtin(instance_index) copy: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    // Where this copy sits, what colour it draws, and the 3x3 it is turned
    // and scaled by. A mesh drawn once gets the identity and white, so a
    // material need not know the difference.
    @location(3) copy_offset: vec3<f32>,
    @location(4) copy_color: vec4<f32>,
    @location(5) copy_x: vec3<f32>,
    @location(6) copy_y: vec3<f32>,
    @location(7) copy_z: vec3<f32>,
    // One colour per vertex -- what a glTF's COLOR_0 carries -- bound only
    // with `features = { vertex_color = true }`.
    @if(vertex_color) @location(8) vertex_tint: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) world: vec3<f32>,
    // What `shade` multiplies the albedo by, over and above the node's own
    // colour. White unless a vertex stage put something there.
    @location(3) tint: vec4<f32>,
}

// Seconds since the run began. Render time, never the simulation tick.
fn time() -> f32 {
    return frame.eye_clock.w;
}

fn eye() -> vec3<f32> {
    return frame.eye_clock.xyz;
}

fn sample_albedo(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(albedo_texture, albedo_sampler, uv);
}

// A tangent-space normal, `(0, 0, 1)` where no map is bound.
fn sample_normal_map(uv: vec2<f32>) -> vec3<f32> {
    return textureSample(normal_texture, normal_sampler, uv).xyz * 2.0 - 1.0;
}

// glTF's packing: green is roughness, blue is metallic.
fn sample_metallic_roughness(uv: vec2<f32>) -> vec2<f32> {
    let mr = textureSample(metallic_roughness_texture, metallic_roughness_sampler, uv);
    return vec2<f32>(mr.b, mr.g);
}

// One where nothing is occluded, which is what an absent map binds.
fn sample_occlusion(uv: vec2<f32>) -> f32 {
    return textureSample(occlusion_texture, occlusion_sampler, uv).r;
}

fn sample_emissive(uv: vec2<f32>) -> vec3<f32> {
    return textureSample(emissive_texture, emissive_sampler, uv).rgb;
}

fn sample_height(uv: vec2<f32>) -> f32 {
    return textureSample(height_texture, height_sampler, uv).r;
}

// A world-space normal with the normal map folded in, using screen-space
// derivatives for the tangent frame: a mesh asset carries no tangents, and
// deriving one costs nothing a material that does not ask for it.
fn shaded_normal(in: VertexOutput) -> vec3<f32> {
    let n = normalize(in.normal);
    let mapped = sample_normal_map(in.uv);
    if abs(mapped.x) + abs(mapped.y) < 1e-4 {
        return n;
    }
    let dp1 = dpdx(in.world);
    let dp2 = dpdy(in.world);
    let duv1 = dpdx(in.uv);
    let duv2 = dpdy(in.uv);
    let denom = duv1.x * duv2.y - duv2.x * duv1.y;
    if abs(denom) < 1e-8 {
        return n;
    }
    let t = normalize((dp1 * duv2.y - dp2 * duv1.y) / denom);
    let b = normalize(cross(n, t));
    return normalize(mapped.x * t + mapped.y * b + mapped.z * n);
}

fn tint() -> vec4<f32> {
    return object.color;
}

// Where a vertex lands, for a caller that computed its own position and
// normal in the node's own space — GPU skinning, or any displacement that
// moves the normal with it.
fn place_deformed(position: vec3<f32>, normal: vec3<f32>, uv: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.tint = vec4<f32>(1.0, 1.0, 1.0, 1.0);
    let world = object.model * vec4<f32>(position, 1.0);
    out.world = world.xyz;
    out.clip_position = frame.proj * frame.view * world;
    out.normal = (object.normal_matrix * vec4<f32>(normal, 0.0)).xyz;
    out.uv = uv;
    return out;
}

// The colour this vertex carries. Only with `features = { vertex_color =
// true }`, and only for a mesh whose asset has them; a mesh without gets
// white, so asking costs a material nothing but the attribute.
@if(vertex_color) fn vertex_color(in: VertexInput) -> vec4<f32> {
    return in.vertex_tint;
}

// Which copy of the mesh this vertex belongs to, counting from zero. One
// mesh drawn once is copy zero, so a material can always ask.
fn copy_index(in: VertexInput) -> u32 {
    return in.copy;
}

// The 3x3 this copy is turned and scaled by, in the node's own space.
fn copy_deform(in: VertexInput) -> mat3x3<f32> {
    return mat3x3<f32>(in.copy_x, in.copy_y, in.copy_z);
}

// The node's tint with this copy's own multiplied in. A copy that asked for
// no colour of its own is white, so this is `tint()` for a lone mesh.
fn copy_tint(in: VertexInput) -> vec4<f32> {
    return object.color * in.copy_color;
}

// Where a vertex lands, with `offset` displacing it in the node's own space.
//
// The copy's deformation applies before the node's transform and its
// translation after, which is the split the instance buffers are written in.
fn place(in: VertexInput, offset: vec3<f32>) -> VertexOutput {
    let deform = copy_deform(in);
    var out = place_deformed(
        deform * (in.position + offset),
        deform * in.normal,
        in.uv,
    );
    out.world = out.world + in.copy_offset;
    out.clip_position = frame.proj * frame.view * vec4<f32>(out.world, 1.0);
    return out;
}

fn vertex(in: VertexInput) -> VertexOutput {
    return place(in, vec3<f32>(0.0, 0.0, 0.0));
}

// How much light `light` puts on a point, direction included.
@const fn contribution(light: Light, world: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let kind = light.position_kind.w;
    var to_light = -light.direction_radius.xyz;
    var attenuation = 1.0;
    if kind > 0.5 {
        let delta = light.position_kind.xyz - world;
        let distance = length(delta);
        let radius = max(light.direction_radius.w, 1e-4);
        if distance > radius {
            return vec3<f32>(0.0, 0.0, 0.0);
        }
        to_light = delta / max(distance, 1e-4);
        // Inverse-square, faded out at the radius so a light ends cleanly.
        let falloff = clamp(1.0 - distance / radius, 0.0, 1.0);
        attenuation = falloff * falloff / (1.0 + distance * distance);
        if kind > 1.5 {
            let theta = dot(-to_light, normalize(light.direction_radius.xyz));
            let inner = light.cone.x;
            let outer = light.cone.y;
            attenuation = attenuation
                * clamp((theta - outer) / max(inner - outer, 1e-4), 0.0, 1.0);
        }
    }
    let ndotl = max(dot(n, normalize(to_light)), 0.0);
    return light.color_intensity.rgb * light.color_intensity.a * ndotl * attenuation * INV_PI;
}

// Lambert diffuse over every light the frame collected, plus ambient.
fn diffuse(in: VertexOutput) -> vec3<f32> {
    var lit = frame.ambient_count.rgb;
    let n = normalize(in.normal);
    let count = u32(frame.ambient_count.w);
    for (var i = 0u; i < count; i = i + 1u) {
        lit = lit + contribution(frame.lights[i], in.world, n);
    }
    return lit;
}

// Blend toward the fog colour by distance from the camera.
fn apply_fog(color: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    let mode = frame.fog.x;
    if mode < 0.5 {
        return color;
    }
    let distance = length(world - eye());
    var density = 0.0;
    if mode < 1.5 {
        let start = frame.fog.y;
        density = (distance - start) / max(frame.fog.z - start, 1e-4);
    } else if mode < 2.5 {
        density = 1.0 - exp(-distance * frame.fog.y);
    } else {
        let d = distance * frame.fog.y;
        density = 1.0 - exp(-d * d);
    }
    if frame.fog.w > 0.0 {
        density = density * exp(-max(world.y, 0.0) * frame.fog.w);
    }
    return mix(color, frame.fog_color.rgb, clamp(density, 0.0, 1.0));
}

// The whole default look: texture, tint, lights and fog.
fn shade(in: VertexOutput) -> vec4<f32> {
    let albedo = sample_albedo(in.uv) * tint() * in.tint;
    return vec4<f32>(apply_fog(albedo.rgb * diffuse(in), in.world), albedo.a);
}
