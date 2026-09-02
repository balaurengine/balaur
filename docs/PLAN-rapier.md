> **Status:** not started. Written 2026-09-02. An inventory of what Rapier
> 0.35.3 and parry 0.30.2 ship **today**, which of it `balaur_physics` does
> not reach, and the order to close the gap in — all of it. Every row exists
> in `Cargo.lock` right now: nothing here waits on upstream. That is what
> separates this file from `docs/PLAN-physics.md`, which is the other half of
> physics — soft bodies, tearing, fluids and granular materials, none of
> which Rapier has written yet.
>
> Nothing is left out on purpose. Where a capability carries a constraint
> (rapier's own `Sync` bound on hooks, a compile error between `simd8` and
> `enhanced-determinism`), the constraint is stated and the capability is
> still scheduled.

# Plan: the rest of Rapier

## Where the tree is today

`crates/balaur_physics` is 1,514 lines over two files and wraps roughly a
fifth of the crate it depends on.

| Wrapped today | Where |
| --- | --- |
| Bodies: `dynamic`, `static`, `kinematic` — position-based only | `body3d` / `body2d` schemas |
| 3D shapes: ball, cuboid, capsule, cylinder, cone, triangle, trimesh, convex_hull, polyline, heightfield | `collider_builder`, `mesh_collider`, `heightfield_collider` |
| 2D shapes: circle, rect, capsule | `dim2::collider_builder` |
| Collider material: restitution, friction, density, sensor | both `collider*` schemas |
| World: pause, gravity setter, sleeping toggle, clear | `physics`, `physics3d.set_gravity`, `physics2d.set_gravity` |
| Per body from script: `apply_impulse`, `set_linear_velocity`, `linear_velocity`, `overlaps`; 2D also has angular velocity and `max_contact_impulse` | `install_body_api`, `install_physics2d_api` |
| Whole-world snapshot; a velocity-and-sleep digest | `build_physics_snapshot`, `build_physics_digest` |
| Presets `rigid_body3d`, `static_body3d`, `rigid_body2d`, `static_body2d` | `register_physics_presets` |

Four constraints govern every row below. They are why this is a plan and not
a weekend of `m.function(...)` calls.

1. **Determinism.** Both worlds are built with `enhanced-determinism` and
   stepped once per `Stage::FixedUpdate` at `FIXED_DT`. Anything that reads
   the world and hands a script an ordered result must impose that order
   itself — rapier's iteration follows its BVH, not entity order.
2. **Snapshot.** `PhysicsFrame` serializes the whole rapier world, so new
   *simulation* state rides along for free; new **entity ↔ handle maps** do
   not, and each one is a line in `PhysicsFrame` and its `Ref` twin.
3. **Digest.** The digest hashes what no component `get` reports. Every
   feature that can make two peers diverge invisibly — a joint's motor, an
   edited voxel grid — owes it a row.
4. **Schema-driven or it is half a feature.** A capability reachable only
   from a script is invisible to the editor, the scene file, the inspector
   and `docs/generated`. Each area below lands as a component schema *and*
   a script call, in one commit, per N16.

## What Rapier has, and what we would call it

### Rigid bodies

| Rapier 0.35 | Balaur surface | Note |
| --- | --- | --- |
| `RigidBodyType::KinematicVelocityBased` | `kind = "kinematic_velocity"` | The one body kind we skip; `kinematic` stays position-based |
| `set_body_type(kind, wake)` | `body3d.kind` changes in place | Today `apply` rebuilds the body and loses its velocity |
| `set_linear_damping`, `set_angular_damping` | `linear_damping`, `angular_damping` | |
| `set_gravity_scale` | `gravity_scale` | Per body: `0` for a platform that hangs in the air, negative for a balloon |
| `set_enabled_translations` / `_rotations`, `lock_*` | `lock_translation`, `lock_rotation` | Needs P2 (`flags`). This is how 2.5D and top-down games are built |
| `enable_ccd`, `set_soft_ccd_prediction`, `set_allow_fast_rotation`, `is_ccd_active` | `ccd`, `soft_ccd`, `fast_rotation`, `is_ccd_active` | Bullets through walls |
| `set_dominance_group`, `effective_dominance_group` | `dominance`, `effective_dominance` | An `i8`: a higher group wins every contact against a lower one, and every non-dynamic body outranks them all. The player shoving crates that cannot shove back |
| `set_additional_mass`, `set_additional_mass_properties`, `recompute_mass_properties_from_colliders` | `mass`, `center_of_mass`, `principal_inertia` | `0` keeps today's density-derived mass |
| `RigidBodyActivation`: `normalized_linear_threshold`, `angular_threshold`, `time_until_sleep` | `sleep_linear_threshold`, `sleep_angular_threshold`, `sleep_time` | Per body; today sleeping is one world-wide toggle |
| `set_additional_solver_iterations` | `solver_iterations` | Per-body accuracy, for the one stack that jitters |
| `set_enabled` | `enabled` | Cheaper than removing and rebuilding a body |
| `add_force`, `add_torque`, `add_force_at_point`, `reset_forces`, `reset_torques`, `user_force`, `user_torque` | same names | Continuous force, as against today's impulse-only; the readers show what a step is about to integrate |
| `apply_torque_impulse`, `apply_impulse_at_point` | same names | |
| `set_angvel`, `angvel` (3D) | `set_angular_velocity`, `angular_velocity` | Exists in 2D only today — a pure asymmetry |
| `velocity_at_point`, `mass`, `kinetic_energy`, `gravitational_potential_energy`, `is_moving` | same names | Readers N8 already wants |
| `predict_position_using_velocity`, `predict_position_using_velocity_and_forces`, `next_position` | `predict_position(node, dt, forces)`, `next_position` | Where a body will be, for aiming and cameras |
| `sleep`, `wake_up`, `is_sleeping`, `PhysicsWorld::wake_up_all` | `sleep`, `wake_up`, `is_sleeping`, `physics3d.wake_all` | |
| `set_position` on a dynamic body | `teleport` | See Open question 1 |
| `enable_gyroscopic_forces`, `angvel_with_gyroscopic_forces` | `gyroscopic` | Tumbling in 3D |
| `colliders()`, `rigid_bodies()`, `active_bodies()` | `colliders(node)`, `physics3d.bodies()`, `physics3d.active_bodies()` | Node lists, sorted by entity bits |

