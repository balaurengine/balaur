// The stacked material a designer builds a look out of, rather than the one a
// programmer writes: colour, image, gradient, noise, matcap, fresnel, toon and
// outline, each a feature that brings its own knobs.
//
// Every layer folds into the surface the physically based contract already
// shades, so a stack is still a PBR material and still lights itself from the
// scene's own lights and sky. The output is readable WESL: a project that
// outgrows the stack copies this file and edits it, which is why each layer is
// a named function rather than a branch inside one.
//
// A layer the material did not ask for links to nothing, so a stack of two
// costs what a stack of two costs.

import package::mesh::{VertexInput, VertexOutput, vertex, frame, eye, sample_albedo};
import package::pbr::{Surface, default_surface, shade_pbr};

struct Params {
    // colour: the flat tint under everything else.
    tint: vec4<f32>,
    // gradient: what the stack fades between.
    gradient_from: vec4<f32>,
    gradient_to: vec4<f32>,
    // gradient: xyz the direction it runs in the world, w where it begins.
    gradient_axis: vec4<f32>,
    // fresnel: the colour the edge glows.
    fresnel_color: vec4<f32>,
    // toon: the colour an unlit band falls to.
    toon_shadow: vec4<f32>,
    // outline: what the silhouette is drawn in.
    outline_color: vec4<f32>,
    metallic: f32,
    roughness: f32,
    reflectance: f32,
    emissive: f32,
    // gradient: how far it runs from where it begins, in world units.
    gradient_length: f32,
    // image: how many times the node's texture repeats across the surface.
    image_tiling: f32,
    // noise: how large its cells are, and how far it lightens and darkens.
    noise_scale: f32,
    noise_amount: f32,
    // fresnel: how tightly the glow is held to the edge.
    fresnel_power: f32,
    // toon: how many bands the lighting is cut into.
    toon_steps: f32,
    // outline: how much of the surface's turn away from the camera the rim
    // covers.
    outline_width: f32,
}

@group(3) @binding(0) var<uniform> params: Params;

// The flat colour under the stack. Always present: a stack with no layers is
// still a surface, and this is what it wears.
fn layer_color() -> vec4<f32> {
    return params.tint;
}

// The node's own texture, repeated across the surface.
@if(image) fn layer_image(in: VertexOutput, under: vec4<f32>) -> vec4<f32> {
    return under * sample_albedo(in.uv * params.image_tiling);
}

@if(!image) fn layer_image(in: VertexOutput, under: vec4<f32>) -> vec4<f32> {
    return under;
}

// A fade between two colours along one direction in the world.
@if(gradient) fn layer_gradient(in: VertexOutput, under: vec4<f32>) -> vec4<f32> {
    let along = dot(in.world, normalize(params.gradient_axis.xyz));
    let at = clamp(
        (along - params.gradient_axis.w) / max(params.gradient_length, 1e-4),
        0.0,
        1.0,
    );
    let faded = mix(params.gradient_from, params.gradient_to, at);
    return under * faded;
}

@if(!gradient) fn layer_gradient(in: VertexOutput, under: vec4<f32>) -> vec4<f32> {
    return under;
}

// One cell's value, with no pattern a viewer can read. The constants are the
// usual ones for this hash; nothing about them is derived.
@if(noise) @const fn noise_hash(cell: vec3<f32>) -> f32 {
    return fract(sin(dot(cell, vec3<f32>(12.9898, 78.233, 37.719))) * 43758.5453);
}

// Value noise: a hash per cell corner, eased between them so a cell's edge is
// not a seam.
@if(noise) @const fn noise_at(at: vec3<f32>) -> f32 {
    let cell = floor(at);
    let within = fract(at);
    let eased = within * within * (3.0 - 2.0 * within);
    let near = mix(
        mix(
            noise_hash(cell),
            noise_hash(cell + vec3<f32>(1.0, 0.0, 0.0)),
            eased.x,
        ),
        mix(
            noise_hash(cell + vec3<f32>(0.0, 1.0, 0.0)),
            noise_hash(cell + vec3<f32>(1.0, 1.0, 0.0)),
            eased.x,
        ),
        eased.y,
    );
    let far = mix(
        mix(
            noise_hash(cell + vec3<f32>(0.0, 0.0, 1.0)),
            noise_hash(cell + vec3<f32>(1.0, 0.0, 1.0)),
            eased.x,
        ),
        mix(
            noise_hash(cell + vec3<f32>(0.0, 1.0, 1.0)),
            noise_hash(cell + vec3<f32>(1.0, 1.0, 1.0)),
            eased.x,
        ),
        eased.y,
    );
    return mix(near, far, eased.z);
}

