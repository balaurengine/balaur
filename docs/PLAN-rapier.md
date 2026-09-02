> **Status:** not started. Written 2026-09-02. An inventory of what Rapier
> 0.35.3 and parry 0.30.2 ship **today**, which of it `balaur_physics` does
> not reach, and the order to close the gap in. Every row exists in
> `Cargo.lock` right now: nothing here waits on upstream. That is what
> separates this file from `docs/PLAN-physics.md`, which is the other half of
> physics — soft bodies, tearing, fluids and granular materials, none of
> which Rapier has written yet.

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
| `set_linear_damping`, `set_angular_damping` | `linear_damping`, `angular_damping` | |
| `set_gravity_scale` | `gravity_scale` | Per body: `0` for a platform that hangs in the air, negative for a balloon |
| `set_enabled_translations` / `_rotations`, `lock_*` | `lock_translation`, `lock_rotation` | Needs P2 (`flags`). This is how 2.5D and top-down games are built |
| `enable_ccd`, `set_soft_ccd_prediction`, `set_allow_fast_rotation` | `ccd`, `soft_ccd` | Bullets through walls |
| `set_dominance_group` | `dominance` | An `i8`: a higher group wins every contact against a lower one, and every non-dynamic body outranks them all. The player shoving crates that cannot shove back |
| `set_additional_mass`, `set_additional_mass_properties`, `recompute_mass_properties_from_colliders` | `mass`, `center_of_mass` | `0` keeps today's density-derived mass |
| `set_additional_solver_iterations` | `solver_iterations` | Per-body accuracy, for the one stack that jitters |
| `set_enabled` | `enabled` | Cheaper than removing and rebuilding a body |
| `add_force`, `add_torque`, `add_force_at_point`, `reset_forces`, `reset_torques` | `add_force`, `add_torque`, `add_force_at_point`, `reset_forces`, `reset_torques` | Continuous force, as against today's impulse-only |
| `apply_torque_impulse`, `apply_impulse_at_point` | same names | |
| `set_angvel`, `angvel` (3D) | `set_angular_velocity`, `angular_velocity` | Exists in 2D only today — a pure asymmetry |
| `velocity_at_point`, `mass`, `kinetic_energy` | `velocity_at_point`, `mass`, `kinetic_energy` | Readers N8 already wants |
| `sleep`, `wake_up`, `is_sleeping` | `sleep`, `wake_up`, `is_sleeping` | Per body; today sleeping is a world-wide toggle |
| `set_position` on a dynamic body | `teleport` | See Open question 1 |
| `enable_gyroscopic_forces` | `gyroscopic` | Tumbling in 3D |

### Colliders

| Rapier 0.35 | Balaur surface | Note |
| --- | --- | --- |
| `position_wrt_parent` | `offset`, `offset_rotation` | Today a collider sits exactly on its node |
| Several colliders per body | Colliders on child nodes attach to the nearest ancestor body | The compound-shape story; see Design |
| `collision_groups`, `solver_groups` (`InteractionGroups`, 32 layers) | `layers`, `mask`, `solver_layers`, `solver_mask` | Needs P2 |
| `active_events` (`COLLISION_EVENTS`, `CONTACT_FORCE_EVENTS`) | `events = ["collision", "contact_force"]` | Gates the events phase |
| `contact_force_event_threshold` | `contact_force_threshold` | |
| `active_collision_types` | `active_collisions` | Static-static pairs, off by default in rapier |
| `active_hooks` + `ContactModificationContext::update_as_oneway_platform` | `one_way`, `one_way_axis` | Platformers |
| `friction_combine_rule`, `restitution_combine_rule` (`CoefficientCombineRule`) | `friction_combine`, `restitution_combine` | `average` \| `min` \| `multiply` \| `max` \| `clamped_sum` |
| `contact_skin` | `contact_skin` | Speculative margin; stops thin-shape tunnelling |
| `set_mass`, `set_mass_properties` | `mass` on the collider | Overrides density |
| `set_enabled` | `enabled` | |
| `set_shape` at run time | `physics3d.set_collider(node, params)` | Already implied by `apply_collider`; make it a call |