### Colliders

| Rapier 0.35 | Balaur surface | Note |
| --- | --- | --- |
| `position_wrt_parent`, `ColliderBuilder::delta` | `offset`, `offset_rotation` | Today a collider sits exactly on its node |
| Several colliders per body | Colliders on child nodes attach to the nearest ancestor body | The compound-shape story; see Design |
| `SharedShape::scaled` / `Shape::scale_dyn` | The collider follows `Transform.scale` | Ignored entirely today; a ball under non-uniform scale becomes the convex approximation parry returns |
| `collision_groups`, `solver_groups` (`InteractionGroups`, 32 layers) | `layers`, `mask`, `solver_layers`, `solver_mask` | Needs P2 |
| `active_events` (`COLLISION_EVENTS`, `CONTACT_FORCE_EVENTS`) | `events = ["collision", "contact_force"]` | Gates the events phase |
| `contact_force_event_threshold` | `contact_force_threshold` | |
| `active_collision_types` | `active_collisions` | Static-static pairs, off by default in rapier |
| `active_hooks` (`FILTER_CONTACT_PAIRS`, `FILTER_INTERSECTION_PAIR`, `MODIFY_SOLVER_CONTACTS`) | `hooks = ["filter_contact", "filter_overlap", "modify_contacts"]` | Gates the script hooks; see Design |
| `ContactModificationContext::update_as_oneway_platform` | `one_way`, `one_way_axis` | Platformers; a built-in `modify_contacts`, no script needed |
| `friction_combine_rule`, `restitution_combine_rule` (`CoefficientCombineRule`) | `friction_combine`, `restitution_combine` | `average` \| `min` \| `multiply` \| `max` \| `clamped_sum` |
| `contact_skin` | `contact_skin` | Speculative margin; stops thin-shape tunnelling |
| `set_mass`, `set_mass_properties`, `volume`, `mass` | `mass` on the collider; `volume`, `mass` readers | Overrides density |
| `set_enabled` | `enabled` | |
| `compute_aabb`, `compute_swept_aabb` | `aabb(node)`, `swept_aabb(node)` | |
| `set_shape` at run time | `physics3d.set_collider(node, params)` | Already implied by `apply_collider`; make it a call |
| `user_data` on bodies and colliders | Carries the entity id | Handle → node in O(1); `overlaps` scans the whole map linearly today |
| `RigidBodyHandle`, `ColliderHandle`, `ImpulseJointHandle` | `physics3d.handles(node)` | Index and generation, for logs and for matching rapier's own debug output |

### Shapes not built today

