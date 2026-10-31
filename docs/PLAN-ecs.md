> **Status:** built 2026-10-31, every step. Steps 1 and 2: the door
> (`Engine::spawn`, `spawn_node`, `insert`, `insert_one`, `remove_one`,
> `remove`, `reparent`, `despawn`) and every crate moved onto it; the
> `world-write` house lint fails a `world_mut()` outside the kernel's own
> code and tests. Step 3 built 2026-10-31: `Engine::change_tick`, stamps in
> `components::ticks` fed by the calls that feed the change log, and
> `scene.added_since`, `changed_since`, `removed_since` for scripts. Two
> calls differ from §2 below, each with its reason there. Step 4 built
> 2026-10-31: `Lifecycle` on every definition, declared on the derive, run by
> the registry's three write paths and a freed node; `Written::Added` gone
> (the timer and the notifier moved to `on_add`), and 36 hand `remove`
> closures that only tore down beside-state moved to `on_remove`. Step 5
> built 2026-10-31: `Engine::defer` and `defer_spawn_node`, `defer_spawn`,
> `defer_insert`, `defer_insert_one`, `defer_remove_one`, `defer_despawn`,
> made at each stage's end and each fixed step's; `Command` is gone, and a
> free is `defer_free`, still the frame's batch. Step 6 built 2026-10-31:
> `App::add_messages`, `Engine::send`, `MessageCursor` (`messages`). Step 7
> built 2026-10-31: `events::observe`, `unobserve`, `bubble`, run first in
> `events::announce`. Step 8 built 2026-10-31: `relationships`, and the scene
> tree's `Parent` and `Children` on it. Step 9 built 2026-10-31:
> `#[component(requires = [..])]`, `ComponentDef::requires`,
> `scene.component_requires`; bodies, bones and characters require
> `transform`, and `transform::ensure` is gone. Step 10 built 2026-10-31:
> `schedule::System` (`in_set`, `before`, `after`, `run_if`), `Set`,
> `App::add_scheduled`, `configure_set`, `check_schedules`; a plain
> `add_system` is named by its function's path. Step 11 built 2026-10-31:
> `modes` (`App::add_mode`, `add_sub_mode`, `add_computed_mode`,
> `on_enter`, `on_exit`; `Engine::mode`, `set_mode`; `in_mode`). Step 12
> built 2026-10-31: `system_param` (`Res`, `ResMut`, their `Option`s,
> `Query`, `Commands`, `Local`, `Step`, `MessageReader`, `MessageWriter`,
> `Changed`, `Added`), `App::add_typed`, `Registry::add_typed`,
> `System::typed`; render's debug-line clear moved over. Step 13 built
> 2026-10-31: `Query::par_for_each` and `par_fold`; the render mirror,
> measured first, went from 2.12 ms to 0.42 ms a frame (3D drop, 8,000
> bodies) and 2.64 ms to 0.51 ms (2D pyramid, 10,000) by sorting the few
> nodes each pass wants by `scene::tree_rank`, not by threads. The costs §3
> asks for are measured and in ARCHITECTURE (*What the ECS work costs*). Steps 9 to 11 of
> PLAN-foundations: after the derive is finished and the core is split.

# Plan: the ECS on hecs

What Bevy's `bevy_ecs` and `bevy_app` give, built into `balaur_core` on
hecs 0.11. Today a system is a `FnMut(&Engine, f32)` in one of eight stages,
run in registration order. Change tracking covers registry writes only;
events are named strings delivered a frame later; the one deferred command is
`Command::Free`; `Parent` and `Children` are kept by hand.

## 1. Why hecs can carry it

hecs hands a query a plain `&mut T` and has no hooks, so two rules do what
it does not.

- **One door for every write.** The `World` goes private behind `Engine`:
  spawn, insert, remove and despawn are `Engine` calls that run what hangs
  off them. On `editor-redesign`, Rust code wrote the world directly at
  `world_mut()` in 75 places in `balaur_render`, 49 in `balaur_core`, 21 in
  `balaur_physics`, 6 in `balaur_animation`, 4 in `balaur_input` and 4 in
  `balaur_ui`. Count again at step 1; the derive work moves some.
- **A tick on what is tracked.** A tracked component carries the tick it
  last changed, stamped from its `DerefMut`, as hecs's own `ChangeTracker`
  docs suggest. The derive writes it, so a component author marks a struct
  and nothing else.

## 2. What each feature is here

Names marked *(name)* wait on NAMING.md: a hook and a state already mean
something else in this engine.

- **Ticks.** One engine counter, `change_tick`, apart from the simulation's
  `tick`: stepped before and after every system run and at each frame's
  start, so a write between two systems is newer than what the first one
  read. A reader keeps the tick it last ran at; "changed since" is a
  comparison. A `u64`, so it never wraps. *Built differently:* the counter
  is not in the snapshot and never goes back. A reader's last tick lives in
  the reader (a closure, a script), which no snapshot reaches, so a counter
  put back behind it would hide every write until it caught up. A restore
  steps it and marks the moment instead: a reader that last ran before it
  hears every holder as changed, so a system driven by changes re-syncs
  everything once, which costs work and never a wrong answer.
