> **Status:** planned 2026-10-30. Steps 1 and 2 built 2026-10-31: the door
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
> tree's `Parent` and `Children` on it. Steps 9 to 11 of
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
  say.
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
  get them.
- **Schedules.** Sets, `before`, `after` and run conditions on a system,
  sorted once when the app builds; a cycle is an error naming its systems.
  Systems with no order between them run in registration order. The eight
  stages stay the frame's spine.
- **App modes** *(name)*. Bevy's `States`: a value with enter and exit
  schedules, sub-modes that exist while a parent mode holds a value, and
  computed modes. In the digest and the snapshot.
- **Typed systems.** A Rust function whose parameters say what it reads and
  writes: a query with filters, a resource, `ResMut`, commands, a message
  reader or writer, a local. Borrowed through the engine's `RefCell`s, and a
  clash panics naming the system and the parameter. A plain
  `FnMut(&Engine, f32)` stays a system.
- **Parallel work inside a system.** Native only, over hecs's
  `iter_batched`: each entity's work alone, and any sum combined in batch
  order so the result is the same on every run. Serial on the web.

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
9. **Required components.**
10. **Schedules**: sets, order, run conditions, cycle errors.
11. **App modes.**
12. **Typed systems**, and the systems that read better with them moved over.
13. **Parallel work inside a system**, for what measures as hot: the render
    mirror's sync is the first candidate.

Each step brings its tests: behaviour a caller sees, a snapshot that
round-trips, and a determinism trace.

## 5. Done when

The scene tree runs on relationships, and the change log and the ticks are
fed by one call per write. The render
mirror syncs only what changed, and no Rust writes the world around the door.
The costs in §3 are measured and written into ARCHITECTURE.
