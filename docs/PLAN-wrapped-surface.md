> **Status:** steps 1 to 10 are built. What is left: the rapier settings in
> Ughuuu/rapier#4, which Balaur takes once a rapier release carries them, and
> the fork branch merging into `balaur-hooks`. A row's constraint still holds
> where it names one. Written
> 2026-10-02 from an audit of every registered component against the crate
> it wraps: the kiss3d fork at `5498f39` (branch `balaur-hooks`), rapier
> 0.36.0, parry 0.31.1, rodio 0.22.2, cosmic-text 0.19.0, egui 0.36.2, taffy
> 0.14.0 and i_overlay 9.0.0. Balaur builds against the fork's
> `balaur-wrapped-surface` branch (Ughuuu/kiss3d#1) until it merges; the
> rapier settings wait in Ughuuu/rapier#4. It replaces `docs/PLAN-rapier.md`
> item 6, which was written against rapier 0.35.

# Plan: every setting the wrapped crates have

## 0. The rule

A component exposes everything the type behind it offers. Nothing is left out
because nobody asked; a setting is a key, a reader, or a row here saying what
stands in the way. `camera3d` was the first component brought up to this
(lens and layers, `crates/balaur_render/src/lens.rs`); this plan does the
same for the rest. The exception is a controller that answers to the mouse or
the keys: kiss3d's camera controllers are game logic, run per rendered frame
outside the fixed step and the input actions, so a camera is placed by its
node and a script, and the editor library's camera rigs are those scripts.

Each row names a constraint where one exists, from this closed set:

- **fork** — the kiss3d fork has to change: a private field, a missing
  accessor, a compile-time constant.
- **upstream** — rapier, parry, rodio or egui keeps it private or hard-coded;
  a patch upstream or a fork of that crate.
- **world** — one per renderer or per physics world, so it lives on
  `environment`, a camera, `[physics]` or a project setting, never on each
  node.
- **create** — rapier takes it only when the object is built, so it applies
  in the create branch and a patch must not re-apply it.
- **digest** — Balaur-side state that changes per step; the snapshot frame and
  the digest must carry it.
- **pipeline** — Balaur's own shader or pipeline draws the node, so it must
  learn the setting; kiss3d's built-in material already honours it.
- **web** — WebGL2, downlevel GL or wasm32 cannot do it.
- **two spellings** — it would be a second way to write a value a key already
  holds. Section 18 lists these as decisions.

Every key lands with its constant in the crate's `src/vocabulary.rs` (N17),
its schema line, its reader, its `get` read-back, a test, and
`python3 scripts/gen_docs.py`.

**Units.** Render components store angles in degrees and name them
`_degrees`, as `fov_degrees`, `inner_angle_degrees` and `spread_degrees` do.
Physics components store radians, rapier's unit and N21's default, as
`offset_rotation` and `floor_max_angle` do, and declare `unit = "degrees"` so
the inspector draws degrees. Under that declaration `min`, `max` and `step`
are written in degrees (`crates/balaur_core/src/components.rs:35-38`). A
component never mixes the two. Seconds carry no suffix (N21); a key whose bare
word would read as a switch takes `_time`, as `blend_time` does. Hertz,
pixels and decibels take `_hz`, `_pixels` and `_db`.

**Where a citation points.** `kiss3d:` is the fork's `src/`
(`~/.cargo/git/checkouts/kiss3d-153469b327a2679d/5498f39/src`, the same tree
as `../kiss3d`). `rapier:` is `rapier3d-0.36.0/src`; `rapier2d-0.36.0` is the
same source byte for byte, and `cfg(feature = "dim2")` decides what compiles.
`parry3d:`, `parry2d:`, `rodio:`, `cosmic-text:`, `egui:`, `taffy:` and
`i_overlay:` are each crate's `src/` under `~/.cargo/registry/src/*/`. A
`crates/...` path is this repository. A bare file name sits in the same tree
as the last prefixed path above it.

## 1. Defects found on the way

The audit found places where the code does something other than its schema or
docs say. They are fixed first: a new key on top of a wrong mapping inherits
the fault.

### Rendering

1. `environment.shadow_distance` is read and stored but never sent to kiss3d
   (`crates/balaur_render/src/light3d.rs:395`, against `:593-595`).
   `ShadowMapper::set_shadow_distance` exists (`kiss3d: builtin/shadow.rs:1367`)
   behind `Window.shadow_mapper`, which is `pub(super)`
   (`kiss3d: window/window.rs:76`). **fork:** an accessor.
2. `text3d.depth_test` is read (`crates/balaur_render/src/text_component.rs:125`)
   and never applied. kiss3d has no per-object depth test. **fork**, or a
   Balaur `Material3d` with `depth_compare: Always`.
3. `alpha_cut` on `text2d` and `text3d` is read
   (`crates/balaur_render/src/text_component.rs:109`) but not declared, so the
   key check refuses a scene that sets it. Declare it as `alpha_cutoff`, the
   `[surface]` spelling, and apply it as `AlphaMode::Mask`.
4. Text quads keep kiss3d's `render_layers: 1` (`kiss3d: scene/object3d.rs:1043`),
   where every other renderable defaults to all layers. A `camera3d` without
   bit 0 never draws text, and no key fixes it.
5. A premultiplied texture draws dark on `polygon`, `multimesh2d` and any 2D
   node with a `material` asset. `attach_texture_2d` sets
   `Blend2d::PremultipliedAlpha` (`crates/balaur_render/src/texture.rs:153`),
   but those nodes draw through Balaur pipelines fixed to `ALPHA_BLENDING`
   (`crates/balaur_render/src/pipeline.rs:64`). `docs/generated/assets.md` says
   `premultiply` is honoured on 2D nodes.
6. Particles are lit by the 2D light map: `sync_particles` runs before
   `light_map.sync` (`crates/balaur_render/src/kiss3d_backend.rs:276-285`),
   while `light_map.rs:11-12` says they stay unlit.
7. Morph weights are ignored on a skinned mesh and on a mesh with a `material`:
   only kiss3d's `ObjectMaterial` reads them, and no Balaur shader does.
8. A 3D node with no material is always `opaque` and backface-culled, so tint
   and `color` alpha do nothing, and a material's `blend` draws in the opaque
   pass (`crates/balaur_render/src/shader_material_3d.rs:352-356`). The
   order-independent pass is reached only by a shader-link fallback.
9. `attenuation_distance = 0` means "takes nothing out" in `pbr.wesl:323` and
   maximum absorption in kiss3d's `ObjectMaterial`
   (`kiss3d: builtin/default.wgsl:1441-1446`); the same 0 is passed to both.
10. With a sky file loaded, `ambient_color` lights nothing even under
    `sky_enabled = false`: the sky is zeroed, not unbound, and still counts as
    found (`crates/balaur_render/src/frame_group.rs:412-428`).
11. `mesh` draws at `[0.8, 0.8, 0.8, 1]` (`crates/balaur_render/src/lib.rs:571`),
    so a textured mesh is 80 % bright and only `node.set_tint` changes it.
12. The offscreen run renders 4 samples whatever `window.msaa` says
    (`crates/balaur_render/src/kiss3d_backend.rs:565`).
13. `text2d` and `particles` skip the draw-order pass
    (`crates/balaur_render/src/sync_2d.rs:144-146`), so `z_index` does nothing
    on them.
14. `screen_notifier2d` tests against `CameraConfig2d`, the requested view, not
    the view after a mouse pan or zoom
    (`crates/balaur_render/src/notifier.rs:81-101`), and its "world units" are
    node units (`:111`).

### Physics

15. `collider3d` with `fit = "aabb"` or `"obb"` loses the fitted pose:
    `converted_trimesh` puts it in `builder.position`
    (`rapier: geometry/collider.rs:1039`) and `add_collider_at` overwrites it
    (`crates/balaur_physics/src/collider.rs:506,514`).
16. `physics3d.swept_aabb` passes the current pose as the next one
    (`crates/balaur_physics/src/collider.rs:995`), so it returns the plain box.
    The broad phase uses `compute_broad_phase_aabb` (`rapier: geometry/collider.rs:623`).
17. A body with `mass > 0`, a non-zero `center_of_mass` and `inertia = 0`
    cannot turn, in both dimensions. Balaur builds `MassProperties` with zero
    inertia and zeroes the colliders' density
    (`crates/balaur_physics/src/body.rs:180-186`,
    `crates/balaur_physics/src/dim2/body.rs:103-110`,
    `crates/balaur_physics/src/shared/body.rs:103-104`); rapier derives inertia
    only for the `Mass` variant (`rapier: dynamics/rigid_body_components.rs:469-480`).
    The schema says 0 lets rapier derive it.
18. `solve_ik` uses default `InverseKinematicsOption`, whose `constrained_axes`
    is all six (`rapier: dynamics/joint/multibody_joint/multibody_ik.rs:35`),
    with an identity target rotation (`crates/balaur_physics/src/joint.rs:385-386`),
    so it also turns the end link to identity.
19. `floor_max_angle` and `min_slide_angle` on both characters declare
    `unit = "degrees"` but write `max` in radians
    (`crates/balaur_physics/src/character.rs:56,60`). The inspector clamps a
    typed 45 to 1.57 and stores 0.027 rad
    (`editor/scripts/inspector.rn:1131-1155`).
20. `can_sleep = false` is stored as negative sleep thresholds
    (`crates/balaur_physics/src/body.rs:141-149`), so `get` reports `false`
    for every body while world sleep is off, and a re-save writes it.
    `physics.set_sleeping_allowed(true)` resets every body's activation
    (`crates/balaur_physics/src/lib.rs:706-708`) and wipes `can_sleep` and
    `time_to_sleep`.
21. On a `generic` joint `limits` and every `motor*` key do nothing:
    `free_axes` returns no axis (`crates/balaur_physics/src/joint.rs:59`,
    `crates/balaur_physics/src/dim2/joint.rs:38-44`). A `ball_socket` writes
    one pair to all three angular axes (`joint.rs:58`). A 2D `groove` leaves
    its rotation free and never limits or drives it.
22. `break_force` is compared with the magnitude of the joint impulse, linear
    and angular parts mixed (`crates/balaur_physics/src/joint.rs:289-291`). It
    is impulse per step, so the threshold moves with the fixed step, and on an
    articulation it never fires (`crates/balaur_physics/src/shared/joint.rs:138-142`).
23. After `set_joint_limits` or `set_motor_*`, `get` still reports the
    authored values and the next patch rebuilds the joint from them
    (`crates/balaur_physics/src/joint.rs:249-283`).
24. On a `rope` joint `limits` overrides `max_length` silently: both write the
    `LinX` limits (`rapier: dynamics/joint/rope_joint.rs:153-156`).
25. A `spring` joint runs rapier's `ForceBased` model
    (`rapier: dynamics/joint/spring_joint.rs:32-39`), reads back
    `motor_model = acceleration`, and takes `motor_model` only while
    `motor ≠ off`, when the motor then replaces the spring.
26. `tile_collision.one_way = true` does nothing on full cells and plain shaped
    tiles: the axis is packed only for tiles the tileset marks one-way
    (`crates/balaur_physics/src/dim2/tiles.rs:179-181,210-212`). `mass` lands
    on each generated collider, so a map weighs `mass` times their count.
27. The 2D snapshot frame does not hold `tile_params`, `tile_built` or
    `tile_colliders` (`crates/balaur_physics/src/dim2/mod.rs:63-65,363-391`),
    so a restore can keep handles from another world. **digest**
28. `body2d.max_contact_impulse()` reads colliders on the body's own node only
    (`crates/balaur_physics/src/dim2/collider.rs:498-515`); a child
    `collider2d` is never counted.
29. `collider2d` shows `resolution = 64` and `max_concavity = 0.01`, parry's 3D
    VHACD defaults. rapier2d's are 256 and 0.1
    (`parry2d: transformation/vhacd/parameters.rs:474-477`), and the fallback
    that reads them never runs because the schema default always arrives.
30. `softbody*.oriented` reads backwards: rapier's oriented closed surface
    holds nothing in, and `false` is the shell that does
    (`rapier: dynamics/soft_body/soft_body_builder/soft_body_builder_settings.rs:335-344`).
    Balaur's `false` is rapier's `None`, "auto". `shape_matching = false` is
    applied after the generator and undoes the `trimesh` and `polyline`
    generators' own choice (`crates/balaur_physics/src/softbody.rs:218`).
31. `move_character` passes `QueryFilter::default()` with only its own collider
    excluded (`crates/balaur_physics/src/character.rs:127`): the sweep hits
    sensors, ignores the character's `collision_mask`, and hits the body's
    other colliders. Wheel rays do the same (`crates/balaur_physics/src/vehicle.rs:137`).
32. `vehicle3d` rebuilds rapier's controller every step
    (`crates/balaur_physics/src/vehicle.rs:95`), so an airborne wheel stops
    dead instead of slowing by 0.99 per step
    (`rapier: control/ray_cast_vehicle_controller.rs:481-486`).
    `wheel_state().suspension_force` reads the force before the
    `suspension_max_force` cap (`:447-450`). Reverse, steer and brake do not
    wake a sleeping chassis (`:442`).
33. `physics.counters` and `physics.quarantined` read the 3D world alone
    (`crates/balaur_physics/src/tuning.rs:257-309`), in a module NAMING D5
    keeps for what spans both.

### Audio, text and UI

34. The audio crate and module docs describe calls that do not exist:
    `audio.play(path, {...})`, `stop`, `set_volume`, `set_pitch`, `play_on`
    (`crates/balaur_audio/src/script_api.rs:69,85-87`, `lib.rs:3-5`).
    `audio.play` takes a node.
35. A font's `scale`, `y_offset` and `hinting` import keys reach egui's
    `FontTweak` alone (`crates/balaur_ui/src/theme.rs:269-273`); `text2d`,
    `text3d` and shaped widget labels ignore them, and glyph hinting there is
    always on.
36. Widget `align_items = "start"`, the default, stretches: every value other
    than center and end maps to `AlignItems::STRETCH`
    (`crates/balaur_ui/src/widget/taffy.rs:96-104`).
37. Widget `wrap` and `truncate` do nothing on a button; its caption is shaped
    with no width (`crates/balaur_ui/src/widget/button.rs:75-79`).
38. Widget `font_weight` and `font_style` reach labels and button captions
    only; every kind egui draws gets size and family alone
    (`crates/balaur_ui/src/widget/theme.rs:843-863`).

## 2. Lights, sky and the 3D post chain

### `light3d`

Exposed: `kind`, `color`, `intensity`, `range`, `inner_angle_degrees`,
`outer_angle_degrees`, `shadow_enabled`, `light_layers`. Every `LightType`
variant is reached.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `Light::with_radius` (`kiss3d: light.rs:89,196`) | `source_radius` | 0.0 | Read by the path tracer only; Balaur hard-codes 0 (`light3d.rs:498`) |
| Per-light shadow bias, size, cascades | none | | **world**: all on `ShadowMapper`, so on `environment` |
| 8 primary lights with shadows (`kiss3d: light.rs:11`), 16 atlas views (`builtin/shadow.rs:42`) | none | | **fork**: compile-time; a point light takes 6 views, a directional one per cascade |

### `environment`

Exposed: fog (every field, four modes), `ambient_color`, `exposure`,
`tonemap`, `saturation`, `contrast`, `gamma`, `shadow_enabled`,
`shadow_resolution`, `shadow_softness`, `shadow_distance` (defect 1), `sky`,
`sky_enabled`, `sky_intensity`, `sky_rotation_degrees`.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `ShadowMapper::set_num_cascades` (`kiss3d: builtin/shadow.rs:1356`) | `shadow_cascades` | 4, range 1–4 | **fork**: the accessor of defect 1 |
| `set_first_cascade_far_bound` (`shadow.rs:1374`) | `shadow_first_cascade_distance` | 12.0 | **fork**: same accessor |
| `ShadowMapper.depth_bias` (`shadow.rs:229`) | `shadow_bias` | 0.0012 | **fork**: private, no setter |
| Raster bias in `shadow_depth_bias()` (`shadow.rs:58-64`) | `shadow_constant_bias`, `shadow_slope_bias` | 1, 1.75 | **fork**: baked into the pipelines |
| `MAX_SHADOW_VIEWS` (`shadow.rs`) | none | 16 | **fork**: constant; filtering is PCF only |
| `Tonemap::TonyMcMapface` (`kiss3d: post_processing/hdr.rs:55`) | `tonemap` word `tony_mcmapface` | | Missing from `words::TONEMAPS` (`crates/balaur_render/src/vocabulary.rs:103`) |
| `ColorGrading::white_balance` (`hdr.rs:78`) | `white_balance` | [1, 1, 1] | |
| `ColorGrading::hue` (`hdr.rs:86`) | `hue_degrees` | 0 | |
| `HdrSettings::auto_exposure`, `_speed`, `_min`, `_max`, `_key` (`hdr.rs:120-128`) | `auto_exposure_enabled`, `auto_exposure_speed`, `auto_exposure_min`, `auto_exposure_max`, `auto_exposure_key` | false, 3.0, 0.05, 8.0, 0.18 | Adapts on the wall clock (`hdr.rs:1500-1505`), so fixed-step captures do not repeat; render-only, no digest |
| `Window::set_exposure_value(Exposure)` (`kiss3d: window/window.rs:895`) | `exposure_ev` | | **two spellings** with `exposure` (section 18) |
| `Window::set_ambient` (`window.rs:540`) | `ambient_intensity` | 0.2 in kiss3d, pinned to 1.0 | **two spellings**: the colour carries brightness today |
| Skybox lighting intensity and image apart from drawing (`kiss3d: renderer/skybox.rs:182-226`) | `sky_light_intensity`, `sky_light` | | **fork**: one intensity and one image for both |
| `Window::set_transmission_enabled`, `TransmissionSettings` (`window.rs:781-792`, `kiss3d: renderer/transmission.rs:22-52`) | `transmission_enabled`, `transmission_blur_quality` (`low`/`medium`/`high`), `transmission_steps` | true, high, 1 | |
| `Window::set_reflection_capture_layers` (`window.rs:713`) | `probe_capture_layers` | -1 | **world**: one mask per renderer |
| `Window::set_background_color` (`window.rs:493`) | none | | Reached by `render.set_background`; it clears the 2D pass too, so it stays a script call or a project setting |

### `reflection_probe`

Exposed: `image`, `image_rotation_degrees`, `intensity`, `size`, `falloff`.
Every `ReflectionProbe` field is reached.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `Window::capture_reflection_probe` (`window.rs:722`) | `update_mode` (`once`/`always`) | `once` | One capture renders the scene six times at 256² |
| Capture size, `FACE = 256` (`kiss3d: window/rendering.rs:561`) | `capture_size_pixels` on `environment` | 256 | **fork**, **world**: every probe shares one texture array |
| Capture clip planes, from the main camera (`rendering.rs:560,586`) | `capture_near`, `capture_far` | | **fork** |
| Box orientation, world-aligned (`kiss3d: renderer/reflection_probe.rs:39-42`) | none | | **fork**: the node's rotation is ignored |
| Per-probe capture mask | none | | **fork**: see `probe_capture_layers` |
| `MAX_PROBES = 8` (`reflection_probe.rs:26`) | none | | **fork**: constant |

### `camera3d`

Exposed after `lens.rs`: projection, lens and layers, the post words and their
bloom, vignette, aberration, grain, pixelate and SSAO knobs. `OrbitCamera3d`'s
mouse controls are not wrapped: no mouse or key moves a camera.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `Projection::Orthographic` scale (`kiss3d: camera/mod.rs:23-26`) | `orthographic_height` | | **fork**: reserved "for future"; height comes from distance and fov (`orbit3d.rs:468`) |
| `HdrSettings::bloom_knee` (`hdr.rs:113`) | `bloom_knee` | 0.5 | |
| `BLOOM_MIPS` (`hdr.rs:32`) | `bloom_mips` | 5 | **fork**: private constant |
| `SsrSettings` (`kiss3d: renderer/ssr.rs:20-47`) | `ssr_max_steps`, `ssr_thickness`, `ssr_max_distance`, `ssr_roughness_cutoff`, `ssr_edge_fade`, `ssr_intensity` | 48, 0.5, 60, 0.6, 0.12, 1.0 | **web**: compute and storage buffers (`kiss3d: context/context.rs:324-333`) |
| `DofSettings` (`kiss3d: renderer/dof.rs:16-71`) | `dof_mode` (`bokeh`/`gaussian`), `dof_focus_distance`, `dof_aperture_f_stops`, `dof_sensor_height`, `dof_max_blur_pixels`, `dof_max_depth`, `dof_taps` | bokeh, 10, 0.125, 0.01866, 64, 1e6, 48 | |
| `Fxaa::set_thresholds` (`kiss3d: post_processing/fxaa.rs:207`) | `fxaa_edge_threshold`, `fxaa_edge_threshold_min` | 0.125, 0.0312 | Both join the chain's rebuild key `Built` (`crates/balaur_render/src/post_material.rs:246`) |
| `Cas::set_sharpness` (`kiss3d: post_processing/cas.rs:187`) | `sharpen_amount` | 0.5 hard-coded today | |
| `Crt` (`kiss3d: post_processing/crt.rs:43,221-238`) | `post` word `crt`; `crt_curvature`, `crt_aberration`, `crt_scanline_intensity`, `crt_scanline_count`, `crt_vignette` | 0.12, 0.004, 0.25, 480, 0.35 | |
| `Grayscales` (`kiss3d: post_processing/grayscales.rs:18`) | `post` word `grayscale` | | |
| `Waves` (`kiss3d: post_processing/waves.rs:34`) | `post` word `waves` | | Speed fixed in the fork |
| `SobelEdgeHighlight::new(threshold)` (`kiss3d: post_processing/sobel_edge_highlight.rs:49`) | `post` word `edges`; `edges_threshold` | | **web**: compiled out on wasm32 |
| `Loupe` (`kiss3d: post_processing/loupe.rs:41-50,262-317`) | `post` word `loupe`; `loupe_zoom`, `loupe_focus`, `loupe_corner` (`top_left`/`top_right`/`bottom_left`/`bottom_right`), `loupe_size`, `loupe_border_color` | 8, [0.5, 0.5], bottom_right, 0.4, [1, 0.9, 0.2] | |
| `OculusStereo` (`kiss3d: post_processing/oculus_stereo.rs:30`) | `post` word `stereo` | | Useful only with the stereo camera below |
| `FirstPersonCamera3d` (`kiss3d: camera/first_person3d.rs`) | none | | Not wrapped. It is a game's controller inside kiss3d's event loop: it moves per rendered frame on raw keys, outside the fixed step and the input actions. A first-person camera is a script moving the node, as the editor library's First person rig does |
| `FirstPersonCamera3dStereo::set_ipd` (`kiss3d: camera/first_person_stereo3d.rs:216`) | `eye_separation`; above 0 the camera is a stereo pair | 0 | Built. Each eye draws its half of the frame and the eyes converge on the point looked at. The node alone places it: its keys, buttons and scroll are off, and +y stays up |
| `FixedView3d` (`kiss3d: camera/fixed_view3d.rs`) | none | | Pinned at the origin; an orbit with both buttons `none` covers it |

The bloom settings on `camera3d` and the exposure settings on `environment`
write one `HdrSettings` per window (`window.rs:871`), so the last current
camera wins.

### The rest of the 3D renderer

- **Path tracer** — `kiss3d: renderer/raytracer/mod.rs`: `set_max_bounces`
  (8), `set_denoise` (off), `set_denoise_iterations` (5),
  `set_samples_per_frame` (1), `set_interactive_scale` (0.5), `set_aperture`,
  `set_f_number`, its own environment (`:387-420`) and presets (`:156-204`).
  Not a component: it draws through `Window::raytrace_3d`, takes the 3D scene
  alone and reads only exposure and tonemap (`rendering.rs:1575-1658`). It is
  the still export of `docs/PLAN-3d-rendering.md` step 7.
- **Clustered lighting** — grid and per-cluster limits are constants in a
  `pub(crate)` module (`kiss3d: builtin/clustered.rs:33-42`). **fork**.
- **MSAA and vsync** — `window.msaa` and `window.vsync` apply at restart, but
  `set_samples` and `set_vsync` (`window.rs:206,224`) work at run time, so the
  settings can apply live. kiss3d offers 1 or 4 samples (`kiss3d: window/canvas.rs:12-15`).
- **AOV output** — `render_aov_3d` and `snap_*` (`kiss3d: window/aov.rs:32-151`).
  Balaur's own debug views cover albedo, normals, uv and depth; camera-space
  normals and segmentation are missing. `snap_*` reads back on the CPU and
  does not work on the web. A script `render.snap_aov(kind)` is the home.
- **Planar reflector** — every knob is on `[surface]`. Its target is viewport
  sized and it draws with the main camera's layers (`rendering.rs:1942,1994`);
  a size or mask of its own is **fork**.

## 3. 3D surfaces and drawn nodes

Most per-object fields in kiss3d's `ObjectData3d` are read only by its
built-in `ObjectMaterial`. Balaur draws with it only for a node with no
`material`, and then sets colour, texture, pose and visibility. A `material`
asset must name a `shader` (`crates/balaur_render/src/material.rs:460`) and
`ShaderMaterial3d` reads colour, texture, reflector, alpha mode,
`transmission > 0`, culling and surface visibility. So two homes:

- **A material asset with no `shader`.** It is drawn by kiss3d's
  `ObjectMaterial`, and its `[params]` and `[surface]` map onto the `Object3d`
  setters. Everything about the surface goes here: PBR, alpha, culling, glass,
  SSR. A renderable does not repeat these keys.
- **Keys on every 3D renderable** for what is about the node, not the
  surface: wireframe, vertices, surface visibility, segmentation. They are
  applied in `lighting_from_params` (`crates/balaur_render/src/lib.rs:589-606`)
  and join `BatchKey3d` (`crates/balaur_render/src/batch_3d.rs:28-38`),
  because batched nodes share one kiss3d object.

Each key that `ShaderMaterial3d` or `SkinnedMaterial3d` does not read is
**pipeline** for a node with a shader, a skin or both.

### `material` and the 3D material asset

Exposed: component `source`; asset `shader`, `features`, `params`, the image
slots `albedo`, `normal`, `metallic_roughness`, `occlusion`, `emissive`,
`height`, and `[surface]`: `alpha`, `alpha_cutoff`, `double_sided`,
`transmission`, `ior`, `thickness`, `attenuation_color`,
`attenuation_distance`, `mirror`, `mirror_intensity`, `mirror_falloff`,
`mirror_normal`.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `AlphaMode::Premultiplied` (`kiss3d: scene/object3d.rs:166-168`) | `[surface] alpha = "premultiplied"` | | **pipeline**: `ShaderMaterial3d` has no premultiplied pipeline |
| `Object3d::set_bsdf`, `Bsdf` (`object3d.rs:124-133,1800`) | `[surface] trace_surface` (`opaque`/`glass`/`metal`/`light`) | opaque | Read by the path tracer only |
| `set_metallic`, `set_roughness` (`object3d.rs:1676,1731`) | `metallic`, `roughness` | 0.0, 0.5 | Unreachable until a shader-less asset exists |
| `set_emissive` (`object3d.rs:1742`) | `emission_color` | black | NAMING's pick; the glTF importer writes `emissive` (section 19) |
| `set_specular_tint`, `set_reflectance` (`object3d.rs:1847,1869`) | `specular_tint`, `reflectance` | white, 0.5 | |
| `set_clearcoat(strength, roughness)` (`object3d.rs:1878`) | `clearcoat`, `clearcoat_roughness` | 0, 0 | |
| `set_anisotropy(strength, rotation)` (`object3d.rs:1889`) | `anisotropy`, `anisotropy_rotation_degrees` | 0, 0 | |
| `set_subsurface(factor, radius)` (`object3d.rs:1856`) | `subsurface`, `subsurface_radius` | 0, 0 | Path tracer only; the radius is reserved in the fork |
| The five map setters (`object3d.rs:1911-2010`) | the existing slot names | | |
| `set_parallax_scale`, `set_parallax_layers`, `set_parallax_method` (`object3d.rs:2024-2038`) | `parallax_scale`, `parallax_layers`, `parallax_method` (`occlusion`/`relief`), `parallax_relief_steps` | 0.1, 16 | |
| Named built-in materials `normals`, `uvs` (`kiss3d: resource/material_manager3d.rs:37-47`) | asset `view` (`normals`/`uvs`) | | Reached window-wide only, by `render.set_debug_view` |
| `set_ssr(SsrMaterial)` (`object3d.rs:1690`, `kiss3d: renderer/ssr.rs:55-80`) | `[surface] ssr`, `ssr_intensity`, `ssr_infinite_thickness`, `ssr_distance_fade`, `ssr_fresnel` | on, 1.0, false, true, false | Works only while the camera's `post` has `ssr`; Balaur's prepass writes no reflection for material nodes (`crates/balaur_render/src/shaders/prepass.wesl:30-37`) |

`pbr.wesl`'s `Surface` (`crates/balaur_render/src/shaders/pbr.wesl:24-34`)
has none of clearcoat, anisotropy, specular tint, subsurface or parallax, and
`sample_height` is never called (`shaders/mesh.wesl:173`). Shader-based
materials need those in `pbr.wesl` to match the built-in one.

The glTF importer drops data kiss3d draws: `BLEND` imports as `opaque`
(`crates/balaur_core/src/glb_material.rs:270-275`), KHR_materials_specular's
colour is not read, and `gltf`'s `KHR_materials_unlit`,
`KHR_texture_transform`, `KHR_materials_pbrSpecularGlossiness`,
`KHR_materials_variants` and `KHR_lights_punctual` features are off
(`crates/balaur_core/Cargo.toml:44-53`).

### Keys every 3D renderable takes

On `mesh`, `shape3d`, `multimesh3d`, `boolean3d` and `text3d`.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `set_lines_width(width, use_perspective)` (`object3d.rs:1473`) | `wireframe_width`, `wireframe_sizing` (`world`/`screen`) | 0 (off), world | **pipeline**; **web**: the storage-backed line renderer (`kiss3d: window/wgpu_canvas.rs:600-607`) |
| `set_lines_color` (`object3d.rs:1486`) | `wireframe_color` | the node's colour | as above |
| `set_points_size`, `set_points_color` (`object3d.rs:1501,1514`) | `dot_size`, `dot_sizing`, `dot_color` | 0 (off) | as above; `dot`, because `shape2d` has `points` and a material has the `vertex_color` feature |
| `set_surface_rendering_activation` (`object3d.rs:1526`) | `draw_surface` | true | `SkinnedMaterial3d` ignores it |
| `set_segmentation_id` (`object3d.rs:1444`) | `segmentation_id` | automatic, ≥ 1 | Read only by the segmentation output |
| Receive shadows | `receive_shadows` | true | **fork**: nothing in the fork; NAMING reserves the key |

### `mesh`

Exposed: `source`, `skeleton`, `texture`, `material`, `cast_shadow`,
`light_layers`, `render_layers`, and `morph.<name>` outside the table.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `Object3d::set_color` (`object3d.rs:1632`) | `color` | [1, 1, 1, 1] | Fixes defect 11 |
| `SceneNode3d::set_morph_weights` (`kiss3d: scene/scene_node3d.rs:1454`) | `morph.<name>` in the generated table | | 64 targets (`kiss3d: builtin/deform.rs:30`); **pipeline** (defect 7); **web**: five storage buffers per stage |
| `Skin3d`, `Object3d::set_skin` (`object3d.rs:42-118,1292`) | none | | **fork**: `pub(crate)`. It would let a skin and morphs combine, with no 128-joint cap |

### `shape3d`

Exposed: `kind`, the shape sizes, `segments`, `rings`, `sides`, `color`,
`material`, `cast_shadow`, `light_layers`, `render_layers`.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `Object3d::set_texture` (`object3d.rs:1665`) | `texture` | "" | The solids carry UVs, but only `Shape3d::Mesh` takes a texture (`crates/balaur_render/src/kiss3d_backend/geometry.rs:61`) |
| `procedural::unit_hemisphere` (`kiss3d: procedural/sphere.rs:201`) | `kind` word `hemisphere` | | Balaur's primitives come from `balaur_core::primitive` so colliders and picking match; each is a new mesher kind there |
| `procedural::bezier_surface` (`kiss3d: procedural/bezier.rs:82`) | `kind` word `bezier_patch`, `control_points` | | as above |
| `procedural::quad(w, h, usubdivs, vsubdivs)` (`kiss3d: procedural/quad.rs:28`) | `segments` as vec2 on `plane` | | as above |
| `procedural::quad_with_vertices` (`quad.rs:65`) | `kind` word `grid`, `heights` | | as above |
| `PolylinePattern` with `ArrowheadCap` (`kiss3d: procedural/path/polyline_pattern.rs:48`, `arrowhead_cap.rs:19`) | `kind` word `extrusion`, `path`, `profile`, `start_cap`, `end_cap` | | as above |

### `multimesh3d`

Exposed: component `source`, `texture`, `material`, `cast_shadow`,
`light_layers`, `render_layers`; asset `mesh`, `instances` (`position`,
`rotation_euler`, `scale`, `color`, `custom`), `visible_instance_count`.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `InstanceData3d::{lines_color, lines_width, points_color, points_size}` (`object3d.rs:843-850`) | instance `wireframe_color`, `wireframe_width`, `dot_color`, `dot_size` | none | as the renderable keys |
| `InstanceData3d::deformation` (`object3d.rs:840`) | instance `basis` (9 floats) | | Shear is dropped today |
| `Object3d::set_color` on the node | `color` | white | Forced to white (`crates/balaur_render/src/multimesh.rs:526-527`); `multimesh2d` has it |
| `instance_compute_buffers` (`object3d.rs:1412`) | none | | A compute pass writes instances; Rust only, not scene data |

### `text3d`

Exposed: the text and font keys, `billboard`, `double_sided`, `depth_test`
(defect 2), `pixels_per_unit`, outline and shadow keys. kiss3d's
`TextRenderer` is screen-space only (`kiss3d: text/renderer.rs:270`), so there
is no other kiss3d text to wrap; the shaping side is section 13.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `AlphaMode::Mask(cutoff)` (`object3d.rs:163`) | `alpha_cutoff` | 0 (blend) | Defect 3 |
| `set_casts_shadows` (`object3d.rs:2295`) | `cast_shadow` | true | |
| `set_light_layers`, `set_render_layers` | `light_layers`, `render_layers` | -1 | Defect 4 |
| `SceneNode3d::set_material` (`scene_node3d.rs:1517`) | `material` | "" | Text nodes ignore `Appearance.material` |

### `boolean3d`

Exposed: `operation`. The result's renderable is hard-coded
(`crates/balaur_render/src/boolean.rs:366-379`).

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `set_color` | `color` | 0.8 grey today | |
| `set_texture` | `texture` | "" | The CSG keeps UVs (`crates/balaur_core/src/csg.rs:410-419`); `Shape3d::Built` attaches none |
| `set_material` | `material` | "" | Works today only through an inherited `material` component |
| `set_casts_shadows`, layers | `cast_shadow`, `light_layers`, `render_layers` | true, -1, -1 | |

The CSG is Balaur's own; its `ON_PLANE` and `MAX_DEPTH` (`csg.rs:16,20`) are
constants, and the operands' own materials are lost in the result.

### `bone3d`

No kiss3d counterpart a scene can build. Balaur's GPU skin reads the first
128 joints (`MAX_JOINTS`, `crates/balaur_render/src/shaders.rs:80`) and drops
the rest without a word; kiss3d's palette has no cap. Raising the cap is
**pipeline**; using kiss3d's skin is **fork** (see `mesh`).

## 4. 2D drawing

Three things draw a 2D node. kiss3d's `ObjectMaterial2d` draws `sprite`,
`shape2d`, `tilemap`, `text2d` and `particles2d`, and honours every per-object
flag. Balaur's `SkinnedMaterial` (`crates/balaur_render/src/skinned_2d.rs`)
draws `polygon`, `multimesh2d` and `boolean2d` and reads colour and texture
alone. Balaur's light map draws `light2d` and `occluder2d`. kiss3d's
`Light2d`, `LitMaterial2d`, `Gi2d`, `FixedView2d`, `scene::Tilemap`,
`SkinnedMesh2d`, `PointRenderer2d` and nine-slice are unused.

### Keys every 2D drawable takes

On `sprite`, `shape2d`, `polygon`, `tilemap`, `text2d`, `multimesh2d`,
`particles2d` and `boolean2d`, from `ObjectData2d`
(`kiss3d: scene/object2d.rs:88-104`). Each joins `BatchKey`
(`crates/balaur_render/src/batch_2d.rs:38-45`) or rides an instance override.
On the `SkinnedMaterial` nodes and any node with a `material` asset each is
**pipeline**. The names match section 3.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `Object2d::set_blend(Blend2d)` (`object2d.rs:24-43,620`) | `blend_mode` (`auto`/`alpha`/`premultiplied`/`add`/`multiply`/`screen`/`opaque`) | `auto`, from the texture's `premultiply` | `Blend2d::blend_state` is `pub(crate)` (`object2d.rs:48`): **fork**, or Balaur copies it |
| `set_lines_width`, `set_lines_color` (`object2d.rs:688,701`) | `wireframe_width`, `wireframe_sizing`, `wireframe_color` | 0, white | Every triangle edge, not the outline |
| `set_points_size`, `set_points_color` (`object2d.rs:716,729`) | `dot_size`, `dot_sizing`, `dot_color` | 0, white | |
| `set_surface_rendering_activation` (`object2d.rs:741`) | `draw_surface` | true | |
| `enable_backface_culling` (`object2d.rs:612`) | `cull_back_faces` | | **fork**: stored, never read; every 2D material builds with `cull_mode: None` |
| `set_normal_map`, `set_lit_params` (`object2d.rs:633,653`; `LitParams` at `kiss3d: builtin/lit_material2d.rs:28-42`) | `normal_map`, `specular_strength`, `shininess`, `normal_strength` | "", 0.0, 16.0, 1.0 | Read only by `LitMaterial2d` (below) |

### `sprite`

Exposed: `texture`, `sheet`, `frame`, `flip_x`, `flip_y`, `pixels_per_unit`,
`offset`, `centered`, `size`, `region_origin`, `region_size`, `color`,
`material`.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `SceneNode2d::nine_slice(size, world, uv)` (`kiss3d: scene/scene_node2d.rs:522`, `Border` at `scene/sprite.rs:58-67`) | `nine_slice_margins_pixels` (left, right, top, bottom) | [0, 0, 0, 0], off | The mesh bakes `size`; `set_uv_rect` flattens the slices; batching needs `nine_slice_mesh`, `pub(crate)` (`sprite.rs:93`): **fork**. A `sprite_sheet` slice's `center` is parsed (`crates/balaur_render/src/sheet.rs:251-255`) and drawn by nothing |

### `polygon`

Exposed: `mesh`, `texture`, `pixels_per_unit`, `skeleton`, `color`.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `Object2d::set_material` (`object2d.rs:677`) | `material` | "" | The sync applies one already (`crates/balaur_render/src/sync_2d.rs:254-262`); `apply_polygon` never writes it |

The 2D drawable keys and defect 5 apply.

### `shape2d`

Exposed: `kind` and its sizes, `points`, `mesh`, stroke keys (`width`,
`closed`, `join`, `cap`, `miter_limit`, `taper`), `gradient`,
`gradient_steps`, `texture`, `color`, `material`.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `Object2d::set_texture` (`object2d.rs:882`) | `texture` on every kind | | The mesher emits UVs; `texture` is read for `polyline` only (`crates/balaur_render/src/shape.rs:150-155`) |

A polyline is a group of pieces, so each 2D drawable key goes through the
`*_recursive` setter.

### `text2d`

Exposed: the same text keys as `text3d` without the 3D ones. Balaur shapes
and draws one mesh node per layer. The shaping side is section 13.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `Object2d::set_material` | `material` | "" | `sync_text` ignores an inherited one too (`crates/balaur_render/src/text_component.rs:329`) |

Each 2D drawable key reaches all three layer nodes. Defect 13 applies.

### `multimesh2d`

Exposed: `source`, `texture`, `color`.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `InstanceData2d::uv` (`object2d.rs:231`) | instance `region_origin`, `region_size`; `set_instance_region` | whole texture | **pipeline**: `SkinnedMaterial` binds no uv buffer |
| `InstanceData2d` overlay overrides (`object2d.rs:221-227`) | instance `wireframe_color`, `wireframe_width`, `dot_color`, `dot_size` | none | **pipeline** |
| `Object2d::set_material` | `material` | "" | `multimesh3d` has it |
| Default UV scale | `pixels_per_unit` | | Fixed to the default (`crates/balaur_render/src/multimesh.rs:540`); `polygon` has it |

### `tilemap`

Exposed: `tileset`, `cells`, `flags`, `terrain`, `origin`, `seed`,
`pixels_per_unit`, `material`. Balaur builds the chunk meshes and covers all
of kiss3d's `scene::Tilemap`.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `Object2d::set_color` | `color` | white | Only the node tint reaches it (`crates/balaur_render/src/tilemap_mesh.rs:134-139`) |

### `camera2d`

Exposed: `current`, `pixels_per_unit`, `ambient_color`, `hidpi`, `post` and
its knobs. `PanZoomCamera2d`'s pan and zoom controls are not wrapped, as in
3D.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `FixedView2d`, `CoordinateSystem2d` (`kiss3d: camera/fixed_view2d.rs:9-53`) | `origin` (`center`/`top_left`) | center | `FixedView2d` has no pan or zoom: a Balaur `Camera2d` implementation (the trait is public) |
| `FixedView2d::apply_hidpi` (`fixed_view2d.rs:52`) | `hidpi` | true | Balaur multiplies zoom by the scale factor itself (`kiss3d_camera.rs:147`) |
| Camera rotation | none | | No kiss3d 2D camera turns; the node's rotation and scale are ignored. A Balaur `Camera2d` |
| The post passes and knobs of `camera3d` | the same keys | | Shared with `camera3d`; `edges` is **web** |
| `Gi2d` | `post` word `gi` and its knobs | | Below |

The `ambient_color` alpha is dropped
(`crates/balaur_render/src/camera.rs:391`).

### `light2d`

Exposed: `kind` (`point`/`directional`), `color`, `range`, `intensity`,
`shadow_enabled`. Balaur's light map draws it.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `Light2dKind::Spot`, `direction`, `inner_angle`, `outer_angle` (`kiss3d: light2d.rs:29,50-68`) | `kind` word `spot`; `inner_angle_degrees`, `outer_angle_degrees` | 20, 35, as `light3d` | **pipeline**: `fs_light` has no cone term (`crates/balaur_render/src/shaders/light2d.wesl:72-82`). Aim from the node's rotation |
| `Light2d::height` (`light2d.rs:40`) | `height` | 1.0 world unit | Built; read only by a node with a `normal_map` |
| `GiEmitter2d::radius` (`kiss3d: post_processing/gi2d.rs:54`) | `source_radius` | 0.25 | Built; read only by `gi` |
| `color` alpha | | | Accepted, then dropped (`crates/balaur_render/src/light.rs:81`) |
| `MAX_LIGHTS_2D` (`light2d.rs:19`) | none | 64 | The fork raised it from 16 and added a directional kind, matching Balaur's light map |

### `occluder2d`

Exposed: `mesh`, `closed`. Balaur's own stencil extrusion. `Gi2d` occluders
are discs (`gi2d.rs:75-80`), capped at 64 (`:43`) and truncated silently, so
an outline under `gi` needs a polygon occluder: **fork**, or a bounding-disc
`radius`.

### `boolean2d`

Exposed: `operation` (`union`/`difference`/`intersection`), over i_overlay.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `OverlayRule::Xor`, `InverseDifference` (`i_overlay: core/overlay_rule.rs:13-21`) | `operation` words `symmetric_difference`, `reverse_difference` | | `Subject` and `Clip` pass one operand through |
| `FillRule` (`i_overlay: core/fill_rule.rs:9-15`) | `fill_rule` (`even_odd`/`non_zero`/`positive`/`negative`) | even_odd today, non_zero in the crate | Hard-coded (`crates/balaur_core/src/geometry2d.rs:341`) |
| `OverlayOptions` (`i_overlay: float/overlay.rs:34-52,496-508`) | `min_area`, `keep_collinear`, `clean_result` | 0.0 | |
| Holes | none | | Balaur keeps the first ring only (`crates/balaur_render/src/boolean.rs:408-430`) |
| `set_color`, `set_texture`, `set_material` | `color`, `texture`, `material` | | |

### `screen_notifier2d`

Balaur's own; defect 14 only.

### The rest of the 2D renderer

**`Gi2d`, 2D global illumination** (`kiss3d: post_processing/gi2d.rs:191-740`),
unwrapped. It belongs on `camera2d` as a `post` word, as bloom and SSAO do.

| Setter | Key | Default |
| --- | --- | --- |
| `set_rays` (`:685`) | `gi_rays` | 8 |
| `set_max_distance` (`:690`) | `gi_max_distance` | 2000.0, pixel scale |
| `set_max_steps` (`:695`) | `gi_max_steps` | 32 |
| `set_resolution_scale` (`:702`) | `gi_downscale` | 2 |
| `set_temporal_blend` (`:709`) | `gi_temporal_blend` | 0.85, 0–0.99 |
| `set_radiance_cascades` (`:718`) | `gi_solver` (`ray_march`/`cascades`) | ray_march |
| `set_cascade_count` (`:724`) | `gi_cascade_count` | 5, 1–8 |
| `set_cascade_base_directions` (`:731`) | `gi_cascade_directions` | 16 |
| `set_sdf_occluders` (`:740`) | `gi_screen_occluders` | false |
| `set_ambient` (`:679`) | `ambient_color` | (0.08, 0.08, 0.1) |
| `probe_spacing` (`:239`) | `gi_probe_spacing` | 2 |

Built. Emitters come from `light2d`, a disc of its `source_radius`, capped at
32; a spot has no cone there and a directional light no form. Occluders are
the `occluder2d` outlines and a tile map's walls as segments, capped at 128.
With `gi` listed the light map does not draw, so nothing is lit twice. The
march is measured in field pixels, so a world of any scale resolves the same.
Whether its `Rgba32Float` target works on the web is not yet checked.

**`LitMaterial2d` with `Light2dManager`** (`kiss3d: builtin/lit_material2d.rs:131`,
`light2d.rs:119-179`) is per-pixel, normal-mapped 2D lighting, and the only
route to the `normal_map` keys without a normal buffer in Balaur's light map.
Built on `sprite` and `shape2d`: a node naming a `normal_map` draws with it,
lit by every `light2d` at its `height`. Such a node draws after the light
map's composite, as particles do, so over every node the light map lit
whatever its `z_index`. No light casts a shadow on it, and the light set is
one per thread, so every camera sees the same lights.

## 5. `particles2d` and `particles3d`

Exposed: `emitting`, `rate`, `lifetime`, `speed`, `direction`,
`spread_degrees`, `gravity`, `size`, `size_end`, `color`, `color_end`,
`texture`, `one_shot`, `explosiveness`. Balaur simulates them and draws one
`add_rectangle` node per live particle in the 2D scene
(`crates/balaur_render/src/particles.rs:345-372`).

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| The 2D drawable keys, `blend_mode = "add"` above all | as section 4 | | Honoured by this draw path |
| `SceneNode2d::set_rotation` (`scene_node2d.rs:1580`) | `rotation_degrees`, `angular_speed_degrees` | 0, 0 | The spread comes from the emitter's own PCG, so determinism holds |
| `SpriteSheet`, `set_sprite_frame` (`kiss3d: scene/sprite.rs:16-56`, `scene_node2d.rs:1201`) | `sheet` | "" | |
| `Object2d::set_material` | `material` | "" | An inherited one is ignored too |
| One instanced node, as `multimesh2d` draws | none | | No schema change; replaces a node per particle |
| A 3D emitter: `SceneNode3d::set_instances` with `InstanceData3d` (`scene_node3d.rs:3101`) | `particles3d`: `direction` and `gravity` as vec3, `size` in world units, `billboard`, `cast_shadow`, `light_layers`, `render_layers`, the 3D overlay keys | | Built, one quad instanced per particle. A sprite sheet does not reach it: a 3D instance carries no texture rectangle |

`size` and `size_end` are logical pixels with no `_pixels` suffix (N21). The
2D emitter was `particles` until `particles3d` gave it a sibling; D5 named it
`particles2d`, with the `2d` tag.

## 6. Rigid bodies

### `body3d` and `body2d`

Exposed: `kind`, `lock_translation`, `lock_rotation`, `mass`, `inertia`,
`center_of_mass`, `linear_damping`, `angular_damping`, `gravity_scale`,
`dominance`, `solver_iterations`, `continuous_collision`,
`speculative_distance`, `allow_fast_rotation`, `can_sleep`, `time_to_sleep`,
`enabled`; 3D adds `gyroscopic_forces`.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `set_additional_pgs_iterations` (`rapier: dynamics/rigid_body.rs:239,1813`) | `internal_iterations` | 0 | Applies to the whole island; the largest request wins. The `[physics]` word |
| `RigidBodyActivation.normalized_linear_threshold` (`rapier: dynamics/rigid_body_components.rs:1327`) | `sleep_threshold` | 0.05, scaled by `length_unit` | `can_sleep` needs its own storage first (defect 20) |
| `RigidBodyActivation.angular_threshold` (`rigid_body_components.rs:1334`) | `sleep_angular_threshold` | 0.5 rad/s | Used only for a body with no collider; as above |
| Builder `sleeping` (`rigid_body.rs:2106`) | `start_asleep` | false | **create** |
| Builder `linvel`, `angvel` (`rigid_body.rs:2040,2048`) | `initial_linear_velocity`, `initial_angular_velocity` | 0 | **create**; the plain names are the handle's methods |
| `MassProperties::with_principal_inertia_frame` (`parry3d: mass_properties/mass_properties.rs:193`) | `inertia_rotation` (3D) | [0, 0, 0] | Identity today (`crates/balaur_physics/src/body.rs:183-185`) |
| `dominance` at -128 (`rigid_body.rs:868`, an `i8`) | widen `dominance` | | Schema and code stop at -127 |
| `set_rotation`, `set_position` (`rigid_body.rs:1153,1180`) | `teleport` takes a rotation | | Script call; `teleport` moves translation only |
| `RigidBodyType::SoftFrame` (`rigid_body_components.rs:41-60`) | none | | rapier builds it itself; read back as `soft_frame` |
| `user_data` (`rigid_body.rs:84`) | none | | Holds the entity |

Readers neither `get` nor a script reports: `is_ccd_active` (`:652`), the
world and local centre of mass (`:346,354`; the reader is
`world_center_of_mass()`, since the property has the plain name), the total
principal inertia and its frame, `effective_mass` and
`effective_angular_inertia` (`rigid_body_components.rs:388,395`),
`additional_pgs_iterations`, `activation().time_since_can_sleep`,
`angvel_with_gyroscopic_forces`, `soft_body` and `soft_cluster`, and the
rotation half of `next_position` and `predict_position*`, which return
translation only (`crates/balaur_physics/src/body.rs:755-786`). 2D also lacks
`predict_position_with_forces` (`rigid_body.rs:1265`).

The docs say more than rapier does in three places, fixed with the keys:
`solver_iterations` substeps the whole island, not one body
(`rigid_body.rs:218-226`); without `continuous_collision` a fast dynamic body
still sweeps fixed colliders, and the flag adds kinematic and dynamic ones
(`:591-598`); and a kinematic body keeps its own dominance group, only `static`
outranks every group (`rigid_body_components.rs:1293-1299`).

## 7. Colliders

### `collider3d`

Exposed: every shape kind Balaur lists, sizes, `mesh`, `heightfield`,
`voxels`, `voxel_size`, `fill`, `fit`, `fix_internal_edges`,
`weld_vertices`, `oriented`, `offset`, `offset_rotation`, the material,
layers, events and one-way keys. Every combine rule, `ActiveEvents` flag and
`ActiveCollisionTypes` flag is reached.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `capsule_x`, `capsule_z` (`rapier: geometry/collider.rs:877,899`) | `up_axis` (`x`/`y`/`z`) | y | The vehicle's axis words |
| `capsule_from_endpoints` (`collider.rs:872`) | `kind = capsule` reads `a`, `b` | | A rule for which wins over `height` |
| `round_convex_hull`, `round_convex_mesh`, `round_convex_decomposition(_with_params)` (`collider.rs:1054-1142`) | `edge_radius` on those kinds | | Rounds box, cylinder, cone and triangle today |
| `convex_mesh` (`collider.rs:1134`) | `kind` word `convex_mesh` | | Points taken as convex, no hull |
| `VHACDParameters` (`parry3d: transformation/vhacd/parameters.rs:157-489`) via `convex_decomposition_with_params` (`collider.rs:1068`) | `max_concavity`, `max_convex_hulls`, `resolution`, `symmetry_bias`, `revolution_bias`, `plane_downsampling`, `hull_downsampling`, `approximate_hulls` | 0.01, 1024, 64, 0.05, 0.05, 4, 4, true | `collider2d` has the first three already; 3D reaches them only through `geometry3d.convex_decomposition` |
| `convex_decomposition` method | `method` (`vhacd`/`voxels`) | vhacd | `voxelized_convex_decomposition` (`parry3d: shape/shared_shape.rs:470,483`) returns several shapes: a compound |
| `FillMode::FloodFill { detect_cavities }` (`parry3d: transformation/voxelization/voxelized_volume.rs:122-125`) | `fill_cavities` | false | |
| `voxels_from_points` (`collider.rs:802`) | `kind` word `voxelized_points` | | |
| `Voxels::set_voxel_size` (`parry3d: shape/voxels/voxels_edition.rs:13`) | `voxel_size` on `kind = voxels` | | The asset decides it today |
| `TriMeshFlags` (`parry3d: shape/trimesh.rs:468-512`) | `topology`, `connected_components`, `drop_duplicate_triangles`, `two_sided_edges`, `deformable`; `weld_vertices` becomes `merge_vertices`, `drop_degenerate_triangles`, `drop_bad_topology` | false | **two spellings** if `weld_vertices` stays beside them. `deformable` needs a script verb over `TriMesh::set_vertices` and a `shape_revision` bump |
| `PolylineFlags::DEFORMABLE` (`parry3d: shape/polyline.rs:41`) | `deformable` | false | as above |
| Polyline index buffer | `edges` (`chain`/`mesh`) | chain | `None` is passed, so the mesh's indices are ignored (`crates/balaur_physics/src/collider.rs:100-102`) |
| `HeightFieldCellStatus` (`parry3d: shape/heightfield3.rs:25-31,517`) | a `holes` row on the `heightfield` asset | | Per-cell data belongs to the asset; a run-time edit bumps `shape_revision` |
| `ColliderBuilder::mass_properties` (`collider.rs:1373,551`) | `center_of_mass`, `inertia`, `inertia_rotation` | | |
| Density 0, a massless collider | `density` minimum 0 | | Balaur uses 0 internally for "the body states its mass" (`crates/balaur_physics/src/shared/body.rs:103-109`); needs a flag of its own first |
| Restitution above 1 (`collider.rs:289`) | raise `restitution`'s `max` | | |
| `InteractionTestMode::Or` (`rapier: geometry/interaction_groups.rs:64-82`) | `collision_test`, `solver_test` (`both`/`either`) | both | When two colliders differ, rapier prefers `And` |
| `update_as_oneway_platform`'s `allowed_angle` (`rapier: pipeline/physics_hooks.rs:135`) | `one_way_angle` | 0.1 rad | Must ride `user_data` (64 bits hold the entity, 3 the axis) or a `Sync` table: **digest** |
| One-way axis precision | `one_way_axis` as given | | Snapped to six directions by a 3-bit code (`collider.rs:463-473`) |
| `SolverContact.tangent_velocity` (`rapier: geometry/contact_pair.rs:810`) | `surface_velocity` | [0, 0, 0] | Conveyor belts; set in the `Sync` hook with the data in `user_data` or a side table |
| `FILTER_CONTACT_PAIRS`, `FILTER_INTERSECTION_PAIR` (`physics_hooks.rs:216-221`) | none | | Script hooks left with the threaded solver; `docs/PLAN-rapier.md` item 1 |
| `ColliderBuilder::compound` (`collider.rs:763`) | none | | Child nodes are the compound: one compound is one material |

Readers: `Collider::mass_properties` (`collider.rs:656`),
`compute_collision_aabb` and `compute_broad_phase_aabb` (`:604,623`),
`position_wrt_parent` (`:411`), `CollisionEventFlags::SENSOR` and `REMOVED`
and the `ContactPair` on collision start (`rapier: geometry/mod.rs:102-108`),
and `ContactForceEvent.total_force` as a vector, `max_force_magnitude` and
`started` (`:204-224`). Every reader acts on the node's first collider
(`crates/balaur_physics/src/shared/collider.rs:84-95`).

### `collider2d`

Exposed: twelve kinds (circle to voxels), sizes, `mesh`, `heightfield`,
`scale`, `voxels`, `fix_internal_edges`, `weld_vertices`, `oriented`,
`overlap`, `method`, `resolution`, `max_concavity`, `max_convex_hulls`,
`offset`, `offset_rotation`, `one_way_axis` and the shared collider keys.

Every 3D row above applies where rapier2d has the feature: the round shapes
(`round_convex_hull` at `collider.rs:1110`; `edge_radius` is ignored on
`convex_hull` today), `mass_properties` (`center_of_mass`, `inertia`),
massless density, `collision_test` and `solver_test`, `one_way_angle`,
`surface_velocity`, the `TriMeshFlags` split, `deformable` on polylines, and
the VHACD keys `symmetry_bias`, `revolution_bias`, `plane_downsampling`,
`hull_downsampling` and `approximate_hulls`. Rows that are 2D's own:

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `voxelized_mesh` (`collider.rs:808`) | `kind` word `voxelized_mesh`, `voxel_size`, `fill`, `fill_cavities`, `fix_self_intersections` | 0.25, solid, false, false | Takes segment indices; `boundary()` already computes them (`crates/balaur_physics/src/dim2/collider.rs:286-298`) |
| `converted_trimesh` with `MeshConverter` (`collider.rs:1033`) | `kind` word `fit`, `fit` (`convex_hull`/`aabb`/`obb`) | | The 2D converter has no decomposition variant; defect 15 must not repeat |
| `convex_polyline`, `convex_polyline_unmodified` (`collider.rs:1118`, `parry2d: shape/shared_shape.rs:595,607`) | `kind` word `convex_polygon`, `keep_collinear` | false | |
| `voxels_from_points` (`collider.rs:802`) | `kind` word `voxelized_points` | | |
| `capsule_x`, `capsule_from_endpoints` (`collider.rs:872,877`) | `up_axis` (`x`/`y`), or `a`/`b` | y | |
| Polyline index buffer (`collider.rs:947`) | `edges` (`chain`/`outline`/`mesh`) | chain | `outline` from `boundary()` |
| `HeightField::set_segment_removed` (`parry2d: shape/heightfield2.rs:191`) | a `holes` row on the asset, or `set_heightfield_hole` | | A run-time edit bumps `shape_revision` |
| `Voxels::set_voxel_size`, `crop`, `split_with_box`, `combine_voxel_states` (`parry2d: shape/voxels/voxels_edition.rs:13,208-281`) | `voxel_size` on `kind = voxels`; script calls for the rest | | |

Readers 2D still lacks, from `docs/PLAN-rapier.md` item 5: `aabb`,
`swept_aabb`, `closest_points`, `time_of_impact`, `collider_mass`,
`collider_volume`, `collider_mesh`, `handles`, `set_collider`,
`predict_position_with_forces` and `solve_ik`. `max_contact_impulse` sits on
`body2d` here and on `collider3d` in 3D. 3D's `convex_decomposition` has no
`method` and no VHACD keys; 2D has both.

### `tile_collision`

Exposed: the shared collider keys.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| One-way axis | `one_way_axis` | [0, 1] | Always +y today (`crates/balaur_physics/src/dim2/tiles.rs:244`), flip flags ignored |
| `allowed_angle` | `one_way_angle` | 0.1 rad | as `collider2d` |
| `round_convex_hull` for shaped tiles | `edge_radius` | 0 | Voxel cells have no border to round |
| Concave tile polygons, hulled silently (`tiles.rs:195`) | `fit` (`convex_hull`/`aabb`/`obb`/`convex_decomposition`) | convex_hull | |
| Per-tile material | none | | One table for every collider; voxels carry no per-cell data (`crates/balaur_core/src/tiles/mod.rs:98-99`), so a collider per material group |
| The `collider2d` mass, test-mode and surface-velocity rows | the same keys | | |

`get` returns the authored table; nothing reports the collider count, mass or
bounds.

## 8. Joints

### `joint3d` and `joint2d`

Exposed: `kind`, `connected_body`, `anchor`, `connected_anchor`, `axis`,
`limits`, `lock_translation`, `lock_rotation`, `motor`, `motor_target`,
`motor_max_force`, `motor_model`, `stiffness`, `damping`, `max_length`,
`rest_length`, `collide_connected`, `break_force`, `articulation`, `enabled`.
2D adds `groove`; the pin-slot joint is 2D only.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `set_local_frame1`, `set_local_frame2` (`rapier: dynamics/joint/generic_joint.rs:427,433`) | `anchor_rotation`, `connected_anchor_rotation` | 0 | The zero a hinge's limits measure from; identity today, so a fixed joint pulls two bodies to equal rotation |
| `set_local_axis2` (`generic_joint.rs:467`) | `connected_axis` | `axis` | `axis` is written to both ends today |
| `set_softness(SpringCoefficients)` (`generic_joint.rs:307,509`; `rapier: dynamics/integration_parameters.rs:78-83`) | `softness_hz`, `softness_damping_ratio` | 1e6, 1.0 | |
| `coupled_axes` (`generic_joint.rs:293,787`) | `coupled_translation`; 3D adds `coupled_rotation` (flags) | none | The first coupled axis's limits and motor apply to all |
| Per-axis limits and motors (`JointAxis`, `generic_joint.rs:103-118,526-579`) | `axes`, a list of records `{ axis, limits, motor, motor_target, motor_target_velocity, motor_max_force, motor_model, stiffness, damping }`, `axis` one of `x`/`y`/`z`/`rotation_x`/`rotation_y`/`rotation_z` (2D: `x`/`y`/`rotation`) | none | Fixes defect 21. The flat keys stay as the shorthand for the joint's own free axis; a joint names one or the other: **two spellings** (section 18) |
| `set_motor(pos, vel, stiffness, damping)` (`generic_joint.rs:596`) | `motor_target_velocity` | 0 | Read when `motor = "position"` |
| `SpringJoint::set_spring_model` (`rapier: dynamics/joint/spring_joint.rs:89`) | `motor_model` reaches the spring | force-based for a spring | Fixes defect 25; the spring kind takes its own default |
| `GenericJoint::set_enabled`, `JointEnabled` (`generic_joint.rs:263-271,405`) | `enabled` sets it instead of deleting | | `DisabledByAttachedBody` is a reader |
| `MultibodyJointSet::insert_kinematic` (`rapier: dynamics/joint/multibody_joint/multibody_joint_set.rs:113`) | `kinematic_link` | false | `PhysicsWorld` wraps only `insert_multibody_joint`, so Balaur goes to `world.multibody_joints` |
| `Multibody::set_self_contacts_enabled` (`multibody.rs:361`) | `self_collision` | true | One setting for the whole chain |
| `Multibody::damping_mut`, `armature_mut`, `frictions_mut` (`multibody.rs:425,439,452`) | `link_damping`, `armature`, `friction` | 0.1 on free angular axes, 0, 0 | Per-chain vectors indexed by degree of freedom; the index moves when a link is added, so they are rewritten after each change |
| `MultibodyJoint::set_spring` (`multibody_joint.rs:57`) | `passive_stiffness`, `passive_rest` | 0, 0 | |
| `Multibody::add_dof_coupling` (`multibody.rs:72-91,1123`) | `gear_with` (node), `gear_ratio`, `gear_offset` | none, 1, 0 | Both joints in one articulation |
| `InverseKinematicsOption` (`multibody_ik.rs:8-40`) | `solve_ik` options `damping`, `iterations`, `tolerance`, `constrain` (flags), `rotation` | 1.0, 10, 1e-3, all | Script options table (N9); fixes defect 18 when `constrain` defaults to translation |

Readers: revolute `angle()` (`rapier: dynamics/joint/revolute_joint.rs:97`),
per-axis `JointLimits.impulse` and `JointMotor.impulse`
(`generic_joint.rs:148,215`), the impulse's six components
(`rapier: dynamics/joint/impulse_joint/impulse_joint.rs:23`; one magnitude is
reported), `MultibodyJoint::coords()` (`multibody_joint.rs:107`) and
`Multibody::joint_velocity` (`multibody.rs:1240`), and the live limits and
motor rather than the authored ones (defect 23). `break_force` becomes a
force with its linear and angular parts apart: `break_force` and
`break_torque`, compared against impulse divided by the step (defect 22).

### `ragdoll`

Exposed: `bodies`, `influence`; the build call `physics3d.ragdoll` takes
`thickness`, `density`, `friction`, `limits`, `influence`. rapier has no
ragdoll type: it is a composition of `body3d`, `collider3d` and `joint3d`.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| Multibody joints | build option `articulation` | false | Never set today (`crates/balaur_physics/src/ragdoll.rs:351-369`) |
| Twist and swing limits | per-axis `axes` on each joint | | Waits on the joint rows above; one pair lands on three axes today |
| `PdController`, joint motors | build option `drive` (`none`/`motors`/`follow`) | none | `influence` blends bone transforms after the step; nothing pulls the bodies toward the clip |
| Body, collider and joint settings | build options `restitution`, `linear_damping`, `angular_damping`, `collision_layer`, `collision_mask`, `continuous_collision`, `gravity_scale`, `collide_connected`, `shape` | | The generated nodes carry ordinary components, each reachable after the build |

The `physics3d.ragdoll` doc says "hinged to its parent's"; 3D builds a
`ball_socket` (`ragdoll.rs:352`).

## 9. Characters, vehicles and a follow controller

### `character3d` and `character2d`

Exposed: `up_direction`, `safe_margin`, `slide`, `step_height`,
`step_min_width`, `step_on_dynamic`, `floor_max_angle`, `min_slide_angle`,
`floor_snap_length`, `normal_nudge`, `push_bodies`, `lengths`. The 2D code
mirrors the 3D code line for line.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `CharacterLength` per field (`rapier: control/character_controller.rs:70-72,180,196`) | `safe_margin_lengths`, `step_height_lengths`, `step_min_width_lengths`, `floor_snap_lengths` (`absolute`/`relative`) | relative in rapier | Replaces `lengths`: **two spellings** if both stay |
| `solve_character_collision_impulses`'s `character_mass` (`:880-886`) | `push_mass` | 0, the body's mass | Floored at 1 today |
| `QueryFilter.flags` (`rapier: pipeline/query_pipeline.rs:593-610`) | `ignore` (flags `static`/`kinematic`/`dynamic`/`sensors`/`solids`) | none | Defect 31: `groups` from the collider's own layers and the own body excluded need no key |
| `QueryFilter.predicate` (`query_pipeline.rs:690`) | `ignore_nodes` (list of node) | [] | A closure would call Rune once per candidate mid-query |
| `CharacterCollision.character_pos`, `translation_applied`, and `hit` (`character_controller.rs:108-110`; `parry3d: query/shape_cast/shape_cast.rs:44-83`) | collision record `position`, `applied`, `distance`, `own_point`, `own_normal`, `status` (`converged`/`out_of_iterations`/`failed`/`penetrating`), `subshape` | | Script return value; `time_of_impact` is a distance here |
| `move_shape` takes any shape (`:297-304`) | none | | Only the first collider is swept (`crates/balaur_physics/src/character.rs:120`); a compound is Balaur work |
| 20 sweep iterations, the depenetration budget, the ground probe, the grounded threshold, platform friction transfer (`:249-612`) | none | | **upstream**: hard-coded |

Defaults that differ from rapier, by choice: `min_slide_angle` π/6 against
π/4, `step_on_dynamic` false against true, stepping on against off, and
absolute lengths against relative. Defect 19 applies to both. The `lengths`
description names rapier's fields and says relative lengths are fractions of
the height; `min_width` is measured against the side extent (`:742`).

### `vehicle3d`

Exposed: `forward_axis`, `up_axis`. rapier's vehicle is 3D only
(`rapier: control/mod.rs:11-19`).

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `current_vehicle_speed` (`rapier: control/ray_cast_vehicle_controller.rs:22,417-422`) | reader `speed()` | | The speed signed by heading; `vehicle_speed` projects onto the forward axis, a different number. Copied out after `update_vehicle` |
| `update_vehicle`'s query filter (`:409`) | `collision_mask`, `ignore` | | Defect 31; one filter per vehicle, not per wheel |
| Axis sign: `index_up_axis`, `index_forward_axis` are 0–2 (`:27-29`) | `forward_axis` words `-x`, `-y`, `-z` | | **upstream**, or Balaur negates engine force and steering |
| The controller across steps (`:14-15`, `Serialize`) | none | | Defect 32; keeping it puts it in the snapshot: **digest** |

### `wheel3d`

Exposed: every `WheelTuning` field and every `add_wheel` argument. Script:
`set_engine_force`, `set_brake`, `set_steering`, `wheel_state()`. Readers
are copied out after the step, because the controller is rebuilt.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `forward_impulse`, `side_impulse` (`ray_cast_vehicle_controller.rs:160-162`) | `wheel_state` fields of the same names | | |
| `RayCastInfo` (`:238-250`) | `wheel_state` fields `contact_normal`, `contact_point`, `suspension_length`, `ray_origin`, `ground` | | Only `in_contact` is read today |
| `center()`, `suspension()`, `axle()` (`:219-229`) | `wheel_state` fields `center`, `suspension`, `axle` | | World space, after steering |
| `Wheel.rotation`, writable (`:153`) | `set_wheel_rotation` | | |
| `roll_influence` (`:155,200`) | `roll_influence` | 0.1 | **upstream**: private; rapier's own TODO asks to make it public |
| `skid_info` (`:175`) | `wheel_state` field `skid` | | **upstream**: private |
| `side_factor`, `fwd_factor` in `update_friction` (`:602-603`) | none | 1.0, 0.5 | **upstream**: hard-coded |

The docs say more than rapier in four places, fixed with the readers: the
suspension moves rest length ± travel, so twice `suspension_travel` in total
(`:371-376`); `brake` is an impulse and is ignored while `engine_force ≠ 0`
(`:614-618`); `suspension_stiffness` is scaled by chassis mass (`:531`); and
the two stiffness defaults differ from rapier's (30 against 5.88, 0.82
against 0.83).

