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
@group(2) @binding(0) var albedo_texture: texture_2d<f32>;
@group(2) @binding(1) var albedo_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) world: vec3<f32>,
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

fn tint() -> vec4<f32> {
    return object.color;
}

// Where a vertex lands, for a caller that computed its own position and
// normal in the node's own space — GPU skinning, or any displacement that
// moves the normal with it.
fn place_deformed(position: vec3<f32>, normal: vec3<f32>, uv: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    let world = object.model * vec4<f32>(position, 1.0);
    out.world = world.xyz;
    out.clip_position = frame.proj * frame.view * world;
    out.normal = (object.normal_matrix * vec4<f32>(normal, 0.0)).xyz;
    out.uv = uv;
    return out;
}

// Where a vertex lands, with `offset` displacing it in the node's own space.
fn place(in: VertexInput, offset: vec3<f32>) -> VertexOutput {
    return place_deformed(in.position + offset, in.normal, in.uv);
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
    let albedo = sample_albedo(in.uv) * tint();
    return vec4<f32>(apply_fog(albedo.rgb * diffuse(in), in.world), albedo.a);
}