### Shapes not built today

| Rapier / parry | Balaur `kind` | Note |
| --- | --- | --- |
| `voxels`, `voxels_from_points`, `voxelized_mesh` (parry `Voxels`, with `set_voxel`, `crop`, `split_with_box`) | `voxels`, `voxelized_mesh` | Both dimensions. Destructible terrain: the grid is editable at run time |
| `compound` | `compound` | Or, better, child colliders — see Design |
| `convex_decomposition` (+ `_with_params`) | `convex_decomposition` | The only way to get a *dynamic* concave shape |
| `halfspace` | `halfspace` | An infinite floor, one plane, no tessellation |
| `segment` | `segment` | |
| `round_cuboid`, `round_cylinder`, `round_cone`, `round_triangle`, `round_convex_*` | `border` float on the existing kinds | A rounded shape is a shape plus a radius, not nine more kinds |
| `convex_polyline` (2D), `convex_mesh` / `ConvexPolyhedron` (3D) | `convex_hull` accepts pre-built hulls | |
| `trimesh_with_flags` (`TriMeshFlags`: `FIX_INTERNAL_EDGES`, `MERGE_DUPLICATE_VERTICES`, `DELETE_DEGENERATE_TRIANGLES`, `ORIENTED`) | `fix_internal_edges`, `clean` | `FIX_INTERNAL_EDGES` is what stops a character controller catching on flat terrain seams |
| 2D: triangle, segment, polyline, trimesh, convex_hull, heightfield, halfspace | all of the above | 2D has three shapes today against 3D's ten |

### Queries — the whole `QueryPipeline`

| Rapier 0.35 | Balaur surface |
| --- | --- |
| `cast_ray`, `cast_ray_and_get_normal`, `intersect_ray` | `raycast`, `raycast_all` |
| `cast_shape`, `cast_shape_nonlinear` | `shapecast` |
| `project_point`, `project_point_and_get_feature`, `intersect_point` | `nearest_point`, `point_hits` |
| `intersect_shape`, `intersect_aabb_conservative` | `shape_hits`, `box_hits` |
| `QueryFilter` (`exclude_sensors`, `exclude_solids`, `only_dynamic`, `only_kinematic`, `only_fixed`, `groups`, `exclude_collider`, `exclude_rigid_body`, `predicate`) | one `filter` options table, everything but `predicate` |

Scripts have `overlaps` today, and it reports only pairs where one side is a
sensor. There is no way to ask a question of the world that is not already a
collision.

### Events and contacts

| Rapier 0.35 | Balaur surface |
| --- | --- |
| `step_with_events`, `EventHandler`, `CollisionEvent::Started` / `Stopped` | `on_collision_start(other)`, `on_collision_stop(other)` on the node's script |
| `ContactForceEvent` | `on_contact_force(other, magnitude)` |
| `contact_pairs_with`, `ContactPair`, manifolds and points | `physics3d.contacts(node)` → point, normal, impulse per contact |
| `intersection_pairs_with` | `overlaps`, kept, but filtered and sorted |

3D has no `max_contact_impulse`; 2D has it and no `contacts`. Both get both.

### Joints — none of them exist today

`fixed`, `revolute`, `prismatic`, `spherical` (3D only), `rope`, `spring`,
`pin_slot`, plus `GenericJoint` behind them all: per-axis limits, motors
(`MotorModel::AccelerationBased` / `ForceBased`, target velocity, target
position, max force, stiffness, damping), `contacts_enabled`, softness, and
two solvers — `insert_impulse_joint` and `insert_multibody_joint` (reduced
coordinates: no drift, no loops).

### Character controller — does not exist today