### `follow3d` and `follow2d`, new

rapier's `PdController` and `PidController`
(`rapier: control/pid_controller.rs:14-73`) turn a target pose into a velocity
change, and no Balaur crate uses them. A body has no target, a joint motor
drives one body against another, and a character is kinematic, so this is a
new component pair, run each fixed step as the vehicle system is
(`crates/balaur_physics/src/vehicle.rs:32-36`).

| Key | Maps to | Default |
| --- | --- | --- |
| `kind` (`pd`/`pid`) | which controller | pd |
| `target` (node) | the target pose | |
| `target_linear_velocity`, `target_angular_velocity` | target velocity | 0 |
| `position_gain`, `velocity_gain`, `integral_gain` | `lin_kp`, `lin_kd`, `lin_ki` | 60, 0.8, 1.0 |
| `rotation_gain`, `spin_gain`, `rotation_integral_gain` | `ang_kp`, `ang_kd`, `ang_ki` (scalars in 2D) | 60, 0.8, 1.0 |
| `translation_axes`, `rotation_axes` | `axes` (`rapier: dynamics/rigid_body_components.rs:228-244`), the words `lock_*` uses | all |
| `follow_rotation` in 2D, for the one turn | `AxesMask::ANG_Z` | true |

Built. A script `reset_follow()` maps to `reset_integrals` (`pid_controller.rs:277-300`).
A ragdoll's `drive = "follow"` puts a `follow3d` or `follow2d` on every body,
its keys in a `follow` table, aimed at the bone the body was built from.
The PID integrals change each step: **digest**. The gains are tied to the
fixed step.

