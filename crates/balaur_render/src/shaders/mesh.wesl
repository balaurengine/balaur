// The contract a `material` shader draws 3D against: the uniforms Balaur's
// mesh pipeline binds, and the shading every such shader would repeat.

import constants::{MAX_LIGHTS, MAX_PROBES, MAX_SHADOW_VIEWS, MAX_SHADOW_LIGHTS, MAX_MORPH_WEIGHT_ROWS};

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
    // x the light layers it lights, y its row in the shadow uniform:
    // `NO_SHADOW`, or `FIND_SHADOW` for one past the primary eight.
    bits: vec4<u32>,
}

// One reflection probe: a box the scene was captured inside, which a
// reflective surface within it samples instead of the sky.
private struct Probe {
    // xyz where it was captured from and the box's centre, w whether this
    // slot is live.
    center_live: vec4<f32>,
    // xyz the box's half extents, w its layer in the array.
    extent_layer: vec4<f32>,
    // The box's turn, a unit quaternion: its node's rotation.
    orientation: vec4<f32>,
    // Turn about y, the soft edge's width, the coarsest mip, brightness.
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
    // The viewport in pixels, how many probes are live, and how many mips the
    // resolved scene behind glass has: none when it is not bound.
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
    // x whether shadows land on it, y whether its colour is see-through: a
    // translucent caster's tint is not laid on the surface that cast it.
    flags: vec4<f32>,
    // x the light layers that light it.
    layers: vec4<u32>,
}

// One light's rows in the shadow atlas, as the fork's `GpuLightShadow` lays
// them out.
private struct LightShadow {
    base_view: u32,
    num_views: u32,
    // 0 point, 1 directional, 2 spot: the fork's numbering, not `Light`'s.
    light_type: u32,
    enabled: f32,
    light_pos: vec3<f32>,
    far_plane: f32,
}

// The fork's `ShadowUniforms`: every view's matrix, every light's rows, and
// the filter. The primary eight lights hold rows 0-7; the rest of the rows
// belong to lights past them.
private struct ShadowUniforms {
    view_proj: array<mat4x4<f32>, MAX_SHADOW_VIEWS>,
    lights: array<LightShadow, MAX_SHADOW_LIGHTS>,
    shadows_enabled: f32,
    texel_size: f32,
    depth_bias: f32,
    transmittance_enabled: f32,
    softness: f32,
    pad0: f32,
    pad1: f32,
    pad2: f32,
    cascade_splits: vec4<f32>,
}

