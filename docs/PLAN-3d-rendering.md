> **Status:** not started. Written down on 2026-09-05, after comparing the
> engine against Spline, the 3D design tool. The order is what is visible
> first: lights, because a scene lights itself with one hard-coded sun today
> and nothing a designer places changes that; then the material contract,
> because every map and knob below sits on it; then the sky, which is what
> makes a physically based material look like anything; then the finishing
> passes, which are cheap once the HDR film they read exists. Almost all of
> it is exposure rather than rendering work: the kiss3d fork already carries
> each pass, and what Balaur draws with today uses a fraction of it.

# Plan: the 3D look

Lights as nodes, shadows, a physically based material contract with texture
maps, sky and image-based lighting, fog, tonemapping and grading, glass,
mirrors and probes, and the finishing passes a design tool ships: vignette,
chromatic aberration, grain, pixelation. `docs/PLAN-shaders.md` owns the
shader system this builds on, and its phase 9, post-process materials, is
where four of those passes land. `docs/PLAN-rendering.md` is the 2D half;
`docs/PLAN-views-and-culling.md` owns the camera's projection, cull masks
and MSAA; `docs/PLAN-textures.md` owns how an image is imported, which is
where a normal map's sRGB flag lives.

## 0. Where the tree is today

Built, and not built for this:

| Have | Where |
| --- | --- |
| One directional light, added by the backend and never authored | `kiss3d_backend.rs::Frontend::new`, `add_light(Light::directional(...))` |
| A 3D material contract with sixteen lights of three kinds, ambient and fog in its frame uniforms | `shaders/mesh.wesl`, `shader_material_3d.rs` (`MAX_LIGHTS`) |
| `camera.post` flags applied to the fork's passes: bloom, SSAO, SSR, depth of field | `kiss3d_backend.rs:365-369`, `window.set_bloom_enabled` and friends |
| A `material` asset: a WESL shader, `features`, `params` read off the shader's `Params` struct | `material.rs`, `render.material_params` |
| One albedo `texture` per `mesh` node | `mesh.rs`, bind group 2 |
| 2D lights, occluders and a light map, resolved headless | `light.rs`, `light_map.rs` |
| glTF import that keeps the base colour texture and drops the rest of the material | `balaur_core::glb` |
| An HDR film, tonemap and bloom in the fork, driven by Balaur for bloom only | `kiss3d::post_processing::hdr` |
| Headless `wesl` evaluation of pure shader functions, and offscreen golden frames | `render.shader_probe`, `balaur run --offscreen`, `scripts/showcase.sh` |

Missing:

- **A `light3d` component.** Scenes cannot place a light. `LIGHT_DIRECTIONAL`
  and `LIGHT_POINT` are `render` constants, and only `light2d` reads them.
- **Any authoring of ambient, fog, sky, exposure or tonemap.** `camera.ambient`
  is read by the 2D camera only; the fork's `set_fog`, `set_skybox_*`,
  `set_exposure`, `set_tonemap` and `set_ambient` are never called.
- **Shadows.** `set_shadows_enabled` is never called; no node says whether it
  casts.
- **A physically based contract.** `mesh.wesl` shades Lambert over one albedo.
  `shader_material_3d.rs` says what it does not bind: image-based lighting,
  reflection probes, SSAO, the transmission background and the clustered
  light buffers.
- **Texture maps beyond albedo.** `Param` is float, vec2, vec3, vec4; a
  material cannot name a normal, roughness, occlusion or emissive map.
- **Transparency modes.** Alpha blending in 2D; in 3D nothing chooses opaque,
  mask or blend, and there is no glass.
- **Four finishing passes.** Vignette, chromatic aberration, grain and
  pixelation are in neither Balaur nor the fork.

## 1. Design

**A light is a node, and the first one replaces the default.** `light3d`
follows `light2d` exactly: the node's global pose places and aims it, the
component holds what a pose cannot, and `resolve_lights_system` builds the
frame's light list headless so a test asserts on it with no GPU. A scene with
no `light3d` keeps the backend's sun so every example draws as it does today;
the first `light3d` in tree order retires it. `LIGHT_SPOT` joins the
constants.

```toml
[[nodes]]
name = "Key"
position = [4.0, 6.0, 2.0]
look_at = [0.0, 0.0, 0.0]
light3d = { kind = "spot", color = "#fff2e0", intensity = 8.0, radius = 30.0, inner = 20.0, outer = 35.0, shadows = true }
```

**The scene's atmosphere is one component, `environment`.** Sky, ambient,
fog, exposure, tonemap, grading and the shadow budget are scene-wide, not per
view, so they do not belong on `camera`; `camera.post` keeps the per-view
passes. Like `camera`, the last `current` one in tree order wins, so a level
can carry two and switch. Every value is data the digest ignores, because
rendering is an observer.