## 10. Soft bodies

### `softbody3d` and `softbody2d`

Every `SoftBodyMaterial` and `SoftBodyParticleSettings` field is reached
(`crates/balaur_physics/src/shared/softbody.rs:31-79`). Rows apply to both
unless marked. `sb/` is `rapier: dynamics/soft_body/`, `sbb/` its
`soft_body_builder/`.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `new(positions)` with `edges`, `bend_edges`, `cells`, `surface`, and 3D `dihedrals`, `wire` (`sbb/soft_body_builder_settings.rs:59-240`) | `kind` word `custom`; `points`, `edges`, `bend_edges`, `surface`, `wire`, `cells`, `dihedrals` | | Index lists |
| `add_edges` (`soft_body_builder_settings.rs:66`) | `seams`, a list of `{ a, b }` | [] | `append` would need two layouts in one schema |
| `tension_only_edges` (`sbb/soft_body_builder.rs:63-65`) | `tension_only_edges` | [] | `tension_only` is all or nothing; **two spellings** decided as `axes` is |
| `edge_softness` over bending edges (`soft_body_builder.rs:66-68`) | `edge_springs` reaches them | | Structural edges only today (`shared/softbody.rs:710`) |
| `oriented: Option<bool>` (`soft_body_builder.rs:96-98`) | `orientation` (`auto`/`solid`/`shell`) | auto | Replaces `oriented` (defect 30) |
| 3D `cloth_tube(radius_start, radius_end)` (`sbb/soft_body_builder_shapes.rs:258-265`) | `end_radius` | 0, `radius` | |
| 3D `cloth_anisotropic` springs (`shapes.rs:318-327`) | `warp_damping`, `weft_damping`, `shear_damping` | 0, `edge_damping` | |
| `volumetric_with(VolumeMeshParameters)` (`shapes.rs:443-456`; `parry3d: transformation/volume_mesh/mod.rs:35-76`) | `fill` (`solid`/`surface`), `smoothing`, `smoothing_guard`, `boundary_subdivisions`; 2D `min_angle` | cover, 0, 0.15, 0; π/6 | |
| `.skin()` on any body with cells (`soft_body_builder_settings.rs:215`) | `skin` on every kind with cells | | Reached through `volumetric` only; 2D `volumetric` is always skinned |
| 2D `polyline(vertices, indices)` (`shapes.rs:140`) | `edges` (`chain`/`mesh`) | chain | `None` is passed |
| 2D `polygon(points)` (`shapes.rs:532`) | `kind` word `outline` | | Balaur's `polygon` is built another way |
| Generator defaults (`shapes.rs:79,130,174`) | `shape_matching` gains `auto` | auto | Defect 30 |
| Surface `ColliderBuilder` (`sb/soft_body_set/soft_body_set_proxies.rs:117-121`) | `friction_combine`, `restitution_combine`, `solver_layer`, `solver_mask`, `contact_pairs`, `sensor` | | Friction, restitution, groups and events only today |
| `PhysicsWorld::insert_deformable` with `Direct`, `DirectByPosition`, `Skinned` (`rapier: pipeline/physics_world.rs:446-460`, `sb/collision_mesh/collision_mesh_binding.rs:140-195`) | `collision_mesh`, `collision_binding` (`nearest`/`particles`/`cells`), `collision_binding_distance`, `collision_self_contacts` | nearest, 0.01 | Built on the whole-body cluster, beside the layout's own surface. `particles` binds vertex i to particle i |
| `set_enabled` (`sb/soft_body_accessors.rs:291`) | `enabled` | true | Live, no rebuild |
| Live material edits | none | | `linear_damping`, `gravity_scale`, `solver_substeps`, `can_sleep` and `dominance` rebuild from rest: no setter in rapier. Friction, restitution, layers and events rebuild by Balaur's choice; rapier's collider has setters |

