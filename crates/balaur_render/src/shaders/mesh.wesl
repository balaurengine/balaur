// The contract a `material` shader draws 3D against: the uniforms Balaur's
// mesh pipeline binds, and the shading every such shader would repeat.

import constants::{MAX_LIGHTS, MAX_PROBES};

// Lambert's normalisation. A light's intensity is radiance, and the engine's
// own default is 3.0, which reads as white without this.
private const INV_PI: f32 = 0.31830988;

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

// One reflection probe: a box the scene was captured inside, which a
// reflective surface within it samples instead of the sky.
private struct Probe {
    // xyz where it was captured from, w whether this slot is live.
    center_live: vec4<f32>,
    // xyz the influence box's low corner, w its layer in the array.
    low_layer: vec4<f32>,
    // xyz the box's high corner, w the brightness multiplier.
    high_intensity: vec4<f32>,
    // Turn about y, the soft edge's width, the coarsest mip, spare.
    params: vec4<f32>,
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
    // Whether a sky is bound, its coarsest mip, its brightness, its turn.
    environment: vec4<f32>,
    // The viewport in pixels, how many probes are live, and whether the
    // resolved scene behind glass is bound.
    screen_probes: vec4<f32>,
    // xyz a world plane's normal, w its offset; a point behind it is dropped.
    // All zero draws everything, which is every pass but a mirror's own.
    clip_plane: vec4<f32>,
    lights: array<Light, MAX_LIGHTS>,
    probes: array<Probe, MAX_PROBES>,
}