```toml
[nodes.environment]
sky = "skies/studio.hdr"           # equirectangular; drives image-based lighting too
sky_intensity = 1.0
sky_rotation = 90.0
show_sky = true                     # false: the sky lights the scene and `set_background` paints
ambient = "#202428"
fog = { kind = "exponential", color = "#9fb4c8", density = 0.02, height_falloff = 0.1 }
exposure = 1.0
tonemap = "neutral"                 # none, aces, reinhard, agx, neutral
grading = { saturation = 1.1, contrast = 1.0, gamma = 1.0, hue = 0.0, white_balance = [1.0, 1.0, 1.0] }
shadows = { resolution = 2048, softness = 1.0, distance = 60.0 }
```

**The built-in material becomes a `material` asset the engine ships.** Today
a node with no `material` draws with the fork's default `ObjectMaterial` and
a node with one draws Lambert, which is two looks for one inspector. Instead
`shaders/pbr.wesl` is the built-in: its `Params` struct is the PBR surface —
`metallic`, `roughness`, `emissive`, `reflectance`, `clearcoat`,
`alpha_mode`, `transmission`, `ior`, `thickness` — and a node with no
material draws it with the defaults. `material_params` already derives the
inspector's rows from the struct, so there is one mechanism and it cannot
drift. `color` on the node stays the tint.

**Texture maps are params that name an image.** `Param` grows
`Texture(path)`: a `[params]` string ending in an image extension binds a
texture slot from a fixed set the contract declares — `albedo`, `normal`,
`metallic_roughness`, `occlusion`, `emissive`, `height` — each with a
one-pixel fallback so a shader never branches on absence. WebGPU guarantees
four bind groups and the contract uses all four, so the slots share group 2
rather than take a fifth; shadows, the environment map, SSAO and the
clustered buffers join group 0 with the lights, as the fork lays them out.

**Layers for designers are `features` on one shader.** Spline's stacked
material — colour, image, gradient, noise, fresnel, matcap, toon, outline,
depth — is `shaders/layers.wesl` with one `@if` feature per layer and the
layer's knobs in `Params`; the inspector shows a layer as a fold. The output
is readable WESL a user can take over, which is `docs/PLAN-shaders.md`
question 5's answer applied to materials instead of graphs.

**Finishing passes are the first post-process materials.** Vignette,
aberration, grain and pixelation are each a dozen lines over the resolved
frame, and building them as `material` assets on `camera.post` settles the
open question in `docs/PLAN-shaders.md` — where a user pass sits — with four
passes that have to sit somewhere. FXAA and CAS come from the fork as flags.

**Rendering stays an observer.** Nothing here writes simulation state. A
light's resolved list, a probe's box and a material's params are inputs the
backend reads; a headless run computes the same world with none of it, which
is what keeps the digest honest.

## 2. The surface

Everything the fork offers, and where each lands. A row marked *fork* names
the module.

| Piece | Decision |
| --- | --- |
| Point, directional and spot lights: colour, intensity, attenuation radius, cone angles, enabled (*fork* `light.rs`) | Step 1, `light3d`. `enabled` is the node's `visible` |
| `casts_shadows` per light; the shadow atlas, cascades, softness, resolution (*fork* `builtin/shadow.rs`) | Step 1: `light3d.shadows` and `environment.shadows`. One cascade first; `num_cascades` when a scene asks |
| `casts_shadows` per object | Step 1, a `shadows` bool on `mesh` and `shape3d` |
| Light layers and render layers (*fork* `light_layers`, `render_layers`) | Step 1, as `layers` on `light3d` and on the renderables, named as collision layers will be (`docs/PLAN-rapier.md`); a bitmask never reaches a scene file. `docs/PLAN-views-and-culling.md` step 2 puts the matching `cull_mask` on a camera |
| Ambient; fog with linear, exponential and squared modes and height falloff (*fork* `Fog`, `set_ambient`) | Step 2, `environment`. Balaur's contract already carries both in its frame uniforms |
| Equirectangular skybox, orientation, intensity (*fork* `renderer/skybox.rs`) | Step 2, `environment.sky`. `.hdr` and `.exr` load through `image`, which the window build already enables |
| Image-based lighting, mip-as-prefilter (*fork* `renderer/ibl.rs`) | Step 2, on by the sky; no separate setting |
| Exposure, auto exposure, five tonemaps, colour grading (*fork* `HdrSettings`, `ColorGrading`) | Step 2, `environment` |
| Bloom threshold, knee, intensity (*fork* `HdrSettings`) | Have on `camera.post`; `bloom_knee` joins |
| Metallic, roughness, emissive, reflectance, clearcoat, anisotropy, specular tint, subsurface (*fork* `ObjectData3d`) | Step 3, `pbr.wesl` params; subsurface and anisotropy last, they are the ones a design tool hides |
| Normal, metallic-roughness, occlusion, emissive and height maps, parallax (*fork* `set_*_map`) | Step 3, texture params. Parallax is a `features` flag |
| Alpha modes opaque, mask, blend; order-independent transparency (*fork* `AlphaMode`, `hdr_oit`) | Step 4. OIT is the backend's business when a material blends |
| Glass: transmission, ior, thickness, attenuation, the transmission background (*fork* `Bsdf::Glass`, `renderer/transmission.rs`) | Step 4 |
| Planar mirror (*fork* `renderer/reflector.rs`) | Step 6, `mirror` on the material, with `intensity` and `normal_falloff` |
| Reflection probes, parallax-corrected (*fork* `renderer/reflection_probe.rs`) | Step 6, a `reflection_probe` component: box extents, a baked `.hdr` or a capture |
| SSR, SSAO, depth of field (*fork*) | Have as flags. Their settings become `<pass>_<knob>` properties beside `post`, as bloom's did |
| FXAA, contrast-adaptive sharpening (*fork* `post_processing/fxaa.rs`, `cas.rs`) | Step 5, two more `post` names. MSAA is `docs/PLAN-views-and-culling.md`'s |
| Vignette, chromatic aberration, grain, pixelation | Step 5, four post-process materials shipped with the engine |
| Grayscale, sobel edge highlight, CRT, waves, loupe (*fork* `post_processing`) | Not surfaced. Each is a post-process material a project writes in minutes once step 5 lands |
| Clustered forward+ lights beyond the primary sixteen (*fork* `builtin/clustered.rs`) | Step 1 binds the buffers; the split is the backend's. `MAX_LIGHTS` stays the primary tier |
| The progressive path tracer, denoise, aperture (*fork* `renderer/raytracer`) | Step 7: a still from the editor's Export sheet. Never a run mode; a game never depends on it |
| AOVs: depth, normals, segmentation (*fork* `builtin/aov.rs`) | Not planned for games. `docs/PLAN-editor-ergonomics.md` may borrow the normals view |
| 2D global illumination (*fork* `post_processing/gi2d.rs`) | Not planned; the light map is 2D's answer. Revisit only if `light2d` shadows prove too hard-edged |
| Morph targets and vertex colours (*fork* `builtin/deform.rs`) | `docs/PLAN-objects.md` |
| Instancing (*fork* `set_instances`) | `docs/PLAN-objects.md`, the cloner |
| Baked lightmaps | Not planned; nothing in the fork bakes, and IBL plus shadows is what a design tool ships |