Script calls and readers:

- **Forces and attachments** — `add_particle_force` (`sb/soft_body_motion.rs:100`),
  `particle_attachments()` with each attachment's `impulse()` (`:13-27,83`).
- **Particles** — force, kinematic target, rest position, initial rest
  position, mass, inverse mass, pinned, damaged, on the surface
  (`sb/soft_body_elements.rs:81-131`).
- **Edges, dihedrals, cells** — rest length, kind, softness, tear resistance,
  impulse, plastic strain, initial rest length; 3D rest angle and plastic set;
  cell rest volume, stiffness scale, plastic stretch, stress
  (`soft_body_elements.rs:149-335`, `sb/soft_body_plasticity.rs:38,44`).
  Balaur reads vertices and edge `stress()` alone.
- **Shape** — `boundary()`, `volume_pieces()`, `particle_radius()`, `mass()`
  (`soft_body_accessors.rs:102,169,205,263`).
- **Tearing and cutting** — `tear_edge`, `tear_cell`, `has_pending_tears`,
  `crossing_elements` (`sb/tearing/tearing.rs:29-56`), `cut_soft_body`,
  `tear_soft_body` (`physics_world.rs:476,495`), `set_edge_tear_resistance`,
  `set_cell_tear_resistance`, `set_particle_damaged`
  (`sb/tearing/tearing_helpers.rs:151-169`), `reset_plasticity()`
  (`soft_body_plasticity.rs:236`).
