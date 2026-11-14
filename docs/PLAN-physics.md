# Plan: fluids, gases and granular materials

How each simulated material beyond rigid and soft bodies reaches a Balaur
scene. The solvers are built in rapier; each material follows within one
release of its solver landing there.

Each material is one component per dimension, registered by the physics plugin
like the rigid ones. It steps on the fixed tick under `enhanced-determinism`,
and is in the snapshot and the digest from its first commit. A material that
cannot be made deterministic does not ship.

## Steps

1. **Soft bodies in the cross-OS digest.** `scripts/determinism_trace.sh`
   traces five examples, and neither soft-body one is among them. Add
   `examples/cloth` and `examples/jelly` to `PROJECTS`. Done when CI's
   simulation job diffs their traces across Linux, macOS and Windows.
2. **Fluids**, below, in 2D with an emitter and point drawing, then 3D.
3. **Gases and smoke**, below, as a fluid with buoyancy.
4. **Granular materials**, below, as a fluid variant.

Each material step includes its editor gizmos: an emitter's and a volume's
extent drawn in the viewport.

### Fluids

`fluid2d` and `fluid3d`: a volume of particles with a rest density and a
viscosity, with emitter and drain components beside them. They draw as
instanced points first and as a surface later. Waits on a rapier fluid solver.

### Gases and smoke

`gas2d` and `gas3d`: a buoyant volume that rises, spreads and cools, with a
density and a temperature per particle and a wind field it answers. The
renderer reads the density, so smoke draws as a volume rather than a point
cloud. Waits on the fluid solver, plus buoyancy and a diffusion term.

### Granular materials

`granular2d` and `granular3d`: sand, mud and snow as a fluid with friction and
a yield stress. Waits on the fluid solver, plus a granular model.

## Open questions

1. **Cost per frame.** The headless benchmark suite gets a row per material
   before the material ships.
2. **Rollback with fluids.** Snapshotting a particle field every tick is
   memory; a game that rolls back may have to keep fluids cosmetic.
3. **Where a gas is drawn.** A density field wants a volumetric pass, the one
   [PLAN-3d-rendering.md](PLAN-3d-rendering.md)'s fog needs too.
