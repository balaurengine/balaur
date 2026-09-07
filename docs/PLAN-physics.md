> **Status:** not started on the engine side. Written 2026-09-02, extended
> 2026-09-07 with cloth, rope and gases. The simulation work is being built in
> Rapier itself; this plan is how each material reaches a Balaur scene.

# Plan: soft bodies, cloth, tearing, fluids, gases, granular materials

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

Each material is one component per dimension, registered by the physics plugin
like the rigid ones, so it is a scene key, a script call and an inspector row
at once. The name `particles` is taken by the render emitter, so simulated
particles are `fluid`, `gas` and `granular`.

Determinism is the constraint, not a feature: every solver ships behind
`enhanced-determinism`, steps on the fixed tick, and is part of the snapshot
and the digest from its first commit. A material that cannot be made
deterministic does not ship in the plugin.

Scripts get what rigid bodies have: a call that adds the body, a way to apply
forces and read positions, and the same `on_collision`-style events.

## The materials

Each one waits on its solver landing in Rapier and follows within one release
of it. The order below is the order they can be built in: every material after
the first two is a variant of one before it.

### Soft bodies

`softbody2d` and `softbody3d`: a deformable body built from a mesh, with
stiffness, damping and pressure. Rendering reads back what the solver produced.
A 3D soft body's vertices go down the `modify_vertices` path the skin uses, and
a 2D one is a `polygon` whose positions come from the solver instead of the
joint palette. **Needs:** a soft-body solver.

### Cloth and rope

A sheet that hangs and a rope of linked segments, both a `softbody` with its
constraints laid out rather than a component of their own, pinned to a node by
index and cut by a script. **Needs:** the soft-body solver, plus pinning
constraints against a rigid body.

### Tearing

`tearable` on a soft body: a threshold that, once a constraint gives, splits
the body into two bodies and two meshes mid-step. The split is a scene edit on
the fixed tick, so it is in the snapshot and the digest like any other.
**Needs:** tearing in the solver.

### Fluids

`fluid2d` and `fluid3d`: a volume of particles with a rest density and a
viscosity, with emitter and drain components beside them. They draw as
instanced points first and as a surface later. **Needs:** a fluid solver.

### Gases and smoke

`gas2d` and `gas3d`: a buoyant volume that rises, spreads and cools, with a
density and a temperature per particle and a wind field it answers. The
renderer reads the density rather than the particles, so smoke draws as a
volume and not as a point cloud. **Needs:** the fluid solver, plus buoyancy and
a diffusion term.

### Granular materials

`granular2d` and `granular3d`: sand, mud and snow as a fluid with friction and
a yield stress. **Needs:** the fluid solver, plus a granular model.

## Phases

1. Soft bodies in 2D as `softbody2d` drawing through `polygon`; snapshot,
   digest and a cross-OS CI digest from day one.
2. Soft bodies in 3D through the mesh path; cloth and rope over the same
   solver; tearing in both dimensions.
3. Fluids in 2D with an emitter component and point rendering; then 3D.
4. Gases as a fluid with buoyancy, drawn from a density field.
5. Granular materials as a fluid variant.
6. Editor: gizmos for emitters and volumes; the Physics persona lists them.

## Open questions

1. **Cost per frame.** A fluid with ten thousand particles on the fixed step
   is a budget question before it is a feature; the headless benchmark
   suite gets a row per material before the material ships.
2. **Rollback with fluids.** Snapshotting a particle field 60 times a second
   is memory; a ring of a few snapshots may be the limit, and a game that
   rolls back may have to keep fluids cosmetic.
3. **Where a gas is drawn.** A density field wants a volumetric pass, which is
   the one this plan shares with
   [PLAN-3d-rendering.md](PLAN-3d-rendering.md)'s fog.
