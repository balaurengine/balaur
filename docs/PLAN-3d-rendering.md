> **Status:** steps 1 to 6 and 8 are built. Lights and the environment
> landed 2026-09-07; image-based lighting, occlusion, glass, mirrors, probes,
> the finishing passes and the layer stack landed 2026-09-14. Left: the path
> tracer behind the Export sheet (step 7), and decals and volumetrics (step
> 9). Written down on 2026-09-05, after comparing the engine against Spline,
> the 3D design tool. The order is what is visible
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
| `light3d`: point, directional and spot, with shadows and light layers, resolved headless. The backend's own sun retires when a scene places one | `light3d.rs`, `LightSlots` |
| `environment`: sky, ambient, fog, exposure, tonemap, grading, the shadow budget | `light3d.rs::Environment`, `sync_environment` |
| A physically based surface over the frame's lights: GGX, Smith, Schlick, with a normal map over a tangent frame solved from screen-space derivatives, so a mirrored UV shell lights as its twin does | `shaders/pbr.wesl`, mounted as `package::pbr`, `mesh::tangent_frame` |
| Six texture slots on group 2, each with the fork's one-pixel neutral | `material::TEXTURE_SLOTS`, `Param::Texture` |
| `shadows` and `layers` on `mesh` and `shape3d` | `Renderable3d`, `lighting_from_params` |
| A 3D material contract with sixteen lights of three kinds, ambient and fog in its frame uniforms | `shaders/mesh.wesl`, `shader_material_3d.rs` (`MAX_LIGHTS`) |
| `camera.post` flags applied to the fork's passes: bloom, SSAO, SSR, depth of field | `kiss3d_backend.rs::apply_post`, `window.set_bloom_enabled` and friends |
| Image-based lighting, screen occlusion, reflection probes and the refraction background, bound for every 3D material | `frame_group.rs`, group 0 of `shaders/mesh.wesl` |
| A geometry prepass every 3D material draws, so occlusion and depth of field measure it | `shaders/prepass.wesl`, `pipeline::prepass_pipeline` |
| A `[surface]` on a material: alpha mode, double-sided, glass and its volume, a planar mirror | `material::Surface`, `kiss3d_backend::apply_surface` |
| `reflection_probe`, resolved headless and registered with the window | `reflection.rs`, `ProbeSlots` |
| Four finishing passes and two the fork owns, drawn where `camera.post` lists them | `shaders/finish.wesl`, `post_material::Pass` |
| The layer stack a designer builds a look out of, in the editor's library | `editor/library/shaders/layers.wesl` |
| A `material` asset: a WESL shader, `features`, `params` read off the shader's `Params` struct | `material.rs`, `render.material_params` |
| One albedo `texture` per `mesh` node | `mesh.rs`, bind group 2 |
| 2D lights, occluders and a light map, resolved headless | `light.rs`, `light_map.rs` |
| glTF import that keeps every factor and map, one `material` and one mesh node per material | `balaur_core::glb`, `mesh::parse_part` |
| An HDR film, tonemap and bloom in the fork, driven by Balaur for bloom only | `kiss3d::post_processing::hdr` |
| Headless `wesl` evaluation of pure shader functions, and offscreen golden frames | `render.shader_probe`, `balaur run --offscreen`, `scripts/showcase.sh` |

Missing:

- **Order-independent transparency for a project's material.** A `blend`
  surface draws in the opaque pass with alpha blending, ordered by the scene.
  The fork's own pass wants a second fragment entry point, and the contract
  lets a material declare one. See question 6.
- **The clustered light buffers.** `MAX_LIGHTS` is sixteen and the frame group
  binds no storage buffers; a scene with more lights lights with the first
  sixteen.
- **Screen-space reflections off a Balaur material.** The prepass writes
  geometry, and writes a neutral roughness for it: the pass reads a material's
  surface as diffuse. Occlusion, depth of field and the depth glass tests
  against are exact.