| Rapier / parry | Balaur `kind` | Note |
| --- | --- | --- |
| `voxels`, `voxels_from_points`, `voxelized_mesh` (parry `Voxels`, with `set_voxel`, `crop`, `split_with_box`, `voxel_state`) | `voxels`, `voxelized_mesh` | Both dimensions. Destructible terrain: the grid is editable at run time |
| `compound` | `compound` | Or, better, child colliders — see Design |
| `convex_decomposition` (+ `_with_params`, `VHACDParameters`) | `convex_decomposition` | The only way to get a *dynamic* concave shape |
| `halfspace` | `halfspace` | An infinite floor, one plane, no tessellation |
| `segment` | `segment` | |
| `round_cuboid`, `round_cylinder`, `round_cone`, `round_triangle`, `round_convex_*` | `border` float on the existing kinds | A rounded shape is a shape plus a radius, not nine more kinds |
| `convex_polyline` (2D), `convex_mesh` / `ConvexPolyhedron` (3D) | `convex_hull` accepts pre-built hulls | |
| parry `ear_clipping`, `hertel_mehlhorn` (2D) | `polygon` — a concave outline, partitioned into convex parts | The 2D level-geometry shape every platformer wants |
| `MeshConverter` (`Obb`, `Aabb`, `ConvexHull`, `ConvexDecomposition`), `converted_trimesh` | `kind = "fit"`, `fit = "obb" \| "aabb" \| "convex_hull" \| "convex_decomposition"`, from a `mesh` | Auto-colliders from a model |
| `trimesh_with_flags` (`TriMeshFlags`: `FIX_INTERNAL_EDGES`, `MERGE_DUPLICATE_VERTICES`, `DELETE_DEGENERATE_TRIANGLES`, `DELETE_BAD_TOPOLOGY_TRIANGLES`, `ORIENTED`, `HALF_EDGE_TOPOLOGY`, `CONNECTED_COMPONENTS`) | `fix_internal_edges`, `clean`, `oriented` | `FIX_INTERNAL_EDGES` is what stops a character controller catching on flat terrain seams |
| `heightfield_with_flags` (`HeightFieldFlags`) | `fix_internal_edges` on heightfields too | |
| parry `to_trimesh`, `to_polyline`, `to_outline` for every shape, voxels included | `physics3d.collider_mesh(node)` → mesh data | Any collider becomes drawable; this is how a voxel terrain gets rendered |
| 2D: triangle, segment, polyline, trimesh, convex_hull, heightfield, halfspace | all of the above | 2D has three shapes today against 3D's ten |

### Queries — the whole `QueryPipeline`, and parry's pairwise ones

| Rapier 0.35 | Balaur surface |
| --- | --- |
| `cast_ray`, `cast_ray_and_get_normal`, `intersect_ray` | `raycast`, `raycast_all` |
| `cast_shape`, `cast_shape_nonlinear` | `shapecast` |
| `project_point`, `project_point_and_get_feature`, `intersect_point` | `nearest_point`, `point_hits` |
| `intersect_shape`, `intersect_aabb_conservative` | `shape_hits`, `box_hits` |
| `QueryFilter` (`exclude_sensors`, `exclude_solids`, `only_dynamic`, `only_kinematic`, `only_fixed`, `groups`, `exclude_collider`, `exclude_rigid_body`, `predicate`) | one `filter` options table — `predicate` included, as a script closure |
| parry `query::distance`, `closest_points`, `contact`, `intersection_test`, `cast_shapes`, `cast_shapes_nonlinear` between two shapes | `distance(a, b)`, `closest_points(a, b)`, `contact(a, b)`, `intersects(a, b)`, `time_of_impact(a, b)` on two nodes' colliders |
| `contact_pair(a, b)`, `intersection_pair(a, b)` | `contact(a, b)`, `is_overlapping(a, b)` |

Scripts have `overlaps` today, and it reports only pairs where one side is a
sensor. There is no way to ask a question of the world that is not already a
collision.

### Events, contacts and hooks

| Rapier 0.35 | Balaur surface |
| --- | --- |
| `step_with_events`, `EventHandler`, `CollisionEvent::Started` / `Stopped` | `on_collision_start(other)`, `on_collision_stop(other)` on the node's script |
| `ContactForceEvent` | `on_contact_force(other, magnitude)` |
| `contact_pairs_with`, `ContactPair`, manifolds and points | `physics3d.contacts(node)` → point, normal, impulse per contact |
| `intersection_pairs_with` | `overlaps`, kept, but filtered and sorted |
| `PhysicsHooks::filter_contact_pair` → `SolverFlags` | `filter_contact(other) -> bool` on the node's script |
| `PhysicsHooks::filter_intersection_pair` | `filter_overlap(other) -> bool` |
| `PhysicsHooks::modify_solver_contacts` (`ContactModificationContext`: normal, points, friction, restitution, user data per pair) | `modify_contacts(other, contacts) -> contacts` |

3D has no `max_contact_impulse`; 2D has it and no `contacts`. Both get both.

### Joints — none of them exist today

`fixed`, `revolute`, `prismatic`, `spherical` (3D only), `rope`, `spring`,
`pin_slot`, plus `GenericJoint` behind them all — exposed as `generic` with
per-axis `locked_axes` and `coupled_axes`; per-axis limits; motors
(`MotorModel::AccelerationBased` / `ForceBased`, target velocity, target
position, max force, stiffness, damping); `contacts_enabled`; softness;
`JointEnabled`; and two solvers — `insert_impulse_joint` and
`insert_multibody_joint` (reduced coordinates: no drift, no loops).

`ImpulseJoint::impulses` is the reaction force, which is what a breakable
joint reads. `Multibody` carries `damping`, `armature`, `frictions`,
`self_contacts_enabled`, `forward_kinematics`, `joint_velocity`, and
**`inverse_kinematics`** with `InverseKinematicsOption` (`damping`,
`max_iters`, `constrained_axes`, `epsilon_linear`, `epsilon_angular`) — an IK
solver already in the tree, which `docs/PLAN-animation-and-resources.md`
lists as future work.

