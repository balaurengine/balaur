> **Status:** not started on the engine side. Written 2026-09-02. The
> simulation work — soft bodies, tearing, fluids, granular materials — is
> being built in Rapier itself; this plan is how it reaches a Balaur scene.

# Plan: soft bodies, tearing, fluids, granular materials

## Where the tree is today

- Rigid bodies and colliders in 2D and 3D through `balaur_physics`, both
  worlds built with `enhanced-determinism`, stepped in `Stage::FixedUpdate`,
  snapshotted whole through rapier's `serde-serialize`.
- Every physics quantity a script reads comes from the fixed step, and every
  transcendental on the simulation path goes through `libm`.
- Rendering deforms a mesh from data already: skinned polygons in 2D, CPU
  skinning in 3D, both from the scene tree. A soft body is another source
  of vertex positions for the same path.

## Design

Each new material is one component per dimension, registered by the physics
plugin like the rigid ones, so it is a scene key, a script call and an
inspector row at once:

| Component | What it is | What it needs from Rapier |
| --- | --- | --- |
| `softbody2d`, `softbody3d` | A deformable body from a mesh: stiffness, damping, pressure | soft-body solver |
| `tearable` on a soft body | A tear threshold; a tear makes two bodies and two meshes | tearing |
| `fluid2d`, `fluid3d` | A volume of particles with a rest density and viscosity; emitters and drains | fluid solver |
| `granular2d`, `granular3d` | Sand, mud, snow: a fluid with friction and a yield stress | granular model |

The name `particles` is taken by the render emitter, so simulated
particles are `fluid` and `granular`.

Rendering reads back what the solver produced: a soft body's vertices go
down the `modify_vertices` path the 3D skin uses, and a 2D soft body is a
`polygon` whose positions come from the solver instead of the joint
palette. Fluids draw as instanced points first; a surface later.

Determinism is the constraint, not a feature: every solver ships behind
`enhanced-determinism`, steps on the fixed tick, and is part of the
snapshot and the digest from its first commit. A material that cannot be
made deterministic does not ship in the plugin.

Scripts get what rigid bodies have: `physics2d::add_soft_body`, a way to
apply forces and read positions, and the same `on_collision`-style events.

## Phases

1. Soft bodies in 2D as `softbody2d` drawing through `polygon`; snapshot,
   digest and a cross-OS CI digest from day one.
2. Soft bodies in 3D through the mesh path; tearing in both.
3. Fluids in 2D with an emitter component and point rendering; then 3D.
4. Granular materials as a fluid variant.
5. Editor: gizmos for emitters and volumes; the Physics persona lists them.

Each phase waits on its solver landing in Rapier and follows within one
release of it.

## Open questions

1. **Cost per frame.** A fluid with ten thousand particles on the fixed step
   is a budget question before it is a feature; the headless benchmark
   suite gets a row per material before the material ships.
2. **Rollback with fluids.** Snapshotting a particle field 60 times a second
   is memory; a ring of a few snapshots may be the limit, and a game that
   rolls back may have to keep fluids cosmetic.