- **Regions** — `add_soft_body_cluster`, `remove_soft_body_cluster`
  (`physics_world.rs:517,529`) and the six `set_cluster_*` and
  `enable_cluster_shape_matching` calls (`sb/soft_body_cluster.rs:131,216-319`),
  as a `regions` list `{ particles, stiffness_scale, tear_resistance,
  shape_matching, pinned, hz, damping }`, built. Each cluster is a rapier body
  of kind `SoftFrame`, so the snapshot carries it and a tear splits it.
- **Joints to a soft body** — the root body and cluster proxies take joints
  (`soft_body_accessors.rs:209-215`); `connected_body` resolves rigid bodies
  only.
- **Contacts** — `edge_contact_segments`, `vertex_contact_segments`,
  `volume_contacts` (`sb/soft_body_contacts.rs:108-138`).
- **The tear event** — `torn_cells`, `removed_edges`, `split_particles`,
  `inserted_particles`, per-piece `particles`, `clusters`, `moved_joints`
  (`sb/tearing/tearing_event.rs:66-101`); Balaur sends a piece count and the
  torn edges. `pieces` is empty when nothing split off and otherwise counts
  the body that kept the handle.
- **Torn-off pieces** — every script call goes through the root handle
  (`shared/softbody.rs:389-405`), so a particle torn into a new piece cannot
  be read or moved.