// The fork's `DeformControl`: how many targets are live, the vertex count
// each spans, two flags, and four weights to a row.
private struct MorphControl {
    num_targets: u32,
    num_vertices: u32,
    has_skin: u32,
    has_morph_normals: u32,
    weights: array<vec4<f32>, MAX_MORPH_WEIGHT_ROWS>,
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
// The fork's shadow atlas, one depth layer per light view, with the uniform
// that says which layers belong to which light and how to filter them, and
// the colour translucent casters leave on the light they let through.
@group(0) @binding(7) var shadow_atlas: texture_depth_2d_array;
@group(0) @binding(8) var shadow_sampler: sampler_comparison;
@group(0) @binding(9) var<uniform> shadow: ShadowUniforms;
@group(0) @binding(10) var shadow_tint_atlas: texture_2d_array<f32>;
@group(0) @binding(11) var shadow_tint_sampler: sampler;
@group(1) @binding(0) var<uniform> object: ObjectUniforms;
// The scene as this surface's own plane sees it, rendered from a mirrored
// camera before the frame. One pixel of black on a surface that is not a
// mirror, which `mirror_at` reports as nothing rather than as black.
@group(1) @binding(1) var mirror_texture: texture_2d<f32>;
@group(1) @binding(2) var mirror_sampler: sampler;
// The mesh's morph targets, every target's deltas one after another, and how
// far it is blended towards each. Bound only where a vertex stage may read
// storage buffers, which WebGL2 cannot: there a morphed mesh draws at rest.
@if(morph) @group(1) @binding(3) var<storage, read> morph_positions: array<vec4<f32>>;
@if(morph) @group(1) @binding(4) var<storage, read> morph_normals: array<vec4<f32>>;
@if(morph) @group(1) @binding(5) var<uniform> morph: MorphControl;
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
    // A multimesh instance's four floats of custom data, bound only with
    // `features = { instance_custom = true }`; zero on anything else.
    @if(instance_custom) @location(9) copy_custom: vec4<f32>,
    // Which vertex this is, the row its morph deltas sit at.
    @if(morph) @builtin(vertex_index) vertex: u32,
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

// The height map at its full size, brighter standing higher. Read at mip
// zero, so a parallax march may call it in a loop.
fn sample_height(uv: vec2<f32>) -> f32 {
    return textureSampleLevel(height_texture, height_sampler, uv, 0.0).r;
}

// Every map the contract binds, read at one point.
struct Maps {
    albedo: vec4<f32>,
    // Tangent space, `(0, 0, 1)` where no normal map is bound.
    normal: vec3<f32>,
    // Metallic in x, roughness in y.
    metallic_roughness: vec2<f32>,
    occlusion: f32,
    emissive: vec3<f32>,
}

// Every map at `uv`, filtered by the uv's screen derivatives handed in rather
// than taken here, so it reads correctly after a parallax march.
fn maps_at(uv: vec2<f32>, duv_dx: vec2<f32>, duv_dy: vec2<f32>) -> Maps {
    var out: Maps;
    out.albedo = textureSampleGrad(albedo_texture, albedo_sampler, uv, duv_dx, duv_dy);
    out.normal = textureSampleGrad(normal_texture, normal_sampler, uv, duv_dx, duv_dy).xyz * 2.0 - 1.0;
    let mr = textureSampleGrad(metallic_roughness_texture, metallic_roughness_sampler, uv, duv_dx, duv_dy);
    out.metallic_roughness = vec2<f32>(mr.b, mr.g);
    out.occlusion = textureSampleGrad(occlusion_texture, occlusion_sampler, uv, duv_dx, duv_dy).r;
    out.emissive = textureSampleGrad(emissive_texture, emissive_sampler, uv, duv_dx, duv_dy).rgb;
    return out;
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

// `v` turned by the unit quaternion `q`.
@const fn quat_rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

// `v` in probe `i`'s box frame: its axes the box's, which turn with its node.
private fn probe_local(i: u32, v: vec3<f32>) -> vec3<f32> {
    let q = frame.probes[i].orientation;
    return quat_rotate(vec4<f32>(-q.xyz, q.w), v);
}

// How much probe `i` speaks for a point: one well inside its box, ramping to
// nothing over the soft edge, and nothing outside.
private fn probe_weight(i: u32, world: vec3<f32>) -> f32 {
    let local = probe_local(i, world - frame.probes[i].center_live.xyz);
    let to_face = frame.probes[i].extent_layer.xyz - abs(local);
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
// captured in instead of sliding as the camera moves. The box is met in its
// own frame, so a turned probe's walls stand where its node turned them.
fn probe_sample(i: u32, world: vec3<f32>, d: vec3<f32>, lod: f32) -> vec3<f32> {
    let half = frame.probes[i].extent_layer.xyz;
    let local_p = probe_local(i, world - frame.probes[i].center_live.xyz);
    let local_d = probe_local(i, d);
    let inverse = 1.0 / local_d;
    let far = max((-half - local_p) * inverse, (half - local_p) * inverse);
    let hit = local_p + local_d * max(min(far.x, min(far.y, far.z)), 0.0);
    let aimed = normalize(quat_rotate(frame.probes[i].orientation, hit));
    let turn = frame.probes[i].params.x;
    let c = cos(turn);
    let s = sin(turn);
    let turned = vec3<f32>(c * aimed.x + s * aimed.z, aimed.y, -s * aimed.x + c * aimed.z);
    let layer = i32(frame.probes[i].extent_layer.w + 0.5);
    let map = textureSampleLevel(probe_textures, environment_sampler, equirect_uv(turned), layer, lod);
    return map.rgb * frame.probes[i].params.w;
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
// reads. From the uniform: GLES has no `textureNumLevels`.
fn behind_max_lod() -> f32 {
    return max(frame.screen_probes.w - 1.0, 0.0);
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

// This copy's custom data: what a multimesh instance's `custom` holds, and
// zero for a mesh drawn once. Only with `features = { instance_custom = true }`.
@if(instance_custom) fn copy_custom(in: VertexInput) -> vec4<f32> {
    return in.copy_custom;
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

// A vertex blended towards the mesh's morph targets by this frame's weights.
struct Morphed {
    position: vec3<f32>,
    normal: vec3<f32>,
}

// `position` and `normal` of vertex `vertex` moved by every morph target's
// delta times its weight, the way the fork's own deform pass blends them. A
// mesh with no targets, or every weight at zero, comes back as it went in.
@if(morph) fn morph_vertex(vertex: u32, position: vec3<f32>, normal: vec3<f32>) -> Morphed {
    var out = Morphed(position, normal);
    for (var t = 0u; t < morph.num_targets; t = t + 1u) {
        let weight = morph.weights[t >> 2u][t & 3u];
        if weight != 0.0 {
            let row = t * morph.num_vertices + vertex;
            out.position = out.position + weight * morph_positions[row].xyz;
            if morph.has_morph_normals != 0u {
                out.normal = out.normal + weight * morph_normals[row].xyz;
            }
        }
    }
    return out;
}

@if(morph) private fn rest_or_morphed(in: VertexInput) -> Morphed {
    return morph_vertex(in.vertex, in.position, in.normal);
}

@if(!morph) private fn rest_or_morphed(in: VertexInput) -> Morphed {
    return Morphed(in.position, in.normal);
}

// Where a vertex lands, with `offset` displacing it in the node's own space.
//
// Morph targets apply first, then the copy's deformation, then the node's
// transform and the copy's translation, which is the split the instance
// buffers are written in.
fn place(in: VertexInput, offset: vec3<f32>) -> VertexOutput {
    let deform = copy_deform(in);
    let shape = rest_or_morphed(in);
    var out = place_deformed(
        deform * (shape.position + offset),
        deform * shape.normal,
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

// Lambert diffuse over every light the frame collected that shares a layer
// with this node, each dimmed by the shadow it casts here, plus ambient.
fn diffuse(in: VertexOutput) -> vec3<f32> {
    var lit = frame.ambient_count.rgb;
    let n = normalize(in.normal);
    // In uniform control flow: the shadow filter projects these per light.
    let dpos_dx = dpdx(in.world);
    let dpos_dy = dpdy(in.world);
    let count = u32(frame.ambient_count.w);
    for (var i = 0u; i < count; i = i + 1u) {
        if !lights_this(i) {
            continue;
        }
        lit = lit + contribution(frame.lights[i], in.world, n) * shadow_at(i, in.world, dpos_dx, dpos_dy);
    }
    return lit;
}

// Whether light `i` shares a light layer with this node.
fn lights_this(i: u32) -> bool {
    return (object.layers.x & frame.lights[i].bits.x) != 0u;
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

// What a material writes in the order-independent transparency pass: the
// fork's two targets, a weighted sum of colour and what still shows through.
struct OitOutput {
    @location(0) accum: vec4<f32>,
    @location(1) reveal: f32,
}

// A straight-alpha colour as that pass sums it, weighted by view depth as the
// fork's own `fs_oit` weighs a built-in surface, so the two resolve together.
fn oit_output(color: vec4<f32>, in: VertexOutput) -> OitOutput {
    let z = abs((frame.view * vec4<f32>(in.world, 1.0)).z);
    let w = clamp(10.0 / (1e-5 + pow(z / 5.0, 2.0) + pow(z / 200.0, 6.0)), 1e-2, 3e3);
    var out: OitOutput;
    out.accum = vec4<f32>(color.rgb * color.a, color.a) * w;
    out.reveal = color.a;
    return out;
}

// The whole default look: texture, tint, lights and fog.
fn shade(in: VertexOutput) -> vec4<f32> {
    clip(in);
    let albedo = sample_albedo(in.uv) * tint() * in.tint;
    return vec4<f32>(apply_fog(albedo.rgb * diffuse(in), in.world), albedo.a);
}

// A light that casts no shadow here, and one past the primary eight whose
// row has to be looked up: the two markers `Light.bits.y` takes besides a row.
const NO_SHADOW: u32 = 0xffffffffu;
const FIND_SHADOW: u32 = 0xfffffffeu;

// The colour translucent casters leave at a shadow texel; white with none.
private fn shadow_tint(layer: u32, uv: vec2<f32>) -> vec3<f32> {
    if shadow.transmittance_enabled < 0.5 {
        return vec3<f32>(1.0, 1.0, 1.0);
    }
    return textureSampleLevel(shadow_tint_atlas, shadow_tint_sampler, uv, i32(layer), 0.0).rgb;
}

// One comparison tap against the receiver's own plane at that tap, which is
// what keeps a wide filter from shadowing the surface itself.
private fn shadow_tap(layer: u32, tap_uv: vec2<f32>, ref_uv: vec2<f32>, base_z: f32, grad: vec2<f32>) -> f32 {
    let compare = base_z + dot(grad, tap_uv - ref_uv);
    return textureSampleCompareLevel(shadow_atlas, shadow_sampler, tap_uv, i32(layer), compare);
}

// The fork's filter: a tent-weighted 3x3 of bilinear comparison taps, spread
// by the environment's `shadow_softness`. One where lit, zero where hidden.
private fn shadow_pcf(layer: u32, uv: vec2<f32>, base_z: f32, grad: vec2<f32>) -> f32 {
    let map_size = vec2<f32>(textureDimensions(shadow_atlas));
    let inv_size = 1.0 / map_size;
    let coord = uv * map_size;
    var base_uv = floor(coord + 0.5);
    let s = coord.x + 0.5 - base_uv.x;
    let t = coord.y + 0.5 - base_uv.y;
    base_uv = (base_uv - 0.5) * inv_size;
    let spread = inv_size * shadow.softness;
    let uw = vec3<f32>(4.0 - 3.0 * s, 7.0, 1.0 + 3.0 * s);
    let u = vec3<f32>((3.0 - 2.0 * s) / uw.x - 2.0, (3.0 + s) / uw.y, s / uw.z + 2.0);
    let vw = vec3<f32>(4.0 - 3.0 * t, 7.0, 1.0 + 3.0 * t);
    let v = vec3<f32>((3.0 - 2.0 * t) / vw.x - 2.0, (3.0 + t) / vw.y, t / vw.z + 2.0);
    var sum = 0.0;
    for (var j = 0u; j < 3u; j = j + 1u) {
        for (var i = 0u; i < 3u; i = i + 1u) {
            let tap = base_uv + vec2<f32>(u[i], v[j]) * spread;
            sum = sum + uw[i] * vw[j] * shadow_tap(layer, tap, uv, base_z, grad);
        }
    }
    return sum * (1.0 / 144.0);
}

// How the receiver's light-space depth changes per unit of shadow-map uv,
// from the world position's screen derivatives, capped so an edge-on face
// cannot bias light through.
private fn receiver_plane_grad(layer: u32, clip: vec4<f32>, ndc: vec3<f32>, dpos_dx: vec3<f32>, dpos_dy: vec3<f32>) -> vec2<f32> {
    let m = shadow.view_proj[layer];
    let dclip_dx = m[0] * dpos_dx.x + m[1] * dpos_dx.y + m[2] * dpos_dx.z;
    let dclip_dy = m[0] * dpos_dy.x + m[1] * dpos_dy.y + m[2] * dpos_dy.z;
    let inv_w = 1.0 / clip.w;
    let dndc_dx = (dclip_dx.xyz - ndc * dclip_dx.w) * inv_w;
    let dndc_dy = (dclip_dy.xyz - ndc * dclip_dy.w) * inv_w;
    let du_dx = dndc_dx.x * 0.5;
    let dv_dx = -dndc_dx.y * 0.5;
    let du_dy = dndc_dy.x * 0.5;
    let dv_dy = -dndc_dy.y * 0.5;
    let det = du_dx * dv_dy - dv_dx * du_dy;
    if abs(det) < 1e-12 {
        return vec2<f32>(0.0, 0.0);
    }
    let inv_det = 1.0 / det;
    let grad = vec2<f32>(
        (dv_dy * dndc_dx.z - dv_dx * dndc_dy.z) * inv_det,
        (du_dx * dndc_dy.z - du_dy * dndc_dx.z) * inv_det,
    );
    let per_texel = max(abs(grad.x), abs(grad.y)) / f32(textureDimensions(shadow_atlas).x);
    if per_texel > 0.5 {
        return grad * (0.5 / per_texel);
    }
    return grad;
}

// One atlas layer at a world point: x the filtered visibility, yzw the tint.
// Lit where the point falls outside the layer's view.
private fn shadow_layer(layer: u32, world: vec3<f32>, dpos_dx: vec3<f32>, dpos_dy: vec3<f32>) -> vec4<f32> {
    let light_clip = shadow.view_proj[layer] * vec4<f32>(world, 1.0);
    if light_clip.w <= 0.0 {
        return vec4<f32>(1.0, 1.0, 1.0, 1.0);
    }
    let ndc = light_clip.xyz / light_clip.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, -ndc.y * 0.5 + 0.5);
    if uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || ndc.z > 1.0 || ndc.z < 0.0 {
        return vec4<f32>(1.0, 1.0, 1.0, 1.0);
    }
    let grad = receiver_plane_grad(layer, light_clip, ndc, dpos_dx, dpos_dy);
    return vec4<f32>(shadow_pcf(layer, uv, ndc.z - shadow.depth_bias, grad), shadow_tint(layer, uv));
}

// A directional light's cascades: the first whose far split lies past the
// point, cross-faded into the next over the last fifth of its range.
private fn shadow_cascades(base: u32, count: u32, depth: f32, world: vec3<f32>, dpos_dx: vec3<f32>, dpos_dy: vec3<f32>) -> vec4<f32> {
    var c = count - 1u;
    for (var i = 0u; i < count; i = i + 1u) {
        if depth < shadow.cascade_splits[i] {
            c = i;
            break;
        }
    }
    let near = shadow_layer(base + c, world, dpos_dx, dpos_dy);
    if c + 1u < count {
        let split = shadow.cascade_splits[c];
        let band = split * 0.2;
        if depth > split - band {
            let far = shadow_layer(base + c + 1u, world, dpos_dx, dpos_dy);
            return mix(near, far, clamp((depth - (split - band)) / band, 0.0, 1.0));
        }
    }
    return near;
}

// The cube face a point light's shadow is read from, in the fork's order:
// +x, -x, +y, -y, +z, -z.
@const fn cube_face(d: vec3<f32>) -> u32 {
    let a = abs(d);
    if a.x >= a.y && a.x >= a.z {
        return select(1u, 0u, d.x > 0.0);
    }
    if a.y >= a.z {
        return select(3u, 2u, d.y > 0.0);
    }
    return select(5u, 4u, d.z > 0.0);
}

// The shadow row of a light past the primary eight: the one the fork filled
// for a light of the same kind at the same place.
private fn find_shadow_row(light: Light) -> u32 {
    let kind = u32(light.position_kind.w + 0.5);
    // The fork numbers point 0 and directional 1; `Light` the other way.
    let fork_kind = select(select(1u, 0u, kind == 1u), 2u, kind == 2u);
    for (var row = MAX_SHADOW_LIGHTS - MAX_SHADOW_VIEWS; row < MAX_SHADOW_LIGHTS; row = row + 1u) {
        let ls = shadow.lights[row];
        if ls.enabled > 0.5 && ls.light_type == fork_kind && all(ls.light_pos == light.position_kind.xyz) {
            return row;
        }
    }
    return NO_SHADOW;
}

// How much of light `i` reaches a point past whatever stands between them:
// white where nothing does, black in full shadow, and tinted behind a
// translucent caster. White on a node that receives no shadows. The
// derivatives are the world position's, taken where control flow is uniform.
fn shadow_at(i: u32, world: vec3<f32>, dpos_dx: vec3<f32>, dpos_dy: vec3<f32>) -> vec3<f32> {
    if object.flags.x < 0.5 || shadow.shadows_enabled < 0.5 {
        return vec3<f32>(1.0, 1.0, 1.0);
    }
    var row = frame.lights[i].bits.y;
    if row == FIND_SHADOW {
        row = find_shadow_row(frame.lights[i]);
    }
    if row == NO_SHADOW {
        return vec3<f32>(1.0, 1.0, 1.0);
    }
    let ls = shadow.lights[row];
    if ls.enabled < 0.5 {
        return vec3<f32>(1.0, 1.0, 1.0);
    }
    var seen: vec4<f32>;
    if ls.light_type == 0u {
        seen = shadow_layer(ls.base_view + cube_face(world - ls.light_pos), world, dpos_dx, dpos_dy);
    } else if ls.light_type == 1u {
        let depth = -(frame.view * vec4<f32>(world, 1.0)).z;
        seen = shadow_cascades(ls.base_view, ls.num_views, depth, world, dpos_dx, dpos_dy);
    } else {
        seen = shadow_layer(ls.base_view, world, dpos_dx, dpos_dy);
    }
    let tint = select(seen.yzw, vec3<f32>(1.0, 1.0, 1.0), object.flags.y > 0.5);
    return seen.x * tint;
}