- **Shadows on a material.** The fork hands every material a shadow atlas in
  `RenderContext::shadow` and the contract never binds it, so a node naming a
  `material` casts shadows and receives none. `light3d.shadows` defaults to
  true, which reads as a promise this does not keep.
- **`environment.show_sky = false`.** The fork has one intensity for the drawn
  sky and the light it casts, so switching the sky off stops both. Lighting
  from a sky that is not drawn wants a second dial on the fork.
- **A cutout in the prepass.** `shaders/prepass.wesl` does not discard, so an
  alpha-masked leaf writes its whole rectangle into the depth the occlusion
  and refraction passes read. The fork's own prepass does the same.
- **The path tracer, decals and volumetric fog**, which are steps 7 and 9.

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
transform = { position = [4.0, 6.0, 2.0] }
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
| Image-based lighting, mip-as-prefilter (*fork* `renderer/ibl.rs`) | Built, on by the sky. It replaces `environment.ambient` rather than adding to it: both stand for the same bounced light |
| Exposure, auto exposure, five tonemaps, colour grading (*fork* `HdrSettings`, `ColorGrading`) | Step 2, `environment` |
| Bloom threshold, knee, intensity (*fork* `HdrSettings`) | Have on `camera.post`; `bloom_knee` joins |
| Metallic, roughness, emissive, reflectance, clearcoat, anisotropy, specular tint, subsurface (*fork* `ObjectData3d`) | Step 3, `pbr.wesl` params; subsurface and anisotropy last, they are the ones a design tool hides |
| Normal, metallic-roughness, occlusion, emissive and height maps, parallax (*fork* `set_*_map`) | Step 3, texture params. Parallax is a `features` flag |
| Alpha modes opaque, mask, blend; order-independent transparency (*fork* `AlphaMode`, `hdr_oit`) | Built as `surface.alpha`. A project's material blends in the opaque pass, not the order-independent one: see question 6 |
| Glass: transmission, ior, thickness, attenuation, the transmission background (*fork* `Bsdf::Glass`, `renderer/transmission.rs`) | Built as `[surface]` plus `pbr::shade_glass` |
| Planar mirror (*fork* `renderer/reflector.rs`) | Built as `surface.mirror`, with `mirror_intensity`, `mirror_falloff` and the plane's own `mirror_normal` |
| Reflection probes, parallax-corrected (*fork* `renderer/reflection_probe.rs`) | Built as `reflection_probe`. A probe the scene stops placing keeps its array slot and is shrunk to nothing: the fork registers into a fixed array and takes none back |
| SSR, SSAO, depth of field (*fork*) | Flags on `camera.post`, and every 3D material now draws the prepass they read. SSAO's radius, bias, intensity and power are `ssao_*` on the camera, because they are in world units and a scene's scale decides them; SSR's and depth of field's are still the fork's defaults |
| FXAA, contrast-adaptive sharpening (*fork* `post_processing/fxaa.rs`, `cas.rs`) | Built as `fxaa` and `sharpen`. They are chain effects rather than pipeline flags, so they draw where the list puts them. MSAA is `docs/PLAN-views-and-culling.md`'s |
| Vignette, chromatic aberration, grain, pixelation | Built as `shaders/finish.wesl`, one variant each, turned by `vignette_amount` and its four neighbours on `camera` |
| Grayscale, sobel edge highlight, CRT, waves, loupe (*fork* `post_processing`) | Not surfaced. Each is a post-process material a project writes in minutes once step 5 lands |
| Clustered forward+ lights beyond the primary sixteen (*fork* `builtin/clustered.rs`) | Not built. The frame group binds no storage buffers, so `MAX_LIGHTS` is the whole tier; see question 4 |
| The progressive path tracer, denoise, aperture (*fork* `renderer/raytracer`) | Step 7: a still from the editor's Export sheet. Never a run mode; a game never depends on it |
| AOVs: depth, normals, segmentation (*fork* `builtin/aov.rs`) | Not planned for games. `docs/PLAN-editor-ergonomics.md` may borrow the normals view |
| 2D global illumination (*fork* `post_processing/gi2d.rs`) | Not planned; the light map is 2D's answer. Revisit only if `light2d` shadows prove too hard-edged |
| Morph targets and vertex colours | Built: `MeshData` carries both, and a material asks for the colours with `features = { vertex_color = true }` |
| Instancing (*fork* `set_instances`) | Built: `balaur_render::instancing`, which the `cloner` draws through |
| Baked lightmaps | Not planned; nothing in the fork bakes, and IBL plus shadows is what a design tool ships |
| Decals: a texture projected onto what is under it | Step 9. Not in the fork: a screen-space pass over the depth buffer, with a `decal` component carrying a projector box |
| Volumetric fog and light shafts | Step 9. Not in the fork either: a froxel march the shadow atlas already has the data for |