### Character controller — does not exist today

`KinematicCharacterController`: `up`, `offset`, `slide`, `autostep`
(`max_height`, `min_width`, `include_dynamic_bodies`), `max_slope_climb_angle`,
`min_slope_slide_angle`, `snap_to_ground`, `normal_nudge_factor`;
`CharacterLength::Absolute` / `Relative`; `move_shape` returning
`EffectiveCharacterMovement { translation, grounded, is_sliding_down_slope }`
and every `CharacterCollision` on the way (handle, hit, translation applied and
remaining); `solve_character_collision_impulses` so a character can push what
it walks into.

### Vehicle and PID controllers — do not exist today

`DynamicRayCastVehicleController` (`chassis`, `index_up_axis`,
`index_forward_axis`, `current_vehicle_speed`, `update_vehicle`) with
`WheelTuning` (`suspension_stiffness`, `suspension_compression`,
`suspension_damping`, `max_suspension_travel`, `side_friction_stiffness`,
`friction_slip`, `max_suspension_force`) and per `Wheel`: connection point,
direction, axle, `suspension_rest_length`, `radius`, `steering`,
`engine_force`, `brake`, and the readers `rotation`, `forward_impulse`,
`side_impulse`, `wheel_suspension_force`, `raycast_info` (ground contact).
`PidController` / `PdController` with `set_axes`, `reset_integrals`,
`linear_rigid_body_correction`, `angular_rigid_body_correction`,
`rigid_body_correction`, for driving a kinematic body toward a target pose.

### Geometry — parry's toolkit for cutting, merging and fitting

| parry 0.30 | Balaur surface |
| --- | --- |
| `TriMesh::split`, `canonical_split`, `Cuboid::canonical_split`, `Segment::local_split` (`SplitResult`) | `geometry3d.split(mesh, plane)` → two meshes |
| `TriMesh::intersection_with_plane`, `intersection_with_aabb`, `intersection_with_cuboid` | `geometry3d.slice(mesh, plane)` → outline, `clip(mesh, box)` |
| `transformation::intersect_meshes` (+ `_with_tolerances`) | `geometry3d.intersect(a, b)` — a boolean intersection of two meshes |
| `polygons_intersection`, `convex_polygons_intersection` (2D) | `geometry2d.intersect(a, b)` |
| `TriMesh::connected_components`, `connected_component_meshes` | `geometry3d.pieces(mesh)` — a broken mesh into its parts |
| `transformation::convex_hull`, `try_convex_hull` | `geometry3d.convex_hull(points)` |
| `vhacd` (`VHACDParameters`) | `geometry3d.convex_decomposition(mesh, opts)` |
| `voxelization` (`FillMode::SurfaceOnly` / `FloodFill`) | `geometry3d.voxelize(mesh, size, fill)` → a `voxels` asset |
| `hertel_mehlhorn`, ear clipping (2D) | `geometry2d.convex_parts(polygon)`, `triangulate(polygon)` |
| `to_trimesh`, `to_polyline`, `to_outline` | `collider_mesh(node)` above |
| `wavefront` | `geometry3d.to_obj(mesh)` — export for tools |

Together with voxels, this is the destruction toolkit: slice a mesh, spawn
each piece as a body with a `convex_decomposition` collider, dig the voxel
terrain under it. Every one of these is a pure function on mesh data, which
is why it is a script module of its own (`geometry3d` / `geometry2d`: D2, a
noun for what it owns) and not another `physics3d` call. `shape` is not
available as the name — it is the render component.

### World tuning, profiling and safety

| Rapier 0.35 | Balaur surface |
| --- | --- |
| `IntegrationParameters`: `num_solver_iterations`, `num_internal_pgs_iterations`, `num_internal_stabilization_iterations`, `max_ccd_substeps`, `min_ccd_dt`, `length_unit`, `contact_softness`, `static_contact_softness`, `warmstart_coefficient`, `warmstart_joints`, `friction_model`, `friction_in_bias_pass`, `contact_clustering`, `contact_recycling`, the `normalized_*` tolerances | `[physics]` in the project manifest, plus setter/reader pairs |
| `PhysicsWorld::gravity` | `physics3d.gravity` — the reader `set_gravity`'s comment says to add when someone needs it |
| `PhysicsWorld::quarantine()` (`bodies`, `colliders`, `is_empty`) — what rapier disabled because its pose, velocity or geometry went non-finite | a WARN naming the node, and `physics3d.quarantined()` |
| `Counters` (`step_time`, `stages`, `cd`, `solver`, `ccd`, `ncontacts`, `nconstraints`; behind the `profiler` feature) | `physics.counters()` → a table, and a row in the editor's profiler (`docs/PLAN-editor.md` §6) |
| `CollisionPipeline` — contacts and overlaps without dynamics | The editor's overlap check while paused; `physics.set_mode("collision")` for a world that only detects |
| `PhysicsWorld::debug_render`, `DebugRenderMode` (colliders, body axes, joints, contacts, solver contacts, AABBs), `DebugRenderStyle` (every colour and length) | `physics.set_debug_draw(opts)` / `physics.debug_draw()` into `DebugLineBuffer` / `DebugLineBuffer2d` |
| `configure_thread_pool`, `set_thread_pool`, `num_threads` (`parallel` feature) | `physics.set_threads(n)` / `physics.threads()` |
| `rapier3d-f64`, `rapier2d-f64` | a `f64` cargo feature on `balaur_physics` |