`KinematicCharacterController`: `up`, `offset`, `slide`, `autostep`
(`max_height`, `min_width`, `include_dynamic_bodies`), `max_slope_climb_angle`,
`min_slope_slide_angle`, `snap_to_ground`, `normal_nudge_factor`,
`move_shape` returning `EffectiveCharacterMovement { translation, grounded,
is_sliding_down_slope }`, and `solve_character_collision_impulses` so a
character can push what it walks into.

### Vehicle and PID controllers — do not exist today

`DynamicRayCastVehicleController` with `WheelTuning` (suspension stiffness,
compression and relaxation damping, max travel, friction slip, force
multipliers), `add_wheel`, `update_vehicle`, engine force, brake and steering
per wheel. `PidController` / `PdController` with `set_axes`, for driving a
kinematic body toward a target pose.

### World tuning, and the two things rapier does for us

| Rapier 0.35 | Balaur surface |
| --- | --- |
| `IntegrationParameters`: `num_solver_iterations`, `num_internal_pgs_iterations`, `num_internal_stabilization_iterations`, `max_ccd_substeps`, `min_ccd_dt`, `length_unit`, `contact_softness`, `static_contact_softness`, `warmstart_coefficient`, `warmstart_joints`, `friction_model`, `contact_clustering`, `contact_recycling`, the `normalized_*` tolerances | `[physics]` in the project manifest, plus setter/reader pairs |
| `PhysicsWorld::quarantine()` — bodies and colliders rapier disabled because their pose, velocity or geometry went non-finite | a WARN naming the node, and `physics3d.quarantined()` |
| `PhysicsWorld::debug_render` | `physics.set_debug_draw(true)` into `DebugLineBuffer` / `DebugLineBuffer2d` |

`length_unit` deserves its own line: it is how rapier is told that a 2D world
measures in pixels rather than metres, and every tolerance in the solver is
scaled by it. A 2D game that feels like soup at 64 px/m is one field away
from feeling right.

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
`events = ["collision"]`, `layers = ["0", "3"]`. An empty array means *none*,
except on `mask`, where it means *everything* — stated once in the schema
description rather than forcing 32 strings into every scene file. This is one
new type against three otherwise-inevitable bespoke encodings.

**P3 — Split `balaur_physics` into files.** `lib.rs` is 853 lines against
`MAX_FILE_LINES = 1200`, and phase 2 alone doubles it. Target layout:

```
crates/balaur_physics/src/
  lib.rs        plugin, state, step, snapshot, digest
  vocabulary.rs dimension-free schema text and param extraction
  body.rs  collider.rs  joint.rs  query.rs  events.rs  character.rs  vehicle.rs
  debug.rs
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
enabled = true

[nodes.collider3d]
kind = "cuboid"
offset = [0.0, 0.5, 0.0]
layers = ["0"]
mask = []                         # empty = every layer
events = ["collision"]
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

### Joints

```toml
[nodes.joint3d]
kind = "revolute"                 # fixed | revolute | prismatic |
                                  # spherical | rope | spring | pin_slot