private struct ObjectUniforms {
    model: mat4x4<f32>,
    // The model's inverse transpose, so a scaled node's normals stay normal.
    normal_matrix: mat4x4<f32>,
    color: vec4<f32>,
    // World to the mirror's own picture, for a surface that is one.
    mirror_view_proj: mat4x4<f32>,
    // How much of the mirror shows, whether there is one, how fast it fades
    // as the surface turns off its plane, and a spare.
    mirror: vec4<f32>,
    // xyz the mirror's plane normal in world space.
    mirror_normal: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
// What the scene lends every 3D material for a frame. Each is bound whether
// or not the scene has one -- a one-pixel stand-in takes its place -- so no
// shader here branches on absence and no pipeline is rebuilt when a sky loads.
// The equirectangular sky, mip-chained: the coarsest mip is its irradiance
// and a mip by roughness is its reflection.
@group(0) @binding(1) var environment_texture: texture_2d<f32>;
@group(0) @binding(2) var environment_sampler: sampler;
// What the geometry prepass measured, one value per pixel of the frame.
@group(0) @binding(3) var occlusion_screen_texture: texture_2d<f32>;
// One layer per reflection probe, laid out as the sky is.
@group(0) @binding(4) var probe_textures: texture_2d_array<f32>;
// The resolved opaque scene with a blurred mip chain, which glass refracts.
@group(0) @binding(5) var behind_texture: texture_2d<f32>;
@group(0) @binding(6) var behind_sampler: sampler;
@group(1) @binding(0) var<uniform> object: ObjectUniforms;
// The scene as this surface's own plane sees it, rendered from a mirrored
// camera before the frame. One pixel of black on a surface that is not a
// mirror, which `mirror_at` reports as nothing rather than as black.
@group(1) @binding(1) var mirror_texture: texture_2d<f32>;
@group(1) @binding(2) var mirror_sampler: sampler;
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
private fn sample_normal_map(uv: vec2<f32>) -> vec3<f32> {
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

private fn sample_height(uv: vec2<f32>) -> f32 {
    return textureSample(height_texture, height_sampler, uv).r;
}

// Drop this fragment when it falls behind the frame's clip plane.
//
// A mirror renders the scene from behind its own surface, so everything on
// the far side of that surface has to go: without this a room's back wall
// lands in the reflection of its floor. `shade` and `shade_pbr` call it, so a
// material drawing through either clips for free.
fn clip(in: VertexOutput) {
    let plane = frame.clip_plane;
    if dot(plane.xyz, plane.xyz) > 0.0 && dot(plane.xyz, in.world) + plane.w < 0.0 {
        discard;
    }
}

// A direction as a coordinate into an equirectangular map: u wraps the
// horizon, v runs from straight up to straight down. The convention the fork
// builds its skies and its probes in, so nothing here may drift from it.
@const fn equirect_uv(d: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(
        atan2(d.z, d.x) / (2.0 * 3.14159265) + 0.5,
        acos(clamp(d.y, -1.0, 1.0)) / 3.14159265,
    );
}

// Whether a sky is bound. Without one the environment reads black, and a
// surface is lit by `ambient` and the placed lights alone.
fn environment_lit() -> bool {
    return frame.environment.x > 0.5;
}

// The coarsest mip of the sky, which is the roughest prefilter it carries.
fn environment_max_lod() -> f32 {
    return frame.environment.y;
}

// Turn a direction by the sky's own rotation, so a turned sky reflects turned.
private fn environment_aim(d: vec3<f32>) -> vec3<f32> {
    let turn = frame.environment.w;
    let c = cos(turn);
    let s = sin(turn);
    return vec3<f32>(c * d.x + s * d.z, d.y, -s * d.x + c * d.z);
}

// The sky in one direction, at one mip. Rough surfaces ask for a coarse mip,
// which is what makes a blurred reflection.
fn environment_sample(d: vec3<f32>, lod: f32) -> vec3<f32> {
    let uv = equirect_uv(environment_aim(d));
    let sky = textureSampleLevel(environment_texture, environment_sampler, uv, lod).rgb;
    return sky * frame.environment.z;
}

// Where this fragment falls on the screen, in pixels.
private fn screen_pixel(in: VertexOutput) -> vec2<i32> {
    return vec2<i32>(in.clip_position.xy);
}

// Where this fragment falls on the screen, from zero to one.
fn screen_uv(in: VertexOutput) -> vec2<f32> {
    return in.clip_position.xy / max(frame.screen_probes.xy, vec2<f32>(1.0, 1.0));
}

// How much of the scene's own light reaches this pixel, as the prepass
// measured it. One where the camera has no occlusion pass running.
fn screen_occlusion(in: VertexOutput) -> f32 {
    return textureLoad(occlusion_screen_texture, screen_pixel(in), 0).r;
}

// How much probe `i` speaks for a point: one well inside its box, ramping to
// nothing over the soft edge, and nothing outside.
private fn probe_weight(i: u32, world: vec3<f32>) -> f32 {
    let low = frame.probes[i].low_layer.xyz;
    let high = frame.probes[i].high_intensity.xyz;
    let to_face = min(world - low, high - world);
    let edge = min(to_face.x, min(to_face.y, to_face.z));
    return clamp(edge / frame.probes[i].params.y, 0.0, 1.0);
}

// Which probe speaks loudest for a point: its index in x and its weight in y,
// with -1 where none reaches.
fn probe_at(world: vec3<f32>) -> vec2<f32> {
    var found = -1.0;
    var weight = 0.0;
    let count = min(u32(frame.screen_probes.z), MAX_PROBES);
    for (var i = 0u; i < count; i = i + 1u) {
        if frame.probes[i].center_live.w < 0.5 {
            continue;
        }
        let w = probe_weight(i, world);
        if w > weight {
            weight = w;
            found = f32(i);
        }
    }
    return vec2<f32>(found, weight);
}

// Probe `i` in one direction, aimed at where the ray actually meets its box
// rather than at infinity: what makes a reflection track the room it was
// captured in instead of sliding as the camera moves.
fn probe_sample(i: u32, world: vec3<f32>, d: vec3<f32>, lod: f32) -> vec3<f32> {
    let low = frame.probes[i].low_layer.xyz;
    let high = frame.probes[i].high_intensity.xyz;
    let center = frame.probes[i].center_live.xyz;
    let inverse = 1.0 / d;
    let far = max((low - world) * inverse, (high - world) * inverse);
    let hit = world + d * max(min(far.x, min(far.y, far.z)), 0.0);
    let aimed = normalize(hit - center);
    let turn = frame.probes[i].params.x;
    let c = cos(turn);
    let s = sin(turn);
    let turned = vec3<f32>(c * aimed.x + s * aimed.z, aimed.y, -s * aimed.x + c * aimed.z);
    let layer = i32(frame.probes[i].low_layer.w + 0.5);
    let map = textureSampleLevel(probe_textures, environment_sampler, equirect_uv(turned), layer, lod);
    return map.rgb * frame.probes[i].high_intensity.w;
}

// The coarsest mip a probe carries.
fn probe_max_lod() -> f32 {
    return frame.probes[0].params.z;
}

// Whether the resolved scene behind this surface is bound, which it is only
// while something in the frame refracts.
private fn behind_lit() -> bool {
    return frame.screen_probes.w > 0.5;
}

// What a mirror shows at this point: rgb the reflection, a how much of it
// applies -- nothing on a surface that is not a mirror, on one whose plane
// does not see this point, and fading as the surface turns off that plane.
fn mirror_at(in: VertexOutput, normal: vec3<f32>) -> vec4<f32> {
    if object.mirror.y < 0.5 {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    let clip = object.mirror_view_proj * vec4<f32>(in.world, 1.0);
    if clip.w <= 0.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    let ndc = clip.xy / clip.w;
    // Clip space runs up the screen and a texture runs down it, so y flips.
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if any(uv < vec2<f32>(0.0, 0.0)) || any(uv > vec2<f32>(1.0, 1.0)) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    var shown = object.mirror.x;
    let falloff = object.mirror.z;
    if falloff > 0.0 {
        let aligned = max(dot(normal, normalize(object.mirror_normal.xyz)), 0.0);
        shown = shown * pow(aligned, falloff);
    }
    let seen = textureSampleLevel(mirror_texture, mirror_sampler, uv, 0.0);
    return vec4<f32>(seen.rgb, shown);
}

// The coarsest mip of the scene behind glass, which the roughest surface
// reads.
fn behind_max_lod() -> f32 {
    return f32(textureNumLevels(behind_texture) - 1u);
}

// The scene behind a point on the screen, at one mip: a coarse mip is what
// frosted glass reads.
fn behind(uv: vec2<f32>, lod: f32) -> vec3<f32> {
    return textureSampleLevel(behind_texture, behind_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), lod).rgb;
}

// The tangent frame a surface's own derivatives imply: its columns are the
// directions u and v run in, and the normal.
//
// Both are solved for. Taking the bitangent as `cross(n, t)` instead assumes
// one handedness, and a model that mirrors a UV shell -- which is most of
// them, because it halves the texture -- has the other on one side. The two
// halves of a mirrored wall then light differently, with a seam down the
// join.
@const fn tangent_frame(
    n: vec3<f32>,
    dp1: vec3<f32>,
    dp2: vec3<f32>,
    duv1: vec2<f32>,
    duv2: vec2<f32>,
) -> mat3x3<f32> {
    let across = cross(dp2, n);
    let down = cross(n, dp1);
    let t = across * duv1.x + down * duv2.x;
    let b = across * duv1.y + down * duv2.y;
    let longest = max(dot(t, t), dot(b, b));
    if longest < 1e-20 {
        // A surface with no UVs to speak of: nothing for a map to tilt.
        return mat3x3<f32>(vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 0.0), n);
    }
    let scale = inverseSqrt(longest);
    return mat3x3<f32>(t * scale, b * scale, n);
}

// A world-space normal with the normal map folded in, using screen-space
// derivatives for the tangent frame: a mesh asset carries no tangents, and
// deriving one costs nothing a material that does not ask for it.
fn shaded_normal(in: VertexOutput) -> vec3<f32> {
    let n = normalize(in.normal);
    // Before the branch: WebGPU refuses a derivative in control flow a sample decides.
    let dp1 = dpdx(in.world);
    let dp2 = dpdy(in.world);
    let duv1 = dpdx(in.uv);
    let duv2 = dpdy(in.uv);
    let mapped = sample_normal_map(in.uv);
    if abs(mapped.x) + abs(mapped.y) < 1e-4 {
        return n;
    }
    let frame = tangent_frame(n, dp1, dp2, duv1, duv2);
    return normalize(frame * mapped);
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
private fn copy_deform(in: VertexInput) -> mat3x3<f32> {
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
    clip(in);
    let albedo = sample_albedo(in.uv) * tint() * in.tint;
    return vec4<f32>(apply_fog(albedo.rgb * diffuse(in), in.world), albedo.a);
}