`length_unit` deserves its own line: it is how rapier is told that a 2D world
measures in pixels rather than metres, and every tolerance in the solver is
scaled by it. A 2D game that feels like soup at 64 px/m is one field away
from feeling right.

### Cargo features, and the one real incompatibility

| rapier feature | What it buys | Balaur |
| --- | --- | --- |
| `enhanced-determinism` | `libm` everywhere, parry's too | On today; stays on |
| `serde-serialize` | The whole-world snapshot | On today |
| `parallel` | rayon in the narrow phase, the solver and the BVH refresh. The solver is colored and staged — its own doc: *"results depend only on the coloring, not on batch distribution"* — and the broad phase states the same for its events, so thread count does not change results | On, with a digest test at 1, 2 and 8 threads to prove it |
| `unsync-callbacks` | Drops the `Sync` bound on `PhysicsHooks` and `EventHandler` — the only way a hook can call into a script host — and cfg's the thread pool out with it | See Design: hooks |
| `debug-render` | `DebugRenderPipeline` | On |
| `profiler` | `Counters` timings | On in the editor build |
| `simd8` | 8-lane SIMD | **Off. `compile_error!` against `enhanced-determinism`** — the one thing rapier itself refuses to combine |
| `solver-bounds-checks`, `dev-remove-slow-accessors`, `debug-disable-legitimate-fe-exceptions` | Debug builds of rapier | Not user-facing |

## Prerequisites

Five things the engine needs before the phases can land cleanly. Each is
small; each is load-bearing for several phases.

**P1 — `type = "node"` in the component schema.** A joint names another node,
and there is no way to say that in a schema today (`components.rs` closes the
set at `float`, `bool`, `string`, `enum`, `vec2`, `vec3`, `color`, `asset`).
The value is a scene-relative path resolved with `scene::find_node`; the
inspector gets a node picker beside the asset picker it already has.

**P2 — `type = "flags"`.** A closed set of names, stored as an array of
strings, drawn as a checkbox grid. `lock_translation = ["y"]`,
`events = ["collision"]`, `hooks = ["filter_contact"]`, `layers = ["0", "3"]`.
An empty array means *none*, except on `mask`, where it means *everything* —
stated once in the schema description rather than forcing 32 strings into
every scene file. This is one new type against four otherwise-inevitable
bespoke encodings.

**P3 — Split `balaur_physics` into files.** `lib.rs` is 853 lines against
`MAX_FILE_LINES = 1200`, and phase 2 alone doubles it. Target layout:

```
crates/balaur_physics/src/
  lib.rs        plugin, state, step, snapshot, digest
  vocabulary.rs dimension-free schema text and param extraction
  body.rs  collider.rs  joint.rs  query.rs  events.rs  hooks.rs
  character.rs  vehicle.rs  geometry.rs  debug.rs
  dim2/         the same files, against rapier2d
```

**P4 — One vocabulary, two dimensions.** `dim2.rs` is a hand-copy of `lib.rs`
today, and this plan roughly quadruples what would be copied. rapier2d and
rapier3d are separate crates with incompatible types, so a generic wrapper is
not available; a macro over both would be, and it would make every one of
these files unreadable. The decision: **duplicate the rapier calls, share the
vocabulary.** Schema strings, option lists, param extraction, filter parsing
and result sorting are `toml` and numbers, and go in `vocabulary.rs`; only the
`ColliderBuilder`/`RigidBody`/`GenericJoint` calls are written twice. Where 2D
has no counterpart (`spherical`, cylinder, cone), the 2D schema simply does
not list the option — N14 rather than a runtime error.

**P5 — Debug draw first.** `PhysicsWorld::debug_render` into the existing
`DebugLineBuffer` is half a day, is render-only so it cannot touch the tick,
and makes every phase after it debuggable by eye instead of by digest. It
goes first for that reason and no other.

## Design

### Bodies and colliders

```toml
[nodes.body3d]
kind = "dynamic"                  # + kinematic_velocity
linear_damping = 0.0
angular_damping = 0.0
gravity_scale = 1.0
lock_translation = []             # flags: x, y, z
lock_rotation = ["x", "z"]        # an upright capsule, in one line
ccd = false
mass = 0.0                        # 0 = from the colliders, as today
dominance = 0
solver_iterations = 0             # additional
sleep_time = 0.4
enabled = true

[nodes.collider3d]
kind = "cuboid"
offset = [0.0, 0.5, 0.0]
layers = ["0"]
mask = []                         # empty = every layer
events = ["collision"]
hooks = []                        # filter_contact | filter_overlap |
                                  # modify_contacts
friction_combine = "average"
one_way = false
border = 0.0                      # round_* for free
```

