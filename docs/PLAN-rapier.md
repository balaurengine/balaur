> **Status:** all thirteen phases shipped on 2026-09-03 in
> `crates/balaur_physics`; see [generated/](generated/) for what the code
> does. What the build left open is below.

# Plan: the rest of Rapier — what is still open

1. **Script hooks on a parallel build.** rapier calls hooks from worker
   threads under `parallel`, so a hook that talks to Rune cannot run there.
   One design lifts the exclusion at the price of one step of latency: the
   `Sync` hook reads a `DetHashMap<(handle, handle), Decision>` filled on the
   main thread, and after each step the plugin asks the script about the
   pairs the hook saw for the first time, so the answer applies from the next
   step. A new pair defaults to *allow* for one step. Whether that latency is
   acceptable is a question for the first game that needs both; until then
   the two are exclusive and say so.
2. **Solver tuning in a recording's header.** Both halves are built — a
   `[physics]` table read once after the project loads, and
   `physics.set_tuning` for a game that tunes at run time. What is open is the
   recording: until its header carries the tuning, a replay trusts that the
   manifest has not changed since.
3. **Layer names.** `layers = ["0", "3"]` is honest and unreadable. Godot
   names layers in project settings and the inspector relabels the
   checkboxes. That needs schema options resolved at inspector time rather
   than at registration, which no other component wants; it can wait until
   someone has shipped a game with 30 layers.
4. **A threaded solver. Built 2026-09-04.** rapier compiles its threaded
   pipeline only under `all(parallel, not(unsync-callbacks))`, so the three
   script physics hooks had to go: a hook runs on rapier's threads and may not
   hold an `Engine`. `filter_contact` and `filter_overlap` cost nothing to
   lose — they were passed only the other node, which is exactly what
   `collision_groups` and `solver_groups` already decide in the broad phase.
   `modify_contacts` is gone outright. One-way platforms stay: their axis
   rides in the collider's `user_data`.

   Threading is not a feature — it is how the solver runs. The thread count is
   one less than `available_parallelism` reports, capped at eight, and
   `physics.set_threads` overrides it. Varying it is safe because the digest
   does not depend on it, which `tests/threads.rs` asserts.

   Still open: the event collector takes a `Mutex` per event now that handlers
   run on rapier's threads. Events are opt-in per collider and a step raises
   tens, so it should be noise — but a game that turns on contact-force events
   broadly would serialise there. `cargo bench -p balaur_bench --bench engine
   -- physics_step` is the measurement; the fix, if it shows, is per-thread
   buffers merged after the step, which costs nothing because `take()` already
   sorts by a total key.

5. **`f64`. Not planned, dropped 2026-09-04.** It bought precision nothing
   asked for, and broke twice unnoticed between CI runs that never built it.
5. **2D parity, which ARCHITECTURE claims and the tree does not have.**
   `physics2d` lacks 21 readers `rapier2d` offers — `aabb`, `swept_aabb`,
   `active_bodies`, `bodies`, `closest_points`, `time_of_impact`, `contacts`,
   `collider_mass`, `collider_volume`, `collider_mesh`, `handles`,
   `effective_dominance`, `is_moving`, `potential_energy`,
   `predict_position_with_forces`, `set_collider`, `solve_ik`, `voxel`,
   `voxel_at`, `set_voxel` — and `collider2d` the `voxels`, `voxelized_mesh`,
   `convex_decomposition` and `fit` kinds. One is owed to a plan:
   `docs/PLAN-tilemap.md` step 1 builds tile collision out of the 2D
   `voxels` kind, for the internal-edge handling that keeps a body from
   catching on a seam.

   The four defects this item listed were re-audited on 2026-09-06 and are
   gone: `one_way` encodes its axis (`dim2/collider.rs:239`) and the 2D hook
   reads it (`dim2/events.rs:166-176`), `move_character` keeps the node's
   rotation (`dim2/character.rs:103`), colliders round-trip through `get`
   (`dim2/collider.rs:244-261`), `body2d` never advertised `gyroscopic`, and
   a script physics hook is gone by design with the threaded solver (item 4).

   One defect is real, in **both** dimensions: a one-way platform fires only
   when it is `collider1` of the pair. `update_as_oneway_platform` reads the
   axis in `collider1`'s local frame, and both hooks test that collider alone
   (`events.rs:215-218`, `dim2/events.rs:166-176`), so which body falls
   through a platform depends on the order rapier happens to give the pair.
   Both sides need testing, with the axis flipped for `collider2`.
