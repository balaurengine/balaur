> **Status:** soft bodies are in, in both dimensions, with cloth, rope and
> tearing over the same component; the fluids below are not started. Written
> 2026-09-02, extended 2026-09-07 with cloth, rope and gases, soft bodies
> landed 2026-09-19. The simulation work is built in Rapier itself; this plan
> is how each material reaches a Balaur scene.

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

`softbody2d` and `softbody3d`: a deformable body of particles linked by elastic
constraints. **Done**, on rapier 0.36.

- `kind` lays the particles out: generators (`cuboid`, `sphere`, `cloth`,
  `cloth_tube`, `rope` in 3D; `grid`, `disk`, `rope` in 2D) or a mesh
  (`trimesh` for a surface, `volumetric` for the approximate tetrahedrization
  of a closed mesh, `polygon` and `polyline` in 2D).
- The material rows set a spring frequency and damping ratio per constraint
  family, a cell model (`volume`, `corotational`, `neo_hookean`) with a Young
  modulus and a Poisson ratio, plasticity on the cells and on the edges, and
  volume preservation. `solver = "fem"` runs the elasticity as rapier's
  implicit step.
- The body's collision mesh goes onto the node as a `SolvedMesh` each fixed
  step. In 2D a `SolvedPolygon` outranks a deform track on the polygon's own
  vertices. A 2D body hands over its skin, its cells, its outline filled or
  its segments as a ribbon, so every layout draws; `color` tints one with no
  polygon of its own.
- Rapier's `PhysicsWorld` carries the `SoftBodySet` in the snapshot, and the
  digest hashes every particle's velocity and the body's topology version.
- The editor makes one from what a node draws, in one undo step
  (`editor/scripts/recipes.rn`): a sprite is traced into a textured polygon
  the cells bend, a shape becomes its generator, a mesh is filled. The
  outliner's Change type, the Physics panel's Make row and the new-node
  picker all reach it.

### Cloth and rope

A sheet that hangs and a rope of linked segments, both a `softbody` with its
constraints laid out rather than a component of their own. **Done:** the
`cloth`, `cloth_tube` and `rope` layouts, `pinned` naming the particles held in
place, and `pin_particle` doing it from a script.

### Tearing

A threshold that, once an element gives, breaks it mid-step. **Done:**
`tear_strain` and `tear_force` on the component, with `tear_smoothing`,
`interior_strength`, `max_tears_per_step` and `min_piece` shaping how a crack
runs, and the node's `on_tear` hearing about it. The topology version is in the
digest, so two machines that tore differently are caught on the step they did.

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

1. **Done.** Soft bodies in 2D as `softbody2d` drawing through `polygon`;
   snapshot and digest from day one. The cross-OS CI digest is still to add.
2. **Done.** Soft bodies in 3D through the mesh path; cloth and rope over the
   same solver; tearing in both dimensions.
3. Fluids in 2D with an emitter component and point rendering; then 3D.
4. Gases as a fluid with buoyancy, drawn from a density field.
5. Granular materials as a fluid variant.
6. Editor: **done for soft bodies** — the Physics workspace shows every
   physics-tagged component, and one click turns a mesh into a filled, skinned
   or surface soft body. Gizmos for emitters and volumes are still to come.
7. Something to look at: **built**. `examples/cloth` drapes one sheet over a
   block, and drops a ball through a second, pinned sheet with a `tear_strain`.
   `examples/jelly` drops one 2D body of each layout.

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
