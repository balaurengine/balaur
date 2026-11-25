# Plan: richer particles

What `particles2d` and `particles3d` (`crates/balaur_render/src/particles.rs`
and `particles_3d.rs`) still lack. Each step lands in both dimensions at
once. The live particles stay backend state, so a headless run digests as a
windowed one.

## Steps

1. **Emission shapes.** `emission = "point" | "sphere" | "box" | "cone" |
   "ring" | "mesh"` with `emission_size`; the cone the emitter has becomes the
   direction every shape sends its particles in, and `mesh` emits from a mesh
   asset's surface.
2. **Per-particle variety.** `speed_random`, `lifetime_random`,
   `size_random`, `angle_random`, `color_random`; `align = "none" |
   "velocity"`; damping, radial and tangential acceleration; `preprocess` in
   seconds, `speed_scale` and `fixed_fps`.
3. **A particle contract.** A `particle.wesl` handing a material each
   particle's age, life, seed and velocity, so a fire shader is a material.
   The emitters' `material` property draws with the standard contract today.
4. **Attractors and colliders.** `particle_attractor` (`kind`, `strength`,
   `radius`, `directionality`) and `particle_collider` (`kind`, `bounce`,
   `friction`, particles that die on contact), as components of
   `balaur_render`, which does not depend on physics.
5. **Trails and sub-emitters.** `trail_length` in seconds, a ribbon per
   particle; `sub_emitter` as a node path with `sub_emitter_mode =
   "on_death" | "on_collision" | "interval"`.
6. **Mesh, soft and lit 2D particles.** `mesh` on `particles3d`, drawn through
   `balaur_render::instancing`; `soft_distance` against depth; `particles2d`
   lit by every `light2d`.
7. **A compute stepper.** Behind a feature flag on adapters that have
   compute; the CPU stepper stays the fallback and the only path on WebGL.
   It writes no simulation state. CI asserts counts and bounds agree with the
   CPU stepper, not pixels.
8. **Editor gizmos.** The emission shape drawn in the viewport, and the
   inspector's `emitting` toggle restarting a one-shot.