Colliders on **child** nodes attach to the nearest ancestor body rather than
inserting a standalone static collider, which is how a compound shape gets
authored — a capsule and a sensor sphere under one body, each a node with its
own transform, each pickable in the editor. `apply_collider` already replaces
"the" collider per entity; the map is already `Vec<ColliderHandle>` per
entity, so the change is in `add_collider`'s parent lookup and in
`get_collider_params`, which must stop reading `.first()`.

`user_data` on every body and collider holds the entity's bits. That is the
reverse lookup `overlaps` does today by scanning the whole `colliders` map,
and it is what makes every query result and every event O(1) to name.

### Joints

```toml
[nodes.joint3d]
kind = "revolute"                 # fixed | revolute | prismatic |
                                  # spherical | rope | spring | pin_slot |
                                  # generic
body = "../Cart"                  # type = "node"; this node is the other end
anchor = [0.0, 0.5, 0.0]
other_anchor = [0.0, -0.5, 0.0]
axis = [0.0, 0.0, 1.0]
limits = [-0.7, 0.7]              # empty = free
motor = "off"                     # off | velocity | position
motor_target = 0.0
motor_max_force = 0.0
motor_model = "acceleration"      # acceleration | force
stiffness = 0.0
damping = 1.0
length = 0.0                      # rope: max; spring: rest
locked_axes = []                  # generic only: x, y, z, ang_x, ang_y, ang_z
contacts = false                  # let the two bodies collide
break_force = 0.0                 # 0 = unbreakable; else on_joint_break(other)
solver = "impulse"                # impulse | reduced (rapier's multibody joint)
enabled = true
```

The joint lives on a node so it can be selected, gizmo-drawn and deleted like
anything else; `joints: DetHashMap<Entity, JointHandle>` joins the two maps in
`PhysicsState` and the two lines in `PhysicsFrame`. Motors are the digest's
business: a motor target is state a peer can disagree about, so the digest
gains a per-joint row. `break_force` reads `ImpulseJoint::impulses` after the
step and, past the threshold, removes the joint and calls `on_joint_break` —
the events phase's dispatch, reused.

Reduced-coordinate chains get the two calls the multibody offers and nothing
else has:

```rune
let pose = physics3d::forward_kinematics(hand);
physics3d::solve_ik(hand, target_pose, #{ max_iters = 20, damping = 0.1 });
```

### Queries

```rune
let hit = physics3d::raycast(#{
    from = [0.0, 2.0, 0.0],
    dir = [0.0, -1.0, 0.0],
    max = 100.0,
    filter = #{
        exclude = self.node,
        only = "dynamic",
        mask = ["0"],
        predicate = |node| node.get("tag") != "ghost",
    },
});
if hit is Object { log::info(`hit ${hit.node} at ${hit.distance}`); }
```