## 3. Steps

1. **Lights.** `light3d`, the default-sun rule, `resolve_lights_system`
   headless, the frame group bound with shadows and the clustered buffers,
   `shadows` on the renderables, `environment.shadows`. Ends with: a spot
   light in `examples/hello` casting a shadow, and a test that reads the
   resolved list with no GPU.
2. **Environment.** The component, sky and IBL, fog, exposure, tonemap,
   grading. Ends with: `examples/rig3d` under a studio sky.
3. **The contract.** `Param::Texture`, `shaders/pbr.wesl` as the built-in,
   `mesh.wesl` gaining the BRDF, shadow, IBL and SSAO reads, `glb.rs` keeping
   factors and maps as a material definition per primitive. Ends with: a
   glTF sample looking like its reference render.
4. **Transparency and glass.** Alpha modes, OIT when blending, transmission.
5. **Finishing.** Post-process materials on `camera.post`
   (`docs/PLAN-shaders.md` phase 9) with the four passes; FXAA and CAS as
   flags.
6. **Mirrors and probes.**
7. **A still.** The path tracer behind the Export sheet's Image, with samples
   and a denoise toggle.
8. **Layers.** `shaders/layers.wesl` and its inspector folds, last because it
   is the designer's face over everything above and should not be designed
   before the parts exist.

## 4. What CI can prove, and what it cannot

- The resolved light list, the environment's parsed values and every
  material's params are headless: unit tests, no GPU.
- Every stock shader links under `wesl` `eval` in the test suite, as today.
- Offscreen golden frames through `scripts/showcase.sh` for each step's
  example, diffed with a tolerance; a shadow that moves or a sky that
  vanishes fails the job.
- The web module's size after every stock shader is added, in
  `docs/generated/features.md`.
- What it cannot: how the look reads on a real display, HDR output, and the
  path tracer's convergence time. Those are a manual pass per release.

## 5. Open questions

1. **Where a light's `layers` vocabulary comes from.** `docs/PLAN-rapier.md`
   wants named collision layers; a scene should not learn two spellings. One
   `[layers]` table in `project.toml` serving both is the guess.
2. **Whether `camera.ambient` moves.** It is 2D-only today and
   `environment.ambient` is the 3D one. Two keys for one word is a N1 smell;
   the 2D one may move onto `environment` too.
3. **HDR images headless.** `image` is `png`-only in a headless build. A sky
   is never read headless, so `environment.sky` validates the path and
   nothing else there; whether `balaur check` should open the file is open.
4. **Sixteen or clustered.** The contract's fixed sixteen is what makes a
   `material` shader simple to write. When the clustered tier is bound,
   whether a user shader sees it or only the built-in does decides how much
   of the fork leaks into `mesh.wesl`.
5. **WebGL2.** The fork's compute passes have no WebGL2 path.
   `docs/PLAN-embed.md` carries that question; it is the same one.