body = "../Cart"                  # type = "node"; this node is the other end
anchor = [0.0, 0.5, 0.0]
other_anchor = [0.0, -0.5, 0.0]
axis = [0.0, 0.0, 1.0]
limits = [-0.7, 0.7]              # empty = free
motor = "off"                     # off | velocity | position
motor_target = 0.0
motor_max_force = 0.0
stiffness = 0.0
damping = 1.0
length = 0.0                      # rope: max; spring: rest
contacts = false                  # let the two bodies collide
solver = "impulse"                # impulse | reduced (rapier's multibody joint)
```

The joint lives on a node so it can be selected, gizmo-drawn and deleted like
anything else; `joints: DetHashMap<Entity, ImpulseJointHandle>` joins the two
maps in `PhysicsState` and the two lines in `PhysicsFrame`. Motors are the
digest's business: a motor target is state a peer can disagree about, so the
digest gains a per-joint row.

### Queries

```rune
let hit = physics3d::raycast(#{
    from = [0.0, 2.0, 0.0],
    dir = [0.0, -1.0, 0.0],
    max = 100.0,
    filter = #{ exclude = self.node, only = "dynamic", mask = ["0"] },
});
if hit is Object { log::info(`hit ${hit.node} at ${hit.distance}`); }
```

A table rather than the float-triple idiom `apply_impulse` uses: a ray is
eight numbers before the filter, and the filter is genuinely optional (N9's
options table, not N9's constant argument). Every `*_all` result is sorted by
distance, then by entity bits, before it crosses the binding seam — rapier
walks its BVH, and BVH order is not something a replay may depend on.

### Events

`collider3d.events` sets `ActiveEvents`; the step becomes `step_with_events`
with a collector owned by `PhysicsState`; the collector is drained **inside**
the fixed step, immediately after `world.step()`, and dispatched through
`ScriptHost::call_on` — the same seam `balaur_anim`, `balaur_net`,
`balaur_gamend` and `balaur_ui` already use. Draining after the step and
before the next `fixed_update` is what keeps a handler's impulse landing on
the step it was meant for. Events are sorted by `(entity bits, entity bits)`
before dispatch; a handler is opt-in, and a node without one costs a hash
lookup.

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
push_bodies = true
lengths = "absolute"              # absolute | relative to the shape
```

```rune
let moved = physics3d::move_character(self.node, dx, dy, dz);
if moved.grounded { self.jumps = 0; }
```

`move_character` applies the effective translation to the node's transform
and returns `#{ x, y, z, grounded, sliding }`. It reads the query pipeline, so
it must run in `fixed_update` — the binding says so, and the docs page says so
twice.

### Voxels

A `voxels` asset (`voxel_size` plus filled cell coordinates, the same shape of
thing `heightfield` already is), a `voxelized_mesh` kind that builds one from
a `mesh` asset at a given cell size and fill mode, and run-time edits:

```rune
physics3d::set_voxel(node, x, y, z, false);   // dig
let filled = physics3d::voxel(node, x, y, z);
let cell = physics3d::voxel_at(node, wx, wy, wz);
```

An edited grid is simulation state that no velocity reflects until something
falls into the hole, so the digest gains a revision counter per voxel
collider, bumped by every edit. Balaur does not draw voxels: the collider is
the shape, and the mesh beside it is the game's business until the renderer
has a voxel component of its own.

## Phases

Ordered by what unblocks the most game code per week of work, not by where the
gap is widest.

1. **Foundation.** P3 (file split), P5 (debug draw), P1 and P2 (`node` and
   `flags` schema types, with inspector rows). *Done when* the editor draws
   every collider as debug lines under a Physics-persona toggle, and
   `parse_schema` accepts and validates both new types.
2. **Bodies, complete.** Everything in the rigid-body table, both dimensions,
   including the 3D angular-velocity readers that close the asymmetry with 2D.
   *Done when* a scene can lock rotation on a capsule, and forces, torques,
   damping, gravity scale, CCD, dominance and per-body sleep all round-trip
   through the snapshot with a digest row each.
3. **Colliders, complete.** Offsets, child colliders on the ancestor body,
   layers and masks, combine rules, contact skin, collider mass, `enabled`,
   `set_collider`, and 2D shape parity (triangle, segment, polyline, trimesh,
   convex_hull, heightfield, halfspace). *Done when* `examples/` has one body
   with three child colliders and the editor can pick each of them.
4. **Queries.** The whole `QueryPipeline` behind the filter table, both
   dimensions; `overlaps` reimplemented on top of it — sorted, filtered, and
   without today's quadratic `contains` scan. *Done when* a script can raycast
   the world, and two machines agree on the hit list for 10,000 random rays.
5. **Events and contacts.** `ActiveEvents`, `step_with_events`, the three
   handlers, `contacts(node)` in both dimensions, `max_contact_impulse` in 3D.
   *Done when* a trigger volume calls `on_collision_start` and a replay of the
   same recording calls it in the same order.