## 3. Steps

1. **Lights.** *Built 2026-09-07.* `light3d`, the default-sun rule,
   `light3d::lights` headless, `shadows` and `layers` on the renderables,
   `environment.shadows`. A spot light lights `examples/hello`.
2. **Environment.** *Built in part.* The component, fog, exposure, tonemap and
   grading are pushed to the window; the sky loads and orients. Image-based
   lighting is not bound, so a sky lights nothing yet.
3. **The contract.** *Built.* `Param::Texture` and its six slots,
   `package::pbr` as the surface a material imports, and group 0 carrying the
   sky, the occlusion, the probes and the scene behind glass. `glb.rs` keeps
   every factor and map, splitting a model into one mesh node per material.
   Not built: `pbr.wesl` as the *built-in* a node with no material draws,
   which would change every existing scene's look, and the shadow atlas read.
4. **Transparency and glass.** *Built.* `[surface]` carries the alpha mode and
   the cutout, the transmission, index of refraction, thickness and volume;
   `shade_glass` refracts the resolved scene and layers the surface's own
   reflections over it. A `blend` surface draws in the opaque pass rather than
   the order-independent one: see question 6.
5. **Finishing.** *Built.* Vignette, chromatic aberration, grain and
   pixelation as one shader with a variant each, resolved by name off
   `camera.post`; FXAA and contrast-adaptive sharpening from the fork, in the
   chain rather than as pipeline flags, so the order a list gives is the order
   they draw in.
6. **Mirrors and probes.** *Built.* `surface.mirror` puts a planar reflector on
   the node and binds its picture in group 1; `reflection_probe` places a box,
   baked from an image or captured from the scene.
7. **A still.** The path tracer behind the Export sheet's Image, with samples
   and a denoise toggle.
8. **Layers.** *Built.* `editor/library/shaders/layers.wesl`: colour, image,
   gradient, noise, matcap, fresnel, toon and outline, each an `@if` feature
   bringing its own knobs, over the same physically based surface. It ships in
   the library rather than the engine because a project copies it and edits
   it. Not built: the inspector folds that would group a layer's knobs.
9. **Decals and volumetrics.** A `decal` component projecting onto the depth
   buffer, and a froxel march for fog a light shafts through. Both are new
   passes rather than fork features, and both want step 1's shadow atlas.

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
6. **Whether the contract inverts so a material can blend properly.** The
   order-independent pass wants a second fragment entry point writing two
   accumulation targets, and a material declares its own `fs_main`. The fix is
   for a material to write `fn surface(in) -> vec4<f32>` and the engine to
   write every entry point around it — which is a better contract and breaks
   every material written against this one.
7. **Whether the fork should let a probe be taken back.** `ReflectionProbes`
   registers into a fixed array with no `clear`, so a scene that removes a
   probe leaves its layer allocated and shrunk to nothing. Eight layers is
   enough that this has not bitten, and a `clear` on the fork is two lines.
