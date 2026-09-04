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
4. **`f64` for the whole engine.** Phase 13 converts at the physics seam. If
   a game needs `f64` in `Transform` too — a solar system, not a large map —
   that is glamx's `D*` types through every crate, and a plan of its own.
5. **2D parity, which ARCHITECTURE claims and the tree does not have.**
   `physics2d` lacks 21 readers `rapier2d` offers — `aabb`, `swept_aabb`,
   `active_bodies`, `bodies`, `closest_points`, `time_of_impact`, `contacts`,
   `collider_mass`, `collider_volume`, `collider_mesh`, `handles`,
   `effective_dominance`, `is_moving`, `potential_energy`,
   `predict_position_with_forces`, `set_collider`, `solve_ik`, `voxel`,
   `voxel_at`, `set_voxel` — and `collider2d` the `voxels`, `voxelized_mesh`,
   `convex_decomposition` and `fit` kinds. Beside the gaps, four defects:
   `collider2d.one_way` is a no-op (`dim2/collider.rs:262-274` never calls
   `encode_one_way`), the 2D `modify_contacts` hook never reaches a script
   (`dim2/events.rs:160-220`), 2D `move_character` drops the node's rotation
   (`dim2/character.rs:130`), and 2D colliders do not round-trip through
   `get` (`dim2/collider.rs:276-285`: no `collider_params`, no offset, no
   mesh kinds). `body2d` advertises `gyroscopic` and never applies it.
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

One question is advice rather than code: in 2D a `heightfield` is a polyline
over a height array, which is a side-scroller's ground. Whether that beats
authoring a `polygon` is for whoever builds the second 2D example — both are
built.