Docs to fix with the keys: `softbody_stress` is the load as a fraction of the
tear threshold, 0 with no tear criterion (`soft_body_elements.rs:191-197`), not
a stretch; the 3D `mesh` description names a `polyline` kind 3D lacks; and
the `*_frequency` keys are hertz with no `_hz` suffix (N21).

## 11. The physics world

Exposed: `[physics]` and `physics.set_tuning`, 19 keys with all 19 read back
(`crates/balaur_physics/src/tuning.rs:23-76,163-224`); `[physics] threads`;
script `physics3d.set_gravity`, `physics.set_sleeping_allowed`, `set_paused`,
`set_debug_draw` with six modes, `counters` with four numbers, `quarantined`.

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `IntegrationParameters.friction_model` (`rapier: dynamics/integration_parameters.rs:16-30,330`) | `friction_model` (`simplified`/`per_contact`) | simplified | 3D only; multibodies always use Coulomb |
| `normalized_contact_recycle_distance` (`integration_parameters.rs:316,427`) | `contact_recycle_distance` | 0.05 | |
| `PhysicsWorld.gravity` (`rapier: pipeline/physics_world.rs:66,101`) | `gravity_3d`, `gravity_2d` | (0, -9.81, 0) | A recording's header carries both, with the tuning |
| One table for both worlds (`tuning.rs:152-159,240-246`) | `[physics.2d]`, `[physics.3d]` over the shared table | | A 2D game's pixel `length_unit` lands in the 3D world today |
| `IntegrationParameters.dt` (`:208`) | none | | Set from the fixed step each step; a key would split physics from it |
| `SoftBodiesSettings` (`sb/soft_body_settings.rs:15-69`) | `soft_resweep_strain`, `soft_max_extra_substeps`, `soft_contact_stiffening`, `soft_linear_tolerance`, `soft_max_linear_iterations`, `soft_max_dense_dofs` | 0.75, 4, 4.0, 1e-5, 20, 600 | A recording's header carries them |
| `SoftRecoverySettings`, 27 fields (`sb/soft_recovery_settings.rs:27-134`) | `[physics.soft_recovery]`, one key per field in Balaur words | as rapier | |
| `Counters` (`rapier: counters/mod.rs:22-34` and its four modules) | `physics.counters()` returns every stage, solver and CCD timer and count, per world | | Defect 33 |
| Quarantine `colliders()`, `soft_bodies()` (`rapier: pipeline/physics_pipeline/quarantine.rs:42,49`) | `physics.quarantined()` reports them | | Bodies only today |
| `DebugRenderMode` (`rapier: pipeline/debug_render_pipeline/debug_render_pipeline.rs:24-54`) | `set_debug_draw` words `impulse_joints`, `multibody_joints`, `soft_bodies`, `pseudo_normals`, `soft_volume_contacts`, `soft_stress` | | `joints` covers both joint kinds today |
| `DebugRenderStyle` (`debug_render_style.rs:16-82`) | `[physics.debug]`, one key per field | as rapier | Drawing only; no digest |
| `DebugRenderBackend::filter_object` (`debug_render_backend.rs:37`) | `set_debug_draw` option `nodes` | all | Alpha is dropped today (`crates/balaur_physics/src/debug.rs:105`) |
| `PhysicsWorld::configure_thread_pool` (`physics_world.rs:977-1004`) | none | | Balaur keeps one global pool on purpose (`tuning.rs:404-410`) |