A table rather than the float-triple idiom `apply_impulse` uses: a ray is
eight numbers before the filter, and the filter is genuinely optional (N9's
options table, not N9's constant argument). Every `*_all` result is sorted by
distance, then by entity bits, before it crosses the binding seam — rapier
walks its BVH, and BVH order is not something a replay may depend on.

`predicate` is a script closure run through `ScriptHost::invoke` during the
binding call — exactly the window `Value::Callback` is valid for. The
predicate is called with `PhysicsState` borrowed by the query, so a predicate
that calls back into `physics3d` gets an error naming the reason, not a
`RefCell` panic: the bindings use `try_borrow` and say so.

### Events and hooks

`collider3d.events` sets `ActiveEvents`; the step becomes `step_with_events`
with a collector owned by `PhysicsState`; the collector is drained **inside**
the fixed step, immediately after `world.step()`, and dispatched through
`ScriptHost::call_on` — the same seam `balaur_anim`, `balaur_net`,
`balaur_gamend` and `balaur_ui` already use. Draining after the step and
before the next `fixed_update` is what keeps a handler's impulse landing on
the step it was meant for. Events are sorted by `(entity bits, entity bits)`
before dispatch; a handler is opt-in, and a node without one costs a hash
lookup.

Hooks are the same seam called **during** the step. `collider3d.hooks` sets
`ActiveHooks`, so only a collider that asked pays; rapier then calls
`filter_contact_pair` once per candidate pair per step for those, and the
plugin's `PhysicsHooks` forwards to the node's `filter_contact(other)`.
`modify_contacts(other, contacts)` receives the manifold as a table (normal,
points, friction, restitution) and returns it changed — a custom one-way
platform, a conveyor belt, a sticky wall. Three things to say plainly:

1. **Cost.** One Rune call per opted-in pair per step, not per contact point.
   A platformer's twenty pairs are nothing; ten thousand marbles in a box
   whose walls opted in are ten thousand calls a step. The flag is on the
   collider so the author chooses.
2. **The `Sync` bound.** `PhysicsHooks` and `EventHandler` are
   `MaybeSync` — `Sync` unless rapier's `unsync-callbacks` feature is on. A
   script host is `Rc`-and-`RefCell`, not `Sync`. So script hooks need
   `unsync-callbacks`, and `unsync-callbacks` removes the thread pool: rapier
   made the two exclusive, and Balaur inherits that. `balaur_physics` gets
   a `parallel` cargo feature; with it on, `hooks = [...]` on a collider is a
   scene-load error that says why, and `one_way` and layers keep working
   because they never leave Rust. Open question 2 has the design that would
   lift this.
3. **Reentrancy.** Inside a hook the world is borrowed by the step. The hook
   gets the two nodes, the normal and the manifold, and returns a value; a
   `physics3d` call that mutates from inside one errors with a message.

### Character controller

```toml
[nodes.character3d]
up = [0.0, 1.0, 0.0]
offset = 0.01
slide = true
autostep = 0.3                    # 0 disables
autostep_min_width = 0.2
autostep_dynamic = false
max_climb_angle = 45.0            # degrees, as the inspector shows them
min_slide_angle = 30.0
snap_to_ground = 0.2              # 0 disables
normal_nudge = 0.0001
push_bodies = true
lengths = "absolute"              # absolute | relative to the shape
```

```rune
let moved = physics3d::move_character(self.node, dx, dy, dz);
if moved.grounded { self.jumps = 0; }
for c in moved.collisions { ... }  // node, point, normal, remaining
```

`move_character` applies the effective translation to the node's transform
and returns `#{ x, y, z, grounded, sliding, collisions }`. It reads the query
pipeline, so it must run in `fixed_update` — the binding says so, and the
docs page says so twice.

### Voxels

A `voxels` asset (`voxel_size` plus filled cell coordinates, the same shape of
thing `heightfield` already is), a `voxelized_mesh` kind that builds one from
a `mesh` asset at a given cell size and fill mode, and run-time edits:

```rune
physics3d::set_voxel(node, x, y, z, false);   // dig
let filled = physics3d::voxel(node, x, y, z);
let cell = physics3d::voxel_at(node, wx, wy, wz);
let halves = physics3d::split_voxels(node, box);   // two colliders from one
let mesh = physics3d::collider_mesh(node);         // parry's to_trimesh
```

An edited grid is simulation state that no velocity reflects until something
falls into the hole, so the digest gains a revision counter per voxel
collider, bumped by every edit. Drawing goes through `collider_mesh`: parry
renders every shape to triangles, voxels included, and the result is what a
`mesh` component takes. A voxel renderer with per-face materials is the
renderer's business, not this plan's.

### Parallel step and f64

`parallel` is a cargo feature on `balaur_physics` mapping to rapier's, with
`physics.set_threads(n)` / `physics.threads()` over `configure_thread_pool`.
The claim that thread count cannot change results is rapier's, and it is
tested here rather than trusted: the cross-OS digest test runs at 1, 2 and 8
threads and must agree bit for bit. The default build is serial, because
script hooks need it and because the digest test has to pass first.

`f64` is a cargo feature on `balaur_physics` that swaps `rapier3d` and
`rapier2d` for their `-f64` twins and uses glamx's `D*` types at the seam.
`Transform` stays `f32` — the conversion happens where the step writes poses
back — unless the engine goes `f64` as a whole, which is a different plan.
The use case is a world larger than a few kilometres from the origin; the
cost is a second digest test matrix and a snapshot twice the size.

## Phases

Ordered by what unblocks the most game code per week of work, not by where the
gap is widest.

1. **Foundation.** P3 (file split), P5 (debug draw with `DebugRenderMode` and
   `DebugRenderStyle` behind `set_debug_draw`), P1 and P2 (`node` and `flags`
   schema types, with inspector rows). *Done when* the editor draws every
   collider as debug lines under a Physics-persona toggle, and `parse_schema`
   accepts and validates both new types.
2. **Bodies, complete.** Everything in the rigid-body table, both dimensions,
   including the 3D angular-velocity readers that close the asymmetry with
   2D, `set_body_type` in place, sleep thresholds, and the prediction
   readers. *Done when* a scene can lock rotation on a capsule, and forces,
   torques, damping, gravity scale, CCD, dominance and per-body sleep all
   round-trip through the snapshot with a digest row each.
3. **Colliders, complete.** Offsets, child colliders on the ancestor body,
   node scale, layers and masks, combine rules, contact skin, collider mass,
   `enabled`, `set_collider`, `aabb`, `handles`, `user_data` as the reverse
   lookup, and 2D shape parity (triangle, segment, polyline, trimesh,
   convex_hull, heightfield, halfspace, `polygon`). *Done when* `examples/`
   has one body with three child colliders and the editor can pick each of
   them.
4. **Queries.** The whole `QueryPipeline` behind the filter table including
   `predicate`, the pairwise parry queries, both dimensions; `overlaps`
   reimplemented on top — sorted, filtered, and O(1) per hit through
   `user_data`. *Done when* a script can raycast the world, and two machines
   agree on the hit list for 10,000 random rays.
5. **Events, contacts and hooks.** `ActiveEvents`, `step_with_events`, the
   three event handlers, `contacts(node)` in both dimensions,
   `max_contact_impulse` in 3D, `ActiveHooks` with the three script hooks
   and the built-in `one_way`. *Done when* a trigger volume calls
   `on_collision_start`, a script `filter_contact` makes a wall passable from
   one side, and a replay of the same recording does both in the same order.
6. **Joints.** All eight kinds — seven in 2D, which has no spherical — both
   solvers, motors and limits, `break_force`, forward and inverse kinematics
   on reduced chains, the `joint3d` / `joint2d` components, editor gizmos for
   the anchors. *Done when* a ragdoll, a motorised wheel and an IK arm are in
   `examples/`, and a rollback test restores a joint mid-motor.
7. **Character controller.** Both dimensions, `push_bodies`, the collision
   list, and `FIX_INTERNAL_EDGES` on trimesh and heightfield colliders so it
   does not snag. *Done when* the 3D and 2D examples both ship a controllable
   character with no hand-rolled ground check.
8. **The remaining shapes.** Voxels (asset, `voxelized_mesh`, run-time edits
   and splits, digest revision, `collider_mesh`), convex decomposition,
   compound, halfspace, round borders, `fit`, trimesh and heightfield flags.
   *Done when* a voxel terrain can be dug into, drawn from its own collider,
   and the hole survives a save/load.
9. **World tuning, profiling and safety.** `[physics]` in the project
   manifest, setter/reader pairs for the integration parameters and gravity,
   `length_unit` wired to the 2D pixel scale, quarantine reported as a WARN
   naming the node, `counters()` in the editor profiler, `CollisionPipeline`
   for the paused editor. *Done when* the manifest, not a script, is what a
   replay agrees with, and the profiler shows the solver's share of a frame.
10. **Vehicle and PID.** `vehicle3d` with `wheel3d` children, every tuning
    field and every wheel reader; the PID controller as a kinematic-body
    driver. *Done when* a drivable car is an example.
11. **Geometry.** The `geometry3d` / `geometry2d` modules: split, slice, clip,
    boolean intersection, pieces, hull, decomposition, voxelize, convex
    partition, OBJ export. *Done when* a script slices a crate in two and both
    halves fall as bodies.
12. **Parallel step.** The `parallel` feature, `set_threads`, and the
    1/2/8-thread digest matrix on all three CI OSes. *Done when* the matrix
    agrees bit for bit, and scene load refuses `hooks` with a message on a
    parallel build.
13. **f64.** The `f64` feature and its digest matrix. *Done when* a body at
    100 km from the origin rests without jitter.

Every phase carries the same four-line acceptance beyond its own: a test in
`crates/balaur_physics/tests/`, a row in the cross-OS determinism digest, a
snapshot round-trip, and `scripts/gen_docs.py` re-run so `docs/generated` and
the website's reference pages carry the new surface.

## Open questions

1. **Teleporting a dynamic body.** The step writes simulated poses into
   `Transform`, and nothing reads `Transform` back for a dynamic body, so a
   script that assigns `node.position` is silently overwritten one step later.
   `physics3d.teleport(node, x, y, z)` (set the pose, clear velocity, wake)
   is the honest fix; the alternative — a dirty flag on `Transform` feeding
   the body every step — is cheaper to use and costs a comparison per body per
   step. Decide in phase 2, because every phase after it assumes an answer.
2. **Script hooks on a parallel build.** rapier calls hooks from worker
   threads under `parallel`, so a hook that talks to Rune cannot run there.
   One design lifts the exclusion at the price of one step of latency: the
   `Sync` hook reads a `DetHashMap<(handle, handle), Decision>` filled on the
   main thread, and after each step the plugin asks the script about the
   pairs the hook saw for the first time, so the answer applies from the next
   step. A new pair defaults to *allow* for one step. Whether that latency is
   acceptable is a question for the first game that needs both; until then
   the two are exclusive and say so.
3. **Where world tuning lives.** Script setters make a replay depend on
   script order; a manifest section makes a game unable to tune at run time.
   Phase 9 proposes both with the manifest as the truth a recording carries —
   which needs the recording header to include it, and that is a change in
   `balaur_core`, not here.
4. **Layer names.** `layers = ["0", "3"]` is honest and unreadable. Godot
   names layers in project settings and the inspector relabels the
   checkboxes. That needs schema options resolved at inspector time rather
   than at registration, which no other component wants; it can wait until
   someone has shipped a game with 30 layers.
5. **`f64` for the whole engine.** Phase 13 converts at the physics seam. If
   a game needs `f64` in `Transform` too — a solar system, not a large map —
   that is glamx's `D*` types through every crate, and a plan of its own.
6. **What the 2D `heightfield` is for.** In 2D it is a polyline over a height
   array, which is a side-scroller's ground. Whether that beats authoring a
   `polygon` is a question for whoever builds the second 2D example.