@if(noise) fn layer_noise(in: VertexOutput, under: vec4<f32>) -> vec4<f32> {
    let value = noise_at(in.world * params.noise_scale) - 0.5;
    return vec4<f32>(under.rgb + value * params.noise_amount, under.a);
}

@if(!noise) fn layer_noise(in: VertexOutput, under: vec4<f32>) -> vec4<f32> {
    return under;
}

// A matcap: the node's texture read by which way the surface faces the camera
// rather than by the surface's own coordinates, so one painted sphere is a
// whole material.
@if(matcap) fn layer_matcap(in: VertexOutput, under: vec4<f32>, normal: vec3<f32>) -> vec4<f32> {
    // The view matrix's upper 3x3 turns a world normal into the camera's own
    // frame, which is the space a matcap is painted in.
    let view = mat3x3<f32>(frame.view[0].xyz, frame.view[1].xyz, frame.view[2].xyz);
    let facing = normalize(view * normal);
    return under * sample_albedo(facing.xy * 0.5 + 0.5);
}

@if(!matcap) fn layer_matcap(in: VertexOutput, under: vec4<f32>, normal: vec3<f32>) -> vec4<f32> {
    return under;
}

// A rim that brightens where the surface turns away from the camera: what
// makes a hologram read as one and a bubble as thin. Added as emission, so it
// glows rather than waiting on a light.
@if(fresnel) fn layer_fresnel(in: VertexOutput, normal: vec3<f32>) -> vec3<f32> {
    let v = normalize(eye() - in.world);
    let facing = 1.0 - clamp(dot(normal, v), 0.0, 1.0);
    return params.fresnel_color.rgb * pow(facing, max(params.fresnel_power, 1e-3));
}

@if(!fresnel) fn layer_fresnel(in: VertexOutput, normal: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(0.0, 0.0, 0.0);
}

// Lighting cut into bands rather than left smooth, over the shaded colour: a
// toon surface still knows where its lights are.
@if(toon) fn layer_toon(lit: vec4<f32>) -> vec4<f32> {
    let steps = max(params.toon_steps, 1.0);
    let level = max(max(lit.r, lit.g), lit.b);
    let banded = floor(level * steps + 0.5) / steps;
    let hue = lit.rgb / max(level, 1e-4);
    return vec4<f32>(mix(params.toon_shadow.rgb, hue * banded, clamp(banded, 0.0, 1.0)), lit.a);
}

@if(!toon) fn layer_toon(lit: vec4<f32>) -> vec4<f32> {
    return lit;
}

// A silhouette drawn where the surface turns hardest away from the camera.
// Cheaper than a second pass over a swollen mesh, and it needs no geometry.
@if(outline) fn layer_outline(in: VertexOutput, lit: vec4<f32>, normal: vec3<f32>) -> vec4<f32> {
    let v = normalize(eye() - in.world);
    let facing = clamp(dot(normal, v), 0.0, 1.0);
    let rim = 1.0 - smoothstep(0.0, max(params.outline_width, 1e-4), facing);
    return vec4<f32>(mix(lit.rgb, params.outline_color.rgb, rim), lit.a);
}

@if(!outline) fn layer_outline(in: VertexOutput, lit: vec4<f32>, normal: vec3<f32>) -> vec4<f32> {
    return lit;
}

// The whole stack over one surface, in the order the layers read: a colour,
// what is painted on it, what is faded across it, what roughens it, then the
// light, then what is drawn over the light.
fn stacked(in: VertexOutput) -> vec4<f32> {
    var s = default_surface(in);
    var base = layer_color();
    base = layer_image(in, base);
    base = layer_gradient(in, base);
    base = layer_noise(in, base);
    base = layer_matcap(in, base, s.normal);
    s.albedo = s.albedo * base.rgb;
    s.alpha = s.alpha * base.a;
    s.metallic = params.metallic;
    s.roughness = max(params.roughness, 0.045);
    s.reflectance = params.reflectance;
    s.emissive = s.emissive + s.albedo * params.emissive + layer_fresnel(in, s.normal);
    let lit = layer_toon(shade_pbr(in, s));
    return layer_outline(in, lit, s.normal);
}

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in);
}

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return stacked(in);
}