6. **Joints.** All seven kinds — six in 2D, which has no spherical — both
   solvers, motors and limits, the `joint3d` / `joint2d` components, editor
   gizmos for the anchors. *Done when* a ragdoll and a motorised wheel are in
   `examples/`, and a rollback test restores a joint mid-motor.
7. **Character controller.** Both dimensions, `push_bodies`, and
   `FIX_INTERNAL_EDGES` on trimesh colliders so it does not snag. *Done when*
   the 3D and 2D examples both ship a controllable character with no
   hand-rolled ground check.
8. **The remaining shapes.** Voxels (asset, `voxelized_mesh`, run-time edits,
   digest revision), convex decomposition, compound, halfspace, round borders,
   trimesh flags. *Done when* a voxel terrain can be dug into and the hole
   survives a save/load.
9. **World tuning and safety.** `[physics]` in the project manifest,
   setter/reader pairs for the integration parameters, `length_unit` wired to
   the 2D pixel scale, quarantine reported as a WARN naming the node.
   *Done when* the manifest, not a script, is what a replay agrees with.
10. **Vehicle and PID.** `vehicle3d` with `wheel3d` children; the PID
    controller as a kinematic-body driver. *Done when* a drivable car is an
    example.

Every phase carries the same four-line acceptance beyond its own: a test in
`crates/balaur_physics/tests/`, a row in the cross-OS determinism digest, a
snapshot round-trip, and `scripts/gen_docs.py` re-run so `docs/generated` and
the website's reference pages carry the new surface.

## Not exported, deliberately

| Rapier offers | Why not |
| --- | --- |
| `configure_thread_pool`, `set_thread_pool`, `num_threads` | The gameplay tick is serial by design (ARCHITECTURE.md's roadmap says so for systems generally); a thread count that changes results is the exact thing `enhanced-determinism` is bought to prevent |
| `rapier3d-f64` | f32 plus `enhanced-determinism` is the cross-platform contract; a second float width doubles the digest problem |
| `PhysicsHooks::filter_contact_pair` driven by a script predicate | A script call inside the narrow phase, once per pair per step. Layers and `one_way` cover the real uses declaratively; revisit only with a profile in hand |
| `user_data` on bodies and colliders | The entity ↔ handle maps are the mapping, and two mappings drift |
| Raw `RigidBodyHandle` / `ColliderHandle` in scripts | A node is the handle scripts get; handles outlive the nodes that own them, and a stale one indexes into someone else's body |
| `QueryFilter::predicate` | Same reason as the contact-pair hook, one layer up |
| `debug_render` styling knobs | One debug style, drawn through `DebugLineBuffer`; a game that wants prettier draws it itself |

## Open questions

1. **Teleporting a dynamic body.** The step writes simulated poses into
   `Transform`, and nothing reads `Transform` back for a dynamic body, so a
   script that assigns `node.position` is silently overwritten one step later.
   `physics3d.teleport(node, x, y, z)` (set the pose, clear velocity, wake)
   is the honest fix; the alternative — a dirty flag on `Transform` feeding
   the body every step — is cheaper to use and costs a comparison per body per
   step. Decide in phase 2, because every phase after it assumes an answer.
2. **Where world tuning lives.** Script setters make a replay depend on
   script order; a manifest section makes a game unable to tune at run time.
   Phase 9 proposes both with the manifest as the truth a recording carries —
   which needs the recording header to include it, and that is a change in
   `balaur_core`, not here.
3. **Layer names.** `layers = ["0", "3"]` is honest and unreadable. Godot
   names layers in project settings and the inspector relabels the
   checkboxes. That needs schema options resolved at inspector time rather
   than at registration, which no other component wants; it can wait until
   someone has shipped a game with 30 layers.
4. **What the 2D `heightfield` is for.** In 2D it is a polyline over a height
   array, which is a side-scroller's ground. Whether that beats authoring a
   polyline is a question for whoever builds the second 2D example.
