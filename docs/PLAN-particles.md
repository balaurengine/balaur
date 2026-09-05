> **Status:** not started. Written 2026-09-05 from the Godot parity
> investigation: there is no particle in 3D, and the 2D emitter has a
> point and a cone and nothing else to emit from.

# Plan: particles in 3D, and what both emitters lack

## 0. Where the tree is today

- `particles` is a 2D emitter: rate, lifetime, speed, angle, spread, size and
  `size_end`, gravity, `color` and `color_end`, a texture, `one_shot` and
  `explosiveness` (`crates/balaur_render/src/particles.rs`).
- The live particles and the PCG that scatters them are backend state,
  seeded from the entity, so the simulation never sees one and a headless
  run digests as a windowed one. That contract is the one thing every phase
  here keeps.
- Particles draw as kiss3d 2D nodes after the light map, unlit, one mesh
  per emitter.
- `docs/PLAN-2d-games.md` decided curve assets are not planned: a ramp is
  two keys.

## 1. Design

**Two emitters, one model.** `particles3d` is the 2D emitter with a third
axis: camera-facing quads in the 3D pass, a `vec3` gravity, an emission
shape with a volume. Both components read the same settings struct and the
same stepping code, so a feature added to one arrives in the other in the
same step. The 2D key becomes `particles2d`, because D5 marks both sides
once a sibling exists; `particles` is read for one release with a warning
and listed under Breaking. The billboards draw through the fork's
`set_instances`, the one instancing seam `docs/PLAN-objects.md` opens for
its cloner and names as its step 6; this plan is what the emitter offers,
that one is where the draw lands.

**Shapes, not points.** `emission = "point" | "sphere" | "box" | "cone" |
"ring" | "mesh"` with `emission_size`, and the cone the emitter already has
becomes the direction every shape sends its particles in. `mesh` emits from
a mesh asset's surface, which is how a burning model burns everywhere.

**Attractors and colliders are nodes of the render crate.** A particle
bending toward a node or bouncing off a floor reads simulation state and
writes none, so it keeps the observer contract — but the render crate does
not depend on physics, so it cannot ask a rapier world. `particle_attractor`
(sphere, box, a direction) and `particle_collider` (sphere, box, plane) are
components in `balaur_render`, as Godot's `GPUParticlesAttractor` and
`GPUParticlesCollision` are nodes of the renderer.

**CPU first, compute where it exists.** The CPU stepper is what ships on
every target, capped by `amount`; a compute path lands later for the
adapters that have one, and WebGL through wgpu's `gles` never does, so the
CPU path is never removed.

**A material per particle.** `material` on both emitters, against a
`particle.wesl` contract that hands the shader age, life, seed and velocity
per instance, so a fire shader is a material and not a feature.

## 2. The surface

| Need | Decision |
| --- | --- |
| Particles in a 3D scene | Step 1: `particles3d`, billboard quads, `blend = "alpha" \| "add" \| "premultiplied"`, sorted back to front |
| Emission shapes | Step 1 in both: `emission`, `emission_size`, `emission_texture` for a mesh |
| Per-particle randomness | Step 2 in both: `speed_random`, `lifetime_random`, `size_random`, `angle_random`, `color_random` |
| Rotation and angular velocity | Step 2 in both: `rotation`, `angular_velocity`, `align = "none" \| "velocity"` |
| A sheet animated over a life | Step 2 in both: `columns`, `rows`, `frame_speed` or one cycle per life |
| Damping, radial and tangential acceleration | Step 2 in both |
| Attractors | Step 3: `particle_attractor` with `kind`, `strength`, `radius`, `directionality` |
| Collision | Step 3: `particle_collider` with `kind`, `bounce`, `friction`; particles that die on contact |
| Trails | Step 4: `trail_length` in seconds; a ribbon per particle |
| Sub-emitters | Step 4: `sub_emitter` node path, `sub_emitter_mode = "on_death" \| "on_collision" \| "interval"` |
| A particle that is a mesh | Step 5: `mesh` on `particles3d`, drawn through the instancing `docs/PLAN-views-and-culling.md` step 3 adds |
| Lit particles | Step 5: `lit = true` samples the light array `docs/PLAN-3d-rendering.md` step 1 fills |
| Soft particles against depth | Step 5: `soft_distance` |
| Preprocess, speed scale, fixed steps | Step 2: `preprocess` seconds, `speed_scale`, `fixed_fps` |
| GPU simulation | Step 6, and only once `docs/PLAN-shaders.md` question 6 reverses its *out of scope* on compute: a compute stepper behind a feature flag, the CPU stepper staying the fallback on every adapter and the only path on WebGL |
| A material per emitter | Step 1: `material` and the `particle.wesl` contract |
| Curve assets for ramps | **Not planned**, as `docs/PLAN-2d-games.md` decided; a ramp is two keys and a texture is a gradient |
| Emitter gizmos and a restart button | Step 7: the shape drawn in the viewport; the inspector's `emitting` toggle restarts a one-shot |

## 3. Steps

1. `particles3d`, `particles2d`, emission shapes, blend modes, materials.
2. Randomness, rotation, sheets, damping, preprocess.
3. Attractors and colliders.
4. Trails and sub-emitters.
5. Mesh particles, lit and soft particles.
6. Compute.
7. Editor gizmos.

## 4. What CI can prove, and what it cannot

The headless-versus-offscreen digest job proves every phase stays an
observer. Offscreen renders one burst per phase into the showcase at a fixed
frame, and the per-emitter seed makes that image the same run to run. CI
cannot prove a compute stepper matches the CPU one to the pixel; it asserts
the counts and bounds agree.

## 5. Open questions

1. **The rename.** `particles` to `particles2d` touches every example and
   showcase scene; the warning-for-one-release rule is the same the
   `shape` to `shape3d` rename used, and the cost is the same.
2. **Determinism of a compute stepper.** Two drivers rounding differently
   draw different sparks. Acceptable, because nothing reads them back; the
   plan says so rather than promising a pixel.
