> **Status:** not started. Written 2026-09-05 from the Godot parity
> investigation: nothing in the tree pathfinds, and behaviour over a path
> stays a script's job.

# Plan: navigation — a navmesh, paths over it, and agents that avoid each other

## 0. Where the tree is today

- No navmesh, no path, no agent.
- What a baker reads exists: static `collider2d` and `collider3d` shapes,
  `tilemap` cells (`docs/PLAN-tilemap.md` step 1 gives them collision),
  `mesh`, `heightfield` and `voxels` assets.
- What a baker computes with exists: `geometry2d.union`, `difference` and
  `convex_hull` over `i_overlay`, the ear-clipper in core, `libm`, ordered
  collections.
- The fixed step, the digest and the snapshot are where an agent's state
  has to live, because where an agent goes decides the game.
- In the registry: `polyanya` (any-angle pathfinding over a navmesh, pure
  Rust, `f32`), `dodgy_2d` (ORCA avoidance), `pathfinding` (A*, Dijkstra,
  a grid), `cavalier_contours` (polyline offsetting). `landmass` ties agents,
  avoidance and a navmesh together; `oxidized_navigation` is a Rust port of
  Recast's baker written against Bevy's types.

## 1. Design

**Navigation is simulation.** Every query runs on the fixed step in `f32`
on `libm`, agents live in a `DetHashMap`, their state is in the snapshot and
their positions are in the digest from the first phase. A crate that
disagrees between two platforms gets the triangulator's treatment: written
out, in core, with a test that runs it twice.

**The navmesh is an asset.** `navmesh` in `navigation/`: `positions` and
`polygons` as index loops, in the same shape a `mesh` asset uses, so an
authored one is a mesh and a baked one is a file the editor writes and the
pack carries. 2D and 3D share the type; a 2D mesh is flat.

**Baking is a tool, not a load.** A bake is slow and its inputs are the
scene's static geometry, so `balaur bake` and the editor's Bake button write
the asset and a game loads it; `navigation.bake(node)` at run time is the
same code for a level a script assembled, and it says how long it took.

**An agent computes a velocity; the game applies it.** `agent2d` and
`agent3d` replan when their target moves past `path_tolerance`, follow the
next corner, and run ORCA against their neighbours and obstacles; the result
is `agent.velocity()`, which a script hands to `move_character` or a body,
as a Godot script does after `NavigationAgent`. `drive = "transform" |
"character" | "none"` lets an agent move its own node for the common case.

**Two bakers.** 2D unions the obstacle polygons inflated by the agent radius
through `i_overlay` and `cavalier_contours`, subtracts them from the walkable
area and triangulates. 3D voxelises the static geometry and follows Recast's
pipeline — regions, contours, polygons — as a Rust port in core, taking
`oxidized_navigation`'s baker as the reference rather than its dependency.

## 2. The surface

| Need | Decision |
| --- | --- |
| A path over a navmesh | Step 1: `navigation2d.find_path(from, to)`, `navigation3d.find_path`, over `polyanya`; `closest_point`, `raycast`, `random_point` |
| An agent that walks the path | Step 2: `agent2d` / `agent3d` with `radius`, `height`, `max_speed`, `max_acceleration`, `target`, `path_tolerance`, `drive`; `on_target_reached`, `on_path_changed` |
| Agents avoiding each other | Step 3: `avoidance = true`, `neighbour_distance`, `max_neighbours`, `time_horizon`, `avoidance_layers`, `avoidance_mask`, through `dodgy_2d` in 2D and on the ground plane in 3D |
| Obstacles that move | Step 3: `obstacle2d` / `obstacle3d` with `radius` or a polygon, `carve = false`: avoidance only |
| Obstacles that carve the mesh | Step 3: `carve = true` rebakes the region around them on the next step, bounded by a budget |
| Baking a 2D level | Step 4: from colliders and tile maps, `agent_radius` |
| Baking a 3D level | Step 5: from colliders, meshes, heightfields and voxels; `agent_radius`, `agent_height`, `max_slope`, `max_step`, `cell_size` |
| Links: jumps, ladders, doors | Step 6: `navlink` with `end`, `bidirectional`, `cost`, `layers` |
| Layers and several regions | Step 6: `layers` on a region and `mask` on an agent; regions merge at bake, a region disables at run time |
| A grid instead of a mesh | Step 7: `find_path_grid` over a `tilemap`'s collision through the `pathfinding` crate; 4- or 8-connected |
| Seeing it | Step 8: the navmesh and every agent's path in the Physics persona through `debug_lines`; a Bake button |
| Crowd formations, flocking, steering behaviours | **Not planned**; a script over `agent.velocity()` |
| Godot's NavigationServer as an API | **Not planned**; components and two modules are the shape every other subsystem has |

## 3. Steps

1. The asset, `find_path` in both dimensions, the determinism audit of
   `polyanya`.
2. Agents.
3. Avoidance and obstacles, the audit of `dodgy_2d`.
4. The 2D baker.
5. The 3D baker.
6. Links, layers, regions.
7. Grid paths.
8. The editor.

## 4. What CI can prove, and what it cannot

Headless proves a path over a fixture mesh equals a golden corner list on
every OS, that a hundred agents crossing a room digest identically across
the matrix, and that a bake of a fixture scene hashes the same. CI cannot
prove an agent looks natural; the showcase clip is where that is judged.

## 5. Open questions

1. **What the audits find.** `polyanya` orders a binary heap by `f32` cost
   and `dodgy_2d` solves a small linear program; both are the kind of code
   that can differ by a rounding across compilers. If either does, the
   port into core is the plan, and step 1's estimate doubles.
2. **A bake at export.** Baking in the editor and shipping the asset is the
   proposal; a level assembled at run time bakes itself. Whether `balaur
   export` should refuse a scene whose navmesh is older than its colliders
   is a question for the first stale one.