The whole `PhysicsWorld` is serialized (`crates/balaur_physics/src/lib.rs:356-372`),
so a rapier-side setting rides the snapshot; a Balaur-side one needs a field in
the snapshot frame.

## 12. Audio

### `sound`

Exposed: `file`, `autoplay`, `volume_linear`, `pitch_scale`, `loop`, `bus`,
`positional`, `min_distance`, `max_distance`, `doppler_level`. rodio's
`Decoder`, `amplify`, `Player` volume and speed, `ChannelVolume`,
`repeat_infinite`, `buffered` and `skip_duration` are reached
(`crates/balaur_audio/src/lib.rs:132-163`).

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `Player::pause`, `play`, `is_paused` (`rodio: player.rs:212,264,272`) | `paused` | false | The fixed-step countdown (`lib.rs:774-793`) holds while paused |
| `skip_duration` (`rodio: source/mod.rs:283`) | `start_time` | 0 | The countdown subtracts it |
| `try_seek`, `get_pos` (`player.rs:238,340`) | `node.sound.seek(seconds)`, read-only `playback_time` | | The decoder is built non-seekable (`rodio: decoder/builder.rs:82-93`); the read-back comes from the fixed-step count, so headless runs match |
| `take_duration`, `set_filter_fadeout` (`source/mod.rs:260`, `take.rs:85`) | `end_time` | 0, to the end | |
| `delay` (`source/mod.rs:272`) | `delay` | 0 | The countdown includes it |
| `fade_in` (`source/mod.rs:441`) | `fade_in_time` | 0 | |
| `fade_out` (`source/mod.rs:450`) | `fade_out_time` | 0 | rodio fades from the first sample and `stop` is instant; a fade at the end needs `take_duration` with a known length, or a volume ramp |
| `take_crossfade_with` (`source/mod.rs:431`) | `crossfade_time` | 0 | A new `file` stops the old handle outright today |
| `ChannelVolume` for a flat sound (`rodio: source/channel_volume.rs:27,39`) | `pan` | 0, −1 to 1 | Only positional sounds get one, mixed to mono |
| `amplify_decibel`, `amplify_normalized` (`source/mod.rs:301,315`) | `volume_db`, `volume_curve` | | **two spellings** with `volume_linear` |
| `low_pass_with_q`, `high_pass_with_q` (`source/mod.rs:669-699`, `blt.rs:15-22`) | `low_pass_hz`, `low_pass_q`, `high_pass_hz`, `high_pass_q` | 0 (off), 0.5 | Baked in at decode; a live change goes through `BltFilter::to_low_pass*` behind a handle, as pan is |
| `reverb(duration, amplitude)` (`source/mod.rs:611`) | `reverb_time`, `reverb_level` | 0, 0 | Needs `buffered()`, the whole file in memory |
| `distortion(gain, threshold)` (`source/mod.rs:709`) | `distortion_gain`, `distortion_threshold` | | |
| `automatic_gain_control` (`source/mod.rs:407`, `agc.rs:72-79,365`) | `auto_gain`, `auto_gain_target`, `auto_gain_attack_time`, `auto_gain_release_time`, `auto_gain_max`, `auto_gain_floor` | false, 1.0, 4, 0, 7.0 | Attack and release cap at 10 s |
| `limit(LimitSettings)` (`source/mod.rs:504`, `limit.rs:177-370`) | on a bus: `limit`, `limit_threshold_db`, `limit_knee_db`, `limit_attack_time`, `limit_release_time` | false, −1, 4, 0.005, 0.1 | Balaur's buses are gains (`crates/balaur_audio/src/bus.rs:16-18`); a per-bus effect needs a rodio `Mixer` per bus |
| `dither(bits, algorithm)` (`source/mod.rs:229`) | `[audio] dither_bits` | 0 | An output-stage effect |
| `mix` (`source/mod.rs:238`) | `layers` (list of file) | [] | |
| `Player::append`, `skip_one`, `len`, `clear` (`player.rs:104-328`) | `queue` (list of file), `node.sound.skip()` | [] | One source per player today, counted down per file |
| rodio's `Spatial` 1/d² (`rodio: source/spatial.rs:48-69`) | `attenuation` (`inverse`/`inverse_square`) | inverse | Balaur's spatial model is its own (`crates/balaur_audio/src/spatial.rs:157-190`) |
| Loop offset per sound | `loop_offset` | from the import file | Read from the sidecar alone today (`lib.rs:654`) |
| `DecoderBuilder` (`builder.rs:192-261`) | import keys `gapless`, `seekable` | true, false | The file's settings, not the component's |

### `listener`

Exposed: `current`. The pose, pan, attenuation and doppler are Balaur's own
(`crates/balaur_audio/src/spatial.rs:75-218`).

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `SpatialPlayer` ear positions (`rodio: spatial_player.rs:27-54`) | `ear_distance` | | Balaur pans on the listener's right axis alone; it takes rodio's `Spatial` or feeds the distance into its own model |
| Speed of sound 343, maximum doppler 2.0 (`spatial.rs:27,31`) | `speed_of_sound`, `max_doppler` | 343, 2.0 | Balaur constants, not rodio's |

### `[audio]` settings

`DeviceSinkBuilder` (`rodio: stream.rs:198-390`) takes a device, channels,
sample rate, buffer size, sample format and an error callback; Balaur calls
`open_default_sink` (`lib.rs:105`). Keys: `device`, `channels`,
`sample_rate_hz`, `buffer_frames`. Listing devices
(`rodio: speakers.rs:165`) needs rodio's `experimental` feature.

## 13. Text shaping

For `text2d`, `text3d` and the shaped widget labels, over cosmic-text and
swash. The shaper gets a family, weight, italic or not, letter spacing, size
and line height; it wraps word-or-glyph, aligns start, center or end, always
shapes complex scripts, and rasterises with default flags
(`crates/balaur_text/src/lib.rs:647-737`).

| Wrapped item | Key | Default | Constraint |
| --- | --- | --- | --- |
| `Attrs::stretch` (`cosmic-text: attrs.rs:335`) | `font_stretch` (`ultra_condensed` to `ultra_expanded`) | normal | Picks a shipped face; nothing is synthesised |
| `Style::Oblique` | `font_style` word `oblique` | | cosmic-text fakes a 14° skew with no italic face (`shape.rs:616-626`) |
| `Family::Name` and generic families | `font_name` | "", the chain | Measurement sees project faces alone (`lib.rs:348-376`), so an OS family draws but does not measure |
| `Attrs::font_features` (`attrs.rs:137-195,377`) | `font_features` (list: `"smcp"`, `"liga=0"`) | [] | |
| `underline`, `strikethrough`, `overline` and their colours (`attrs.rs:227-233,382-407`) | the same six names, `underline` an enum `none`/`single`/`double` | none | Balaur paints glyph quads and drops run decorations (`lib.rs:589-628`); markup gains `[u]`, `[s]` |
| `Wrap::Word`, `Wrap::Glyph` (`cosmic-text: layout.rs:128-137`) | `line_break` (`word_or_glyph`/`word`/`glyph`) | word_or_glyph | |
| `Buffer::set_ellipsize` (`buffer.rs:773`, `layout.rs:173-193`) | `truncate`, `truncate_at` (`end`/`start`/`middle`), `max_lines` | false, end, 0 | The widget's truncation is Balaur's own end-only search; it moves onto this |
| `set_size` height (`buffer.rs:818`) | `max_height` | 0, none | |
| `Align::Justified`, `Left`, `Right` (`layout.rs:152-158`) | `text_align` words `justify`, `left`, `right` | | Start and end follow the direction; left and right do not |
| `Shaping::Basic` (`shape.rs:28-44`) | `shaping` (`complex`/`simple`) | complex | Basic has no font fallback |
| `Buffer::set_hinting` (`buffer.rs:745`) | `snap_advances` | false | Layout turns resolution-dependent and measurement stops being platform-independent |
| Cache flags `DISABLE_HINTING`, `PIXEL_FONT` (`glyph_cache.rs:9-13`) | `hinting`, `pixel_snap` | true, false | Defect 35: the font import's `hinting` is the same switch |
| `set_monospace_width` (`buffer.rs:787`) | `monospace_width` | 0, none | |
| `set_tab_width` (`buffer.rs:801`) | `tab_width` | 8 | |
| `Attrs::metrics`, family per span (`attrs.rs:365`) | markup `[size=N]`, `[font=chain]` | | The parser knows b, i, color, wave, align, img, url, hint (`crates/balaur_text/src/markup.rs:218-242`) |

## 14. Widgets

### Layout, over taffy

Every node is flex today; direction comes from the kind, wrap is on for
`flow` alone, shrink and basis are derived from `grow`, 0 means auto, one gap
goes to both axes (`crates/balaur_ui/src/widget/taffy.rs:209-266`). The
`flexbox`, `flexbox_balance` and `content_size` features are on
(`Cargo.toml:94`).