- **Change detection.** Added, changed and removed since a tick, by registry
  index (`components::ticks`), which is how scripts address components too;
  what came off is kept for the frame it came off in and the next. A write
  of what a component already holds stamps nothing, as it logs nothing.
  *Built differently:* the change log and the stamps are fed by the same
  call per write (`changes::component`, `property`, `attached`, `removed`,
  and a freed node's components), not the log from the stamps: the log
  carries which properties moved and what they held, which a stamp cannot
  say. A system's own writes are seen through `Tracked<T>` in a typed query,
  built 2026-10-31: its item, `Mut<'_, T>`, notes the write on its first
  `DerefMut`, on the writing thread (a batch on the pool hands its notes
  over when it ends), and the schedule stamps them all in one pass with the
  system's change tick when the system ends. They stamp, they do not log:
  undo and the editor hear the registry's writes. A plain `&mut T` stays
  free and unseen. A tracked write costs 10 ns, bevy's 0.65: bevy keeps a
  tick column beside each component, which is the step past this one.
- **Lifecycle calls**. Bevy's component hooks: `on_add`, `on_insert`,
  `on_replace`, `on_remove` and `on_despawn`, plain `fn(&Engine, Entity)`,
  declared on the derive (`#[component(on_add = f)]`) or on a hand
  definition's `lifecycle`. The registry runs them: a whole write, a write of
  one property, a removal, a freed node (every `on_despawn` first). A call
  never refuses; `after_write`, which can, runs before them, and a refused
  write's `on_replace` is followed by an `on_insert` on the values put back.
  Writes made around the registry (the door's `insert_one` of a mirror
  component) run none: those are the engine's own bookkeeping. They never
  call a script: anything a script should hear goes through
  `events::announce`'s queue. The command queue they were to get arrives
  with step 5.
- **Observers.** A Rust function run at once when an event is announced on
  one node or on any, carried up the tree when the event says so. Script
  listeners keep their delivery at the top of the next `Update`
  (PLAN-events). `events::observe(eng, name, Some(node) | None, f)` answers
  an `ObserverId` for `unobserve`; `events::bubble(eng, name)` declares a
  name that climbs. Observers run before the node's rows and hook, the
  node's own, then each ancestor's until one calls `Observed::stop`, then
  any node's once. Everything that will hear an event is taken before the
  first runs, so one added meanwhile hears the next. A freed node's go with
  the pump's sweep of listeners.
- **Messages.** A typed Rust event, written by one system and read by others
  with a cursor each, kept two frames, beside the string events scripts use.
  Declared with `App::add_messages::<T>()`, sent with `Engine::send`, read
  with a `MessageCursor<T>` the reader keeps, which also answers how many it
  missed. Not in a snapshot: a restore drops every one kept, and the
  numbering goes on, so no cursor is left ahead.
- **Commands.** Spawn, insert, remove, despawn and a closure over `Engine`,
  queued and applied in order at the end of each stage, and after each fixed
  step; one a command queues runs in the same pass. A spawn reserves its
  entity at once (`World::reserve_entity` takes a shared borrow), so a
  system iterating a query can spawn and then name what it spawned. Freeing
  a node stays the frame's: `defer_free` queues it for the one batch in
  `Last`, what a script's `queue_free` has always said.
- **Relationships.** A pair of components kept consistent: set the one side,
  the other follows, and despawning a target can take its sources with it.
  The scene tree is the first: `Parent` and `Children` keep their names and
  their order, which the digest walks. *Built differently:* the calls
  (`relationships::relate`, `link`, `unlink`, `place`, `unlink_all`,
  `linked`) keep the two sides in step, not lifecycle calls: the tree is not
  a registry component, and every write of it already went through
  `scene`, which now writes both sides through these. `linked` walks a
  subtree in the order a free always went, the last source first, so no
  free and no digest moved.
- **Required components.** A component that brings others with their
  defaults, declared on the derive, so a scene file, a script and Rust all
  get them. `#[component(requires = [name, ..])]` (a name or a `COMPONENT`
  const), or a hand definition's `requires`: before the component's first
  write the registry adds each one the node lacks, at its defaults, and
  each brings its own. A refused first write keeps what it brought; taking
  the component off leaves them.
- **Schedules.** Sets, `before`, `after` and run conditions on a system,
  sorted once when the app builds; a cycle is an error naming its systems.
  Systems with no order between them run in registration order. The eight
  stages stay the frame's spine. A stage sorts the first time it runs after
  a system was added (`App::check_schedules` sorts every stage up front and
  answers the cycle instead of panicking), taking the earliest-added system
  free to run each time. A label naming no system or set orders nothing and
  is logged, so a plugin may order itself against an optional one. A set's
  conditions are asked once a stage run. A plain `add_system` is named by its
  function's path (`std::any::type_name`), so a `fn` item is a label as it
  stands.
- **App modes**. Bevy's `States`: a value with enter and exit
  schedules, sub-modes that exist while a parent mode holds a value, and
  computed modes. In the digest and the snapshot. Named *mode*, since
  `states` is the component of named looks. Any `Copy + Eq + Debug + serde`
  type is one. A mode set now changes at the next frame's start, before
  `First`, in the order the modes were declared, so a sub-mode follows its
  parent and a computed mode the ones before it in the same frame. A
  sub-mode starts at its first value each time its parent comes to the
  value. A restore puts the value back and enters or leaves nothing.
- **Typed systems.** A Rust function whose parameters say what it reads and
  writes: a query with filters, a resource, `ResMut`, commands, a message
  reader or writer, a local. Borrowed through the engine's `RefCell`s, and a
  clash panics naming the system and the parameter. A plain
  `FnMut(&Engine, f32)` stays a system. Up to eight parameters, registered
  with `add_typed` (a separate call, so a plain closure keeps its argument
  types inferred). A query's filters are `Changed<C>` and `Added<C>`, the
  nodes a registered component moved on since the system last ran, beside
  the query rather than inside it, since change ticks are kept by registry
  index. `Step` is the seconds the stage handed. Most systems in the tree
  stay plain: they reach the engine through helpers (`node_at`, `announce`)
  that take `&Engine`, and moving them would only hide that behind a
  parameter.
- **Parallel work inside a system.** Native only, over hecs's
  `iter_batched`: each entity's work alone, and any sum combined in batch
  order so the result is the same on every run. Serial on the web.
  `Query::par_for_each(batch, f)` and `par_fold(batch, start, fold,
  combine)` on rayon; a fold's batches are combined in batch order, so its
  bits match run to run. The render mirror was the first candidate, and the
  measurement said otherwise: in a 3D scene most of its 2.1 ms was the 2D
  passes walking every node of the tree in order (draw order, layer order,
  2D and 3D lights, the environment), each looking up components node by
  node. `scene::tree_rank` keeps the tree's order until its shape moves, and
  each pass now queries the few nodes holding its component and sorts them
  by rank. What is left writes kiss3d's scene graph, which is `Rc` and
  single-threaded, so it stays serial.

## 3. What every step keeps

- **The tick reproduces.** `scripts/determinism_trace.sh compare` stays
  clean, and a digest that moves says why in the commit.
- **A rollback restores what is new**: ticks, relationship pairs, queued
  messages and commands, either drained at the tick's edge or in the
  snapshot.
- **Scripts address components by registry index.** Nothing typed crosses
  the script seam.
- **Hot reload, the editor's undo and `balaur check`** keep working.
- **Costs are measured** against the numbers in PLAN-foundations §1: write
  and spawn time, memory per node (0.35 KB on the site's compare page), web
  bytes.

## 4. Steps

1. **The door.** `Engine` gains the structural calls and the world goes
   private to the kernel. A `world-write` house lint fails a `world_mut()`
   outside it, written first and reporting until the migration ends.
2. **The migration**, a crate a commit, render last as the largest.
3. **Ticks and change detection**, fed by the change log's calls. *Built.*
4. **Lifecycle calls**, and `after_write`'s added and changed cases moved
   onto them. *Built*: `Written` is `Whole` or `Property`.
5. **Commands.** *Built.*
6. **Messages.** *Built.*
7. **Observers**, inside `events::announce`. *Built.*
8. **Relationships**, then the scene tree on them. The digest walks children
   in the same order, so no recording moves; a test proves it. *Built*; the
   determinism trace matches the one before step 3.
9. **Required components.** *Built.*
10. **Schedules**: sets, order, run conditions, cycle errors. *Built.*
11. **App modes.** *Built.*
12. **Typed systems**, and the systems that read better with them moved over.
    *Built.*
13. **Parallel work inside a system**, for what measures as hot: the render
    mirror's sync is the first candidate. *Built*; the mirror's cost was the
    tree walks, cut without threads.

Each step brings its tests: behaviour a caller sees, a snapshot that
round-trips, and a determinism trace.

## 5. Done when

The scene tree runs on relationships, and the change log and the ticks are
fed by one call per write. The render
mirror syncs only what changed, and no Rust writes the world around the door.
The costs in §3 are measured and written into ARCHITECTURE.

Where it stands, 2026-10-31: every step is built, the tree runs on
relationships, the door's lint is an error, and the costs are in
ARCHITECTURE. The mirror walks only what changed in the tree's order and
the nodes each pass draws; it still sets every drawn node's pose, colour
and visibility each frame. Most nodes draw batched, where that is an array
copy (the `kind/shape3d` case's mirror is 0.02 ms for 2,000 spheres), so a
value check per node is left for when an unbatched scene measures hot.