6. **What rapier 0.35 still has that no scene or script reaches.** The rule
   is wrap everything and state the constraint, so each of these is a phase
   when someone asks: `IntegrationParameters.friction_model` and
   `normalized_contact_recycle_distance`; `RigidBodyBuilder::linvel`,
   `angvel` and `sleeping` as initial state, and the readers
   `is_ccd_active`, `center_of_mass`, `mass_properties` (body `get` reports
   none of `mass`, `inertia`, `center_of_mass`); `ColliderBuilder::capsule_x`,
   `capsule_z`, `capsule_from_endpoints`, `oriented_polyline`,
   `convex_polyline` and the `round_*` shapes, `convex_mesh`,
   `convex_decomposition_with_params` (VHACD parameters),
   `heightfield_with_flags` (`FIX_INTERNAL_EDGES`), `voxels_from_points`,
   `mass_properties`; `QueryFilterFlags` singly, `cast_shape_nonlinear`,
   `cast_ray_and_get_normal`, `project_point_and_get_feature`,
   `ShapeCastOptions.target_distance`, a rotated shape cast;
   `GenericJoint::set_softness`, `coupled_axes`, `set_local_frame1/2`,
   per-axis motors on a spherical joint, `MultibodyJointSet::insert_kinematic`,
   `InverseKinematicsOption` and an IK target rotation;
   `CharacterCollision.character_pos`, `translation_applied`,
   `hit.time_of_impact` and a `filter` table for `move_character`;
   `DebugRenderMode` per joint kind and `DebugRenderStyle`; the wheel readers
   `forward_impulse`, `side_impulse` and `RayCastInfo`, and
   `current_vehicle_speed` (`vehicle_speed` hard-codes `Z`, `vehicle.rs:206`);
   `PidController` and `PdController`; `Counters::enable` and its timers
   (`counters()` reads zeros today, `tuning.rs:218-236`); the `SENSOR` and
   `REMOVED` flags and the contact pair on `on_collision_start`. Also:
   `physics3d.contacts` reports a local-space point where every other query
   is world-space (`query.rs:568-577`), and `tuning()` reads back 12 of the
   19 keys `set_tuning` accepts. The defects that break determinism — stale
   handles after a free, wheel state outside the snapshot — are
   `docs/PLAN-hardening.md` phase 1.

7. **Internal edges are fixed on one shape in four.** A 3D `trimesh`
   collider sets `TriMeshFlags::FIX_INTERNAL_EDGES` by default
   (`collider.rs:44-46`), which is what keeps a character from catching on
   the seam between two triangles of a flat floor. The other three shapes
   with the same problem do not:

   | Shape | Today | Should be |
   | --- | --- | --- |
   | `collider3d` `heightfield` | `ColliderBuilder::heightfield(grid, extent)` (`collider.rs:166`) | `heightfield_with_flags` with `HeightFieldFlags::FIX_INTERNAL_EDGES`, on by default like the trimesh |
   | `collider2d` `trimesh` | `ColliderBuilder2::trimesh` (`dim2/collider.rs:131`) | `trimesh_with_flags`; the flag is not dimension-gated |
   | `collider2d` `polyline` | `ColliderBuilder2::polyline(points, None)` (`dim2/collider.rs:142`) | `PolylineFlags::ORIENTED`, which parry documents as removing "the spurious sideways push a body gets at a convex corner" — the 2D form of the same defect, on the shape a side-scroller's ground uses |

   A 2D `heightfield` has no flags to set: parry's 2D heightfield is a
   polyline of segments, so `ORIENTED` on the polyline is the whole story.
   The same family is why `docs/PLAN-tilemap.md` step 1 builds tile collision
   out of parry's `Voxels` rather than a row of cuboids, and why
   `docs/PLAN-voxels.md` needs no fix at all — the voxel shape classifies its
   own cells.

8. **Where parry may be used, and what stays hand-written.** The rule is in
   ARCHITECTURE.md ("parry: geometry every crate may use, for capability"),
   and it replaces the older reflex that core must not learn parry. parry is
   already linked into every build, the wasm bundle included, so a dependency
   on it costs a shipped game nothing; it is taken for capability — `Bvh`
   for culling and picking, exact ray and point queries — and never merely to
   delete equivalent code. `primitive` (no UVs in parry's tessellations),
   `csg` (parry has no mesh union or difference), `geometry2d`'s booleans on
   `i_overlay` (parry has polygon intersection alone) and its convex hull
   stay as they are.

   Three follow-ups when that lands:

   - `balaur_render`'s `pick` is bounds-only and becomes an exact ray cast
     against the mesh, over a `Bvh` (`docs/PLAN-editor-ergonomics.md`).
   - `crates/balaur_core/src/csg.rs` opens with a rationale that says core
     "should not learn to" depend on parry, which is no longer the rule — the
     paragraph keeps its real reason, that parry cannot do union or
     difference at all.
   - **Triangulation consolidates onto `i_triangle`.** parry's ear clipper is
     `pub(crate)` and cannot be called, so the duplication to remove is our
     own: `balaur_core::triangulate` and the `i_triangle` that
     `balaur_ui::glyph` already fills glyph outlines with. `i_triangle`
     depends on `i_overlay`, which core has, so it costs two small crates.
     The work: `uncheck_triangulate` for a `mesh` asset's authored
     `polygons`, whose vertices carry uvs, colours and skin weights that an
     inserted point would not have — assert it returns the input points
     before relying on it, and interpolate attributes for any it does add;
     `triangulate`, which resolves self-intersections, for the paths that
     take a loop from a tool or a boolean, where
     `a_self_crossing_loop_still_yields_a_triangulation` is the behaviour to
     keep; holes become real, so `mesh::triangulated` stops filling every
     ring on its own; `crates/balaur_core/src/triangulate.rs` and its 120
     lines go, and `tests/polygon_mesh.rs` asserts through `mesh` and
     `geometry2d.triangulate` instead. Every polygon mesh's triangle list
     moves, so the fixtures and the digests are part of the same commit.

   Everything else already wraps rapier or parry: the character and vehicle
   controllers, the debug render pipeline, the query pipeline, and
   `geometry3d`'s hull, convex decomposition, voxelisation, split, intersect
   and pieces. What 2D lacks there is item 5's list, not new code.

One question is advice rather than code: in 2D a `heightfield` is a polyline
over a height array, which is a side-scroller's ground. Whether that beats
authoring a `polygon` is for whoever builds the second 2D example — both are
built.
