# Plan: navigation

A navmesh, paths over it, and agents that avoid each other. Navigation is
simulation: every query runs on the fixed step in `f32` on `libm`, agents
live in a `DetHashMap`, their state is in the snapshot and their positions
are in the digest. A crate that differs between two platforms is written out
in core with a test that runs it twice, as the triangulator was.

## Steps

1. **The asset and paths.** A `navmesh` asset in `navigation/`: `positions`
   and `polygons` as index loops in a `mesh` asset's shape, shared by 2D and
   3D. `navigation2d.find_path(from, to)` and `navigation3d.find_path` over
   `polyanya`, with `closest_point`, `raycast` and `random_point`, after a
   determinism audit of `polyanya`. Ends with: a path over a fixture mesh
   equals a golden corner list on every OS.
2. **Agents.** `agent2d` and `agent3d` with `radius`, `height`, `max_speed`,
   `max_acceleration`, `target`, `path_tolerance` and `drive` (`transform`,
   `character` or `none`), and `on_target_reached` and `on_path_changed`. An
   agent replans when its target moves past `path_tolerance` and answers
   `agent.velocity()`, which a script hands to `move_by` or a body. Ends with:
   an agent walks a fixture mesh to its target.
3. **Avoidance and obstacles.** ORCA through `dodgy_2d`, after its audit, in
   2D and on the ground plane in 3D: `avoidance`, `neighbour_distance`,
   `max_neighbours`, `time_horizon`, `avoidance_layers` and `avoidance_mask`.
   `obstacle2d` and `obstacle3d` with a radius or a polygon; `carve = true`
   rebakes the region around them within a budget. Ends with: a hundred
   agents crossing a room digest identically across the CI matrix.
4. **The 2D baker.** From colliders and tile maps: grow each obstacle by
   `agent_radius` with `i_overlay`'s `OutlineOffset` in `MathMode::Integer`,
   union, subtract from the walkable area, triangulate, and merge into convex
   polygons. `balaur bake` and the editor's Bake button write the asset, and
   `navigation.bake(node)` runs the same code at run time. Ends with: a bake
   of a fixture scene hashes the same on every OS.
5. **The 3D baker.** Voxelise the static colliders, meshes, heightfields and
   voxels, then regions, contours and polygons as Recast does, in core, with
   `oxidized_navigation`'s baker as the reference: `agent_radius`,
   `agent_height`, `max_slope`, `max_step`, `cell_size`. Ends with: the same
   hash test in 3D.
6. **Links, layers and regions.** `navlink` with `end`, `bidirectional`,
   `cost` and `layers`; `layers` on a region and `mask` on an agent; regions
   merge at bake and disable at run time.
7. **Grid paths.** `find_path_grid` over a `tilemap`'s collision through the
   `pathfinding` crate, 4- or 8-connected.
8. **The editor.** The navmesh and every agent's path in the Physics
   workspace through `debug_lines`, and the Bake button.

Not planned: crowd formations, flocking and steering behaviours (a script
over `agent.velocity()`), and Godot's NavigationServer as an API.

## Open questions

1. **What the audits find.** `polyanya` orders a heap by `f32` cost and
   `dodgy_2d` solves a small linear program. If either differs across
   compilers, the port into core is the plan.
2. **A bake at export.** Whether `balaur export` should refuse a scene whose
   navmesh is older than its colliders waits for the first stale one.