| Wrapped item (`taffy: style/mod.rs` unless named) | Key | Default | Constraint |
| --- | --- | --- | --- |
| `position: Absolute`, `inset` (`:632,635`) | `absolute`, `inset` | false | `inset` is a filling root's margins today |
| `max_size` (`:646`) | `max_width`, `max_height` | 0, none | |
| `aspect_ratio` (`:650`) | `aspect_ratio` | 0, none | |
| `margin` (`:655`) | `margin` (vec4) | 0 | |
| `border` (`:661`) | `border` (vec4) | 0 | `stroke` paints and reserves no space |
| `box_sizing` (`:611`) | `box_sizing` (`border`/`content`) | border | |
| `direction` (`:613`) | `direction` (`left_to_right`/`right_to_left`) | left_to_right | |
| `overflow`, `scrollbar_width` (`:617,619`) | `overflow` (`visible`/`clip`/`hidden`/`scroll`) | visible | Sizing only; egui clips, and the `scroll` kind scrolls |
| `contain` (`:621`) | `contain` (flags) | none | |
| `align_self` (`:670`) | `align_self` | the parent's | |
| `AlignItems` keywords (`taffy: style/alignment.rs:24-56`) | `align_items` words `stretch`, `baseline` | `stretch` | Defect 36: `start` starts |
| `align_content` (`:680`) | `align_content` (`start`/`center`/`end`/`stretch`/`between`/`around`/`evenly`) | | |
| `justify` keywords `stretch`, `flex-start`, `flex-end` (`alignment.rs:66-87`) | `justify` word `stretch` | | |
| `AlignmentSafety::Safe` (`alignment.rs:125-131`) | `safe_align` | false | |
| `gap` as a size (`:687`) | `gap_x`, `gap_y` | | **two spellings** with `gap` |
| `percent()` lengths (`taffy: style/dimension.rs:53,199,400`) | `width_percent`, `height_percent` | | The size keys are floats |
| `RowReverse`, `ColumnReverse` (`taffy: style/flex.rs:198-215`) | `reverse` on row, column and flow | false | It means reversed order on a table already |
| `FlexWrap` with `Balance` (`flex.rs:87-106`) | `wrap_children` (`none`/`wrap`/`wrap_reverse`/`balance`/`balance_reverse`) | none | `wrap` is text wrap on this component |
| `flex_line_count` (`:708`) | `min_lines` | 1 | |
| `flex_basis`, `flex_shrink` (`:713,723`) | `basis`, `shrink` | auto, 1 in taffy, 0 in Balaur | They override the derived values |
| `item_is_replaced` (`:609`) | set for the `image` kind | | |
| `grid_template_rows`, `grid_template_columns`, `grid_auto_*`, `grid_auto_flow`, `grid_template_areas`, `grid_row`, `grid_column` (`:728-759`) | `grid_rows`, `grid_columns`, `auto_rows`, `auto_columns`, `auto_flow` (`row`/`column`/`row_dense`/`column_dense`), `areas`, and on a child `row`, `column` | | The `grid` feature goes on in every build, web included. `columns` becomes shorthand for `grid_columns`: **two spellings** |
| `float`, `clear`, `text_align` (`:625,628,692`) | none | | Block layout; `float_layout` and `block_layout` are off and Balaur lays out no blocks |

### Per kind, over egui

- **label** — `line_height`, `letter_spacing` and `bitmap_font` are fixed
  (`crates/balaur_ui/src/widget/text.rs:40-45`); the `text2d` keys and the
  outline and shadow apply. The fallback `egui::Label` adds `halign`,
  `sense` and `show_tooltip_when_elided` (`egui: widgets/label.rs:86-132`).
  Kinds egui draws take `RichText`'s `underline`, `strikethrough`,
  `italics`, `background_color`, `extra_letter_spacing`, `line_height` and a
  `wght` `variation` for `font_weight` (`egui: widget_text.rs:160-310`).
- **button** — `wrap` and `truncate` (defect 37); `gap` inside the button,
  fixed at half the font size (`button.rs:89`); a drag-sensing `sense`
  (`egui: widgets/button.rs:186`).
- **checkbox** — `indeterminate` (`egui: widgets/checkbox.rs:56`), false.
- **slider** — `show_value` (forced off), `prefix`, `suffix`, `axis` for
  `vertical`, `logarithmic`, `smallest_positive`, `clamp`
  (`never`/`edits`/`always`), `smart_aim`, `drag_speed`, `decimals`,
  `trailing_fill`, `handle` and `handle_aspect`, `number_format`
  (`decimal`/`binary`/`octal`/`hex`), `update_while_editing`
  (`egui: widgets/slider.rs:178-642`). A custom formatter calls Rune every
  frame.
- **number_field** — `clamp_existing` (forced off), `decimals` (script only
  today, `ui.number_field`), `number_format`, `update_while_editing`
  (`egui: widgets/drag_value.rs:163-426`). A range of exactly 0 to 1 means
  unbounded today (`kinds.rs:420`).
- **progress_bar** — `show_percentage`, `animate`, `corner_radius`
  (`egui: widgets/progress_bar.rs:69-93`). `animate` repaints every frame,
  which defeats the low-processor sleep.
- **separator** — `spacing` (fixed at 6, `kinds.rs:531`), `axis`, `overhang`
  (`egui: widgets/separator.rs:45-87`).
- **dropdown** — `list_height`, `wrap`, `truncate`, `keep_open`
  (`egui: containers/combo_box.rs:112-189`).
- **color_picker** — `alpha` (`none`/`blend`/`additive`, blend fixed today),
  `inline` (`egui: widgets/color_picker.rs:267-276,493-510`).
- **text_field, text_area, code** — `text_align`, `prefix`, `suffix`,
  `editable`, `padding` for the margin, `tab_inserts`, `caret_at_end`,
  `clip_text`, `submit_key` (`egui: widgets/text_edit/builder.rs:216-422`).
  Whether `fill` reaches `background_color` through the theme
  (`widget/theme.rs:731-771`) is not yet checked.
- **scroll, list, tree, table** — `scrollbar` (`auto`/`always`/`never`),
  `stick_to_end` and `scroll_offset` (script only today, `ui.scroll`),
  `min_scrolled_width`, `min_scrolled_height`, `animated`, `wheel_speed`,
  `drag_scroll`, `wheel_scroll`, `drag_cursor`
  (`egui: containers/scroll_area.rs:444-655`).
- **image** — `tint`, `region` (script only today, `ui.image`), `fill`,
  `angle` and `angle_origin`, `corner_radius`, `alt_text`, `loading_spinner`
  (`egui: widgets/image.rs:210-273`).
- **menu** — `popup_gap`, `popup_width`, `placement` words for the side and
  centred alignments, `placement_fallbacks`, `close_on`
  (`click`/`click_outside`/`never`) (`egui: containers/popup.rs:289-399`,
  `emath: rect_align.rs:46-112`).
- **dialog** — `backdrop_color` (140 alpha fixed, `layer.rs:263-270`) and
  `dismissable`; egui closes a modal on Escape or a backdrop click with no
  switch, so `dismissable` is Balaur's.
- **window** — `resizable`, `collapsible`, `closable` (script only today,
  `ui.window`), `title_bar`, `movable`, `min_size`, `max_size`, `constrain`,
  `scroll`, `default_open` (`egui: containers/window.rs:201-498`). The
  component draws its own window, so these need `egui::Window` or Balaur
  handles.
- **root areas** — `fade_in` (off by choice, `layer.rs:244-246`), `movable`
  (`egui: containers/area.rs:198,351`).
- **cursor** — the `none` word, `CursorIcon::None` (`egui: data/output.rs:353`).

switch, fold, tabs, toast, stack, grid, flow, row, column, panel and draw are
painted and laid out by Balaur, with no egui widget behind them.

## 15. Not wrapping a crate

`touch_button` and `touch_stick` (`crates/balaur_input/src/touch_controls.rs`),
`animation` and `state_machine` (`crates/balaur_animation`), `bone2d`,
`modifier2d` and `modifier3d` (Balaur's own solvers), `timer`, `states`,
`meta`, `bindings` and `transform` (`crates/balaur_core`) wrap no crate. The
one choice `transform` takes from glam is the Euler order, ZYX
(`crates/balaur_core/src/transform.rs:52,57`).

## 16. Changes outside this repository

**The kiss3d fork,** one branch of commits on `balaur-hooks`:

1. A `ShadowMapper` accessor on `Window`; then `depth_bias`, the raster bias
   and `MAX_SHADOW_VIEWS` as settings.
2. The orthographic scale; `render_layers` on the stereo camera; per-eye
   viewports.
3. A per-object depth test and a receive-shadows flag on `Object3d`.
4. `Skin3d::new`, `Object3d::set_skin`, `AlphaMode::is_transparent`,
   `nine_slice_mesh` and `Blend2d::blend_state` made public.
5. The 2D `cull` flag read by the 2D materials.
6. A setter for `Gi2d::probe_spacing`; polygon occluders in `Gi2d`.
7. A directional kind and a larger `MAX_LIGHTS_2D` for `LitMaterial2d`.
8. Reflection probes: capture size, clip planes, orientation, a mask per
   probe.
9. `BLOOM_MIPS`, the clustered-lighting limits, `MAX_MORPH_TARGETS` and the
    planar reflector's size and mask as settings.
10. A sky lighting intensity and image apart from the drawn sky.
11. Morph weights checked against the mesh's own targets, and an offscreen
    depth target a post pass can sample.
12. The stereo camera's eyes converge on the point it looks at.
13. A translucent instance sends its object to the transparent pass.
14. 2D GI marches in field pixels and thins a segment occluder to half of one.

**rapier,** as a pull request on the fork `Ughuuu/rapier`, to go upstream
later: `roll_influence` and `skid_info` public; `side_factor` and
`fwd_factor` as wheel settings; the character controller's sweep iterations,
depenetration budget, probe distance and platform friction as settings.
Balaur does not take these until a rapier release carries them. Signed
vehicle axes need no patch: Balaur negates the engine force and steering.

## 17. Order

Each step closes with `scripts/precommit.sh`, and with `--e2e` where a socket
suite is touched.

1. **The decisions** of section 18 written down.
2. **Defects** (section 1) that need no fork change.
3. **The kiss3d fork** (section 16), on a branch of `balaur-hooks`, built
   against Balaur through a `paths` override until it is merged.
4. **Settings with a public setter.** The rows marked with no constraint:
   environment, both cameras, the post passes, `boolean2d`, the rapier
   builder rows, the world rows, the audio effects, the text attributes, the
   taffy fields and the egui options.
5. **The shader-less material and the renderable keys** (section 3), and the
   2D drawable keys (section 4) on `ObjectMaterial2d` nodes.
6. **Pipeline work.** The same keys on `ShaderMaterial3d`,
   `SkinnedMaterial3d` and `SkinnedMaterial`; the missing features in
   `pbr.wesl`; a spot term in the 2D light map.
7. **The rows that waited on the fork.**
8. **New components and kinds:** `follow3d` and `follow2d`, `particles3d`,
   the `camera3d` kinds, `gi` on `camera2d`, `LitMaterial2d` lighting,
   custom soft-body layouts and regions, the missing `physics2d` readers.
9. **Snapshot state:** one-way angles and surface velocities, PID
   integrals, soft-body regions, and the world settings in a recording's
   header with `docs/PLAN-rapier.md` item 2.
10. **Migration:** the renamed keys in the editor, templates, examples,
    presets, the Godot importer and the GDScript shim; `docs/generated`; the
    docs of section 19; a devlog post per feature group.

## 18. One spelling per value

Decided 2026-10-02. Where a crate offers two ways to write one value, the
schema keeps one, and a renamed key is migrated rather than kept beside its
replacement.

| Value | Kept | Gone or never added |
| --- | --- | --- |
| Exposure | `exposure`, linear | `exposure_ev` |
| Ambient brightness | `ambient_color`, HDR | `ambient_intensity` |
| Sound gain | `volume_linear` | `volume_db`, `volume_curve` |
| Mesh cleanup on a collider | `merge_vertices`, `drop_degenerate_triangles`, `drop_bad_topology` | `weld_vertices` |
| Character length modes | `safe_margin_lengths`, `step_height_lengths`, `step_min_width_lengths`, `floor_snap_lengths` | `lengths` |
| Joint limits and motors | `axes`, one record per axis | `limits`, `motor`, `motor_target`, `motor_max_force`, `motor_model`, `stiffness`, `damping` as flat keys |
| Tension-only soft-body edges | `tension_only` (`none`/`all`/`listed`) with `tension_only_edges` read under `listed` | `tension_only` as a bool |
| Widget spacing | `gap` as a vec2 | `gap_x`, `gap_y` |
| Widget grid columns | `grid_columns` | `columns` |
| Soft-body orientation | `orientation` (`auto`/`solid`/`shell`) | `oriented` |

The joint change is the widest: scenes, presets, the Godot importer and the
editor move to `axes`, and `set_joint_limits` and `set_motor_*` take an axis.

## 19. Docs to correct with the work

Corrected 2026-10-02: `docs/PLAN-3d-rendering.md`, `docs/NAMING.md`,
`docs/PLAN-rapier.md` items 2 and 5, `ARCHITECTURE.md`'s 2D readers and audio
lines, the modifier count in `crates/balaur_animation/src/modifier.rs` and
`docs/ROADMAP.md`, and the generated asset and component pages.
